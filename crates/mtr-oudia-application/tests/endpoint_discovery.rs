use mtr_oudia_application::{
    ApplicationError, ListeningPortProvider, MtrApiClient, MtrEndpoint, MtrEndpointDiscovery,
    MtrEndpointDiscoveryService, MtrSnapshotResponse, async_trait,
};
use mtr_oudia_domain::MtrNetworkSnapshot;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct FakePortProvider {
    result: Result<Vec<u16>, ApplicationError>,
    calls: AtomicUsize,
}

impl ListeningPortProvider for FakePortProvider {
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }
}

struct FakeClient {
    valid: HashSet<String>,
    errors: HashMap<String, ApplicationError>,
    calls: Mutex<Vec<String>>,
    active: AtomicUsize,
    maximum_active: AtomicUsize,
}

#[async_trait]
impl MtrApiClient for FakeClient {
    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        _: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum_active.fetch_max(active, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap()
            .push(endpoint.as_url().as_str().to_owned());
        tokio::task::yield_now().await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        if self.valid.contains(endpoint.as_url().as_str()) {
            return Ok(MtrSnapshotResponse {
                snapshot: MtrNetworkSnapshot::new(endpoint.as_url().as_str(), 0, 0, 0, vec![])
                    .unwrap(),
                available_dimensions: vec![],
            });
        }
        Err(self
            .errors
            .get(endpoint.as_url().as_str())
            .cloned()
            .unwrap_or(ApplicationError::Transport {
                message: "unreachable".to_owned(),
            }))
    }
}

impl FakeClient {
    fn with_valid(endpoints: &[&str]) -> Self {
        Self {
            valid: endpoints
                .iter()
                .map(|endpoint| (*endpoint).to_owned())
                .collect(),
            errors: HashMap::new(),
            calls: Mutex::new(Vec::new()),
            active: AtomicUsize::new(0),
            maximum_active: AtomicUsize::new(0),
        }
    }
}

#[tokio::test]
async fn previous_valid_endpoint_skips_port_enumeration() {
    let provider = FakePortProvider {
        result: Ok(vec![40100]),
        calls: AtomicUsize::new(0),
    };
    let client = FakeClient::with_valid(&["http://127.0.0.1:40000/"]);
    let service = MtrEndpointDiscoveryService::new(&provider, &client);

    let result = service
        .detect(Some(MtrEndpoint::parse("http://127.0.0.1:40000/").unwrap()))
        .await;

    assert!(matches!(result, MtrEndpointDiscovery::Single(_)));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_previous_probes_only_listed_ports_with_ipv4_and_ipv6_candidates() {
    let provider = FakePortProvider {
        result: Ok(vec![41000, 41000, 0, 41001]),
        calls: AtomicUsize::new(0),
    };
    let client = FakeClient::with_valid(&[]);
    let service = MtrEndpointDiscoveryService::new(&provider, &client);

    let result = service
        .detect(Some(MtrEndpoint::parse("http://127.0.0.1:40000/").unwrap()))
        .await;

    assert!(matches!(
        result,
        MtrEndpointDiscovery::NoValidEndpoints {
            attempted: 5,
            unreachable: 5,
            invalid_responses: 0,
        }
    ));
    let calls = client.calls.lock().unwrap();
    assert_eq!(calls.len(), 5);
    assert!(calls.contains(&"http://127.0.0.1:40000/".to_owned()));
    for endpoint in [
        "http://127.0.0.1:41000/",
        "http://[::1]:41000/",
        "http://127.0.0.1:41001/",
        "http://[::1]:41001/",
    ] {
        assert!(calls.contains(&endpoint.to_owned()));
    }
}

#[tokio::test]
async fn returns_single_or_multiple_valid_endpoints_and_never_exceeds_probe_limit() {
    let provider = FakePortProvider {
        result: Ok((42000..42020).collect()),
        calls: AtomicUsize::new(0),
    };
    let client = Arc::new(FakeClient::with_valid(&[
        "http://127.0.0.1:42000/",
        "http://[::1]:42001/",
    ]));
    let result = MtrEndpointDiscoveryService::new(&provider, client.as_ref())
        .detect(None)
        .await;

    assert!(matches!(result, MtrEndpointDiscovery::Multiple(endpoints) if endpoints.len() == 2));
    assert_eq!(client.maximum_active.load(Ordering::SeqCst), 16);
}

#[tokio::test]
async fn invalid_json_and_unreachable_candidates_are_reported_separately() {
    let provider = FakePortProvider {
        result: Ok(vec![43000]),
        calls: AtomicUsize::new(0),
    };
    let mut client = FakeClient::with_valid(&[]);
    client.errors.insert(
        "http://127.0.0.1:43000/".to_owned(),
        ApplicationError::InvalidResponse {
            reason: "invalid JSON".to_owned(),
        },
    );
    let result = MtrEndpointDiscoveryService::new(&provider, &client)
        .detect(None)
        .await;

    assert!(matches!(
        result,
        MtrEndpointDiscovery::NoValidEndpoints {
            attempted: 2,
            unreachable: 1,
            invalid_responses: 1,
        }
    ));
}

#[tokio::test]
async fn no_ports_and_provider_error_are_distinct() {
    let empty_provider = FakePortProvider {
        result: Ok(vec![]),
        calls: AtomicUsize::new(0),
    };
    let client = FakeClient::with_valid(&[]);
    assert_eq!(
        MtrEndpointDiscoveryService::new(&empty_provider, &client)
            .detect(None)
            .await,
        MtrEndpointDiscovery::NoListeningPorts
    );

    let failed_provider = FakePortProvider {
        result: Err(ApplicationError::ListeningPortEnumeration {
            message: "access denied".to_owned(),
        }),
        calls: AtomicUsize::new(0),
    };
    assert!(matches!(
        MtrEndpointDiscoveryService::new(&failed_provider, &client)
            .detect(None)
            .await,
        MtrEndpointDiscovery::PortEnumerationFailed(_)
    ));
}
