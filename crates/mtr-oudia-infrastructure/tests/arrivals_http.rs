use mtr_oudia_application::{ApplicationError, MtrApiClient, MtrEndpoint};
use mtr_oudia_infrastructure::ReqwestMtrApiClient;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn server(response: Vec<u8>, pause: Duration) -> (MtrEndpoint, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint =
        MtrEndpoint::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = stream.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let len = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + len {
                    break;
                }
            }
        }
        std::thread::sleep(pause);
        let _ = stream.write_all(&response);
        request
    });
    (endpoint, handle)
}

#[tokio::test]
async fn posts_station_hex_request_with_dimension_and_no_global_truncation() {
    let body = include_str!("../../../fixtures/mtr/arrivals-normal.json");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (endpoint, handle) = server(response.into_bytes(), Duration::ZERO);
    let result = ReqwestMtrApiClient::new()
        .unwrap()
        .fetch_arrivals(&endpoint, 2, "1")
        .await
        .unwrap();
    assert_eq!(result.arrivals.len(), 1);
    let request = String::from_utf8(handle.join().unwrap()).unwrap();
    assert!(request.starts_with("POST /mtr/api/map/arrivals?dimension=2 HTTP/1.1"));
    let body: serde_json::Value =
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["stationIdsHex"][0], "0000000000000001");
    assert_eq!(body["maxCountTotal"], 0);
    assert_eq!(body["maxCountPerPlatform"], i32::MAX);
}

#[tokio::test]
async fn refuses_http_error_invalid_json_redirect_and_declared_size_overflow() {
    for response in [
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nbad",
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\n\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\n\r\n",
    ] {
        let (endpoint, handle) = server(response.as_bytes().to_vec(), Duration::ZERO);
        assert!(
            ReqwestMtrApiClient::new()
                .unwrap()
                .fetch_arrivals(&endpoint, 0, "1")
                .await
                .is_err()
        );
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn times_out_without_extending_the_measurement_request() {
    let (endpoint, handle) = server(vec![], Duration::from_millis(1700));
    assert_eq!(
        ReqwestMtrApiClient::new()
            .unwrap()
            .fetch_arrivals(&endpoint, 0, "1")
            .await
            .unwrap_err(),
        ApplicationError::Timeout
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn refuses_actual_body_size_overflow_without_content_length() {
    let mut response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    response.extend(vec![b' '; 8 * 1024 * 1024 + 1]);
    let (endpoint, handle) = server(response, Duration::ZERO);
    assert_eq!(
        ReqwestMtrApiClient::new()
            .unwrap()
            .fetch_arrivals(&endpoint, 0, "1")
            .await
            .unwrap_err(),
        ApplicationError::ResponseTooLarge
    );
    handle.join().unwrap();
}
