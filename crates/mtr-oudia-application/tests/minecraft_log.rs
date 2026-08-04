use mtr_oudia_application::{ApplicationError, endpoint_from_minecraft_log};

#[test]
fn normalizes_default_and_dynamic_minecraft_log_ports() {
    let port_80 = endpoint_from_minecraft_log(
        "[main/INFO] Open the Transport System Map at http://localhost:80",
    )
    .unwrap()
    .unwrap();
    let dynamic = endpoint_from_minecraft_log(
        "[main/INFO] Open the Transport System Map at http://localhost:49182",
    )
    .unwrap()
    .unwrap();
    let implicit = endpoint_from_minecraft_log(
        "[main/INFO] Open the Transport System Map at http://localhost",
    )
    .unwrap()
    .unwrap();

    assert_eq!(port_80.as_url().as_str(), "http://127.0.0.1/");
    assert_eq!(implicit.as_url().as_str(), "http://127.0.0.1/");
    assert_eq!(dynamic.as_url().as_str(), "http://127.0.0.1:49182/");
}

#[test]
fn uses_the_last_valid_matching_message() {
    let endpoint = endpoint_from_minecraft_log(
        "Open the Transport System Map at http://localhost:8888\n\
         unrelated http://example.com:9999/\n\
         Open the Transport System Map at http://localhost:1025",
    )
    .unwrap()
    .unwrap();

    assert_eq!(endpoint.as_url().as_str(), "http://127.0.0.1:1025/");
}

#[test]
fn ignores_unrelated_urls_and_rejects_invalid_matching_urls() {
    assert_eq!(
        endpoint_from_minecraft_log("website: http://localhost:8888").unwrap(),
        None
    );
    assert!(matches!(
        endpoint_from_minecraft_log("Open the Transport System Map at http://example.com:8888"),
        Err(ApplicationError::InvalidEndpoint { .. })
    ));
}
