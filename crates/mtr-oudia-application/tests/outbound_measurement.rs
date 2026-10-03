use mtr_oudia_application::{
    ApplicationError, MtrApiClient, MtrEndpoint, MtrSnapshotResponse, async_trait,
    measure_outbound_runtime,
};
use mtr_oudia_application::{ArrivalDto, ArrivalsDto, measure_arrivals};
use mtr_oudia_domain::{MtrId, MtrRouteSnapshot, MtrStopSnapshot, ServiceTimeMillis};

struct Client;
#[async_trait]
impl MtrApiClient for Client {
    async fn fetch_snapshot(
        &self,
        _: &MtrEndpoint,
        _: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        panic!("measurement must not fetch or alter OuDia/snapshot")
    }
    async fn fetch_arrivals(
        &self,
        _: &MtrEndpoint,
        dimension: u32,
        station_id: &str,
    ) -> Result<ArrivalsDto, ApplicationError> {
        assert_eq!(dimension, 2);
        assert_eq!(station_id, "0000000000000001");
        Ok(response(vec![arrival()]))
    }
}

#[tokio::test]
async fn measurement_use_case_needs_no_oudia_file_or_conversion_candidate() {
    let result = measure_outbound_runtime(
        &Client,
        &MtrEndpoint::parse("http://127.0.0.1/").unwrap(),
        2,
        &route(),
        "12:00:00",
        "+09:00",
    )
    .await
    .unwrap();
    assert_eq!(result.runtime.millis(), 77_000);
    assert_eq!(result.dimension, 2);
}

fn route() -> MtrRouteSnapshot {
    MtrRouteSnapshot {
        route_id: "FFFFFFFFFFFFFFFF".into(),
        display_name: "中央線".into(),
        stations_signature: vec![],
        stops: vec![MtrStopSnapshot {
            station_id: "0000000000000001".into(),
            station_name: "始発".into(),
            platform_name: "1".into(),
            dwell_millis: ServiceTimeMillis::new(30_000).unwrap(),
            run_millis_to_next: None,
        }],
    }
}
fn arrival() -> ArrivalDto {
    ArrivalDto {
        route_id: MtrId::from_java_long(-1),
        platform_id: MtrId::from_java_long(2),
        platform_name: "1".into(),
        arrival: 10_877_000,
        departure: 10_907_000,
        realtime: false,
        deviation: 0,
        departure_index: 0,
    }
}
fn response(arrivals: Vec<ArrivalDto>) -> ArrivalsDto {
    ArrivalsDto {
        current_time_millis: 10_800_000,
        arrivals,
    }
}

#[test]
fn measures_first_arrival_in_minecraft_timezone_excluding_station_dwell() {
    let measured = measure_arrivals(
        0,
        &route(),
        "12:00:00",
        "+09:00",
        &response(vec![arrival()]),
    )
    .unwrap();
    assert_eq!(measured.runtime.millis(), 77_000);
    assert_eq!(measured.measured_at, 10_800_000);
}

#[test]
fn wraps_midnight_and_supports_non_jst_timezone() {
    let mut a = arrival();
    a.arrival = 30_000;
    a.departure = 40_000;
    assert_eq!(
        measure_arrivals(
            0,
            &route(),
            "23:59:10",
            "+00:00",
            &response(vec![a.clone()])
        )
        .unwrap()
        .runtime
        .millis(),
        80_000
    );
    assert_eq!(
        measure_arrivals(0, &route(), "18:59:10", "-05:00", &response(vec![a]))
            .unwrap()
            .runtime
            .millis(),
        80_000
    );
}

#[test]
fn rejects_zero_and_multiple_candidates_instead_of_picking_nearest() {
    assert!(measure_arrivals(0, &route(), "12:00:00", "+09:00", &response(vec![])).is_err());
    let mut second = arrival();
    second.departure += 60_000;
    assert!(
        measure_arrivals(
            0,
            &route(),
            "12:00:00",
            "+09:00",
            &response(vec![arrival(), second])
        )
        .is_err()
    );
}

#[test]
fn filters_selected_route_and_first_platform() {
    let mut other_route = arrival();
    other_route.route_id = MtrId::from_java_long(3);
    let mut other_platform = arrival();
    other_platform.platform_name = "2".into();
    assert_eq!(
        measure_arrivals(
            0,
            &route(),
            "12:00:00",
            "+09:00",
            &response(vec![other_route, other_platform, arrival()])
        )
        .unwrap()
        .runtime
        .millis(),
        77_000
    );
}

#[test]
fn rejects_invalid_clock_offset_and_missing_first_platform() {
    for clock in ["24:00:00", "12:60:00", "12:00:60", "-1:00:00", "12:00"] {
        assert!(
            measure_arrivals(0, &route(), clock, "+00:00", &response(vec![arrival()])).is_err()
        );
    }
    for offset in ["JST", "+24:00", "+09:60", "09:00"] {
        assert!(
            measure_arrivals(0, &route(), "12:00:00", offset, &response(vec![arrival()])).is_err()
        );
    }
    let mut r = route();
    r.stops[0].platform_name.clear();
    assert!(measure_arrivals(0, &r, "12:00:00", "+09:00", &response(vec![arrival()])).is_err());
}
