use super::common::*;
use mtr_oudia_application::*;
use mtr_oudia_gpui::carbon::*;
use mtr_oudia_gpui::shell_state::*;
use std::path::PathBuf;

#[test]
fn apply_inspection_result_updates_state_and_preserves_consistency_on_error() {
    let mut state = MtrShellState::new(Ok(()));
    assert!(state.oudia_path.is_none());
    assert!(state.inspection.is_none());
    assert!(!state.is_step_completed(AppStep::Step2Oudia));

    // 初期状態での失敗時は oudia_path / inspection ともに未設定のまま。
    state.apply_inspection_result(
        PathBuf::from("/tmp/broken.oud2"),
        Err(BusinessError {
            kind: BusinessErrorKind::Io,
            message: "OuDia ファイルを開けません".to_string(),
            detail: Some("No such file".to_string()),
        }),
    );
    assert!(state.oudia_path.is_none());
    assert!(state.inspection.is_none());
    assert!(!state.is_step_completed(AppStep::Step2Oudia));
    assert_eq!(
        state.status,
        AppStatus::InspectError {
            title: "OuDia ファイルを開けません".to_string(),
            detail: "No such file".to_string(),
        }
    );

    // 成功時に oudia_path・inspection が確定し、Step2Oudia が完了になる。
    let first_path = PathBuf::from("/tmp/yamanote.oud2");
    state.apply_inspection_result(
        first_path.clone(),
        Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 30)),
    );
    assert_eq!(state.oudia_path.as_ref(), Some(&first_path));
    assert_eq!(
        state
            .inspection
            .as_ref()
            .and_then(|dto| dto.line_name.as_deref()),
        Some("山手線")
    );
    assert!(state.is_step_completed(AppStep::Step2Oudia));
    assert_eq!(
        state.status,
        AppStatus::InspectSuccess {
            file_type: "OuDiaSecond.1.04".to_string(),
            station_count: 30,
            diagram_count: 2,
        }
    );

    // 別ファイルでの失敗時はエラー状態になりつつ、既存の oudia_path / inspection と矛盾しない。
    state.apply_inspection_result(
        PathBuf::from("/tmp/invalid.oud2"),
        Err(BusinessError {
            kind: BusinessErrorKind::ParseUnsupported,
            message: "OuDia ファイルの解析に失敗しました".to_string(),
            detail: None,
        }),
    );
    assert_eq!(state.oudia_path.as_ref(), Some(&first_path));
    assert_eq!(
        state
            .inspection
            .as_ref()
            .and_then(|dto| dto.line_name.as_deref()),
        Some("山手線")
    );
    assert!(state.is_step_completed(AppStep::Step2Oudia));
    assert_eq!(
        state.status,
        AppStatus::InspectError {
            title: "OuDia ファイルの解析に失敗しました".to_string(),
            detail: String::new(),
        }
    );
}

#[test]
fn status_notification_maps_step2_statuses() {
    let inspecting = AppStatus::InspectingOudia {
        path: "/tmp/line.oud2".to_string(),
    };
    assert!(inspecting.is_busy());
    assert!(inspecting.is_inspecting());
    assert_eq!(
        status_notification(&inspecting),
        (
            NotificationKind::Info,
            "解析中".to_string(),
            "/tmp/line.oud2 を読み込んでいます...".to_string(),
        )
    );

    let success = AppStatus::InspectSuccess {
        file_type: "OuDiaSecond.1.04".to_string(),
        station_count: 24,
        diagram_count: 2,
    };
    assert!(!success.is_busy());
    assert_eq!(
        status_notification(&success),
        (
            NotificationKind::Success,
            "解析完了".to_string(),
            "OuDiaSecond.1.04: 24 駅 / 2 ダイヤを読み込みました".to_string(),
        )
    );

    let err = AppStatus::InspectError {
        title: "OuDia ファイルの解析に失敗しました".to_string(),
        detail: "Rosenmei が見つかりません".to_string(),
    };
    assert!(!err.is_busy());
    assert_eq!(
        status_notification(&err),
        (
            NotificationKind::Error,
            "OuDia ファイルの解析に失敗しました".to_string(),
            "Rosenmei が見つかりません".to_string(),
        )
    );
}
