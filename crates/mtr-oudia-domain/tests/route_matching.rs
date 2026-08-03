use std::collections::BTreeMap;

use mtr_oudia_domain::{
    KijunDiaIndex, OudiaDirection, OudiaRouteTemplate, OudiaRouteTemplates,
    ReferenceDiagramSelection, RouteMatchOutcome, RouteMatchRank, SourceRange,
    build_oudia_route_templates, match_mtr_route, normalize_station_name, parse_oudia,
};

const ROUTE_FIXTURE: &str = r#"FileType=OuDiaSecond.1.16
KijunDiaIndex=0
Rosen.
Eki.
Ekimei= 日本語|English||metadata 
.
Eki.
Ekimei=Ｂ　Ｓｔａｔｉｏｎ
.
Eki.
Ekimei=C
.
Eki.
Ekimei=D
.
.
Dia.
Kudari.
Ressya.
Ressyasyubetsu=2
EkiJikoku=1;10:00:00/10:00:00,,1;10:02:00/10:02:00,;/
.
.
.
"#;

fn range() -> SourceRange {
    SourceRange::new(0, 0).unwrap()
}

fn template(direction: OudiaDirection, slots: &[usize], names: &[&str]) -> OudiaRouteTemplate {
    OudiaRouteTemplate {
        diagram_index: 0,
        direction,
        train_index: 0,
        train_type_index: Some(1),
        active_station_slots: slots.to_vec(),
        stop_pattern: Vec::new(),
        source_train_range: range(),
        eki_jikoku_value_range: range(),
        station_slot_names: names.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn extracts_active_slots_and_requires_manual_dia_for_invalid_reference_index() {
    let source = parse_oudia(ROUTE_FIXTURE.as_bytes().to_vec()).unwrap();
    let templates = build_oudia_route_templates(&source.document);

    assert_eq!(
        source.document.station_slots[0].name,
        " 日本語|English||metadata "
    );
    assert!(
        matches!(templates, ReferenceDiagramSelection::Selected(OudiaRouteTemplates { ref templates, .. }) if templates[0].active_station_slots == [0, 2] && templates[0].train_type_index == Some(2))
    );
    assert!(!source.document.diagrams[0].trains[0].eki_jikoku.cells[3].is_empty());

    let mut document = source.document;
    document.kijun_dia_index = KijunDiaIndex::Invalid;
    assert!(matches!(
        build_oudia_route_templates(&document),
        ReferenceDiagramSelection::NeedsSelection { candidates, .. } if candidates.len() == 1
    ));
}

#[test]
fn normalizes_multilingual_unicode_and_alias_names() {
    assert_eq!(normalize_station_name(" Ｂ　Ｓｔａｔｉｏｎ "), "b station");

    let multilingual = match_mtr_route(
        &["English".to_string(), "Ｂ　Ｓｔａｔｉｏｎ".to_string()],
        &[template(
            OudiaDirection::Kudari,
            &[0, 1],
            &["日本語|English||metadata", "B Station"],
        )],
        &BTreeMap::new(),
        Some(1),
    );
    assert!(
        matches!(multilingual, RouteMatchOutcome::Automatic { candidate } if candidate.rank == RouteMatchRank::MultilingualExact)
    );

    let mut aliases = BTreeMap::new();
    aliases.insert("Central".to_string(), "中央".to_string());
    let outcome = match_mtr_route(
        &["中央".to_string(), "終点".to_string()],
        &[template(
            OudiaDirection::Kudari,
            &[0, 1],
            &["Central", "終点"],
        )],
        &aliases,
        Some(1),
    );

    assert!(
        matches!(outcome, RouteMatchOutcome::Automatic { candidate } if candidate.rank == RouteMatchRank::AliasExact)
    );
}

#[test]
fn requires_manual_confirmation_when_train_type_is_not_selected() {
    let outcome = match_mtr_route(
        &["A".to_string(), "B".to_string()],
        &[template(OudiaDirection::Kudari, &[0, 1], &["A", "B"])],
        &BTreeMap::new(),
        None,
    );

    assert!(
        matches!(outcome, RouteMatchOutcome::Manual { diagnostics, .. } if diagnostics.contains(&mtr_oudia_domain::RouteMatchDiagnostic::TrainTypeNotSelected))
    );
}

#[test]
fn distinguishes_no_single_and_multiple_candidates_without_station_only_selection() {
    let no_match = match_mtr_route(
        &["A".to_string(), "Z".to_string()],
        &[template(OudiaDirection::Kudari, &[0, 1], &["A", "B"])],
        &BTreeMap::new(),
        Some(1),
    );
    assert!(matches!(no_match, RouteMatchOutcome::NoCandidate { .. }));

    let one = match_mtr_route(
        &["A".to_string(), "B".to_string()],
        &[template(OudiaDirection::Kudari, &[3, 8], &["A", "B"])],
        &BTreeMap::new(),
        Some(1),
    );
    assert!(
        matches!(one, RouteMatchOutcome::Automatic { ref candidate } if candidate.station_mapping[0].oudia_slot_index == 3)
    );

    let many = match_mtr_route(
        &["A".to_string(), "B".to_string()],
        &[
            template(OudiaDirection::Kudari, &[0, 1], &["A", "B"]),
            template(OudiaDirection::Nobori, &[5, 2], &["A", "B"]),
        ],
        &BTreeMap::new(),
        Some(1),
    );
    assert!(matches!(many, RouteMatchOutcome::Manual { candidates, .. } if candidates.len() == 2));
}

#[test]
fn keeps_direction_order_duplicate_context_and_partial_matches_manual() {
    // スロット番号が増減しないデルタ線相当でも、駅列と所属方向で照合する。
    let forward = template(OudiaDirection::Kudari, &[4, 1, 3], &["A", "B", "C"]);
    let reversed = template(OudiaDirection::Nobori, &[7, 2, 6], &["C", "B", "A"]);
    let outcome = match_mtr_route(
        &["A".to_string(), "B".to_string(), "C".to_string()],
        &[forward.clone(), reversed],
        &BTreeMap::new(),
        Some(1),
    );
    assert!(
        matches!(outcome, RouteMatchOutcome::Automatic { candidate } if candidate.direction == OudiaDirection::Kudari)
    );

    let duplicate = match_mtr_route(
        &["A".to_string(), "X".to_string(), "A".to_string()],
        &[
            template(OudiaDirection::Kudari, &[0, 1, 2], &["A", "X", "A"]),
            template(OudiaDirection::Kudari, &[3, 4, 5], &["A", "Y", "A"]),
        ],
        &BTreeMap::new(),
        Some(1),
    );
    assert!(
        matches!(duplicate, RouteMatchOutcome::Automatic { candidate } if candidate.station_mapping[1].oudia_slot_index == 1)
    );

    let partial = match_mtr_route(
        &["A".to_string(), "X".to_string(), "C".to_string()],
        &[forward],
        &BTreeMap::new(),
        Some(1),
    );
    assert!(
        matches!(partial, RouteMatchOutcome::Manual { candidates, .. } if candidates[0].rank == RouteMatchRank::Partial)
    );
}
