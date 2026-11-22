use super::common::*;
use mtr_oudia_application::*;
use mtr_oudia_gpui::backend::*;
use mtr_oudia_gpui::carbon::*;
use mtr_oudia_gpui::shell_state::*;
use std::path::PathBuf;

#[test]
fn select_endpoint_clears_old_snapshot_only_when_endpoint_changes() {
    let mut state = MtrShellState::new(Ok(()));
    state.candidates = vec![
        "http://127.0.0.1:49182/".to_string(),
        "http://127.0.0.1:49183/".to_string(),
    ];
    assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
    state.dimension = 2;
    state.apply_snapshot_result(2, Ok(dummy_snapshot(vec![("山手線", 30)])));
    assert!(state.snapshot.is_some());
    assert!(state.is_step_completed(AppStep::Step1Mtr));

    // 同一接続先の再選択では既存のスナップショットを保持する。
    assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
    assert!(state.snapshot.is_some());
    assert_eq!(state.dimension, 2);

    // 別の接続先へ切り替えると旧スナップショットと dimension をクリアし、完了状態を戻す。
    assert!(state.select_endpoint("http://127.0.0.1:49183/".to_string()));
    assert!(state.snapshot.is_none());
    assert_eq!(state.dimension, 0);
    assert!(!state.is_step_completed(AppStep::Step1Mtr));
    assert_eq!(
        state.status,
        AppStatus::DetectedMultiple {
            count: 2,
            selected: "http://127.0.0.1:49183/".to_string(),
        }
    );
}

#[test]
fn apply_detection_result_handles_single_multiple_and_error() {
    let mut state = MtrShellState::new(Ok(()));

    state.apply_detection_result(Ok(vec![EndpointDto {
        url: "http://127.0.0.1:49182/".to_string(),
    }]));
    assert_eq!(
        state.selected_endpoint.as_deref(),
        Some("http://127.0.0.1:49182/")
    );
    assert_eq!(
        state.status,
        AppStatus::DetectedSingle {
            endpoint: "http://127.0.0.1:49182/".to_string(),
        }
    );

    state.apply_snapshot_result(0, Ok(dummy_snapshot(vec![("中央線", 24)])));
    assert!(state.snapshot.is_some());

    // 異なる接続先が検出された場合は旧スナップショットをクリアする。
    state.apply_detection_result(Ok(vec![
        EndpointDto {
            url: "http://127.0.0.1:49183/".to_string(),
        },
        EndpointDto {
            url: "http://127.0.0.1:49184/".to_string(),
        },
    ]));
    assert!(state.snapshot.is_none());
    assert_eq!(state.candidates.len(), 2);
    assert_eq!(
        state.status,
        AppStatus::DetectedMultiple {
            count: 2,
            selected: "http://127.0.0.1:49183/".to_string(),
        }
    );

    state.apply_detection_result(Err(BusinessError {
        kind: BusinessErrorKind::NoEndpoints,
        message: "待受ポートが見つかりません".to_string(),
        detail: Some("Minecraft ログが見つかりません".to_string()),
    }));
    assert_eq!(
        state.status,
        AppStatus::DetectionError {
            title: "待受ポートが見つかりません".to_string(),
            detail: "Minecraft ログが見つかりません".to_string(),
        }
    );
}

#[test]
fn status_notification_maps_statuses_and_business_errors() {
    let init_err = MtrShellState::new(Err("runtime init error".to_string()));
    assert!(init_err.status.is_init_failed());
    assert_eq!(
        status_notification(&init_err.status),
        (
            NotificationKind::Error,
            "バックエンドの初期化に失敗しました".to_string(),
            "runtime init error".to_string(),
        )
    );

    let idle = status_notification(&AppStatus::Idle);
    assert_eq!(idle.0, NotificationKind::Info);
    assert_eq!(idle.1, "待機中");

    let detecting = status_notification(&AppStatus::Detecting);
    assert_eq!(detecting.0, NotificationKind::Info);
    assert_eq!(detecting.1, "検出中");

    let fetching = status_notification(&AppStatus::Fetching {
        endpoint: "http://127.0.0.1:49182/".to_string(),
        dimension: 1,
    });
    assert_eq!(fetching.0, NotificationKind::Info);
    assert_eq!(fetching.1, "取得中");
    assert!(fetching.2.contains("dimension=1"));

    let success = status_notification(&AppStatus::FetchSuccess {
        route_count: 3,
        stop_count: 45,
    });
    assert_eq!(
        success,
        (
            NotificationKind::Success,
            "取得完了".to_string(),
            "3 路線 / 45 駅を取得しました".to_string(),
        )
    );

    let mut state = MtrShellState::new(Ok(()));
    state.apply_snapshot_result(
        1,
        Err(BusinessError {
            kind: BusinessErrorKind::Connection,
            message: "MTR API へ接続できません".to_string(),
            detail: None,
        }),
    );
    assert_eq!(
        status_notification(&state.status),
        (
            NotificationKind::Error,
            "MTR API へ接続できません".to_string(),
            String::new(),
        )
    );
}

#[test]
fn format_dimension_label_handles_string_and_number_values() {
    assert_eq!(format_dimension_label(0, &"\"overworld\""), "0: overworld");
    assert_eq!(format_dimension_label(1, &1), "1: 1");
}

#[test]
fn apply_refresh_snapshot_result_preserves_downstream_when_unchanged_and_clears_when_updated() {
    let mut state = MtrShellState::new(Ok(()));
    state.candidates = vec!["http://127.0.0.1:49182/".to_string()];
    assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
    let initial_snapshot = dummy_detailed_snapshot(
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
    state.apply_snapshot_result(0, Ok(initial_snapshot));
    state.apply_inspection_result(
        PathBuf::from("/tmp/yamanote.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 30)),
    );
    assert!(state.select_route("r1".to_string()));
    assert!(state.select_diagram_index(Some(1)));
    assert!(state.select_train_type(Some(0)));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-1",
        1,
        0,
        "PrimaryExact",
        true,
        2,
    )]));
    state.apply_preview_result(Ok(dummy_preview("preview-1", 2, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        2048,
        "sha-1",
    )));

    // 1. 変化なし適用時は既存スナップショットおよび Step 3/4/5 の状態がすべて維持される
    state.apply_refresh_snapshot_result(0, Ok(RefreshSnapshotOutcome::Unchanged));
    assert_eq!(state.status, AppStatus::RefreshUnchanged);
    assert_eq!(
        state.snapshot.as_ref().map(|s| s.api_current_time_millis),
        Some(1_000)
    );
    assert_eq!(state.selected_route_id.as_deref(), Some("r1"));
    assert_eq!(state.selected_diagram_index, Some(1));
    assert_eq!(state.selected_train_type, Some(0));
    assert_eq!(state.route_candidates.len(), 1);
    assert_eq!(
        state.selected_candidate_id,
        Some(CandidateId("cand-1".to_string()))
    );
    assert!(state.preview.is_some());
    assert!(state.save_receipt.is_some());

    // 2. エラー時も既存スナップショットおよび Step 3/4/5 の状態は維持される
    state.apply_refresh_snapshot_result(
        0,
        Err(BusinessError {
            kind: BusinessErrorKind::Connection,
            message: "MTR API へ接続できません".to_string(),
            detail: None,
        }),
    );
    assert!(matches!(state.status, AppStatus::FetchError { .. }));
    assert_eq!(state.selected_route_id.as_deref(), Some("r1"));
    assert!(state.preview.is_some());
    assert!(state.save_receipt.is_some());

    // 3. 変化ありの更新時は snapshot を更新し、Step 3〜5 の下流状態をクリアする
    let updated_snapshot = dummy_detailed_snapshot(
        vec![(
            "r1",
            "山手線（外回り）",
            vec![
                ("東京", "1", 10_000, Some(120_000)),
                ("神田", "2", 10_000, Some(90_000)),
                ("秋葉原", "1", 10_000, None),
            ],
        )],
        32_000,
    );
    state.apply_refresh_snapshot_result(0, Ok(RefreshSnapshotOutcome::Updated(updated_snapshot)));
    assert_eq!(
        state.status,
        AppStatus::RefreshUpdated {
            route_count: 1,
            stop_count: 3,
        }
    );
    assert_eq!(
        state.snapshot.as_ref().map(|s| s.routes[0].name.as_str()),
        Some("山手線（外回り）")
    );
    assert!(state.selected_route_id.is_none());
    assert!(state.route_candidates.is_empty());
    assert!(state.selected_candidate_id.is_none());
    assert!(state.preview.is_none());
    assert!(state.save_receipt.is_none());
    assert_eq!(state.selected_diagram_index, Some(1));
    assert_eq!(state.selected_train_type, Some(0));
}

#[test]
fn status_notification_and_busy_cover_refresh_statuses() {
    let refreshing = AppStatus::RefreshingCache {
        endpoint: "http://127.0.0.1:49182/".to_string(),
        dimension: 0,
    };
    assert!(refreshing.is_busy());
    assert!(refreshing.is_refreshing());
    assert_eq!(refresh_button_label(true), "キャッシュ待機中...");
    assert_eq!(refresh_button_label(false), "キャッシュ待機して再取得");
    let notif_refreshing = status_notification(&refreshing);
    assert_eq!(notif_refreshing.0, NotificationKind::Info);
    assert_eq!(notif_refreshing.1, "キャッシュ待機中");
    assert!(notif_refreshing.2.contains("最大 30 秒"));

    let updated = AppStatus::RefreshUpdated {
        route_count: 2,
        stop_count: 18,
    };
    assert!(!updated.is_busy());
    assert!(!updated.is_refreshing());
    assert_eq!(
        status_notification(&updated),
        (
            NotificationKind::Success,
            "更新完了".to_string(),
            "駅・路線データの変化を検出し、2 路線 / 18 駅に更新しました".to_string(),
        )
    );

    let unchanged = AppStatus::RefreshUnchanged;
    assert!(!unchanged.is_busy());
    assert!(!unchanged.is_refreshing());
    let notif_unchanged = status_notification(&unchanged);
    assert_eq!(notif_unchanged.0, NotificationKind::Info);
    assert_eq!(notif_unchanged.1, "変化なし（キャッシュ未更新）");
}
