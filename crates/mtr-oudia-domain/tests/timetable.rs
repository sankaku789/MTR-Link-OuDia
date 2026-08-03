use mtr_oudia_domain::{
    DomainError, MtrRouteSnapshot, MtrStopSnapshot, ServiceTimeMillis, format_preview_time,
    generate_timetable, generate_timetable_from_durations,
};

fn time(millis: i64) -> ServiceTimeMillis {
    ServiceTimeMillis::new(millis).unwrap()
}

#[test]
fn generates_three_stop_timetable_in_milliseconds() {
    let result = generate_timetable_from_durations(
        &[time(9_999), time(2_000), time(8_888)],
        &[time(1_240), time(2_490)],
    )
    .unwrap();
    assert_eq!(result.stops[0].departure.unwrap().millis(), 36_000_000);
    assert_eq!(result.stops[1].arrival.unwrap().millis(), 36_001_240);
    assert_eq!(result.stops[1].departure.unwrap().millis(), 36_003_240);
    assert_eq!(result.stops[2].arrival.unwrap().millis(), 36_005_730);
    assert!(result.stops[0].arrival.is_none());
    assert!(result.stops[2].departure.is_none());
}

#[test]
fn ignores_terminal_dwells_and_only_rounds_after_accumulation() {
    let result = generate_timetable_from_durations(
        &[time(99_999), time(500), time(99_999)],
        &[time(499), time(499)],
    )
    .unwrap();
    assert_eq!(result.stops[2].arrival.unwrap().millis(), 36_001_498);
    assert_eq!(result.stops[1].rounded_arrival_seconds, Some(36_000));
    assert_eq!(result.stops[1].rounded_departure_seconds, Some(36_001));
    assert_eq!(result.stops[2].rounded_arrival_seconds, Some(36_001));
}

#[test]
fn rounds_half_up_and_formats_24_hours_or_later() {
    assert_eq!(time(499).rounded_seconds().unwrap(), 0);
    assert_eq!(time(500).rounded_seconds().unwrap(), 1);
    assert_eq!(format_preview_time(86_400), "24:00:00");
    let result =
        generate_timetable_from_durations(&[time(0), time(0)], &[time(50_400_000)]).unwrap();
    assert!(result.crosses_midnight);
    assert_eq!(
        result.stops[1].rounded_arrival_display.as_deref(),
        Some("24:00:00")
    );
}

#[test]
fn rejects_invalid_lengths_and_checked_overflow() {
    assert!(matches!(
        generate_timetable_from_durations(&[], &[]),
        Err(DomainError::InvalidTimetable { .. })
    ));
    assert!(matches!(
        generate_timetable_from_durations(&[time(0)], &[]),
        Err(DomainError::InvalidTimetable { .. })
    ));
    assert!(matches!(
        generate_timetable_from_durations(&[time(0), time(0)], &[]),
        Err(DomainError::InvalidTimetable { .. })
    ));
    assert!(matches!(
        generate_timetable_from_durations(&[time(0), time(0)], &[time(i64::MAX)]),
        Err(DomainError::TimeOverflow)
    ));
    assert!(matches!(
        time(i64::MAX).rounded_seconds(),
        Err(DomainError::TimeOverflow)
    ));
}

#[test]
fn converts_an_mtr_route_without_changing_the_fixed_start() {
    let route = MtrRouteSnapshot::new(
        "r",
        "r",
        vec!["a".into(), "b".into()],
        vec![
            MtrStopSnapshot::new("a", "a", "", time(777), Some(time(1_000))).unwrap(),
            MtrStopSnapshot::new("b", "b", "", time(888), None).unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(
        generate_timetable(&route).unwrap().stops[0]
            .departure
            .unwrap()
            .millis(),
        36_000_000
    );
}
