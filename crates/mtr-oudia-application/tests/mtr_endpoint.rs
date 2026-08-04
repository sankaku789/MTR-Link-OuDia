use mtr_oudia_application::{ApplicationError, MtrEndpoint};

#[test]
fn loopback_endpoints_build_the_fixed_api_url() {
    let ipv4 = MtrEndpoint::parse("http://127.12.34.56:49182/").unwrap();
    let ipv6 = MtrEndpoint::parse("http://[::1]:49182").unwrap();

    assert_eq!(
        ipv4.stations_and_routes_url(7).as_str(),
        "http://127.12.34.56:49182/mtr/api/map/stations-and-routes?dimension=7"
    );
    assert_eq!(
        ipv6.stations_and_routes_url(0).as_str(),
        "http://[::1]:49182/mtr/api/map/stations-and-routes?dimension=0"
    );
}

#[test]
fn accepts_explicit_http_port_80() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1:80/").unwrap();

    assert_eq!(endpoint.as_url().port_or_known_default(), Some(80));
}

#[test]
fn accepts_implicit_http_port_80() {
    let endpoint = MtrEndpoint::parse("http://127.0.0.1/").unwrap();

    assert_eq!(endpoint.as_url().port_or_known_default(), Some(80));
    assert_eq!(
        endpoint.stations_and_routes_url(0).as_str(),
        "http://127.0.0.1/mtr/api/map/stations-and-routes?dimension=0"
    );
}

#[test]
fn explicit_and_implicit_http_port_80_are_the_same_endpoint() {
    let implicit = MtrEndpoint::parse("http://127.0.0.1/").unwrap();
    let explicit = MtrEndpoint::parse("http://127.0.0.1:80/").unwrap();

    assert_eq!(implicit, explicit);
}

#[test]
fn endpoint_rejects_non_loopback_or_unsafe_url_parts() {
    for input in [
        "http://example.test:8080/",
        "http://192.168.1.10:80/",
        "http://10.0.0.5:8888/",
        "http://example.com:80/",
        "http://server.example.net/",
        "http://localhost:8080/",
        "https://127.0.0.1:8080/",
        "http://user@127.0.0.1:8080/",
        "http://127.0.0.1:8080/other",
        "http://127.0.0.1/mtr/api/map/stations-and-routes",
        "http://127.0.0.1:8080/?dimension=1",
        "http://127.0.0.1:8080/#part",
    ] {
        assert!(matches!(
            MtrEndpoint::parse(input),
            Err(ApplicationError::InvalidEndpoint { .. })
        ));
    }
}
