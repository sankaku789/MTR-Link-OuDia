use mtr_oudia_application::{
    ApplicationError, ConversionService, ConversionSessionStore, MtrApiClient, MtrEndpoint,
    MtrSnapshotResponse, async_trait,
};
use mtr_oudia_domain::{MtrNetworkSnapshot, MtrRouteSnapshot, MtrStopSnapshot, ServiceTimeMillis};
use mtr_oudia_infrastructure::{
    FileOudiaRepository, JsonSettingsRepository, SafeOudiaWriter, WindowsListeningPortProvider,
};
use std::sync::Mutex;

struct Client(Mutex<(String, String)>);
#[async_trait]
impl MtrApiClient for Client {
    async fn fetch_arrivals(
        &self,
        _: &MtrEndpoint,
        _: u32,
        station_id: &str,
    ) -> Result<mtr_oudia_application::ArrivalsDto, ApplicationError> {
        assert_eq!(station_id, "0000000000000002");
        Ok(mtr_oudia_application::ArrivalsDto {
            current_time_millis: 1000,
            arrivals: vec![mtr_oudia_application::ArrivalDto {
                route_id: mtr_oudia_domain::MtrId::from_java_long(1),
                platform_id: mtr_oudia_domain::MtrId::from_java_long(9),
                platform_name: "1".into(),
                arrival: 77_000,
                departure: 107_000,
                deviation: 0,
                realtime: false,
                departure_index: 0,
            }],
        })
    }
    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        let (station, platform) = self.0.lock().unwrap().clone();
        let route = MtrRouteSnapshot::new(
            "0000000000000001",
            "中央線",
            vec![station.clone(), "3".into()],
            vec![
                MtrStopSnapshot::new(
                    station,
                    "始発駅",
                    platform,
                    ServiceTimeMillis::new(0).unwrap(),
                    Some(ServiceTimeMillis::new(1000).unwrap()),
                )
                .unwrap(),
                MtrStopSnapshot::new("3", "終点", "2", ServiceTimeMillis::new(0).unwrap(), None)
                    .unwrap(),
            ],
        )
        .unwrap();
        Ok(MtrSnapshotResponse {
            snapshot: MtrNetworkSnapshot::new(
                endpoint.as_url().to_string(),
                dimension,
                0,
                0,
                vec![route],
            )
            .unwrap(),
            available_dimensions: vec![],
        })
    }
}

#[tokio::test]
async fn manual_save_and_reload_work_without_any_oudia_file() {
    let directory = tempfile::tempdir().unwrap();
    let settings = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let client = Client(Mutex::new(("2".into(), "1".into())));
    let ports = WindowsListeningPortProvider;
    let repository = FileOudiaRepository;
    let saver = SafeOudiaWriter::new();
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &client,
        &repository,
        &settings,
        &saver,
    );
    let session = service.create_session();
    let endpoint = MtrEndpoint::parse("http://127.0.0.1/").unwrap();
    service
        .fetch_mtr_snapshot(&session, &endpoint, 0)
        .await
        .unwrap();
    assert!(
        service
            .outbound_status(&session, "0000000000000001")
            .unwrap()
            .setting
            .is_none()
    );
    assert!(
        service
            .save_manual_outbound(&session, "0000000000000001", -1)
            .is_err()
    );
    let status = service
        .save_manual_outbound(&session, "0000000000000001", 107)
        .unwrap();
    assert!(status.valid);
    assert!(status.duration_label.contains("1分47秒"));
    let new_service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &client,
        &repository,
        &settings,
        &saver,
    );
    let new_session = new_service.create_session();
    new_service
        .fetch_mtr_snapshot(&new_session, &endpoint, 0)
        .await
        .unwrap();
    let status = new_service
        .outbound_status(&new_session, "0000000000000001")
        .unwrap();
    assert!(status.valid);
    assert_eq!(status.setting.unwrap().runtime.millis(), 107_000);
    let measured = new_service
        .measure_and_save_outbound(&new_session, "0000000000000001", "00:00:00", "+00:00")
        .await
        .unwrap();
    assert!(measured.valid);
    assert_eq!(
        measured.setting.as_ref().unwrap().source,
        mtr_oudia_application::OutboundRuntimeSource::Measured
    );
    assert_eq!(measured.setting.as_ref().unwrap().runtime.millis(), 107_000);
    assert_eq!(measured.setting.as_ref().unwrap().measured_at, 1000);
}

#[tokio::test]
async fn station_or_platform_changes_make_saved_value_stale_without_deleting_it() {
    let directory = tempfile::tempdir().unwrap();
    let settings = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let client = Client(Mutex::new(("2".into(), "1".into())));
    let ports = WindowsListeningPortProvider;
    let repository = FileOudiaRepository;
    let saver = SafeOudiaWriter::new();
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &client,
        &repository,
        &settings,
        &saver,
    );
    let session = service.create_session();
    let endpoint = MtrEndpoint::parse("http://127.0.0.1/").unwrap();
    service
        .fetch_mtr_snapshot(&session, &endpoint, 0)
        .await
        .unwrap();
    service
        .save_manual_outbound(&session, "0000000000000001", 107)
        .unwrap();
    for changed in [("4", "1"), ("2", "2")] {
        *client.0.lock().unwrap() = (changed.0.into(), changed.1.into());
        service
            .fetch_mtr_snapshot(&session, &endpoint, 0)
            .await
            .unwrap();
        let status = service
            .outbound_status(&session, "0000000000000001")
            .unwrap();
        assert!(!status.valid);
        assert!(status.setting.is_some());
        assert!(status.message.unwrap().contains("再測定"));
    }
    *client.0.lock().unwrap() = ("2".into(), "1".into());
    service
        .fetch_mtr_snapshot(&session, &endpoint, 0)
        .await
        .unwrap();
    assert!(
        service
            .outbound_status(&session, "0000000000000001")
            .unwrap()
            .valid
    );
    service
        .fetch_mtr_snapshot(&session, &endpoint, 1)
        .await
        .unwrap();
    assert!(
        service
            .outbound_status(&session, "0000000000000001")
            .unwrap()
            .setting
            .is_none()
    );
}
