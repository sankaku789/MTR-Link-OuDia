use mtr_oudia_infrastructure::parse_arrivals_response;

#[test]
fn normalizes_official_schema_without_losing_integer_precision() {
    let response =
        parse_arrivals_response(include_str!("../../../fixtures/mtr/arrivals-normal.json"))
            .unwrap();
    assert_eq!(response.current_time_millis, 1_780_000_000_000);
    let arrival = &response.arrivals[0];
    assert_eq!(arrival.route_id.to_hex(), "FFFFFFFFFFFFFFFF");
    assert_eq!(arrival.platform_id.to_hex(), "8000000000000000");
    assert_eq!(arrival.platform_name, "1");
    assert_eq!(arrival.departure, 1_780_014_107_000);
    assert!(!arrival.realtime);
}

#[test]
fn invalid_or_ambiguous_wire_values_fail_closed() {
    let normal = include_str!("../../../fixtures/mtr/arrivals-normal.json");
    for invalid in [
        "not json".to_string(),
        normal.replace("\"status\": 200", "\"status\": 500"),
        normal.replace("\"routeId\": -1", "\"routeId\": 1.5"),
        normal.replace("\"routeId\": -1", "\"routeId\": 18446744073709551615"),
        normal.replace("\"departure\": 1780014107000", "\"departure\": -1"),
        normal.replace("\"realtime\": false", "\"realtime\": \"false\""),
    ] {
        assert!(parse_arrivals_response(&invalid).is_err());
    }
}
