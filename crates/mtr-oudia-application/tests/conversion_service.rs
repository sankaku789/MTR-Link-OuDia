use mtr_oudia_application::{
    ApplicationError, BusinessError, ConversionService, ConversionSessionStore,
    ListeningPortProvider, MtrApiClient, MtrEndpoint, MtrSnapshotResponse, OudiaRepository,
    SaveReceipt, SettingsRepository, SettingsSnapshot, ValidatedSavePort, async_trait,
};
use mtr_oudia_domain::{
    MtrNetworkSnapshot, MtrRouteSnapshot, MtrStopSnapshot, OperationPolicy, OudiaPatch,
    ServiceTimeMillis, parse_oudia,
};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

const OUDIA: &str = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nRosen.\nEki.\nEkimei=A\n.\nEki.\nEkimei=B\n.\n.\nDia.\nKudari.\nRessya.\nRessyasyubetsu=1\nEkiJikoku=1;/10:00:00,1;10:01:00/\n.\n.\n.\n";
struct Ports {
    saved: Mutex<Vec<String>>,
}
impl ListeningPortProvider for Ports {
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError> {
        Ok(vec![12345])
    }
}
#[async_trait]
impl MtrApiClient for Ports {
    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        _: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        let time = |v| ServiceTimeMillis::new(v).unwrap();
        let route = MtrRouteSnapshot::new(
            "route",
            "Route",
            vec!["a".into(), "b".into()],
            vec![
                MtrStopSnapshot::new("a", "A", "", time(0), Some(time(1_000))).unwrap(),
                MtrStopSnapshot::new("b", "B", "", time(0), None).unwrap(),
            ],
        )
        .unwrap();
        Ok(MtrSnapshotResponse {
            snapshot: MtrNetworkSnapshot::new(endpoint.as_url().to_string(), 0, 0, 0, vec![route])
                .unwrap(),
            available_dimensions: vec![],
        })
    }
}
impl OudiaRepository for Ports {
    fn read(&self, _: &Path) -> Result<mtr_oudia_domain::OudiaSource, BusinessError> {
        Ok(parse_oudia(OUDIA.as_bytes().to_vec()).unwrap())
    }
}
impl SettingsRepository for Ports {
    fn load(&self) -> Result<SettingsSnapshot, BusinessError> {
        Ok(SettingsSnapshot {
            station_aliases: BTreeMap::new(),
            ..SettingsSnapshot::default()
        })
    }
    fn save_last_successful_endpoint(&self, endpoint: &MtrEndpoint) -> Result<(), BusinessError> {
        self.saved
            .lock()
            .unwrap()
            .push(endpoint.as_url().to_string());
        Ok(())
    }
}
impl ValidatedSavePort for Ports {
    fn save(
        &self,
        _: &Path,
        _: &Path,
        _: [u8; 32],
        _: &OudiaPatch,
    ) -> Result<SaveReceipt, BusinessError> {
        Ok(SaveReceipt {
            output_path: "out.oud2".into(),
            bytes: 1,
            sha256: "x".into(),
        })
    }
}

#[tokio::test]
async fn happy_path_uses_session_owned_candidate_and_preview() {
    let ports = Ports {
        saved: Mutex::new(Vec::new()),
    };
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &ports,
        &ports,
        &ports,
        &ports,
    );
    let session = service.create_session();
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:12345/").unwrap();
    service
        .fetch_mtr_snapshot(&session, &endpoint, 0)
        .await
        .unwrap();
    service
        .inspect_oudia(&session, Path::new("input.oud2"))
        .unwrap();
    let candidates = service
        .find_route_candidates(&session, "route", None, Some(1))
        .unwrap();
    assert_eq!(candidates.len(), 1);
    let preview = service
        .build_preview(&session, Some(&candidates[0].id), None)
        .unwrap();
    assert_eq!(preview.fixed_base_time, "10:00:00");
    service
        .save_conversion(
            &session,
            &preview.id,
            Path::new("out.oud2"),
            OperationPolicy::Preserve,
        )
        .unwrap();
    assert_eq!(ports.saved.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn candidate_from_other_session_and_upstream_change_are_stale() {
    let ports = Ports {
        saved: Mutex::new(Vec::new()),
    };
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &ports,
        &ports,
        &ports,
        &ports,
    );
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:12345/").unwrap();
    let one = service.create_session();
    let two = service.create_session();
    for id in [&one, &two] {
        service.fetch_mtr_snapshot(id, &endpoint, 0).await.unwrap();
        service.inspect_oudia(id, Path::new("input.oud2")).unwrap();
    }
    let candidate = service
        .find_route_candidates(&one, "route", None, Some(1))
        .unwrap()
        .remove(0)
        .id;
    assert!(service.build_preview(&two, Some(&candidate), None).is_err());
    service
        .fetch_mtr_snapshot(&one, &endpoint, 0)
        .await
        .unwrap();
    assert!(service.build_preview(&one, Some(&candidate), None).is_err());
}
