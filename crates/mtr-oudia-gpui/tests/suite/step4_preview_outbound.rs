use super::common::*;
use mtr_oudia_application::*;
use mtr_oudia_domain::OperationPolicy;
use mtr_oudia_gpui::carbon::*;
use mtr_oudia_gpui::shell_state::*;
use std::path::PathBuf;

#[test]
fn apply_preview_result_updates_state_and_preserves_consistency_on_error() {
    let mut state = MtrShellState::new(Ok(()));
    assert!(state.preview.is_none());
    assert!(!state.is_step_completed(AppStep::Step4Preview));

    // 初期状態での失敗時は preview 未設定のままエラー状態になる。
    state.apply_preview_result(Err(BusinessError {
        kind: BusinessErrorKind::StaleState,
        message: "候補が現在のセッションにありません".to_string(),
        detail: None,
    }));
    assert!(state.preview.is_none());
    assert!(!state.is_step_completed(AppStep::Step4Preview));
    assert_eq!(
        state.status,
        AppStatus::PreviewError {
            title: "候補が現在のセッションにありません".to_string(),
            detail: String::new(),
        }
    );

    // 成功時は preview が確定し、Step4Preview が完了になる。
    state.apply_preview_result(Ok(dummy_preview("preview-1", 4, false, false, Vec::new())));
    assert_eq!(
        state.preview.as_ref().map(|dto| dto.id.0.as_str()),
        Some("preview-1")
    );
    assert!(state.is_step_completed(AppStep::Step4Preview));
    assert_eq!(
        state.status,
        AppStatus::PreviewSuccess {
            stop_count: 4,
            warning_count: 0,
        }
    );

    // 以降の生成失敗時もエラー状態になりつつ、既存の preview と矛盾しない。
    state.apply_preview_result(Err(BusinessError {
        kind: BusinessErrorKind::Validation,
        message: "時刻表を生成できません".to_string(),
        detail: Some("station mismatch".to_string()),
    }));
    assert_eq!(
        state.preview.as_ref().map(|dto| dto.id.0.as_str()),
        Some("preview-1")
    );
    assert!(state.is_step_completed(AppStep::Step4Preview));
    assert_eq!(
        state.status,
        AppStatus::PreviewError {
            title: "時刻表を生成できません".to_string(),
            detail: "station mismatch".to_string(),
        }
    );
}

#[test]
fn upstream_updates_clear_downstream_step4_preview() {
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
    state.apply_candidates_result(Ok(vec![
        dummy_candidate("cand-1", 0, 0, "PrimaryExact", true, 30),
        dummy_candidate("cand-2", 0, 1, "Partial", false, 20),
    ]));
    state.apply_preview_result(Ok(dummy_preview("preview-1", 30, false, false, Vec::new())));
    assert!(state.preview.is_some());
    assert!(state.is_step_completed(AppStep::Step4Preview));

    // Step 3 同一候補の再選択では preview を保持する。
    assert!(state.select_candidate(CandidateId("cand-1".to_string())));
    assert!(state.preview.is_some());

    // Step 3 別候補への変更で preview がクリアされる。
    assert!(state.select_candidate(CandidateId("cand-2".to_string())));
    assert!(state.preview.is_none());
    assert!(!state.is_step_completed(AppStep::Step4Preview));

    // Step 3 候補再検索で preview がクリアされる。
    state.apply_preview_result(Ok(dummy_preview("preview-2", 30, false, false, Vec::new())));
    assert!(state.preview.is_some());
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-3",
        0,
        2,
        "PrimaryExact",
        true,
        30,
    )]));
    assert!(state.preview.is_none());

    // Step 2 OuDia 更新で preview がクリアされる。
    state.apply_preview_result(Ok(dummy_preview("preview-3", 30, false, false, Vec::new())));
    assert!(state.preview.is_some());
    state.apply_inspection_result(
        PathBuf::from("/tmp/chuo.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("中央線"), 24)),
    );
    assert!(state.preview.is_none());

    // Step 1 スナップショット変更で preview がクリアされる。
    assert!(state.select_route("route-0".to_string()));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-4",
        0,
        0,
        "PrimaryExact",
        true,
        24,
    )]));
    state.apply_preview_result(Ok(dummy_preview("preview-4", 24, false, false, Vec::new())));
    assert!(state.preview.is_some());
    state.apply_snapshot_result(1, Ok(dummy_snapshot(vec![("京浜東北線", 35)])));
    assert!(state.preview.is_none());

    // Step 1 検出成功で preview がクリアされる。
    assert!(state.select_route("route-0".to_string()));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-5",
        0,
        0,
        "PrimaryExact",
        true,
        35,
    )]));
    state.apply_preview_result(Ok(dummy_preview("preview-5", 35, false, false, Vec::new())));
    assert!(state.preview.is_some());
    state.apply_detection_result(Ok(vec![EndpointDto {
        url: "http://127.0.0.1:49182/".to_string(),
    }]));
    assert!(state.preview.is_none());
}

#[test]
fn status_notification_maps_step4_statuses() {
    let previewing = AppStatus::Previewing;
    assert!(previewing.is_busy());
    assert!(previewing.is_previewing());
    assert_eq!(status_notification(&previewing).0, NotificationKind::Info);

    let success = AppStatus::PreviewSuccess {
        stop_count: 10,
        warning_count: 0,
    };
    assert!(!success.is_busy());
    assert_eq!(
        status_notification(&success),
        (
            NotificationKind::Success,
            "プレビュー生成完了".to_string(),
            "10 駅の時刻表プレビューを生成しました".to_string(),
        )
    );

    let warned = AppStatus::PreviewSuccess {
        stop_count: 10,
        warning_count: 1,
    };
    assert!(!warned.is_busy());
    assert_eq!(
        status_notification(&warned),
        (
            NotificationKind::Warning,
            "プレビュー生成完了（警告あり）".to_string(),
            "10 駅の時刻表を生成しましたが、1 件の警告があります".to_string(),
        )
    );

    let err = AppStatus::PreviewError {
        title: "時刻表を生成できません".to_string(),
        detail: "candidate expired".to_string(),
    };
    assert!(!err.is_busy());
    assert_eq!(
        status_notification(&err),
        (
            NotificationKind::Error,
            "時刻表を生成できません".to_string(),
            "candidate expired".to_string(),
        )
    );
}

#[test]
fn preview_formatting_helpers_format_times_and_dwell_seconds() {
    assert_eq!(
        format_arrival_departure(Some("10:02:00"), Some("10:02:30")),
        "10:02:00 / 10:02:30"
    );
    assert_eq!(
        format_arrival_departure(None, Some("10:00:00")),
        "- / 10:00:00"
    );
    assert_eq!(
        format_arrival_departure(Some("10:05:00"), None),
        "10:05:00 / -"
    );
    assert_eq!(format_arrival_departure(None, None), "-");

    assert_eq!(format_dwell_seconds(0), "0 秒");
    assert_eq!(format_dwell_seconds(30_000), "30 秒");
    assert_eq!(format_dwell_seconds(15_500), "15.5 秒");
    assert_eq!(format_dwell_seconds(12_340), "12.34 秒");
}

#[test]
fn operation_policy_and_generate_outbound_toggle_clear_downstream() {
    let mut state = MtrShellState::new(Ok(()));
    assert_eq!(state.operation_policy, OperationPolicy::Preserve);
    assert!(!state.generate_outbound);

    state.apply_preview_result(Ok(dummy_preview("preview-1", 3, false, true, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt("/tmp/out.oud2", 1024, "sha-initial")));

    // 同一 OperationPolicy の再選択は no-op として下流を保持する。
    assert!(!state.set_operation_policy(OperationPolicy::Preserve));
    assert!(state.preview.is_some());
    assert!(state.save_receipt.is_some());

    // OperationPolicy を切り替えると Step 4 プレビューと Step 5 save_receipt がクリアされる。
    assert!(state.set_operation_policy(OperationPolicy::RemoveTargetTrain));
    assert_eq!(state.operation_policy, OperationPolicy::RemoveTargetTrain);
    assert!(state.preview.is_none());
    assert!(state.save_receipt.is_none());

    // 再度プレビュー・保存済み状態を作り、generate_outbound のトグルを検証する。
    state.apply_preview_result(Ok(dummy_preview("preview-2", 3, false, true, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt("/tmp/out.oud2", 1024, "sha-2")));

    // 同一値の指定は no-op。
    assert!(!state.set_generate_outbound(false));
    assert!(state.preview.is_some());
    assert!(state.save_receipt.is_some());

    // generate_outbound を ON にすると下流がクリアされる。
    assert!(state.toggle_generate_outbound());
    assert!(state.generate_outbound);
    assert!(state.preview.is_none());
    assert!(state.save_receipt.is_none());

    // 再びプレビュー・保存済み状態を作って OFF にしても下流がクリアされる。
    state.apply_preview_result(Ok(dummy_preview("preview-3", 3, false, true, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt("/tmp/out.oud2", 1024, "sha-3")));
    assert!(state.toggle_generate_outbound());
    assert!(!state.generate_outbound);
    assert!(state.preview.is_none());
    assert!(state.save_receipt.is_none());
}

#[test]
fn apply_outbound_result_updates_state_clears_downstream_and_maps_notifications() {
    let mut state = MtrShellState::new(Ok(()));
    state.apply_preview_result(Ok(dummy_preview("preview-1", 3, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt("/tmp/out.oud2", 512, "sha-1")));
    assert!(state.set_generate_outbound(true));
    state.apply_preview_result(Ok(dummy_preview("preview-2", 3, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt("/tmp/out.oud2", 512, "sha-2")));

    // 1. 計測中の通知変換と busy 判定
    let measuring = AppStatus::MeasuringOutbound;
    assert!(measuring.is_busy());
    assert!(measuring.is_measuring_outbound());
    let notif_measuring = status_notification(&measuring);
    assert_eq!(notif_measuring.0, NotificationKind::Info);
    assert_eq!(notif_measuring.1, "出庫時分を計測中");

    // 2. 有効な出庫計測結果の適用で preview / save_receipt がクリアされ、Success 通知になる。
    state.apply_outbound_result(
        Ok(dummy_outbound_status(true, OutboundRuntimeSource::Measured)),
        false,
    );
    assert!(state.outbound_status.as_ref().is_some_and(|s| s.valid));
    assert!(state.preview.is_none());
    assert!(state.save_receipt.is_none());
    assert!(state.generate_outbound);
    assert!(!state.status.is_busy());
    let notif_measured = status_notification(&state.status);
    assert_eq!(notif_measured.0, NotificationKind::Success);
    assert_eq!(notif_measured.1, "出庫時分を計測しました");
    assert!(notif_measured.2.contains("15秒（0分15秒）"));

    // 3. invalid な結果（構成不一致など）の適用時は generate_outbound を自動で OFF にし Warning 通知になる。
    state.apply_outbound_result(
        Ok(dummy_outbound_status(false, OutboundRuntimeSource::Manual)),
        true,
    );
    assert!(!state.generate_outbound);
    let notif_warning = status_notification(&state.status);
    assert_eq!(notif_warning.0, NotificationKind::Warning);
    assert_eq!(notif_warning.1, "出庫時分を保存しました（要確認）");

    // 4. 計測エラー時は既存の outbound_status を維持しつつ Error 通知になる。
    state.apply_outbound_result(
        Err(BusinessError {
            kind: BusinessErrorKind::Validation,
            message: "車庫発はHH:mm:ssで入力してください".to_string(),
            detail: Some("bad clock".to_string()),
        }),
        false,
    );
    assert!(state.outbound_status.is_some());
    assert_eq!(
        status_notification(&state.status),
        (
            NotificationKind::Error,
            "車庫発はHH:mm:ssで入力してください".to_string(),
            "bad clock".to_string(),
        )
    );
}

#[test]
fn precision_train_number_and_policy_helpers_work() {
    assert_eq!(
        operation_policy_from_choice("preserve"),
        Some(OperationPolicy::Preserve)
    );
    assert_eq!(
        operation_policy_from_choice("remove_target_train"),
        Some(OperationPolicy::RemoveTargetTrain)
    );
    assert_eq!(operation_policy_from_choice("unknown"), None);
    assert_eq!(
        operation_policy_label(OperationPolicy::Preserve),
        "元のまま保持 (Preserve)"
    );
    assert_eq!(
        operation_policy_label(OperationPolicy::RemoveTargetTrain),
        "対象列車から削除 (RemoveTargetTrain)"
    );

    assert_eq!(parse_manual_outbound_seconds("13.5").unwrap(), 13.5);
    assert_eq!(parse_manual_outbound_seconds(" 0 ").unwrap(), 0.0);
    assert!(parse_manual_outbound_seconds("").is_err());
    assert!(parse_manual_outbound_seconds("-1").is_err());
    assert!(parse_manual_outbound_seconds("NaN").is_err());
    assert!(parse_manual_outbound_seconds("abc").is_err());

    let mut state = MtrShellState::new(Ok(()));
    state.apply_candidates_result_with_train_number(
        Some("001M".to_string()),
        Ok(vec![dummy_candidate(
            "cand-1",
            0,
            0,
            "PrimaryExact",
            true,
            2,
        )]),
    );
    assert_eq!(state.searched_train_number.as_deref(), Some("001M"));
    state.clear_candidate_selection();
    assert!(state.searched_train_number.is_none());
}
