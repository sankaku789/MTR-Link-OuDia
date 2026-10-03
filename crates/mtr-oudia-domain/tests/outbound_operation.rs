use mtr_oudia_domain::{
    GeneratedStop, GeneratedTimetable, OperationPolicy, OutboundRuntime, ReferenceDiagramSelection,
    ServiceTimeMillis, build_conversion_patch, build_oudia_route_templates, parse_oudia,
};

const FIXTURE: &str = include_str!("../../../fixtures/oudia/outbound-operation.oud2");

fn apply(
    text: &str,
    encoding: &str,
    bom: bool,
    policy: OperationPolicy,
    runtime: Option<OutboundRuntime>,
) -> (
    Vec<u8>,
    mtr_oudia_domain::OudiaPatch,
    mtr_oudia_domain::OudiaSource,
) {
    let mut bytes = match encoding {
        "cp932" => encoding_rs::SHIFT_JIS.encode(text).0.into_owned(),
        _ => text.as_bytes().to_vec(),
    };
    if bom {
        bytes.splice(0..0, [0xef, 0xbb, 0xbf]);
    }
    let source = parse_oudia(bytes).unwrap();
    let patch = plan(&source, policy, runtime).unwrap();
    let saved = patch.apply(&source.bytes).unwrap();
    (saved, patch, source)
}

fn plan(
    source: &mtr_oudia_domain::OudiaSource,
    policy: OperationPolicy,
    runtime: Option<OutboundRuntime>,
) -> Result<mtr_oudia_domain::OudiaPatch, mtr_oudia_domain::EkiJikokuPatchError> {
    let ReferenceDiagramSelection::Selected(templates) =
        build_oudia_route_templates(&source.document)
    else {
        panic!("template")
    };
    let template = &templates.templates[0];
    let timetable = GeneratedTimetable {
        crosses_midnight: false,
        stops: vec![
            GeneratedStop {
                station_index: 0,
                arrival: None,
                departure: Some(ServiceTimeMillis::TEN_OCLOCK),
                rounded_arrival_seconds: None,
                rounded_departure_seconds: Some(36_000),
                rounded_arrival_display: None,
                rounded_departure_display: Some("10:00:00".into()),
            },
            GeneratedStop {
                station_index: 1,
                arrival: Some(ServiceTimeMillis::new(36_120_000).unwrap()),
                departure: None,
                rounded_arrival_seconds: Some(36_120),
                rounded_departure_seconds: None,
                rounded_arrival_display: Some("10:02:00".into()),
                rounded_departure_display: None,
            },
        ],
    };
    let groups = template
        .active_station_slots
        .iter()
        .map(|slot| vec![*slot])
        .collect::<Vec<_>>();
    build_conversion_patch(
        source,
        template,
        &timetable,
        &groups,
        policy,
        runtime,
        ServiceTimeMillis::new(0).unwrap(),
    )
}

fn runtime() -> Option<OutboundRuntime> {
    Some(OutboundRuntime::from_seconds(107).unwrap())
}

#[test]
fn off_preserves_operation_and_does_not_change_already_matching_bytes() {
    let (bytes, _, _) = apply(FIXTURE, "utf8", false, OperationPolicy::Preserve, None);
    assert_eq!(bytes, FIXTURE.as_bytes());
}

#[test]
fn adds_outbound_when_no_operation_and_updates_only_time_with_existing_link_and_numbers() {
    let missing = FIXTURE.replace("Operation0B=3/2359$/1;2\n", "");
    let (bytes, _, _) = apply(
        &missing,
        "utf8",
        false,
        OperationPolicy::Preserve,
        runtime(),
    );
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("Operation0B=3/095813$/\n")
    );
    let linked = FIXTURE.replace("3/2359$/1;2", "3/2359$連携/1;2");
    let (bytes, _, _) = apply(&linked, "utf8", false, OperationPolicy::Preserve, runtime());
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        linked.replace("3/2359$", "3/095813$")
    );
}

#[test]
fn preserves_multiple_known_operations_including_official_shunt_format() {
    let text = FIXTURE.replace("3/2359$/1;2", "3/514$/C34;G31,0/8$515/$0");
    let (bytes, _, _) = apply(&text, "utf8", false, OperationPolicy::Preserve, runtime());
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("3/095813$/C34;G31,0/8$515/$0")
    );
}

#[test]
fn encoding_newline_bom_unknown_and_other_train_are_lossless() {
    for encoding in ["utf8", "cp932"] {
        for newline in ["\n", "\r\n"] {
            for bom in [false, true] {
                if encoding == "cp932" && bom {
                    continue;
                }
                let text = FIXTURE
                    .replace("\n", newline)
                    .replace(&format!("Operation0B=3/2359$/1;2{newline}"), "");
                let (bytes, _, source) =
                    apply(&text, encoding, bom, OperationPolicy::Preserve, runtime());
                let parsed = parse_oudia(bytes.clone()).unwrap();
                assert_eq!(parsed.bom, bom);
                assert_eq!(parsed.encoding, source.encoding);
                assert_eq!(parsed.line_ending, source.line_ending);
                let expected = text.replace(
                    &format!("UnknownTrainField=保持{newline}."),
                    &format!("UnknownTrainField=保持{newline}Operation0B=3/095813$/{newline}."),
                );
                let mut expected_bytes = if encoding == "cp932" {
                    encoding_rs::SHIFT_JIS.encode(&expected).0.into_owned()
                } else {
                    expected.into_bytes()
                };
                if bom {
                    expected_bytes.splice(0..0, [0xef, 0xbb, 0xbf]);
                }
                assert_eq!(bytes, expected_bytes);
            }
        }
    }
}

#[test]
fn directional_index_is_not_fixed_to_zero_for_partial_service() {
    for direction in ["Kudari", "Nobori"] {
        let text = FIXTURE
            .replace(
                "Eki.\nEkimei=始発",
                "Eki.\nEkimei=路線端\n.\nEki.\nEkimei=始発",
            )
            .replace("Kudari.", &format!("{direction}."))
            .replace(
                "Eki.\nEkimei=終点\n.\n.\nDia.",
                "Eki.\nEkimei=終点\n.\nEki.\nEkimei=路線端2\n.\n.\nDia.",
            )
            .replace("EkiJikoku=1;1000,1;1002/", "EkiJikoku=,1;1000,1;1002/")
            .replace("Operation0B=3/2359$/1;2", "Operation1B=3/2359$/1;2");
        let (bytes, _, _) = apply(&text, "utf8", false, OperationPolicy::Preserve, runtime());
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("Operation1B=3/095813$/1;2")
        );
    }
}

#[test]
fn expected_bytes_mismatch_refuses_operation_update_and_insertion() {
    for text in [
        FIXTURE.to_string(),
        FIXTURE.replace("Operation0B=3/2359$/1;2\n", ""),
    ] {
        let (_, patch, source) = apply(&text, "utf8", false, OperationPolicy::Preserve, runtime());
        let replacement = patch
            .replacements()
            .iter()
            .find(|r| r.kind == mtr_oudia_domain::ByteReplacementKind::Operation)
            .unwrap();
        let mut altered = source.bytes.clone();
        altered[replacement.range.start()] ^= 1;
        assert!(patch.apply(&altered).is_err());
    }
}

#[test]
fn conflicts_and_malformed_or_duplicate_operations_fail_closed() {
    for value in [
        "5/1000$A",
        "4/0$0900/1000$/A",
        "3/0950$/A,5/1000$B",
        "3/0950$/A,3/0951$/B",
        "3/not-time$/A",
        "3/0950$A",
        "9/unknown",
        "3/0950$/A,,0/1$1000/$0",
    ] {
        let text = FIXTURE.replace("3/2359$/1;2", value);
        let source = parse_oudia(text.into_bytes()).unwrap();
        assert!(
            plan(&source, OperationPolicy::Preserve, runtime()).is_err(),
            "{value}"
        );
    }
    let duplicated = FIXTURE.replace(
        "Operation0B=3/2359$/1;2",
        "Operation0B=3/2359$/1;2\nOperation0B=3/2358$/3",
    );
    assert!(
        plan(
            &parse_oudia(duplicated.into_bytes()).unwrap(),
            OperationPolicy::Preserve,
            runtime()
        )
        .is_err()
    );
}

#[test]
fn explicit_removal_then_generation_replaces_all_selected_operations_only() {
    let text = FIXTURE.replace(
        "Operation0B=3/2359$/1;2",
        "Operation0B=5/1000$A\nOperation1A=3/1002$",
    );
    let (bytes, _, _) = apply(
        &text,
        "utf8",
        false,
        OperationPolicy::RemoveTargetTrain,
        runtime(),
    );
    let saved = String::from_utf8(bytes).unwrap();
    assert!(saved.contains("Operation0B=3/095813$/"));
    assert!(!saved.contains("Operation1A="));
    assert!(!saved.contains("5/1000"));
}
