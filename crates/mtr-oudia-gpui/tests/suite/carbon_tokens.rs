use gpui::{Rgba, px, rgb};
use mtr_oudia_gpui::carbon::*;

const fn with_alpha(mut color: Rgba, alpha: f32) -> Rgba {
    color.a = alpha;
    color
}

#[test]
fn theme_kind_toggle_switches_between_gray100_and_gray10() {
    assert_eq!(CarbonThemeKind::Gray100.toggle(), CarbonThemeKind::Gray10);
    assert_eq!(CarbonThemeKind::Gray10.toggle(), CarbonThemeKind::Gray100);
}

#[test]
fn carbon_theme_default_is_gray_100() {
    let theme = CarbonTheme::default();
    assert_eq!(theme.kind, CarbonThemeKind::Gray100);
    assert_eq!(theme.colors, CarbonColors::gray_100());
}

#[test]
fn carbon_theme_from_kind_and_toggle_update_colors() {
    let mut theme = CarbonTheme::from_kind(CarbonThemeKind::Gray100);
    assert_eq!(theme.kind, CarbonThemeKind::Gray100);
    assert_eq!(theme.colors, CarbonColors::gray_100());

    theme.toggle();
    assert_eq!(theme.kind, CarbonThemeKind::Gray10);
    assert_eq!(theme.colors, CarbonColors::gray_10());

    theme.toggle();
    assert_eq!(theme.kind, CarbonThemeKind::Gray100);
    assert_eq!(theme.colors, CarbonColors::gray_100());
}

#[test]
fn gray_100_colors_match_carbon_v11_spec() {
    let c = CarbonColors::gray_100();
    assert_eq!(c.background, rgb(0x161616));
    assert_eq!(c.layer_01, rgb(0x262626));
    assert_eq!(c.layer_02, rgb(0x393939));
    assert_eq!(c.layer_hover_01, rgb(0x333333));
    assert_eq!(c.border_subtle, rgb(0x393939));
    assert_eq!(c.border_strong, rgb(0x6f6f6f));
    assert_eq!(c.text_primary, rgb(0xf4f4f4));
    assert_eq!(c.text_secondary, rgb(0xc6c6c6));
    assert_eq!(c.text_disabled, with_alpha(rgb(0xf4f4f4), 0.25));
    assert_eq!(c.text_disabled.a, 0.25);
    assert_eq!(c.text_on_color, rgb(0xffffff));
    assert_eq!(c.text_on_color_disabled, with_alpha(rgb(0xffffff), 0.25));
    assert_eq!(c.button_primary, rgb(0x0f62fe));
    assert_eq!(c.button_primary_hover, rgb(0x0353e9));
    assert_eq!(c.button_primary_active, rgb(0x002d9c));
    assert_eq!(c.button_secondary, rgb(0x6f6f6f));
    assert_eq!(c.button_secondary_hover, rgb(0x606060));
    assert_eq!(c.button_secondary_active, rgb(0x393939));
    assert_eq!(c.button_disabled, rgb(0x525252));
    assert_eq!(c.support_info, rgb(0x4589ff));
    assert_eq!(c.support_success, rgb(0x42be65));
    assert_eq!(c.support_warning, rgb(0xf1c21b));
    assert_eq!(c.support_error, rgb(0xfa4d56));
    assert_eq!(c.focus, rgb(0xffffff));
    assert_eq!(c.focus_inset, rgb(0x161616));
}

#[test]
fn gray_10_colors_match_carbon_v11_spec() {
    let c = CarbonColors::gray_10();
    assert_eq!(c.background, rgb(0xf4f4f4));
    assert_eq!(c.layer_01, rgb(0xffffff));
    assert_eq!(c.layer_02, rgb(0xf4f4f4));
    assert_eq!(c.layer_hover_01, rgb(0xe8e8e8));
    assert_eq!(c.border_subtle, rgb(0xe0e0e0));
    assert_eq!(c.border_strong, rgb(0x8d8d8d));
    assert_eq!(c.text_primary, rgb(0x161616));
    assert_eq!(c.text_secondary, rgb(0x525252));
    assert_eq!(c.text_disabled, with_alpha(rgb(0x161616), 0.25));
    assert_eq!(c.text_disabled.a, 0.25);
    assert_eq!(c.text_on_color, rgb(0xffffff));
    assert_eq!(c.text_on_color_disabled, rgb(0x8d8d8d));
    assert_eq!(c.button_primary, rgb(0x0f62fe));
    assert_eq!(c.button_primary_hover, rgb(0x0353e9));
    assert_eq!(c.button_primary_active, rgb(0x002d9c));
    assert_eq!(c.button_secondary, rgb(0x393939));
    assert_eq!(c.button_secondary_hover, rgb(0x4c4c4c));
    assert_eq!(c.button_secondary_active, rgb(0x6f6f6f));
    assert_eq!(c.button_disabled, rgb(0xc6c6c6));
    assert_eq!(c.support_info, rgb(0x0043ce));
    assert_eq!(c.support_success, rgb(0x198038));
    assert_eq!(c.support_warning, rgb(0xf1c21b));
    assert_eq!(c.support_error, rgb(0xda1e28));
    assert_eq!(c.focus, rgb(0x0f62fe));
    assert_eq!(c.focus_inset, rgb(0xffffff));
}

#[test]
fn spacing_constants_and_px_helper_match_spec() {
    assert_eq!(CarbonSpacing::SP_01, 2.0);
    assert_eq!(CarbonSpacing::SP_02, 4.0);
    assert_eq!(CarbonSpacing::SP_03, 8.0);
    assert_eq!(CarbonSpacing::SP_04, 12.0);
    assert_eq!(CarbonSpacing::SP_05, 16.0);
    assert_eq!(CarbonSpacing::SP_06, 24.0);
    assert_eq!(CarbonSpacing::SP_07, 32.0);

    assert_eq!(CarbonSpacing::px(CarbonSpacing::SP_01), px(2.0));
    assert_eq!(CarbonSpacing::px(CarbonSpacing::SP_03), px(8.0));
    assert_eq!(CarbonSpacing::px(CarbonSpacing::SP_07), px(32.0));
}

#[test]
fn typography_constants_and_px_helper_match_spec() {
    assert_eq!(CarbonTypography::LABEL_01, CarbonTypeStyle::new(12.0, 16.0));
    assert_eq!(
        CarbonTypography::BODY_COMPACT_01,
        CarbonTypeStyle::new(14.0, 18.0)
    );
    assert_eq!(CarbonTypography::BODY_01, CarbonTypeStyle::new(14.0, 20.0));
    assert_eq!(
        CarbonTypography::HEADING_COMPACT_01,
        CarbonTypeStyle::new(14.0, 18.0)
    );
    assert_eq!(
        CarbonTypography::HEADING_02,
        CarbonTypeStyle::new(16.0, 24.0)
    );
    assert_eq!(
        CarbonTypography::HEADING_03,
        CarbonTypeStyle::new(20.0, 28.0)
    );
    assert_eq!(
        CarbonTypography::HEADING_04,
        CarbonTypeStyle::new(28.0, 36.0)
    );

    assert_eq!(CarbonTypography::LABEL_01.font_size_px(), px(12.0));
    assert_eq!(CarbonTypography::LABEL_01.line_height_px(), px(16.0));
    assert_eq!(CarbonTypography::HEADING_04.font_size_px(), px(28.0));
    assert_eq!(CarbonTypography::px(14.0), px(14.0));
}
