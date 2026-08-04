use mtr_oudia_domain::{
    GeneratedStop, GeneratedTimetable, OperationPolicy, build_eki_jikoku_patch,
    build_eki_jikoku_patch_with_groups, build_oudia_route_templates, parse_oudia,
};

const FIXTURE: &str = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;/10:00:00$2,,1;10:02:00/10:03:00$3,1;10:05:00/$4\nOperation=target\n.\n.\n.\n";

fn timetable() -> GeneratedTimetable {
    GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            stop(0, None, Some("10:10:00")),
            stop(1, Some("10:12:00"), Some("10:13:00")),
            stop(2, Some("10:15:00"), None),
        ],
    }
}

fn stop(index: usize, arrival: Option<&str>, departure: Option<&str>) -> GeneratedStop {
    GeneratedStop {
        station_index: index,
        arrival: None,
        departure: None,
        rounded_arrival_seconds: arrival.map(|_| 0),
        rounded_departure_seconds: departure.map(|_| 0),
        rounded_arrival_display: arrival.map(ToString::to_string),
        rounded_departure_display: departure.map(ToString::to_string),
    }
}

#[test]
fn updates_only_existing_time_substrings_and_preserves_cell_parts() {
    let source = parse_oudia(FIXTURE.as_bytes().to_vec()).unwrap();
    let templates = build_oudia_route_templates(&source.document);
    let template = match templates {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => panic!("reference template is required"),
    };
    let patch = build_eki_jikoku_patch(&source, &template, &timetable(), OperationPolicy::Preserve)
        .unwrap();
    let saved = patch.apply(&source.bytes).unwrap();

    assert_eq!(
        std::str::from_utf8(&saved).unwrap(),
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;/10:10:00$2,,1;10:12:00/10:13:00$3,1;10:15:00/$4\nOperation=target\n.\n.\n.\n"
    );
}

#[test]
fn updates_compact_times_and_departure_only_cells_without_changing_their_shape() {
    let fixture = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1000$4,1;100236/100253$2,1;100753/$1\n.\n.\n.\n";
    let source = parse_oudia(fixture.as_bytes().to_vec()).unwrap();
    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    let timetable = GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            stop(0, None, Some("10:10:00")),
            stop(1, Some("10:12:00"), Some("10:13:05")),
            stop(2, Some("10:15:00"), None),
        ],
    };

    let patch =
        build_eki_jikoku_patch(&source, &template, &timetable, OperationPolicy::Preserve).unwrap();
    let saved = patch.apply(&source.bytes).unwrap();

    assert_eq!(
        std::str::from_utf8(&saved).unwrap(),
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1010$4,1;101200/101305$2,1;101500/$1\n.\n.\n.\n"
    );
    parse_oudia(saved).unwrap();
}

#[test]
fn inserts_generated_times_into_untimed_handling_cells() {
    let fixture = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1000$4,1$1,1;100753/$1\n.\n.\n.\n";
    let source = parse_oudia(fixture.as_bytes().to_vec()).unwrap();
    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    assert_eq!(template.active_station_slots, [0, 1, 2]);
    let timetable = GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            stop(0, None, Some("10:10:00")),
            stop(1, Some("10:12:00"), Some("10:13:00")),
            stop(2, Some("10:15:00"), None),
        ],
    };

    let patch =
        build_eki_jikoku_patch(&source, &template, &timetable, OperationPolicy::Preserve).unwrap();
    let saved = patch.apply(&source.bytes).unwrap();

    assert_eq!(
        std::str::from_utf8(&saved).unwrap(),
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1010$4,1;1012/1013$1,1;101500/$1\n.\n.\n.\n"
    );
    parse_oudia(saved).unwrap();
}

#[test]
fn keeps_pass_cells_in_the_route_but_out_of_timetable_updates() {
    let fixture = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1000$4,2$1,1;100753/$1\n.\n.\n.\n";
    let source = parse_oudia(fixture.as_bytes().to_vec()).unwrap();
    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    assert_eq!(template.active_station_slots, [0, 2]);
    assert_eq!(template.route_station_slots, [0, 1, 2]);
    let timetable = GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            stop(0, None, Some("10:10:00")),
            stop(1, Some("10:15:00"), None),
        ],
    };

    let patch =
        build_eki_jikoku_patch(&source, &template, &timetable, OperationPolicy::Preserve).unwrap();
    let saved = patch.apply(&source.bytes).unwrap();

    assert_eq!(
        std::str::from_utf8(&saved).unwrap(),
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1010$4,2$1,1;101500/$1\n.\n.\n.\n"
    );
}

#[test]
fn updates_nobori_cells_in_travel_order() {
    let fixture = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nRosen.\nEki.\nEkimei=A\n.\nEki.\nEkimei=B\n.\nEki.\nEkimei=C\n.\n.\nDia.\nNobori.\nRessya.\nEkiJikoku=1;1000,1;1001/1002,1;1003/\n.\n.\n.\n";
    let source = parse_oudia(fixture.as_bytes().to_vec()).unwrap();
    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    assert_eq!(template.active_station_slots, [2, 1, 0]);
    let timetable = GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            stop(0, None, Some("10:10:00")),
            stop(1, Some("10:12:00"), Some("10:13:00")),
            stop(2, Some("10:15:00"), None),
        ],
    };

    let patch =
        build_eki_jikoku_patch(&source, &template, &timetable, OperationPolicy::Preserve).unwrap();
    let saved = patch.apply(&source.bytes).unwrap();

    assert_eq!(
        std::str::from_utf8(&saved).unwrap(),
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nRosen.\nEki.\nEkimei=A\n.\nEki.\nEkimei=B\n.\nEki.\nEkimei=C\n.\n.\nDia.\nNobori.\nRessya.\nEkiJikoku=1;1010,1;1012/1013,1;1015/\n.\n.\n.\n"
    );
}

#[test]
fn expands_one_logical_branch_station_to_multiple_oudia_slots() {
    let fixture = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nRosen.\nEki.\nEkimei=A\n.\nEki.\nEkimei=B\n.\nEki.\nEkimei=B\n.\nEki.\nEkimei=C\n.\n.\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1000,1;1001/,1;1002,1;1003/\n.\n.\n.\n";
    let source = parse_oudia(fixture.as_bytes().to_vec()).unwrap();
    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    let timetable = GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            stop(0, None, Some("10:10:00")),
            stop(1, Some("10:12:00"), Some("10:13:00")),
            stop(2, Some("10:15:00"), None),
        ],
    };

    let patch = build_eki_jikoku_patch_with_groups(
        &source,
        &template,
        &timetable,
        &[vec![0], vec![1, 2], vec![3]],
        OperationPolicy::Preserve,
    )
    .unwrap();
    let saved = patch.apply(&source.bytes).unwrap();

    assert_eq!(
        std::str::from_utf8(&saved).unwrap(),
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nRosen.\nEki.\nEkimei=A\n.\nEki.\nEkimei=B\n.\nEki.\nEkimei=B\n.\nEki.\nEkimei=C\n.\n.\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1010,1;1012/,1;1013,1;1015/\n.\n.\n.\n"
    );
}

#[test]
fn preserves_existing_time_fields_that_have_no_generated_counterpart() {
    let fixture = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;0959/1000,1;1001/1002,1;1003/1004\n.\n.\n.\n";
    let source = parse_oudia(fixture.as_bytes().to_vec()).unwrap();
    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    let timetable = GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            stop(0, None, Some("10:10:00")),
            stop(1, Some("10:12:00"), Some("10:13:00")),
            stop(2, Some("10:15:00"), None),
        ],
    };

    let patch =
        build_eki_jikoku_patch(&source, &template, &timetable, OperationPolicy::Preserve).unwrap();
    let saved = patch.apply(&source.bytes).unwrap();

    assert_eq!(
        std::str::from_utf8(&saved).unwrap(),
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;0959/1010,1;1012/1013,1;1015/1004\n.\n.\n.\n"
    );
}

#[test]
fn rejects_empty_or_mismatched_cells_and_24_hour_results() {
    let source = parse_oudia(FIXTURE.as_bytes().to_vec()).unwrap();
    let mut template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    template.active_station_slots.push(1);
    assert!(
        build_eki_jikoku_patch(&source, &template, &timetable(), OperationPolicy::Preserve)
            .is_err()
    );

    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    let mut overnight = timetable();
    overnight.crosses_midnight = true;
    assert!(
        build_eki_jikoku_patch(&source, &template, &overnight, OperationPolicy::Preserve).is_err()
    );
}

#[test]
fn removes_only_target_train_operation_when_requested() {
    let source = parse_oudia(FIXTURE.as_bytes().to_vec()).unwrap();
    let template = match build_oudia_route_templates(&source.document) {
        mtr_oudia_domain::ReferenceDiagramSelection::Selected(templates) => {
            templates.templates[0].clone()
        }
        _ => unreachable!(),
    };
    let patch = build_eki_jikoku_patch(
        &source,
        &template,
        &timetable(),
        OperationPolicy::RemoveTargetTrain,
    )
    .unwrap();
    let saved = patch.apply(&source.bytes).unwrap();
    assert!(std::str::from_utf8(&saved).unwrap().contains("\n\n.\n"));
    assert!(
        !std::str::from_utf8(&saved)
            .unwrap()
            .contains("Operation=target")
    );
}
