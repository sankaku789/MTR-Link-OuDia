use mtr_oudia_application::{
    ApplicationError, BusinessError, ConversionService, ConversionSessionStore,
    ListeningPortProvider, MinecraftLogProvider, MinecraftLogRead, MtrApiClient, MtrEndpoint,
    MtrSnapshotResponse, OudiaRepository, SaveReceipt, SettingsRepository, SettingsSnapshot,
    ValidatedSavePort, async_trait,
};
use mtr_oudia_domain::{
    MtrNetworkSnapshot, MtrRouteSnapshot, MtrStopSnapshot, OperationPolicy, OudiaPatch,
    ServiceTimeMillis, parse_oudia,
};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

const OUDIA: &str = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nRosen.\nEki.\nEkimei=A\n.\nEki.\nEkimei=B\n.\n.\nRessyasyubetsu.\nSyubetsumei=普通\n.\nRessyasyubetsu.\nSyubetsumei=快速\n.\nDia.\nKudari.\nRessya.\nRessyasyubetsu=1\nEkiJikoku=1;/10:00:00,1;10:01:00/\n.\n.\n.\n";

#[tokio::test]
async fn display_dtos_include_normalized_route_and_oudia_metadata() {
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

    let snapshot = service
        .fetch_mtr_snapshot(&session, &endpoint, 0)
        .await
        .unwrap();
    assert_eq!(snapshot.api_current_time_millis, 0);
    assert_eq!(snapshot.routes[0].station_count, 2);
    assert_eq!(snapshot.routes[0].total_run_millis, 1_000);
    assert_eq!(snapshot.routes[0].stations[0].station_name, "A");
    assert_eq!(
        snapshot.routes[0].stations[0].run_millis_to_next,
        Some(1_000)
    );
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap()["api_current_time_millis"],
        0
    );

    let inspection = service
        .inspect_oudia(&session, Path::new("input.oud2"))
        .unwrap();
    assert_eq!(inspection.line_name, None);
    assert_eq!(inspection.station_count, 2);
    assert_eq!(inspection.train_type_names, ["普通", "快速"]);
    assert_eq!(inspection.templates[0].active_station_slots[0].name, "A");
    assert_eq!(inspection.templates[0].route_station_slots.len(), 2);
    assert_eq!(
        inspection.templates[0].active_station_slots[0].handling_code,
        Some(1)
    );
    assert_eq!(
        serde_json::to_value(&inspection).unwrap()["station_count"],
        2
    );
}

#[tokio::test]
async fn manual_template_remains_selectable_when_automatic_matching_has_no_candidate() {
    let ports = Ports {
        saved: Mutex::new(Vec::new()),
    };
    let no_match = NoMatchRepository;
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &ports,
        &no_match,
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
    assert!(candidates.iter().all(|candidate| candidate.manual_only));
    let preview = service
        .build_preview(
            &session,
            Some(&candidates[0].id),
            Some(mtr_oudia_application::ManualMappingInput {
                station_mappings: vec![
                    mtr_oudia_application::StationMappingDto {
                        mtr_station_index: 0,
                        oudia_station_slot: 0,
                    },
                    mtr_oudia_application::StationMappingDto {
                        mtr_station_index: 1,
                        oudia_station_slot: 1,
                    },
                ],
            }),
        )
        .unwrap();
    assert_eq!(preview.stops.len(), 2);
}
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
struct NoMatchRepository;
impl OudiaRepository for NoMatchRepository {
    fn read(&self, _: &Path) -> Result<mtr_oudia_domain::OudiaSource, BusinessError> {
        Ok(parse_oudia(
            OUDIA
                .replace("Ekimei=A", "Ekimei=X")
                .replace("Ekimei=B", "Ekimei=Y")
                .into_bytes(),
        )
        .unwrap())
    }
}

struct TwoTrainTypesRepository;
impl OudiaRepository for TwoTrainTypesRepository {
    fn read(&self, _: &Path) -> Result<mtr_oudia_domain::OudiaSource, BusinessError> {
        Ok(parse_oudia(
            "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nRosen.\nEki.\nEkimei=A\n.\nEki.\nEkimei=B\n.\n.\nDia.\nKudari.\nRessya.\nSyubetsu=0\nEkiJikoku=1;1000,1;1001\n.\nRessya.\nSyubetsu=1\nEkiJikoku=1;1000,1;1001\n.\n.\n.\n"
                .as_bytes()
                .to_vec(),
        )
        .unwrap())
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
async fn selected_train_type_filters_candidate_templates() {
    let dependencies = Ports {
        saved: Mutex::new(Vec::new()),
    };
    let repository = TwoTrainTypesRepository;
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &dependencies,
        &dependencies,
        &repository,
        &dependencies,
        &dependencies,
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
    assert_eq!(candidates[0].train_index, 1);
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

struct SinglePort;
impl ListeningPortProvider for SinglePort {
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError> {
        Ok(vec![8888])
    }
}

struct InvalidProbeClient;
#[async_trait]
impl MtrApiClient for InvalidProbeClient {
    async fn fetch_snapshot(
        &self,
        _: &MtrEndpoint,
        _: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        Err(ApplicationError::InvalidResponse {
            reason: "not MTR".to_owned(),
        })
    }
}

struct DetectFailsButManualFetchSucceeds;
#[async_trait]
impl MtrApiClient for DetectFailsButManualFetchSucceeds {
    async fn probe_endpoint(&self, _: &MtrEndpoint, _: u32) -> Result<(), ApplicationError> {
        Err(ApplicationError::Transport {
            message: "unreachable during detection".to_owned(),
        })
    }

    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        _: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        Ok(MtrSnapshotResponse {
            snapshot: MtrNetworkSnapshot::new(endpoint.as_url().as_str(), 0, 0, 0, vec![]).unwrap(),
            available_dimensions: vec![],
        })
    }
}

struct MissingMinecraftLog;
impl MinecraftLogProvider for MissingMinecraftLog {
    fn read_latest_log(&self) -> MinecraftLogRead {
        MinecraftLogRead::NotFound
    }
}

#[tokio::test]
async fn discovery_error_preserves_log_and_probe_diagnostics() {
    let dependencies = Ports {
        saved: Mutex::new(Vec::new()),
    };
    let ports = SinglePort;
    let client = InvalidProbeClient;
    let log = MissingMinecraftLog;
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &client,
        &dependencies,
        &dependencies,
        &dependencies,
    )
    .with_minecraft_log(&log);
    let session = service.create_session();

    let error = service.detect_mtr_endpoint(&session).await.unwrap_err();

    assert_eq!(error.message, "MTR APIの応答形式が一致しません");
    let detail = error.detail.unwrap();
    assert!(detail.contains("Minecraftログが見つかりません"));
    assert!(detail.contains("試行 2 件"));
    assert!(detail.contains("応答不正 2 件"));
}

#[tokio::test]
async fn manual_fetch_uses_the_same_session_after_detection_fails() {
    let dependencies = Ports {
        saved: Mutex::new(Vec::new()),
    };
    let ports = SinglePort;
    let client = DetectFailsButManualFetchSucceeds;
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &client,
        &dependencies,
        &dependencies,
        &dependencies,
    );
    let session = service.create_session();

    assert!(service.detect_mtr_endpoint(&session).await.is_err());

    let endpoint = MtrEndpoint::parse("http://127.0.0.1/").unwrap();
    let snapshot = service
        .fetch_mtr_snapshot(&session, &endpoint, 0)
        .await
        .unwrap();

    assert!(snapshot.routes.is_empty());
}
