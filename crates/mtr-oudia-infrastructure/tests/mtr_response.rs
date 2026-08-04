use mtr_oudia_application::MtrEndpoint;
use mtr_oudia_infrastructure::{parse_mtr_probe, parse_mtr_response};

#[cfg(not(windows))]
use mtr_oudia_application::{ApplicationError, MtrApiClient};
#[cfg(not(windows))]
use mtr_oudia_infrastructure::ReqwestMtrApiClient;
#[cfg(not(windows))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(not(windows))]
use tokio::net::TcpListener;

#[test]
fn parser_normalizes_station_and_platform_names_with_a_fixed_retrieval_time() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:49182/").unwrap();
    let response = parse_mtr_response(
        include_str!("../../../fixtures/mtr/normal.json"),
        &endpoint,
        0,
        42,
    )
    .unwrap();
    let route = &response.snapshot.routes[0];

    assert_eq!(response.snapshot.dimension, 0);
    assert_eq!(response.snapshot.api_current_time_millis, 1_715_000_000_000);
    assert_eq!(response.snapshot.retrieved_at_unix_millis, 42);
    assert_eq!(response.available_dimensions.len(), 1);
    assert_eq!(route.stops[0].station_name, "Central Station");
    assert_eq!(route.stops[0].platform_name, "Platform 1");
    assert_eq!(route.stops[0].run_millis_to_next.unwrap().millis(), 12_345);
    assert_eq!(route.stops[1].run_millis_to_next, None);
}

#[test]
fn parser_accepts_the_official_unwrapped_stations_and_routes_schema() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1/").unwrap();
    let body = include_str!("../../../fixtures/mtr/official-stations-and-routes.json");

    assert_eq!(parse_mtr_probe(body), Ok(()));

    let response = parse_mtr_response(body, &endpoint, 0, 42).unwrap();
    assert_eq!(response.snapshot.api_current_time_millis, 42);
    assert_eq!(response.available_dimensions, ["minecraft:overworld"]);
    assert_eq!(
        response.snapshot.routes[0].stops[0].station_name,
        "Central Station"
    );
    assert_eq!(
        response.snapshot.routes[0].stops[0].platform_name,
        "Platform 1"
    );
}

#[test]
fn parser_accepts_data_envelope_without_status_or_current_time() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:8888/").unwrap();
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/mtr/official-stations-and-routes.json"
    ))
    .unwrap();
    let body = serde_json::json!({ "data": data }).to_string();

    assert_eq!(parse_mtr_probe(&body), Ok(()));

    let response = parse_mtr_response(&body, &endpoint, 0, 42).unwrap();
    assert_eq!(response.snapshot.api_current_time_millis, 42);
    assert_eq!(response.snapshot.routes.len(), 1);
}

#[test]
fn parser_ignores_duration_entries_after_the_last_station_segment() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:8888/").unwrap();
    let mut data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/mtr/official-stations-and-routes.json"
    ))
    .unwrap();
    data["routes"][0]["durations"] = serde_json::json!([12345, 99999]);
    let body = serde_json::json!({ "data": data }).to_string();

    let response = parse_mtr_response(&body, &endpoint, 0, 42).unwrap();
    let stops = &response.snapshot.routes[0].stops;
    assert_eq!(stops[0].run_millis_to_next.unwrap().millis(), 12345);
    assert_eq!(stops[1].run_millis_to_next, None);
}

#[test]
fn parser_skips_unconvertible_routes_but_keeps_valid_routes() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:8888/").unwrap();
    let mut data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/mtr/official-stations-and-routes.json"
    ))
    .unwrap();
    let valid_route = data["routes"][0].clone();
    let mut missing_durations = valid_route.clone();
    missing_durations["id"] = serde_json::json!("route-without-durations");
    missing_durations["durations"] = serde_json::json!([]);
    let mut no_stations = valid_route.clone();
    no_stations["id"] = serde_json::json!("route-without-stations");
    no_stations["stations"] = serde_json::json!([]);
    no_stations["durations"] = serde_json::json!([]);
    data["routes"] = serde_json::json!([missing_durations, no_stations, valid_route]);
    let body = serde_json::json!({
        "code": 200,
        "currentTime": 1785808008627_i64,
        "data": data
    })
    .to_string();

    let response = parse_mtr_response(&body, &endpoint, 0, 42).unwrap();
    assert_eq!(response.snapshot.routes.len(), 1);
    assert_eq!(response.snapshot.routes[0].route_id, "route-1");
}

#[test]
fn endpoint_probe_accepts_mtr_envelope_that_strict_snapshot_parsing_rejects() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:49182/").unwrap();
    let body = r#"{
        "status": 200,
        "currentTime": 1,
        "data": {
            "stations": [],
            "routes": [{ "id": "broken-route" }],
            "dimensions": []
        }
    }"#;

    assert_eq!(parse_mtr_probe(body), Ok(()));
    assert!(parse_mtr_response(body, &endpoint, 0, 42).is_err());
}

#[cfg(not(windows))]
#[tokio::test]
async fn endpoint_probe_accepts_mtr_shaped_http_400_response() {
    let body = r#"{"status":400,"data":{"stations":[],"routes":[]}}"#;
    let endpoint = test_server(format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ))
    .await;
    let client = ReqwestMtrApiClient::new().unwrap();

    assert_eq!(client.probe_endpoint(&endpoint, 0).await, Ok(()));
}

#[cfg(not(windows))]
#[tokio::test]
async fn http_client_does_not_follow_redirects() {
    let endpoint = test_server(
        "HTTP/1.1 302 Found\r\nLocation: http://example.test/\r\nContent-Length: 0\r\n\r\n"
            .to_owned(),
    )
    .await;
    let client = ReqwestMtrApiClient::new().unwrap();

    assert!(matches!(
        client.fetch_snapshot(&endpoint, 0).await,
        Err(ApplicationError::InvalidResponse { .. })
    ));
}

#[cfg(not(windows))]
#[tokio::test]
async fn http_client_rejects_a_declared_oversized_response() {
    let endpoint =
        test_server("HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\n\r\n".to_owned()).await;
    let client = ReqwestMtrApiClient::new().unwrap();

    assert_eq!(
        client.fetch_snapshot(&endpoint, 0).await,
        Err(ApplicationError::ResponseTooLarge)
    );
}

#[cfg(not(windows))]
async fn test_server(response: String) -> MtrEndpoint {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut method_start = [0; 1];
        stream.read_exact(&mut method_start).await.unwrap();
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    MtrEndpoint::parse(&format!("http://127.0.0.1:{port}/")).unwrap()
}

#[test]
fn parser_rejects_malformed_responses() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:49182/").unwrap();
    for fixture in [
        include_str!("../../../fixtures/mtr/status-not-200.json"),
        include_str!("../../../fixtures/mtr/data-null.json"),
        include_str!("../../../fixtures/mtr/arrays-invalid.json"),
        include_str!("../../../fixtures/mtr/negative-times.json"),
        include_str!("../../../fixtures/mtr/negative-duration.json"),
        include_str!("../../../fixtures/mtr/unknown-station.json"),
        include_str!("../../../fixtures/mtr/missing-required.json"),
        include_str!("../../../fixtures/mtr/missing-route-field.json"),
        include_str!("../../../fixtures/mtr/duplicate-station.json"),
    ] {
        assert!(parse_mtr_response(fixture, &endpoint, 0, 42).is_err());
    }
}
