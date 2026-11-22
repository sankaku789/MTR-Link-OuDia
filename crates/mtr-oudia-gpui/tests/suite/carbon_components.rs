use gpui::rgb;
use mtr_oudia_gpui::carbon::*;

const ALL_BUTTON_KINDS: [ButtonKind; 3] = [
    ButtonKind::Primary,
    ButtonKind::Secondary,
    ButtonKind::Ghost,
];
const ALL_TAG_KINDS: [TagKind; 5] = [
    TagKind::Blue,
    TagKind::Green,
    TagKind::Red,
    TagKind::WarmGray,
    TagKind::CoolGray,
];
const ALL_NOTIFICATION_KINDS: [NotificationKind; 4] = [
    NotificationKind::Info,
    NotificationKind::Success,
    NotificationKind::Warning,
    NotificationKind::Error,
];
const ALL_THEME_KINDS: [CarbonThemeKind; 2] = [CarbonThemeKind::Gray100, CarbonThemeKind::Gray10];

#[test]
fn button_style_resolves_all_kinds_states_and_themes() {
    for theme_kind in ALL_THEME_KINDS {
        let theme = CarbonTheme::from_kind(theme_kind);
        let c = &theme.colors;

        let primary_enabled = resolve_button_style(&theme, ButtonKind::Primary, false);
        assert_eq!(primary_enabled.bg, c.button_primary);
        assert_eq!(primary_enabled.hover_bg, c.button_primary_hover);
        assert_eq!(primary_enabled.active_bg, c.button_primary_active);
        assert_eq!(primary_enabled.text, c.text_on_color);
        assert_ne!(primary_enabled.bg, primary_enabled.text);

        let secondary_enabled = resolve_button_style(&theme, ButtonKind::Secondary, false);
        assert_eq!(secondary_enabled.bg, c.button_secondary);
        assert_eq!(secondary_enabled.hover_bg, c.button_secondary_hover);
        assert_eq!(secondary_enabled.active_bg, c.button_secondary_active);
        assert_eq!(secondary_enabled.text, c.text_on_color);
        assert_ne!(secondary_enabled.bg, secondary_enabled.text);

        let ghost_enabled = resolve_button_style(&theme, ButtonKind::Ghost, false);
        assert_eq!(ghost_enabled.bg.a, 0.0);
        assert_eq!(ghost_enabled.hover_bg, c.layer_hover_01);
        assert_ne!(ghost_enabled.bg, ghost_enabled.text);
        assert_ne!(ghost_enabled.hover_bg, ghost_enabled.text);
        assert_ne!(ghost_enabled.active_bg, ghost_enabled.text);

        for kind in ALL_BUTTON_KINDS {
            let disabled_style = resolve_button_style(&theme, kind, true);
            assert_ne!(
                disabled_style.bg, disabled_style.text,
                "disabled button bg and text must differ for {kind:?} in {theme_kind:?}"
            );
            assert_eq!(disabled_style.bg, disabled_style.hover_bg);
            assert_eq!(disabled_style.bg, disabled_style.active_bg);

            match kind {
                ButtonKind::Primary | ButtonKind::Secondary => {
                    assert_eq!(disabled_style.bg, c.button_disabled);
                    assert_eq!(disabled_style.text, c.text_on_color_disabled);
                }
                ButtonKind::Ghost => {
                    assert_eq!(disabled_style.bg.a, 0.0);
                    assert_eq!(disabled_style.text, c.text_disabled);
                }
            }
        }
    }
}

#[test]
fn tile_bg_resolves_layer_01_and_layer_02() {
    for theme_kind in ALL_THEME_KINDS {
        let theme = CarbonTheme::from_kind(theme_kind);
        assert_eq!(
            resolve_tile_bg(&theme, TileLayer::Layer01),
            theme.colors.layer_01
        );
        assert_eq!(
            resolve_tile_bg(&theme, TileLayer::Layer02),
            theme.colors.layer_02
        );
        assert_ne!(
            resolve_tile_bg(&theme, TileLayer::Layer01),
            resolve_tile_bg(&theme, TileLayer::Layer02)
        );
    }
}

#[test]
fn tag_style_has_distinct_bg_and_text_without_reusing_support_tokens() {
    for theme_kind in ALL_THEME_KINDS {
        let theme = CarbonTheme::from_kind(theme_kind);
        let c = &theme.colors;
        let support_colors = [
            c.support_info,
            c.support_success,
            c.support_warning,
            c.support_error,
        ];

        for kind in ALL_TAG_KINDS {
            let style = resolve_tag_style(&theme, kind);
            assert_ne!(
                style.bg, style.text,
                "tag bg and text must differ for {kind:?} in {theme_kind:?}"
            );
            assert!(
                !support_colors.contains(&style.bg),
                "tag bg must not reuse support_* tokens for {kind:?} in {theme_kind:?}"
            );
        }
    }

    let dark = CarbonTheme::from_kind(CarbonThemeKind::Gray100);
    assert_eq!(
        resolve_tag_style(&dark, TagKind::WarmGray).text,
        rgb(0xe5e0df)
    );
    assert_eq!(
        resolve_tag_style(&dark, TagKind::CoolGray).text,
        rgb(0xdde1e6)
    );
}

#[test]
fn notification_style_resolves_support_accents_and_surface_colors() {
    for theme_kind in ALL_THEME_KINDS {
        let theme = CarbonTheme::from_kind(theme_kind);
        let c = &theme.colors;

        for kind in ALL_NOTIFICATION_KINDS {
            let style = resolve_notification_style(&theme, kind);
            let expected_accent = match kind {
                NotificationKind::Info => c.support_info,
                NotificationKind::Success => c.support_success,
                NotificationKind::Warning => c.support_warning,
                NotificationKind::Error => c.support_error,
            };

            assert_eq!(style.accent, expected_accent);
            assert_eq!(style.bg, c.layer_01);
            assert_eq!(style.border, c.border_subtle);
            assert_eq!(style.title_text, c.text_primary);
            assert_eq!(style.subtitle_text, c.text_secondary);
            assert_ne!(style.bg, style.title_text);
            assert_ne!(style.bg, style.subtitle_text);
        }
    }
}
