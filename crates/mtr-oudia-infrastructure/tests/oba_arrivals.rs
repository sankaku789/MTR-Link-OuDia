use mtr_oudia_infrastructure::parse_oba_arrivals_response;

const MAP: &str = r#"{"code":200,"data":{"routes":[
{"id":"F6D1D37EBAB2A77A","name":"進急線北行||快速","number":"快速|RAPID","color":15819777},
{"id":"C2BE3EB9602EC029","name":"進急線南行||快速","number":"快速|RAPID","color":15819777}
]}}"#;
const OBA: &str = r#"{"code":200,"currentTime":1791120803982,"data":{"entry":{
"stopId":"437E2294003812E9","arrivalsAndDepartures":[
{"routeId":"F16401","routeLongName":"進急線北行","routeShortName":"快速 RAPID",
"stopId":"437E2294003812E9","stopSequence":0,"blockTripSequence":0,
"scheduledArrivalTime":1791119023597,"scheduledDepartureTime":1791119063597,
"predictedArrivalTime":1791119999999,"predictedDepartureTime":1791119999999}
]}}}"#;
fn parse(
    map: &str,
    oba: &str,
) -> Result<mtr_oudia_application::ObaArrivalsDto, mtr_oudia_application::ApplicationError> {
    parse_oba_arrivals_response(map, oba, "F6D1D37EBAB2A77A", "437E2294003812E9")
}

#[test]
fn matches_color_and_formatted_names_uses_only_scheduled_integer_milliseconds() {
    let result = parse(MAP, OBA).unwrap();
    assert_eq!(result.arrivals.len(), 1);
    assert_eq!(result.arrivals[0].route_id.to_hex(), "F6D1D37EBAB2A77A");
    assert_eq!(result.arrivals[0].arrival, 1791119023597);
    assert_eq!(
        result.arrivals[0].departure - result.arrivals[0].arrival,
        40_000
    );
    assert!(
        parse(MAP, &OBA.replace("進急線北行", "進急線南行"))
            .unwrap()
            .arrivals
            .is_empty()
    );
}

#[test]
fn rejects_same_color_and_names_collision_instead_of_guessing_route() {
    assert!(parse(&MAP.replace("進急線南行", "進急線北行"), OBA).is_err());
}

#[test]
fn rejects_bad_status_schema_platform_and_unsafe_timestamps() {
    for bad in [
        OBA.replace("\"code\":200", "\"code\":500"),
        OBA.replace("1791119023597", "1791119023597.5"),
        OBA.replace("1791119023597", "9007199254740992"),
        OBA.replace("1791119063597", "0"),
        OBA.replace("437E2294003812E9", "8000000000000000"),
        OBA.replace("\"blockTripSequence\":0", "\"blockTripSequence\":-1"),
        "{}".into(),
    ] {
        assert!(parse(MAP, &bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn http_reads_map_and_oba_with_exact_platform_hex_and_shared_endpoint() {
    use mtr_oudia_application::{MtrApiClient, MtrEndpoint};
    use mtr_oudia_infrastructure::ReqwestMtrApiClient;
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint =
        MtrEndpoint::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for body in [MAP, OBA] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buffer[..n]);
            }
            requests.push(String::from_utf8(request).unwrap());
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        requests
    });
    let response = ReqwestMtrApiClient::new()
        .unwrap()
        .fetch_oba_arrivals(&endpoint, 2, "F6D1D37EBAB2A77A", "437E2294003812E9")
        .await
        .unwrap();
    assert_eq!(response.arrivals[0].arrival, 1791119023597);
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /mtr/api/map/stations-and-routes?dimension=2 HTTP/1.1"));
    assert!(requests[1].starts_with("GET /oba/api/where/arrivals-and-departures-for-stop/437E2294003812E9?dimension=2&minutesBefore=1440&minutesAfter=2880 HTTP/1.1"));
}
