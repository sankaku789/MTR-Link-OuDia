use gpui::WindowAppearance;
use mtr_oudia_gpui::carbon::*;
use mtr_oudia_gpui::shell_state::*;

#[test]
fn app_step_metadata_covers_all_five_steps() {
    assert_eq!(AppStep::ALL.len(), 5);
    let expected = [
        (AppStep::Step1Mtr, 1, "MTR 接続"),
        (AppStep::Step2Oudia, 2, "OuDia 選択"),
        (AppStep::Step3RouteMatch, 3, "路線・経路照合"),
        (AppStep::Step4Preview, 4, "プレビュー"),
        (AppStep::Step5Save, 5, "保存"),
    ];
    for (step, number, label) in expected {
        assert_eq!(step.number(), number);
        assert_eq!(step.label(), label);
        assert!(!step.description().is_empty());
    }
}

#[test]
fn app_step_index_round_trips_for_all_steps() {
    for (expected_idx, step) in AppStep::ALL.into_iter().enumerate() {
        assert_eq!(step.index(), expected_idx);
        assert_eq!(AppStep::from_index(expected_idx), Some(step));
    }
    assert_eq!(AppStep::from_index(5), None);
}

#[test]
fn step_for_scroll_position_tracks_top_item_and_bottom_clamp() {
    assert_eq!(step_for_scroll_position(0, 0.0, 500.0), AppStep::Step1Mtr);
    assert_eq!(
        step_for_scroll_position(2, -240.0, 500.0),
        AppStep::Step3RouteMatch
    );
    assert_eq!(
        step_for_scroll_position(3, -498.0, 500.0),
        AppStep::Step5Save
    );
    // max_offset_height が 0（スクロール不要）のときは top_item を尊重する。
    assert_eq!(step_for_scroll_position(1, 0.0, 0.0), AppStep::Step2Oudia);
    assert_eq!(step_for_scroll_position(99, 0.0, 500.0), AppStep::Step1Mtr);
}

#[test]
fn theme_preference_supports_dark_mode_toggle_and_os_sync() {
    assert!(is_dark_appearance(WindowAppearance::Dark));
    assert!(is_dark_appearance(WindowAppearance::VibrantDark));
    assert!(!is_dark_appearance(WindowAppearance::Light));
    assert!(!is_dark_appearance(WindowAppearance::VibrantLight));

    let mut state = MtrShellState::new(Ok(()));
    assert_eq!(state.theme_preference, ThemePreference::System);

    // OS に合わせるモードでは OS 側の変更に追従する。
    state.set_os_dark(false);
    assert_eq!(state.theme.kind, CarbonThemeKind::Gray10);
    assert_eq!(
        theme_badge_label(state.theme_preference, state.theme.kind),
        "OS 連動・ライト"
    );
    assert_eq!(
        dark_mode_button_label(state.theme.kind),
        "ダークモード: OFF"
    );

    state.set_os_dark(true);
    assert_eq!(state.theme.kind, CarbonThemeKind::Gray100);
    assert_eq!(
        theme_badge_label(state.theme_preference, state.theme.kind),
        "OS 連動・ダーク"
    );
    assert_eq!(dark_mode_button_label(state.theme.kind), "ダークモード: ON");

    // ダークモード ON/OFF を押すと手動モードに切り替わり、OS 変更に上書きされない。
    state.toggle_dark_mode();
    assert_eq!(state.theme_preference, ThemePreference::Light);
    assert_eq!(state.theme.kind, CarbonThemeKind::Gray10);
    assert_eq!(
        theme_badge_label(state.theme_preference, state.theme.kind),
        "ライト"
    );

    state.set_os_dark(true);
    assert_eq!(state.theme.kind, CarbonThemeKind::Gray10);

    state.toggle_dark_mode();
    assert_eq!(state.theme_preference, ThemePreference::Dark);
    assert_eq!(state.theme.kind, CarbonThemeKind::Gray100);
    assert_eq!(
        theme_badge_label(state.theme_preference, state.theme.kind),
        "ダーク"
    );

    // 「OS に合わせる」に戻すと現在の OS 設定を反映する。
    state.set_os_dark(false);
    state.set_theme_preference(ThemePreference::System);
    assert_eq!(state.theme.kind, CarbonThemeKind::Gray10);
}
