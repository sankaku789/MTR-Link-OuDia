use super::common::*;
use mtr_oudia_application::*;
use mtr_oudia_gpui::carbon::*;
use mtr_oudia_gpui::shell_state::*;
use std::path::{Path, PathBuf};

#[test]
fn apply_save_result_updates_state_and_preserves_consistency_on_error() {
    let mut state = MtrShellState::new(Ok(()));
    assert!(state.save_receipt.is_none());
    assert!(!state.is_step_completed(AppStep::Step5Save));

    // 初期状態での保存失敗時は save_receipt 未設定のままエラー状態になる。
    state.apply_save_result(Err(BusinessError {
        kind: BusinessErrorKind::SaveVerification,
        message: "安全な保存計画を作成できません".to_string(),
        detail: Some("hash mismatch".to_string()),
    }));
    assert!(state.save_receipt.is_none());
    assert!(!state.is_step_completed(AppStep::Step5Save));
    assert_eq!(
        state.status,
        AppStatus::SaveError {
            title: "安全な保存計画を作成できません".to_string(),
            detail: "hash mismatch".to_string(),
        }
    );

    // 保存成功時は save_receipt が確定し、Step5Save が完了になる。
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        4096,
        "abcdef0123456789",
    )));
    assert_eq!(
        state.save_receipt.as_ref().map(|r| r.output_path.as_str()),
        Some("/tmp/yamanote_converted.oud2")
    );
    assert_eq!(state.save_receipt.as_ref().map(|r| r.bytes), Some(4096));
    assert!(state.is_step_completed(AppStep::Step5Save));
    assert_eq!(
        state.status,
        AppStatus::SaveSuccess {
            output_path: "/tmp/yamanote_converted.oud2".to_string(),
            bytes: 4096,
        }
    );

    // 以降の保存失敗時はエラー状態になりつつ、既存の save_receipt と矛盾しない。
    state.apply_save_result(Err(BusinessError {
        kind: BusinessErrorKind::Io,
        message: "保存先へ書き込めません".to_string(),
        detail: None,
    }));
    assert_eq!(
        state.save_receipt.as_ref().map(|r| r.output_path.as_str()),
        Some("/tmp/yamanote_converted.oud2")
    );
    assert!(state.is_step_completed(AppStep::Step5Save));
    assert_eq!(
        state.status,
        AppStatus::SaveError {
            title: "保存先へ書き込めません".to_string(),
            detail: String::new(),
        }
    );
}

#[test]
fn upstream_updates_clear_downstream_step5_save_receipt() {
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
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        2048,
        "sha-1",
    )));
    assert!(state.save_receipt.is_some());
    assert!(state.is_step_completed(AppStep::Step5Save));

    // Step 4 プレビュー再生成で save_receipt がクリアされる。
    state.apply_preview_result(Ok(dummy_preview("preview-2", 30, false, false, Vec::new())));
    assert!(state.save_receipt.is_none());
    assert!(!state.is_step_completed(AppStep::Step5Save));

    // Step 3 別候補への変更で save_receipt がクリアされる。
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        2048,
        "sha-2",
    )));
    assert!(state.save_receipt.is_some());
    assert!(state.select_candidate(CandidateId("cand-2".to_string())));
    assert!(state.save_receipt.is_none());

    // Step 3 候補再検索で save_receipt がクリアされる。
    state.apply_preview_result(Ok(dummy_preview("preview-3", 20, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        2048,
        "sha-3",
    )));
    assert!(state.save_receipt.is_some());
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-3",
        0,
        2,
        "PrimaryExact",
        true,
        30,
    )]));
    assert!(state.save_receipt.is_none());

    // Step 2 OuDia 更新で save_receipt がクリアされる。
    state.apply_preview_result(Ok(dummy_preview("preview-4", 30, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/yamanote_converted.oud2",
        2048,
        "sha-4",
    )));
    assert!(state.save_receipt.is_some());
    state.apply_inspection_result(
        PathBuf::from("/tmp/chuo.oud2"),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("中央線"), 24)),
    );
    assert!(state.save_receipt.is_none());

    // Step 1 スナップショット変更で save_receipt がクリアされる。
    assert!(state.select_route("route-0".to_string()));
    state.apply_candidates_result(Ok(vec![dummy_candidate(
        "cand-4",
        0,
        0,
        "PrimaryExact",
        true,
        24,
    )]));
    state.apply_preview_result(Ok(dummy_preview("preview-5", 24, false, false, Vec::new())));
    state.apply_save_result(Ok(dummy_save_receipt(
        "/tmp/chuo_converted.oud2",
        2048,
        "sha-5",
    )));
    assert!(state.save_receipt.is_some());
    state.apply_snapshot_result(1, Ok(dummy_snapshot(vec![("京浜東北線", 35)])));
    assert!(state.save_receipt.is_none());
}

#[test]
fn suggest_save_location_generates_expected_directory_and_filename() {
    assert_eq!(
        suggest_save_location(Some(Path::new("/tmp/routes/sample.oud"))),
        (
            PathBuf::from("/tmp/routes"),
            Some("sample_converted.oud".to_string()),
        )
    );
    assert_eq!(
        suggest_save_location(Some(Path::new("/tmp/routes/sample.oud2"))),
        (
            PathBuf::from("/tmp/routes"),
            Some("sample_converted.oud2".to_string()),
        )
    );
    assert_eq!(
        suggest_save_location(Some(Path::new("/tmp/routes/sample"))),
        (
            PathBuf::from("/tmp/routes"),
            Some("sample_converted".to_string()),
        )
    );
    assert_eq!(
        suggest_save_location(Some(Path::new("relative.oud2"))),
        (
            PathBuf::from("."),
            Some("relative_converted.oud2".to_string()),
        )
    );
    assert_eq!(suggest_save_location(None), (PathBuf::from("."), None));
}

#[test]
fn status_notification_maps_step5_statuses() {
    let saving = AppStatus::Saving {
        path: "/tmp/out.oud2".to_string(),
    };
    assert!(saving.is_busy());
    assert!(saving.is_saving());
    assert_eq!(
        status_notification(&saving),
        (
            NotificationKind::Info,
            "保存中".to_string(),
            "/tmp/out.oud2 に検証付きで保存しています...".to_string(),
        )
    );

    let success = AppStatus::SaveSuccess {
        output_path: "/tmp/out.oud2".to_string(),
        bytes: 1234,
    };
    assert!(!success.is_busy());
    assert!(!success.is_saving());
    assert_eq!(
        status_notification(&success),
        (
            NotificationKind::Success,
            "保存完了".to_string(),
            "/tmp/out.oud2 に保存しました (1234 bytes)".to_string(),
        )
    );

    let err = AppStatus::SaveError {
        title: "安全な保存計画を作成できません".to_string(),
        detail: "expected hash mismatch".to_string(),
    };
    assert!(!err.is_busy());
    assert_eq!(
        status_notification(&err),
        (
            NotificationKind::Error,
            "安全な保存計画を作成できません".to_string(),
            "expected hash mismatch".to_string(),
        )
    );
}
