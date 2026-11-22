use super::common::*;
use mtr_oudia_application::*;
use mtr_oudia_gpui::carbon::*;
use mtr_oudia_gpui::shell_state::*;
use std::path::PathBuf;

#[test]
fn apply_candidates_result_handles_auto_manual_empty_and_error() {
    let mut state = MtrShellState::new(Ok(()));
    assert!(!state.is_step_completed(AppStep::Step3RouteMatch));

    // 1. 自動選択あり
    state.apply_candidates_result(Ok(vec![
        dummy_candidate("cand-1", 0, 2, "PrimaryExact", true, 10),
        dummy_candidate("cand-2", 0, 5, "Partial", false, 6),
    ]));
    assert_eq!(state.route_candidates.len(), 2);
    assert_eq!(
        state.selected_candidate_id,
        Some(CandidateId("cand-1".to_string()))
    );
    assert!(state.is_step_completed(AppStep::Step3RouteMatch));
    assert_eq!(
        state.status,
        AppStatus::CandidatesFound {
            count: 2,
            auto_selected: true,
        }
    );

    // 2. 複数手動候補（auto_selected == false のみ）
    state.apply_candidates_result(Ok(vec![
        dummy_candidate("cand-3", 1, 0, "Partial", false, 5),
        dummy_candidate("cand-4", 1, 1, "Partial", false, 4),
    ]));
    assert_eq!(state.route_candidates.len(), 2);
    assert!(state.selected_candidate_id.is_none());
    assert!(!state.is_step_completed(AppStep::Step3RouteMatch));
    assert_eq!(
        state.status,
        AppStatus::CandidatesFound {
            count: 2,
            auto_selected: false,
        }
    );

    // 手動選択で Step3RouteMatch が完了になる
    assert!(state.select_candidate(CandidateId("cand-4".to_string())));
    assert!(state.is_step_completed(AppStep::Step3RouteMatch));

    // 3. 0件
    state.apply_candidates_result(Ok(Vec::new()));
    assert!(state.route_candidates.is_empty());
    assert!(state.selected_candidate_id.is_none());
    assert!(!state.is_step_completed(AppStep::Step3RouteMatch));
    assert_eq!(
        state.status,
        AppStatus::CandidatesFound {
            count: 0,
            auto_selected: false,
        }
    );

    // 4. エラー
    state.apply_candidates_result(Err(BusinessError {
        kind: BusinessErrorKind::Validation,
        message: "路線が見つかりません".to_string(),
        detail: Some("route-missing".to_string()),
    }));
    assert_eq!(
        state.status,
        AppStatus::CandidatesError {
            title: "路線が見つかりません".to_string(),
            detail: "route-missing".to_string(),
        }
    );
}

#[test]
fn upstream_updates_clear_downstream_step3_state() {
    let mut state = MtrShellState::new(Ok(()));
    state.candidates = vec![
        "http://127.0.0.1:49182/".to_string(),
        "http://127.0.0.1:49183/".to_string(),
    ];
    assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
    state.apply_snapshot_result(0, Ok(dummy_snapshot(vec![("山手線", 30)])));
    state.apply_inspection_result(
        PathBuf::from("/tmp/yamanote.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 30)),
    );

    assert!(state.select_route("route-0".to_string()));
    assert!(state.select_diagram_index(Some(1)));
    assert!(state.select_train_type(Some(0)));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-1",
        1,
        0,
        "PrimaryExact",
        true,
        30,
    )]));
    assert!(state.selected_route_id.is_some());
    assert_eq!(state.selected_diagram_index, Some(1));
    assert_eq!(state.selected_train_type, Some(0));
    assert!(!state.route_candidates.is_empty());
    assert!(state.selected_candidate_id.is_some());

    // Step 1 スナップショット更新で selected_route_id / route_candidates / selected_candidate_id がクリアされる
    state.apply_snapshot_result(1, Ok(dummy_snapshot(vec![("京浜東北線", 35)])));
    assert!(state.selected_route_id.is_none());
    assert!(state.route_candidates.is_empty());
    assert!(state.selected_candidate_id.is_none());
    assert_eq!(state.selected_diagram_index, Some(1));
    assert_eq!(state.selected_train_type, Some(0));

    // 再度選択して、Step 1 接続先変更時もクリアされることを確認
    assert!(state.select_route("route-0".to_string()));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-2",
        1,
        2,
        "PrimaryExact",
        true,
        35,
    )]));
    assert!(state.select_endpoint("http://127.0.0.1:49183/".to_string()));
    assert!(state.selected_route_id.is_none());
    assert!(state.route_candidates.is_empty());
    assert!(state.selected_candidate_id.is_none());

    // Step 2 OuDia 更新で selected_diagram_index / selected_train_type / route_candidates / selected_candidate_id がクリアされる
    state.apply_snapshot_result(0, Ok(dummy_snapshot(vec![("山手線", 30)])));
    assert!(state.select_route("route-0".to_string()));
    assert!(state.select_diagram_index(Some(0)));
    assert!(state.select_train_type(Some(1)));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-3",
        0,
        1,
        "PrimaryExact",
        true,
        30,
    )]));
    state.apply_inspection_result(
        PathBuf::from("/tmp/chuo.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("中央線"), 24)),
    );
    assert_eq!(state.selected_route_id.as_deref(), Some("route-0"));
    assert!(state.selected_diagram_index.is_none());
    assert!(state.selected_train_type.is_none());
    assert!(state.route_candidates.is_empty());
    assert!(state.selected_candidate_id.is_none());
}

#[test]
fn status_notification_maps_step3_statuses() {
    let finding = AppStatus::FindingCandidates;
    assert!(finding.is_busy());
    assert!(finding.is_finding_candidates());
    assert_eq!(status_notification(&finding).0, NotificationKind::Info);

    let auto = AppStatus::CandidatesFound {
        count: 1,
        auto_selected: true,
    };
    assert!(!auto.is_busy());
    assert_eq!(status_notification(&auto).0, NotificationKind::Success);

    let manual = AppStatus::CandidatesFound {
        count: 3,
        auto_selected: false,
    };
    assert_eq!(status_notification(&manual).0, NotificationKind::Info);

    let empty = AppStatus::CandidatesFound {
        count: 0,
        auto_selected: false,
    };
    assert_eq!(status_notification(&empty).0, NotificationKind::Warning);

    let err = AppStatus::CandidatesError {
        title: "路線が見つかりません".to_string(),
        detail: "route-0".to_string(),
    };
    assert_eq!(
        status_notification(&err),
        (
            NotificationKind::Error,
            "路線が見つかりません".to_string(),
            "route-0".to_string(),
        )
    );
}

#[test]
fn normalize_train_number_input_trims_and_handles_empty() {
    assert_eq!(normalize_train_number_input(""), None);
    assert_eq!(normalize_train_number_input("   \t\n "), None);
    assert_eq!(
        normalize_train_number_input("101M"),
        Some("101M".to_string())
    );
    assert_eq!(
        normalize_train_number_input("  101M \t"),
        Some("101M".to_string())
    );
}

#[test]
fn manual_station_mapping_updates_resets_and_clears_downstream() {
    let mut state = MtrShellState::new(Ok(()));
    // 候補未選択時は編集・リセットとも無効
    assert!(!state.set_manual_station_slot(0, 1));
    assert!(!state.reset_manual_station_mappings());
    assert!(state.manual_station_mappings.is_none());

    state.apply_snapshot_result(
        0,
        Ok(dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線",
                vec![
                    ("東京", "1", 10_000, Some(120_000)),
                    ("神田", "2", 10_000, None),
                ],
            )],
            1_000,
        )),
    );
    state.apply_inspection_result(
        PathBuf::from("/tmp/yamanote.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 3)),
    );
    assert!(state.select_route("r1".to_string()));
    state.apply_candidates_result(Ok(vec![
        dummy_candidate("cand-1", 0, 0, "PrimaryExact", true, 2),
        dummy_candidate("cand-2", 0, 1, "manual", false, 0),
    ]));
    assert!(state.manual_station_mappings.is_none());
    assert_eq!(
        mapping_pairs(state.effective_station_mappings()),
        vec![(0, 0), (1, 1)]
    );

    // Step 4 / Step 5 完了状態を構築
    state.apply_preview_result(Ok(dummy_preview("preview-1", 2, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        1024,
        "sha-1",
    )));

    // 別スロットを追加すると 1 対多で手動マッピングが保持され、Step 4/5 がクリアされる
    assert!(state.set_manual_station_slot(0, 2));
    assert_eq!(
        mapping_pairs(state.manual_station_mappings.as_deref()),
        vec![(0, 0), (0, 2), (1, 1)]
    );
    assert!(state.preview.is_none());
    assert!(state.save_receipt.is_none());

    // 割当済みスロットを再指定するとトグル解除される
    assert!(state.set_manual_station_slot(0, 0));
    assert_eq!(
        mapping_pairs(state.manual_station_mappings.as_deref()),
        vec![(0, 2), (1, 1)]
    );
    assert_eq!(
        mapping_pairs(state.effective_station_mappings()),
        vec![(0, 2), (1, 1)]
    );

    // Step 4/5 を再度埋めた後、「自動マッピングに戻す」で手動編集と下流がクリアされる
    state.apply_preview_result(Ok(dummy_preview("preview-2", 2, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        1024,
        "sha-2",
    )));
    assert!(state.reset_manual_station_mappings());
    assert!(state.manual_station_mappings.is_none());
    assert_eq!(
        mapping_pairs(state.effective_station_mappings()),
        vec![(0, 0), (1, 1)]
    );
    assert!(state.preview.is_none());
    assert!(state.save_receipt.is_none());
    assert!(!state.reset_manual_station_mappings());

    // manual_only 候補（初期マッピング空）への手動割当も機能する
    assert!(state.select_candidate(CandidateId("cand-2".to_string())));
    assert!(mapping_pairs(state.effective_station_mappings()).is_empty());
    assert!(state.set_manual_station_slot(1, 2));
    assert_eq!(
        mapping_pairs(state.manual_station_mappings.as_deref()),
        vec![(1, 2)]
    );
}

#[test]
fn manual_station_mapping_clears_on_candidate_and_upstream_changes() {
    let mut state = MtrShellState::new(Ok(()));
    state.candidates = vec![
        "http://127.0.0.1:49182/".to_string(),
        "http://127.0.0.1:49183/".to_string(),
    ];
    assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
    state.apply_snapshot_result(0, Ok(dummy_snapshot(vec![("山手線", 2), ("中央線", 2)])));
    state.apply_inspection_result(
        PathBuf::from("/tmp/yamanote.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 3)),
    );
    assert!(state.select_route("route-0".to_string()));
    state.apply_candidates_result(Ok(vec![
        dummy_candidate("cand-1", 0, 0, "PrimaryExact", true, 2),
        dummy_candidate("cand-2", 0, 1, "Partial", false, 2),
    ]));

    // 1. 同一候補再選択は維持、別候補選択でクリア
    assert!(state.set_manual_station_slot(0, 2));
    assert!(state.select_candidate(CandidateId("cand-1".to_string())));
    assert!(state.manual_station_mappings.is_some());
    assert!(state.select_candidate(CandidateId("cand-2".to_string())));
    assert!(state.manual_station_mappings.is_none());

    // 2. 候補再検索でクリア
    assert!(state.set_manual_station_slot(0, 2));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-3",
        0,
        2,
        "PrimaryExact",
        true,
        2,
    )]));
    assert!(state.manual_station_mappings.is_none());

    // 3. 路線・ダイヤ・種別変更でクリア
    assert!(state.set_manual_station_slot(0, 2));
    assert!(state.select_diagram_index(Some(1)));
    assert!(state.manual_station_mappings.is_none());

    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-4",
        1,
        0,
        "PrimaryExact",
        true,
        2,
    )]));
    assert!(state.set_manual_station_slot(0, 2));
    assert!(state.select_train_type(Some(1)));
    assert!(state.manual_station_mappings.is_none());

    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-5",
        1,
        1,
        "PrimaryExact",
        true,
        2,
    )]));
    assert!(state.set_manual_station_slot(0, 2));
    assert!(state.select_route("route-1".to_string()));
    assert!(state.manual_station_mappings.is_none());

    // 4. Step 2 OuDia 再解析・Step 1 スナップショット更新・接続先変更でクリア
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-6",
        1,
        2,
        "PrimaryExact",
        true,
        2,
    )]));
    assert!(state.set_manual_station_slot(0, 2));
    state.apply_inspection_result(
        PathBuf::from("/tmp/chuo.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("中央線"), 2)),
    );
    assert!(state.manual_station_mappings.is_none());

    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-7",
        0,
        0,
        "PrimaryExact",
        true,
        2,
    )]));
    assert!(state.set_manual_station_slot(0, 2));
    state.apply_snapshot_result(1, Ok(dummy_snapshot(vec![("京浜東北線", 2)])));
    assert!(state.manual_station_mappings.is_none());

    assert!(state.select_route("route-0".to_string()));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-8",
        0,
        0,
        "PrimaryExact",
        true,
        2,
    )]));
    assert!(state.set_manual_station_slot(0, 2));
    assert!(state.select_endpoint("http://127.0.0.1:49183/".to_string()));
    assert!(state.manual_station_mappings.is_none());
}

#[test]
fn station_mapping_issues_detects_missing_and_duplicate_slots() {
    let snapshot = dummy_detailed_snapshot(
        vec![(
            "r1",
            "山手線",
            vec![
                ("東京", "1", 10_000, Some(120_000)),
                ("神田", "2", 10_000, None),
            ],
        )],
        1_000,
    );
    let route = &snapshot.routes[0];
    let template = TemplateDto {
        diagram_index: 0,
        direction: "kudari".to_string(),
        train_index: 0,
        train_type_index: Some(0),
        active_station_slots: vec![
            StationSlotDto {
                index: 0,
                name: "東京".to_string(),
                handling_code: Some(1),
                previous_name: None,
                next_name: Some("神田".to_string()),
            },
            StationSlotDto {
                index: 1,
                name: "神田".to_string(),
                handling_code: Some(1),
                previous_name: Some("東京".to_string()),
                next_name: None,
            },
        ],
        route_station_slots: Vec::new(),
    };

    let valid = vec![
        StationMappingDto {
            mtr_station_index: 0,
            oudia_station_slot: 0,
        },
        StationMappingDto {
            mtr_station_index: 1,
            oudia_station_slot: 1,
        },
    ];
    assert!(station_mapping_issues(route, &template, &valid).is_empty());

    let invalid = vec![
        StationMappingDto {
            mtr_station_index: 0,
            oudia_station_slot: 0,
        },
        StationMappingDto {
            mtr_station_index: 1,
            oudia_station_slot: 0,
        },
    ];
    let issues = station_mapping_issues(route, &template, &invalid);
    assert_eq!(issues.len(), 2);
    assert!(issues.iter().any(|issue| issue.contains("未割当")));
    assert!(issues.iter().any(|issue| issue.contains("重複割当")));
}
