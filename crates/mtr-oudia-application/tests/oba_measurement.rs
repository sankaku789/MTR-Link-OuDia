use mtr_oudia_application::{ObaArrivalDto, ObaArrivalsDto, measure_oba_arrivals};
use mtr_oudia_domain::{MtrId, MtrRouteSnapshot, MtrStopSnapshot, ServiceTimeMillis};

fn route() -> MtrRouteSnapshot {
    MtrRouteSnapshot {
        route_id: "F6D1D37EBAB2A77A".into(),
        display_name: "進急線北行||快速".into(),
        stations_signature: vec![],
        stops: vec![MtrStopSnapshot {
            station_id: "A399F8122E7B42B0".into(),
            station_name: "籠岡".into(),
            platform_name: "1".into(),
            dwell_millis: ServiceTimeMillis::new(40_000).unwrap(),
            run_millis_to_next: None,
        }],
    }
}
fn arrival() -> ObaArrivalDto {
    ObaArrivalDto {
        route_id: MtrId::from_hex("F6D1D37EBAB2A77A").unwrap(),
        platform_id: MtrId::from_hex("437E2294003812E9").unwrap(),
        stop_sequence: 0,
        block_trip_sequence: 0,
        arrival: 1791119023597,
        departure: 1791119063597,
    }
}
fn measure(arrivals: Vec<ObaArrivalDto>) -> Result<i64, mtr_oudia_application::BusinessError> {
    measure_oba_arrivals(
        0,
        &route(),
        arrival().platform_id,
        "22:03:00",
        "+09:00",
        &ObaArrivalsDto {
            current_time_millis: 1791118980000,
            arrivals,
        },
    )
    .map(|s| s.runtime.millis())
}

#[test]
fn excludes_return_trip_and_preserves_milliseconds_without_dwell() {
    let mut returning = arrival();
    returning.block_trip_sequence = 3;
    returning.arrival = 1791120844945;
    returning.departure = 1791120884945;
    assert_eq!(measure(vec![returning, arrival()]).unwrap(), 43_597);
}

#[test]
fn refuses_zero_or_multiple_initial_trips_without_choosing_nearest() {
    assert!(measure(vec![]).is_err());
    let mut second = arrival();
    second.arrival += 60_000;
    second.departure += 60_000;
    assert!(measure(vec![arrival(), second]).is_err());
}

#[test]
fn excludes_other_route_platform_noninitial_stop_and_before_trial_departure() {
    let mut others = vec![arrival(); 5];
    others[0].route_id = MtrId::from_java_long(1);
    others[1].platform_id = MtrId::from_java_long(1);
    others[2].stop_sequence = 1;
    others[3].arrival -= 60_000;
    others[3].departure -= 60_000;
    others[4].arrival += 86_400_000;
    others[4].departure += 86_400_000;
    assert!(measure(others.clone()).is_err());
    others.push(arrival());
    assert_eq!(measure(others).unwrap(), 43_597);
}

#[test]
fn supports_trial_crossing_midnight() {
    let a = ObaArrivalDto {
        arrival: 86_430_000,
        departure: 86_470_000,
        ..arrival()
    };
    let result = measure_oba_arrivals(
        0,
        &route(),
        a.platform_id,
        "23:59:10",
        "+00:00",
        &ObaArrivalsDto {
            current_time_millis: 86_350_000,
            arrivals: vec![a],
        },
    )
    .unwrap();
    assert_eq!(result.runtime.millis(), 80_000);
}

#[test]
fn past_departure_clock_selects_next_day_and_excludes_today() {
    let mut tomorrow = arrival();
    tomorrow.arrival += 86_405_000;
    tomorrow.departure += 86_405_000;
    let result = measure_oba_arrivals(
        0,
        &route(),
        tomorrow.platform_id,
        "22:03:00",
        "+09:00",
        &ObaArrivalsDto {
            current_time_millis: 1791120803982,
            arrivals: vec![arrival(), tomorrow],
        },
    )
    .unwrap();
    assert_eq!(result.runtime.millis(), 48_597);
}

#[test]
fn october_six_evening_finds_october_seven_ten_oclock_trial() {
    let a = ObaArrivalDto {
        arrival: 1791334830169,
        departure: 1791334850169,
        ..arrival()
    };
    let result = measure_oba_arrivals(
        0,
        &route(),
        a.platform_id,
        "10:00:00",
        "+09:00",
        &ObaArrivalsDto {
            current_time_millis: 1791292774083,
            arrivals: vec![a],
        },
    )
    .unwrap();
    assert_eq!(result.runtime.millis(), 30_169);
}
