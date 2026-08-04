use mtr_oudia_application::{
    ApplicationError, ListeningPortProvider, MinecraftLogProvider, MinecraftLogRead,
    MinecraftLogStatus, MtrApiClient, MtrEndpoint, MtrEndpointDiscovery,
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

struct FakeLogProvider(MinecraftLogRead);

impl MinecraftLogProvider for FakeLogProvider {
    fn read_latest_log(&self) -> MinecraftLogRead {
        self.0.clone()
    }
}

impl ListeningPortProvider for FakePortProvider {
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }
}

struct FakeClient {
    valid: HashSet<String>,
    extended_valid: HashSet<String>,
    errors: HashMap<String, ApplicationError>,
    calls: Mutex<Vec<String>>,
    extended_calls: Mutex<Vec<String>>,
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

    async fn fetch_snapshot_extended(
        &self,
        endpoint: &MtrEndpoint,
        _: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum_active.fetch_max(active, Ordering::SeqCst);
        self.extended_calls
            .lock()
            .unwrap()
            .push(endpoint.as_url().as_str().to_owned());
        tokio::task::yield_now().await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        if self.extended_valid.contains(endpoint.as_url().as_str()) {
            return Ok(snapshot_response(endpoint));
        }
        Err(ApplicationError::Timeout)
    }
}

impl FakeClient {
    fn with_valid(endpoints: &[&str]) -> Self {
        Self {
            valid: endpoints
                .iter()
                .map(|endpoint| (*endpoint).to_owned())
                .collect(),
            extended_valid: HashSet::new(),
            errors: HashMap::new(),
            calls: Mutex::new(Vec::new()),
            extended_calls: Mutex::new(Vec::new()),
            active: AtomicUsize::new(0),
            maximum_active: AtomicUsize::new(0),
        }
    }
}

fn snapshot_response(endpoint: &MtrEndpoint) -> MtrSnapshotResponse {
    MtrSnapshotResponse {
        snapshot: MtrNetworkSnapshot::new(endpoint.as_url().as_str(), 0, 0, 0, vec![]).unwrap(),
        available_dimensions: vec![],
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
            timeouts: 0,
            log_status: MinecraftLogStatus::NotConfigured,
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
async fn default_http_port_probes_normalized_ipv4_and_ipv6_candidates() {
    let provider = FakePortProvider {
        result: Ok(vec![80]),
        calls: AtomicUsize::new(0),
    };
    let client = FakeClient::with_valid(&["http://127.0.0.1/"]);
    let service = MtrEndpointDiscoveryService::new(&provider, &client);

    let result = service.detect(None).await;

    assert!(
        matches!(result, MtrEndpointDiscovery::Single(endpoint) if endpoint.as_url().as_str() == "http://127.0.0.1/")
    );
    let calls = client.calls.lock().unwrap();
    assert!(calls.contains(&"http://127.0.0.1/".to_owned()));
    assert!(calls.contains(&"http://[::1]/".to_owned()));
}

#[tokio::test]
async fn minecraft_log_endpoint_is_probed_before_listening_ports() {
    let provider = FakePortProvider {
        result: Ok(vec![49000]),
        calls: AtomicUsize::new(0),
    };
    let log = FakeLogProvider(MinecraftLogRead::Contents(
        "Open the Transport System Map at http://localhost:8888".to_owned(),
    ));
    let client = FakeClient::with_valid(&["http://127.0.0.1:8888/"]);

    let result = MtrEndpointDiscoveryService::new(&provider, &client)
        .with_minecraft_log(&log)
        .detect(None)
        .await;

    assert!(
        matches!(result, MtrEndpointDiscovery::Single(endpoint) if endpoint.as_url().as_str() == "http://127.0.0.1:8888/")
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn multiplayer_fallback_supports_port_1025() {
    let provider = FakePortProvider {
        result: Ok(vec![1025]),
        calls: AtomicUsize::new(0),
    };
    let client = FakeClient::with_valid(&["http://127.0.0.1:1025/"]);

    let result = MtrEndpointDiscoveryService::new(&provider, &client)
        .detect(None)
        .await;

    assert!(
        matches!(result, MtrEndpointDiscovery::Single(endpoint) if endpoint.as_url().as_str() == "http://127.0.0.1:1025/")
    );
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
            timeouts: 0,
            log_status: MinecraftLogStatus::NotConfigured,
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
        MtrEndpointDiscovery::NoListeningPorts {
            log_status: MinecraftLogStatus::NotConfigured,
        }
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
        MtrEndpointDiscovery::PortEnumerationFailed { .. }
    ));
}

#[tokio::test]
async fn fast_timeout_is_retried_with_extended_timeout() {
    let provider = FakePortProvider {
        result: Ok(vec![44000]),
        calls: AtomicUsize::new(0),
    };
    let mut client = FakeClient::with_valid(&[]);
    client.errors.insert(
        "http://127.0.0.1:44000/".to_owned(),
        ApplicationError::Timeout,
    );
    client
        .extended_valid
        .insert("http://127.0.0.1:44000/".to_owned());

    let result = MtrEndpointDiscoveryService::new(&provider, &client)
        .detect(None)
        .await;

    assert!(
        matches!(result, MtrEndpointDiscovery::Single(endpoint) if endpoint.as_url().as_str() == "http://127.0.0.1:44000/")
    );
    assert_eq!(
        client.extended_calls.lock().unwrap().as_slice(),
        ["http://127.0.0.1:44000/"]
    );
}

#[tokio::test]
async fn invalid_response_is_not_retried_with_extended_timeout() {
    let provider = FakePortProvider {
        result: Ok(vec![45000]),
        calls: AtomicUsize::new(0),
    };
    let mut client = FakeClient::with_valid(&[]);
    client.errors.insert(
        "http://127.0.0.1:45000/".to_owned(),
        ApplicationError::InvalidResponse {
            reason: "invalid JSON".to_owned(),
        },
    );

    MtrEndpointDiscoveryService::new(&provider, &client)
        .detect(None)
        .await;

    assert!(client.extended_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn extended_timeout_retries_at_most_sixteen_candidates_concurrently() {
    let provider = FakePortProvider {
        result: Ok((46000..46009).collect()),
        calls: AtomicUsize::new(0),
    };
    let mut client = FakeClient::with_valid(&[]);
    for port in 46000..46009 {
        for endpoint in [
            format!("http://127.0.0.1:{port}/"),
            format!("http://[::1]:{port}/"),
        ] {
            client.errors.insert(endpoint, ApplicationError::Timeout);
        }
    }

    MtrEndpointDiscoveryService::new(&provider, &client)
        .detect(None)
        .await;

    assert_eq!(client.extended_calls.lock().unwrap().len(), 16);
    assert_eq!(client.maximum_active.load(Ordering::SeqCst), 16);
}

#[tokio::test]
async fn previous_timeout_is_the_first_extended_retry() {
    let provider = FakePortProvider {
        result: Ok((47000..47008).collect()),
        calls: AtomicUsize::new(0),
    };
    let previous = MtrEndpoint::parse("http://127.0.0.1:47099/").unwrap();
    let mut client = FakeClient::with_valid(&[]);
    client.errors.insert(
        "http://127.0.0.1:47099/".to_owned(),
        ApplicationError::Timeout,
    );
    for port in 47000..47008 {
        for endpoint in [
            format!("http://127.0.0.1:{port}/"),
            format!("http://[::1]:{port}/"),
        ] {
            client.errors.insert(endpoint, ApplicationError::Timeout);
        }
    }

    MtrEndpointDiscoveryService::new(&provider, &client)
        .detect(Some(previous))
        .await;

    let calls = client.extended_calls.lock().unwrap();
    assert_eq!(calls.len(), 16);
    assert!(calls.contains(&"http://127.0.0.1:47099/".to_owned()));
}

#[tokio::test]
async fn fast_and_extended_valid_candidates_are_combined_in_url_order() {
    let provider = FakePortProvider {
        result: Ok(vec![48000, 48001]),
        calls: AtomicUsize::new(0),
    };
    let mut client = FakeClient::with_valid(&["http://[::1]:48001/"]);
    client.errors.insert(
        "http://127.0.0.1:48000/".to_owned(),
        ApplicationError::Timeout,
    );
    client
        .extended_valid
        .insert("http://127.0.0.1:48000/".to_owned());

    let result = MtrEndpointDiscoveryService::new(&provider, &client)
        .detect(None)
        .await;

    assert!(
        matches!(result, MtrEndpointDiscovery::Multiple(endpoints) if endpoints.iter().map(|endpoint| endpoint.as_url().as_str()).collect::<Vec<_>>() == vec!["http://127.0.0.1:48000/", "http://[::1]:48001/"])
    );
}
