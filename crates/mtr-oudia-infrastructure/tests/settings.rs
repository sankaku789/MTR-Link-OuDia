use mtr_oudia_application::{MtrEndpoint, SettingsRepository};
use mtr_oudia_infrastructure::JsonSettingsRepository;

#[test]
fn malformed_settings_falls_back_with_diagnostic_and_successful_endpoint_is_saved() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, b"not json").unwrap();
    let repository = JsonSettingsRepository::new(&path);

    let settings = repository.load().unwrap();
    assert!(settings.last_endpoint.is_none());
    assert_eq!(settings.diagnostics.len(), 1);

    repository
        .save_last_successful_endpoint(&MtrEndpoint::parse("http://127.0.0.1:12345/").unwrap())
        .unwrap();
    assert_eq!(
        repository.load().unwrap().last_endpoint.as_deref(),
        Some("http://127.0.0.1:12345/")
    );
}
