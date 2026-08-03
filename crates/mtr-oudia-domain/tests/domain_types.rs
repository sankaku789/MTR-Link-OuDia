use mtr_oudia_domain::{
    DomainError, MtrNetworkSnapshot, MtrRouteSnapshot, MtrStopSnapshot, OudiaDirection,
    OudiaStationSlotId, ServiceTimeMillis, SourceRange,
};

#[test]
fn service_time_rejects_negative_values_and_preserves_boundaries() {
    assert!(matches!(
        ServiceTimeMillis::new(-1),
        Err(DomainError::NegativeServiceTime { millis: -1 })
    ));
    assert_eq!(ServiceTimeMillis::new(0).unwrap().millis(), 0);
    assert_eq!(ServiceTimeMillis::new(i64::MAX).unwrap().millis(), i64::MAX);
}

#[test]
fn service_time_checked_add_reports_overflow() {
    assert_eq!(
        ServiceTimeMillis::new(10)
            .unwrap()
            .checked_add(ServiceTimeMillis::new(20).unwrap())
            .unwrap()
            .millis(),
        30
    );
    assert!(matches!(
        ServiceTimeMillis::new(i64::MAX)
            .unwrap()
            .checked_add(ServiceTimeMillis::new(1).unwrap()),
        Err(DomainError::TimeOverflow)
    ));
}

#[test]
fn source_range_uses_half_open_bounds() {
    let empty = SourceRange::new(3, 3).unwrap();
    let range = SourceRange::new(2, 5).unwrap();

    assert!(empty.is_empty());
    assert_eq!(range.len(), 3);
    assert!(range.contains(2));
    assert!(range.contains(4));
    assert!(!range.contains(5));
    assert!(range.validate_bounds(5).is_ok());
    assert!(matches!(
        range.validate_bounds(4),
        Err(DomainError::RangeOutOfBounds { .. })
    ));
    assert!(matches!(
        SourceRange::new(5, 2),
        Err(DomainError::InvalidRange { start: 5, end: 2 })
    ));
}

#[test]
fn source_range_distinguishes_overlap_from_touching() {
    let left = SourceRange::new(2, 5).unwrap();
    let touching = SourceRange::new(5, 7).unwrap();
    let overlapping = SourceRange::new(4, 7).unwrap();

    assert!(!left.overlaps(touching));
    assert!(left.touches(touching));
    assert!(left.overlaps(overlapping));
    assert!(!left.touches(overlapping));
}

#[test]
fn station_slot_id_validates_indices_against_their_bounds() {
    let slot = OudiaStationSlotId::new(1, OudiaDirection::Kudari, 2, 3, 2, 3, 4).unwrap();

    assert_eq!(slot.diagram_index(), 1);
    assert_eq!(slot.direction(), OudiaDirection::Kudari);
    assert_eq!(slot.train_index(), 2);
    assert_eq!(slot.station_slot_index(), 3);
    assert!(matches!(
        OudiaStationSlotId::new(2, OudiaDirection::Nobori, 0, 0, 2, 1, 1),
        Err(DomainError::IndexOutOfRange {
            index_name: "diagram_index",
            ..
        })
    ));
}

#[test]
fn mtr_models_reject_empty_identifiers_and_inconsistent_run_times() {
    let stop = MtrStopSnapshot::new(
        "station-a",
        "Station A",
        "Platform 1",
        ServiceTimeMillis::new(0).unwrap(),
        Some(ServiceTimeMillis::new(1_000).unwrap()),
    )
    .unwrap();
    let last_stop = MtrStopSnapshot::new(
        "station-b",
        "Station B",
        "Platform 2",
        ServiceTimeMillis::new(0).unwrap(),
        None,
    )
    .unwrap();

    let route = MtrRouteSnapshot::new(
        "route-a",
        "Route A",
        vec!["station-a".into(), "station-b".into()],
        vec![stop, last_stop],
    )
    .unwrap();
    let snapshot = MtrNetworkSnapshot::new("http://127.0.0.1:1234", 0, 0, 0, vec![route]);

    assert!(snapshot.is_ok());
    assert!(matches!(
        MtrStopSnapshot::new(
            "",
            "Station",
            "Platform",
            ServiceTimeMillis::new(0).unwrap(),
            None,
        ),
        Err(DomainError::InvalidValue {
            value_name: "station_id"
        })
    ));
    assert!(matches!(
        MtrRouteSnapshot::new("route", "Route", vec!["station".into()], vec![]),
        Err(DomainError::InvalidRoute { .. })
    ));
    assert!(matches!(
        MtrRouteSnapshot::new(
            "route",
            "Route",
            vec!["station-a".into(), "station-b".into()],
            vec![
                MtrStopSnapshot::new(
                    "station-a",
                    "Station A",
                    "Platform 1",
                    ServiceTimeMillis::new(0).unwrap(),
                    None,
                )
                .unwrap(),
                MtrStopSnapshot::new(
                    "station-b",
                    "Station B",
                    "Platform 2",
                    ServiceTimeMillis::new(0).unwrap(),
                    None,
                )
                .unwrap(),
            ],
        ),
        Err(DomainError::InvalidRoute { .. })
    ));
}
