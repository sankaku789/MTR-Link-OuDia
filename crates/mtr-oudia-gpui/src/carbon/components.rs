use gpui::{
    App, ClickEvent, Div, ElementId, FontWeight, InteractiveElement, ParentElement, Rgba,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use super::tokens::{CarbonSpacing, CarbonTheme, CarbonThemeKind, CarbonTypography};

const TRANSPARENT: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// Carbon Design System v11 のボタン種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
}

/// [`resolve_button_style`] が返すボタンの解決済みスタイル色。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonStyle {
    pub bg: Rgba,
    pub hover_bg: Rgba,
    pub active_bg: Rgba,
    pub text: Rgba,
}

/// テーマ・ボタン種別・無効状態からボタンの配色を解決する。
pub fn resolve_button_style(theme: &CarbonTheme, kind: ButtonKind, disabled: bool) -> ButtonStyle {
    let c = &theme.colors;

    if disabled {
        return match kind {
            ButtonKind::Primary | ButtonKind::Secondary => ButtonStyle {
                bg: c.button_disabled,
                hover_bg: c.button_disabled,
                active_bg: c.button_disabled,
                text: c.text_on_color_disabled,
            },
            ButtonKind::Ghost => ButtonStyle {
                bg: TRANSPARENT,
                hover_bg: TRANSPARENT,
                active_bg: TRANSPARENT,
                text: c.text_disabled,
            },
        };
    }

    match kind {
        ButtonKind::Primary => ButtonStyle {
            bg: c.button_primary,
            hover_bg: c.button_primary_hover,
            active_bg: c.button_primary_active,
            text: c.text_on_color,
        },
        ButtonKind::Secondary => ButtonStyle {
            bg: c.button_secondary,
            hover_bg: c.button_secondary_hover,
            active_bg: c.button_secondary_active,
            text: c.text_on_color,
        },
        ButtonKind::Ghost => {
            // Carbon v11 の Ghost ボタンは透明背景に link-primary 文字色、
            // ホバー・アクティブ時は背景トーンを重ねる。
            let (active_bg, text) = match theme.kind {
                CarbonThemeKind::Gray100 => (c.layer_02, rgb(0x78a9ff)),
                CarbonThemeKind::Gray10 => (c.border_subtle, rgb(0x0f62fe)),
            };
            ButtonStyle {
                bg: TRANSPARENT,
                hover_bg: c.layer_hover_01,
                active_bg,
                text,
            }
        }
    }
}

/// Carbon Design System v11 ベースのボタン要素 (`md` 高さ 40px, 角丸なし) を生成する。
///
/// `disabled == true` の場合は `on_click` を登録せず、クリック抑止を保証する。
/// （PoC 用の簡略実装として、フォーカスリングやアイコン予約余白は省略している。）
pub fn carbon_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    disabled: bool,
    theme: &CarbonTheme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let style = resolve_button_style(theme, kind, disabled);

    let base = div()
        .id(id.into())
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .h(px(40.0))
        .px(CarbonSpacing::px(CarbonSpacing::SP_05))
        .rounded_none()
        .bg(style.bg)
        .text_color(style.text)
        .text_size(CarbonTypography::BODY_COMPACT_01.font_size_px())
        .line_height(CarbonTypography::BODY_COMPACT_01.line_height_px())
        .whitespace_nowrap()
        .child(label.into());

    if disabled {
        base.cursor_not_allowed()
    } else {
        base.cursor_pointer()
            .hover(move |s| s.bg(style.hover_bg))
            .active(move |s| s.bg(style.active_bg))
            .on_click(on_click)
    }
}

/// Carbon Design System v11 の Tile レイヤー種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileLayer {
    Layer01,
    Layer02,
}

/// Tile レイヤーに対応する背景色を解決する。
pub fn resolve_tile_bg(theme: &CarbonTheme, layer: TileLayer) -> Rgba {
    match layer {
        TileLayer::Layer01 => theme.colors.layer_01,
        TileLayer::Layer02 => theme.colors.layer_02,
    }
}

/// Carbon Design System v11 準拠のフラットなサーフェスコンテナ (`Tile`) を生成する。
pub fn carbon_tile(layer: TileLayer, theme: &CarbonTheme) -> Div {
    let bg = resolve_tile_bg(theme, layer);

    div()
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .rounded_none()
        .bg(bg)
        .border_1()
        .border_color(theme.colors.border_subtle)
        .p(CarbonSpacing::px(CarbonSpacing::SP_05))
        .text_color(theme.colors.text_primary)
}

/// Carbon Design System v11 の Tag 種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagKind {
    Blue,
    Green,
    Red,
    WarmGray,
    CoolGray,
}

/// [`resolve_tag_style`] が返す Tag の解決済みスタイル色。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TagStyle {
    pub bg: Rgba,
    pub text: Rgba,
}

/// Carbon v11 の Tag 専用配色（`support_*` とは独立した 20/70・70/20 トーン）を解決する。
pub fn resolve_tag_style(theme: &CarbonTheme, kind: TagKind) -> TagStyle {
    match (theme.kind, kind) {
        (CarbonThemeKind::Gray100, TagKind::Blue) => TagStyle {
            bg: rgb(0x0043ce),
            text: rgb(0xd0e2ff),
        },
        (CarbonThemeKind::Gray100, TagKind::Green) => TagStyle {
            bg: rgb(0x0e6027),
            text: rgb(0xa7f0ba),
        },
        (CarbonThemeKind::Gray100, TagKind::Red) => TagStyle {
            bg: rgb(0xa2191f),
            text: rgb(0xffd7d9),
        },
        (CarbonThemeKind::Gray100, TagKind::WarmGray) => TagStyle {
            bg: rgb(0x565151),
            text: rgb(0xe5e0df),
        },
        (CarbonThemeKind::Gray100, TagKind::CoolGray) => TagStyle {
            bg: rgb(0x4d5358),
            text: rgb(0xdde1e6),
        },
        (CarbonThemeKind::Gray10, TagKind::Blue) => TagStyle {
            bg: rgb(0xd0e2ff),
            text: rgb(0x0043ce),
        },
        (CarbonThemeKind::Gray10, TagKind::Green) => TagStyle {
            bg: rgb(0xa7f0ba),
            text: rgb(0x0e6027),
        },
        (CarbonThemeKind::Gray10, TagKind::Red) => TagStyle {
            bg: rgb(0xffd7d9),
            text: rgb(0xa2191f),
        },
        (CarbonThemeKind::Gray10, TagKind::WarmGray) => TagStyle {
            bg: rgb(0xe5e0df),
            text: rgb(0x3c3838),
        },
        (CarbonThemeKind::Gray10, TagKind::CoolGray) => TagStyle {
            bg: rgb(0xdde1e6),
            text: rgb(0x343a3f),
        },
    }
}

/// Carbon Design System v11 ベースのピル型 Tag 要素 (標準高さ 24px, 最小幅 32px) を生成する。
pub fn carbon_tag(label: impl Into<SharedString>, kind: TagKind, theme: &CarbonTheme) -> Div {
    let style = resolve_tag_style(theme, kind);

    div()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .h(px(24.0))
        .min_w(px(32.0))
        .rounded_full()
        .px(CarbonSpacing::px(CarbonSpacing::SP_03))
        .bg(style.bg)
        .text_color(style.text)
        .text_size(CarbonTypography::LABEL_01.font_size_px())
        .line_height(CarbonTypography::LABEL_01.line_height_px())
        .whitespace_nowrap()
        .child(label.into())
}

/// Carbon Design System v11 の InlineNotification 種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationKind {
    Info,
    Success,
    Warning,
    Error,
}

/// [`resolve_notification_style`] が返す通知の解決済みスタイル色。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotificationStyle {
    pub accent: Rgba,
    pub bg: Rgba,
    pub border: Rgba,
    pub title_text: Rgba,
    pub subtitle_text: Rgba,
}

/// テーマと通知種別から InlineNotification の配色を解決する。
pub fn resolve_notification_style(
    theme: &CarbonTheme,
    kind: NotificationKind,
) -> NotificationStyle {
    let c = &theme.colors;
    let accent = match kind {
        NotificationKind::Info => c.support_info,
        NotificationKind::Success => c.support_success,
        NotificationKind::Warning => c.support_warning,
        NotificationKind::Error => c.support_error,
    };

    NotificationStyle {
        accent,
        bg: c.layer_01,
        border: c.border_subtle,
        title_text: c.text_primary,
        subtitle_text: c.text_secondary,
    }
}

/// Carbon Design System v11 ベースの InlineNotification 要素 (最小高さ 48px) を生成する。
///
/// 左側に 3px の `support_*` アクセントボーダーを持ち、残りの 3 辺は `border_subtle` で囲む。
/// 色のみに依存しないよう種別インジケーターを先頭に表示する（PoC 用の簡略実装）。
pub fn carbon_inline_notification(
    kind: NotificationKind,
    title: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
    theme: &CarbonTheme,
) -> Div {
    let style = resolve_notification_style(theme, kind);
    let subtitle: SharedString = subtitle.into();
    let badge = match kind {
        NotificationKind::Info => "[i]",
        NotificationKind::Success => "[OK]",
        NotificationKind::Warning => "[!]",
        NotificationKind::Error => "[X]",
    };

    let mut content = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .justify_center()
        .min_h(px(48.0))
        .gap(CarbonSpacing::px(CarbonSpacing::SP_01))
        .px(CarbonSpacing::px(CarbonSpacing::SP_05))
        .py(CarbonSpacing::px(CarbonSpacing::SP_04))
        .border_t_1()
        .border_r_1()
        .border_b_1()
        .border_color(style.border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .min_w_0()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(FontWeight::BOLD)
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(style.accent)
                        .child(badge),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(CarbonTypography::HEADING_COMPACT_01.font_size_px())
                        .line_height(CarbonTypography::HEADING_COMPACT_01.line_height_px())
                        .text_color(style.title_text)
                        .child(title.into()),
                ),
        );

    if !subtitle.is_empty() {
        content = content.child(
            div()
                .w_full()
                .min_w_0()
                .text_size(CarbonTypography::BODY_COMPACT_01.font_size_px())
                .line_height(CarbonTypography::BODY_COMPACT_01.line_height_px())
                .text_color(style.subtitle_text)
                .child(subtitle),
        );
    }

    div()
        .flex()
        .flex_row()
        .w_full()
        .min_w_0()
        .rounded_none()
        .bg(style.bg)
        .border_l_3()
        .border_color(style.accent)
        .child(content)
}
