use mtr_oudia_domain::{OutboundRuntime, ServiceTimeMillis};

fn time(millis: i64) -> ServiceTimeMillis {
    ServiceTimeMillis::new(millis).unwrap()
}

#[test]
fn measures_until_first_departure_including_seconds_and_dwell() {
    let runtime = OutboundRuntime::between_daily_times(time(43_200_000), time(43_307_000)).unwrap();
    assert_eq!(runtime.millis(), 107_000);
    assert_eq!(
        OutboundRuntime::between_daily_times(time(0), time(0))
            .unwrap()
            .millis(),
        0
    );
}

#[test]
fn daily_measurement_wraps_at_midnight() {
    assert_eq!(
        OutboundRuntime::between_daily_times(time(86_350_000), time(40_000))
            .unwrap()
            .millis(),
        90_000
    );
    assert!(OutboundRuntime::between_daily_times(time(86_400_000), time(0)).is_err());
}

#[test]
fn rejects_negative_duration_and_overflowing_manual_seconds() {
    assert!(OutboundRuntime::new(-1).is_err());
    assert!(OutboundRuntime::from_seconds(-1).is_err());
    assert!(OutboundRuntime::from_seconds(i64::MAX).is_err());
    assert_eq!(
        OutboundRuntime::from_seconds(107).unwrap().millis(),
        107_000
    );
}

#[test]
fn calculates_outbound_clock_time_with_midnight_wrap() {
    let runtime = OutboundRuntime::new(107_000).unwrap();
    assert_eq!(
        runtime.outbound_time(time(36_000_000)).unwrap().millis(),
        35_893_000
    );
    assert_eq!(
        runtime.outbound_time(time(40_000)).unwrap().millis(),
        86_333_000
    );
    assert!(runtime.outbound_time(time(86_400_000)).is_err());
}
