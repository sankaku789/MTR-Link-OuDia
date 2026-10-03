use mtr_oudia_application::{
    MtrEndpoint, OutboundRuntimeSetting, OutboundRuntimeSource, SettingsRepository,
};
use mtr_oudia_domain::OutboundRuntime;
use mtr_oudia_infrastructure::JsonSettingsRepository;

fn record() -> OutboundRuntimeSetting {
    OutboundRuntimeSetting {
        runtime_basis: mtr_oudia_application::OutboundRuntimeBasis::FirstArrival,
        dimension: 0,
        route_id: "0000000000000001".into(),
        first_station_id: "0000000000000002".into(),
        first_platform_name: "1".into(),
        runtime: OutboundRuntime::from_seconds(107).unwrap(),
        measured_at: 1_780_000_000_000,
        source: OutboundRuntimeSource::Manual,
    }
}

#[test]
fn old_settings_without_outbound_values_remain_readable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, r#"{"last_endpoint":"http://127.0.0.1/","station_aliases":{},"route_mappings":[],"last_train_type":null,"diagnostics":[]}"#).unwrap();
    let settings = JsonSettingsRepository::new(&path).load().unwrap();
    assert!(settings.outbound_runtimes.is_empty());
    assert!(settings.diagnostics.is_empty());
    assert_eq!(settings.last_endpoint.as_deref(), Some("http://127.0.0.1/"));
}

#[test]
fn manual_value_round_trips_and_survives_endpoint_save() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let repository = JsonSettingsRepository::new(&path);
    repository.save_outbound_runtime(&record()).unwrap();
    repository
        .save_last_successful_endpoint(&MtrEndpoint::parse("http://127.0.0.1/").unwrap())
        .unwrap();
    let settings = JsonSettingsRepository::new(&path).load().unwrap();
    assert_eq!(settings.outbound_runtimes, vec![record()]);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(json["outbound_runtimes"][0]["outbound_millis"], 107_000);
}

#[test]
fn remeasurement_updates_only_the_matching_dimension_and_route() {
    let directory = tempfile::tempdir().unwrap();
    let repository = JsonSettingsRepository::new(directory.path().join("settings.json"));
    let mut other = record();
    other.dimension = 1;
    repository.save_outbound_runtime(&record()).unwrap();
    repository.save_outbound_runtime(&other).unwrap();
    let mut updated = record();
    updated.first_station_id = "0000000000000003".into();
    updated.runtime = OutboundRuntime::from_seconds(110).unwrap();
    repository.save_outbound_runtime(&updated).unwrap();
    assert_eq!(
        repository.load().unwrap().outbound_runtimes,
        vec![updated, other]
    );
}

#[test]
fn negative_saved_duration_cannot_be_deserialized() {
    let mut json = serde_json::to_value(record()).unwrap();
    json["outbound_millis"] = serde_json::json!(-1);
    assert!(serde_json::from_value::<OutboundRuntimeSetting>(json).is_err());
}
