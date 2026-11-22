use gpui::{Pixels, Rgba, px, rgb};

/// 不透明な `Rgba` に指定のアルファ値 (0.0..=1.0) を設定する。
const fn with_alpha(mut color: Rgba, alpha: f32) -> Rgba {
    color.a = alpha;
    color
}

/// Carbon Design System v11 のテーマ種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CarbonThemeKind {
    /// ダークテーマ (Carbon Gray 100)
    #[default]
    Gray100,
    /// ライトテーマ (Carbon Gray 10)
    Gray10,
}

impl CarbonThemeKind {
    /// Gray100 と Gray10 を相互に切り替える。
    pub const fn toggle(self) -> Self {
        match self {
            Self::Gray100 => Self::Gray10,
            Self::Gray10 => Self::Gray100,
        }
    }
}

/// Carbon Design System v11 (`@carbon/themes@11.0.0`) 準拠のカラートークン。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarbonColors {
    pub background: Rgba,
    pub layer_01: Rgba,
    pub layer_02: Rgba,
    pub layer_hover_01: Rgba,
    pub border_subtle: Rgba,
    pub border_strong: Rgba,
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_disabled: Rgba,
    pub text_on_color: Rgba,
    pub text_on_color_disabled: Rgba,
    pub button_primary: Rgba,
    pub button_primary_hover: Rgba,
    pub button_primary_active: Rgba,
    pub button_secondary: Rgba,
    pub button_secondary_hover: Rgba,
    pub button_secondary_active: Rgba,
    pub button_disabled: Rgba,
    pub support_info: Rgba,
    pub support_success: Rgba,
    pub support_warning: Rgba,
    pub support_error: Rgba,
    pub focus: Rgba,
    pub focus_inset: Rgba,
}

impl CarbonColors {
    /// テーマ種別に対応するカラートークンを生成する。
    pub fn from_kind(kind: CarbonThemeKind) -> Self {
        match kind {
            CarbonThemeKind::Gray100 => Self::gray_100(),
            CarbonThemeKind::Gray10 => Self::gray_10(),
        }
    }

    /// Carbon Gray 100 (ダーク) のカラートークンを生成する。
    pub fn gray_100() -> Self {
        Self {
            background: rgb(0x161616),
            layer_01: rgb(0x262626),
            layer_02: rgb(0x393939),
            layer_hover_01: rgb(0x333333),
            border_subtle: rgb(0x393939),
            border_strong: rgb(0x6f6f6f),
            text_primary: rgb(0xf4f4f4),
            text_secondary: rgb(0xc6c6c6),
            text_disabled: with_alpha(rgb(0xf4f4f4), 0.25),
            text_on_color: rgb(0xffffff),
            text_on_color_disabled: with_alpha(rgb(0xffffff), 0.25),
            button_primary: rgb(0x0f62fe),
            button_primary_hover: rgb(0x0353e9),
            button_primary_active: rgb(0x002d9c),
            button_secondary: rgb(0x6f6f6f),
            button_secondary_hover: rgb(0x606060),
            button_secondary_active: rgb(0x393939),
            button_disabled: rgb(0x525252),
            support_info: rgb(0x4589ff),
            support_success: rgb(0x42be65),
            support_warning: rgb(0xf1c21b),
            support_error: rgb(0xfa4d56),
            focus: rgb(0xffffff),
            focus_inset: rgb(0x161616),
        }
    }

    /// Carbon Gray 10 (ライト) のカラートークンを生成する。
    pub fn gray_10() -> Self {
        Self {
            background: rgb(0xf4f4f4),
            layer_01: rgb(0xffffff),
            layer_02: rgb(0xf4f4f4),
            layer_hover_01: rgb(0xe8e8e8),
            border_subtle: rgb(0xe0e0e0),
            border_strong: rgb(0x8d8d8d),
            text_primary: rgb(0x161616),
            text_secondary: rgb(0x525252),
            text_disabled: with_alpha(rgb(0x161616), 0.25),
            text_on_color: rgb(0xffffff),
            text_on_color_disabled: rgb(0x8d8d8d),
            button_primary: rgb(0x0f62fe),
            button_primary_hover: rgb(0x0353e9),
            button_primary_active: rgb(0x002d9c),
            button_secondary: rgb(0x393939),
            button_secondary_hover: rgb(0x4c4c4c),
            button_secondary_active: rgb(0x6f6f6f),
            button_disabled: rgb(0xc6c6c6),
            support_info: rgb(0x0043ce),
            support_success: rgb(0x198038),
            support_warning: rgb(0xf1c21b),
            support_error: rgb(0xda1e28),
            focus: rgb(0x0f62fe),
            focus_inset: rgb(0xffffff),
        }
    }
}

/// Carbon Design System v11 のスペーシングスケール (8px ベース)。
pub struct CarbonSpacing;

impl CarbonSpacing {
    pub const SP_01: f32 = 2.0;
    pub const SP_02: f32 = 4.0;
    pub const SP_03: f32 = 8.0;
    pub const SP_04: f32 = 12.0;
    pub const SP_05: f32 = 16.0;
    pub const SP_06: f32 = 24.0;
    pub const SP_07: f32 = 32.0;

    /// f32 のスペーシング値を GPUI の `Pixels` に変換する。
    pub const fn px(value: f32) -> Pixels {
        px(value)
    }
}

/// Carbon Design System v11 のタイポグラフィスタイル (フォントサイズ・行高のサブセット)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarbonTypeStyle {
    pub font_size: f32,
    pub line_height: f32,
}

impl CarbonTypeStyle {
    pub const fn new(font_size: f32, line_height: f32) -> Self {
        Self {
            font_size,
            line_height,
        }
    }

    pub const fn font_size_px(self) -> Pixels {
        px(self.font_size)
    }

    pub const fn line_height_px(self) -> Pixels {
        px(self.line_height)
    }
}

/// Carbon Design System v11 のタイポグラフィスケール (サイズ・行高サブセット)。
pub struct CarbonTypography;

impl CarbonTypography {
    pub const LABEL_01: CarbonTypeStyle = CarbonTypeStyle::new(12.0, 16.0);
    pub const BODY_COMPACT_01: CarbonTypeStyle = CarbonTypeStyle::new(14.0, 18.0);
    pub const BODY_01: CarbonTypeStyle = CarbonTypeStyle::new(14.0, 20.0);
    pub const HEADING_COMPACT_01: CarbonTypeStyle = CarbonTypeStyle::new(14.0, 18.0);
    pub const HEADING_02: CarbonTypeStyle = CarbonTypeStyle::new(16.0, 24.0);
    pub const HEADING_03: CarbonTypeStyle = CarbonTypeStyle::new(20.0, 28.0);
    pub const HEADING_04: CarbonTypeStyle = CarbonTypeStyle::new(28.0, 36.0);

    /// f32 のフォントサイズ値を GPUI の `Pixels` に変換する。
    pub const fn px(value: f32) -> Pixels {
        px(value)
    }
}

/// Carbon Design System v11 のテーマ全体。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarbonTheme {
    pub kind: CarbonThemeKind,
    pub colors: CarbonColors,
}

impl Default for CarbonTheme {
    fn default() -> Self {
        Self::from_kind(CarbonThemeKind::default())
    }
}

impl CarbonTheme {
    /// 指定された `CarbonThemeKind` からテーマを生成する。
    pub fn from_kind(kind: CarbonThemeKind) -> Self {
        Self {
            kind,
            colors: CarbonColors::from_kind(kind),
        }
    }

    /// 現在のテーマを Gray100 / Gray10 間で切り替える。
    pub fn toggle(&mut self) {
        *self = Self::from_kind(self.kind.toggle());
    }
}
