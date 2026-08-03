use mtr_oudia_domain::{
    GeneratedStop, GeneratedTimetable, OperationPolicy, build_eki_jikoku_patch,
    build_oudia_route_templates, parse_oudia,
};

const FIXTURE: &str = "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;/10:00:00$2,,2;10:02:00/10:03:00$3,1;10:05:00/$4\nOperation=target\n.\n.\n.\n";

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
        "FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;/10:10:00$2,,2;10:12:00/10:13:00$3,1;10:15:00/$4\nOperation=target\n.\n.\n.\n"
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
