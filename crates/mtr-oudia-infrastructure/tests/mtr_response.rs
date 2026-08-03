use mtr_oudia_application::{ApplicationError, MtrApiClient, MtrEndpoint};
use mtr_oudia_infrastructure::{ReqwestMtrApiClient, parse_mtr_response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
        include_str!("../../../fixtures/mtr/durations-invalid.json"),
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
