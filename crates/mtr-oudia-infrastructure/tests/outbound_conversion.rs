use mtr_oudia_application::{
    ApplicationError, BusinessError, ConversionService, ConversionSessionStore, ManualMappingInput,
    MtrApiClient, MtrEndpoint, MtrSnapshotResponse, SettingsRepository, StationMappingDto,
    async_trait,
};
use mtr_oudia_domain::{
    MtrNetworkSnapshot, MtrRouteSnapshot, MtrStopSnapshot, OperationPolicy, OutboundRuntime,
    ServiceTimeMillis, parse_oudia,
};
use mtr_oudia_infrastructure::{
    FileOudiaRepository, JsonSettingsRepository, SafeOudiaWriter, WindowsListeningPortProvider,
};
use std::path::Path;

const FIXTURE: &str = include_str!("../../../fixtures/oudia/outbound-operation.oud2");
struct Client;
#[async_trait]
impl MtrApiClient for Client {
    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        let route = MtrRouteSnapshot::new(
            "1",
            "中央線",
            vec!["2".into(), "3".into()],
            vec![
                MtrStopSnapshot::new(
                    "2",
                    "始発",
                    "1",
                    ServiceTimeMillis::new(0).unwrap(),
                    Some(ServiceTimeMillis::new(120_000).unwrap()),
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
type Service<'a> = ConversionService<
    'a,
    WindowsListeningPortProvider,
    Client,
    FileOudiaRepository,
    JsonSettingsRepository,
    SafeOudiaWriter,
>;

async fn prepare(
    service: &Service<'_>,
    path: &Path,
    saved: bool,
) -> (
    mtr_oudia_application::SessionId,
    mtr_oudia_application::CandidateId,
) {
    let session = service.create_session();
    service
        .fetch_mtr_snapshot(
            &session,
            &MtrEndpoint::parse("http://127.0.0.1/").unwrap(),
            0,
        )
        .await
        .unwrap();
    if saved {
        service.save_manual_outbound(&session, "1", 107).unwrap();
    }
    service.inspect_oudia(&session, path).unwrap();
    let candidates = service
        .find_route_candidates(&session, "1", None, None)
        .unwrap();
    (session, candidates[0].id.clone())
}
fn mapping() -> Option<ManualMappingInput> {
    Some(ManualMappingInput {
        station_mappings: vec![
            StationMappingDto {
                mtr_station_index: 0,
                oudia_station_slot: 0,
            },
            StationMappingDto {
                mtr_station_index: 1,
                oudia_station_slot: 1,
            },
        ],
    })
}

#[tokio::test]
async fn default_off_does_not_add_or_update_outbound_even_with_saved_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    std::fs::write(&input, FIXTURE).unwrap();
    let settings = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let ports = WindowsListeningPortProvider;
    let repository = FileOudiaRepository;
    let saver = SafeOudiaWriter::new();
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &Client,
        &repository,
        &settings,
        &saver,
    );
    let (session, candidate) = prepare(&service, &input, true).await;
    let preview = service
        .build_preview(&session, Some(&candidate), mapping())
        .unwrap();
    assert!(preview.outbound.is_none());
    service
        .save_conversion(&session, &preview.id, &input, OperationPolicy::Preserve)
        .unwrap();
    assert_eq!(std::fs::read(input).unwrap(), FIXTURE.as_bytes());
}

#[tokio::test]
async fn on_previews_then_safely_updates_only_selected_outbound_time() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    std::fs::write(&input, FIXTURE).unwrap();
    let settings = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let ports = WindowsListeningPortProvider;
    let repository = FileOudiaRepository;
    let saver = SafeOudiaWriter::new();
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &Client,
        &repository,
        &settings,
        &saver,
    );
    let (session, candidate) = prepare(&service, &input, true).await;
    let preview = service
        .build_preview_with_outbound(
            &session,
            Some(&candidate),
            mapping(),
            OperationPolicy::Preserve,
            true,
        )
        .unwrap();
    let outbound = preview.outbound.as_ref().unwrap();
    assert_eq!(outbound.outbound_time, "09:58:13");
    assert_eq!(outbound.first_departure, "10:00:00");
    assert!(outbound.duration_label.contains("107秒"));
    assert_eq!(std::fs::read(&input).unwrap(), FIXTURE.as_bytes());
    assert!(
        service
            .save_conversion(
                &session,
                &preview.id,
                &input,
                OperationPolicy::RemoveTargetTrain
            )
            .is_err()
    );
    service
        .save_conversion(&session, &preview.id, &input, OperationPolicy::Preserve)
        .unwrap();
    let bytes = std::fs::read(input).unwrap();
    assert_eq!(
        bytes,
        FIXTURE.replace("3/2359$/1;2", "3/095813$/1;2").as_bytes()
    );
    parse_oudia(bytes).unwrap();
}

#[tokio::test]
async fn conflicts_refuse_preview_but_explicit_removal_then_generation_is_consistent() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    let fixture = FIXTURE.replace(
        "Operation0B=3/2359$/1;2",
        "Operation0B=5/1000$A\nOperation1A=3/1002$",
    );
    std::fs::write(&input, &fixture).unwrap();
    let settings = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let ports = WindowsListeningPortProvider;
    let repository = FileOudiaRepository;
    let saver = SafeOudiaWriter::new();
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &Client,
        &repository,
        &settings,
        &saver,
    );
    let (session, candidate) = prepare(&service, &input, true).await;
    assert!(
        service
            .build_preview_with_outbound(
                &session,
                Some(&candidate),
                mapping(),
                OperationPolicy::Preserve,
                true
            )
            .is_err()
    );
    assert_eq!(std::fs::read(&input).unwrap(), fixture.as_bytes());
    let preview = service
        .build_preview_with_outbound(
            &session,
            Some(&candidate),
            mapping(),
            OperationPolicy::RemoveTargetTrain,
            true,
        )
        .unwrap();
    service
        .save_conversion(
            &session,
            &preview.id,
            &input,
            OperationPolicy::RemoveTargetTrain,
        )
        .unwrap();
    let text = std::fs::read_to_string(input).unwrap();
    assert!(text.contains("Operation0B=3/095813$/"));
    assert!(!text.contains("Operation1A="));
    assert!(text.contains("UnknownTrainField=対象外"));
}

#[tokio::test]
async fn missing_stale_or_changed_runtime_cannot_be_used_silently() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    std::fs::write(&input, FIXTURE).unwrap();
    let settings = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let ports = WindowsListeningPortProvider;
    let repository = FileOudiaRepository;
    let saver = SafeOudiaWriter::new();
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &Client,
        &repository,
        &settings,
        &saver,
    );
    let (session, candidate) = prepare(&service, &input, false).await;
    assert!(
        service
            .build_preview_with_outbound(
                &session,
                Some(&candidate),
                mapping(),
                OperationPolicy::Preserve,
                true
            )
            .is_err()
    );
    service.save_manual_outbound(&session, "1", 107).unwrap();
    let preview = service
        .build_preview_with_outbound(
            &session,
            Some(&candidate),
            mapping(),
            OperationPolicy::Preserve,
            true,
        )
        .unwrap();
    let mut setting = settings.load().unwrap().outbound_runtimes[0].clone();
    setting.runtime = OutboundRuntime::from_seconds(108).unwrap();
    settings.save_outbound_runtime(&setting).unwrap();
    let error: BusinessError = service
        .save_conversion(&session, &preview.id, &input, OperationPolicy::Preserve)
        .unwrap_err();
    assert_eq!(
        error.kind,
        mtr_oudia_application::BusinessErrorKind::StaleState
    );
    setting.first_station_id = "4".into();
    settings.save_outbound_runtime(&setting).unwrap();
    assert!(
        service
            .build_preview_with_outbound(
                &session,
                Some(&candidate),
                mapping(),
                OperationPolicy::Preserve,
                true
            )
            .is_err()
    );
    assert_eq!(std::fs::read(input).unwrap(), FIXTURE.as_bytes());
}

#[tokio::test]
async fn source_hash_guard_remains_enabled_with_outbound_generation() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    std::fs::write(&input, FIXTURE).unwrap();
    let settings = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let ports = WindowsListeningPortProvider;
    let repository = FileOudiaRepository;
    let saver = SafeOudiaWriter::new();
    let service = ConversionService::new(
        ConversionSessionStore::new(2),
        &ports,
        &Client,
        &repository,
        &settings,
        &saver,
    );
    let (session, candidate) = prepare(&service, &input, true).await;
    let preview = service
        .build_preview_with_outbound(
            &session,
            Some(&candidate),
            mapping(),
            OperationPolicy::Preserve,
            true,
        )
        .unwrap();
    let modified = format!("{FIXTURE}ExternalField=changed\n");
    std::fs::write(&input, &modified).unwrap();
    assert!(
        service
            .save_conversion(&session, &preview.id, &input, OperationPolicy::Preserve)
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(input).unwrap(), modified);
}
