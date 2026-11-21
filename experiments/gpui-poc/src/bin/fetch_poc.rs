//! 既存の reqwest ベース infrastructure を GPUI から呼べるかの最小検証。
//!
//! - 接続先: 環境変数 `MTR_ENDPOINT`（例: `http://127.0.0.1:49182`）
//! - dimension: 環境変数 `MTR_DIMENSION`（未設定なら 0）
//!
//! reqwest は tokio のリアクタを必要とするため、専用スレッドで current-thread
//! ランタイムを回し、結果を oneshot で GPUI の executor に返す。

use futures::channel::oneshot;
use gpui::{
    App, Application, Bounds, ClickEvent, Context, Div, FontWeight, IntoElement, ParentElement,
    Render, SharedString, Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_poc::carbon::{
    ButtonKind, CarbonSpacing, CarbonTheme, CarbonThemeKind, CarbonTypography, NotificationKind,
    TagKind, TileLayer, carbon_button, carbon_inline_notification, carbon_tag, carbon_tile,
};
use mtr_oudia_application::{MtrApiClient, MtrEndpoint, MtrSnapshotResponse};
use mtr_oudia_infrastructure::ReqwestMtrApiClient;

#[derive(Debug, Clone, PartialEq, Eq)]
enum FetchStatus {
    Idle,
    Loading {
        endpoint: String,
        dimension: u32,
    },
    Success {
        route_count: usize,
        stop_count: usize,
    },
    Error(String),
}

impl FetchStatus {
    fn is_loading(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectionInfo {
    endpoint: Option<String>,
    dimension: u32,
}

impl ConnectionInfo {
    fn from_env() -> Self {
        Self::from_env_values(
            std::env::var("MTR_ENDPOINT").ok(),
            std::env::var("MTR_DIMENSION").ok(),
        )
    }

    fn from_env_values(endpoint: Option<String>, dimension: Option<String>) -> Self {
        Self {
            endpoint,
            dimension: parse_dimension(dimension.as_deref()),
        }
    }

    fn endpoint_display(&self) -> String {
        match &self.endpoint {
            Some(value) if !value.trim().is_empty() => value.clone(),
            _ => "未設定（例: http://127.0.0.1:49182）".to_string(),
        }
    }
}

fn parse_dimension(raw: Option<&str>) -> u32 {
    raw.and_then(|value| value.parse::<u32>().ok()).unwrap_or(0)
}

fn summarize_routes(routes: &[(String, usize)]) -> (usize, usize) {
    let stop_count = routes.iter().map(|(_, stops)| *stops).sum();
    (routes.len(), stop_count)
}

fn status_notification(status: &FetchStatus) -> (NotificationKind, &'static str, String) {
    match status {
        FetchStatus::Idle => (
            NotificationKind::Info,
            "待機中",
            "「取得」を押して MTR スナップショットを取得してください".to_string(),
        ),
        FetchStatus::Loading {
            endpoint,
            dimension,
        } => (
            NotificationKind::Info,
            "取得中",
            format!("{endpoint} (dimension={dimension}) から読み込んでいます..."),
        ),
        FetchStatus::Success {
            route_count,
            stop_count,
        } => (
            NotificationKind::Success,
            "取得完了",
            format!("{route_count} 路線 / {stop_count} 駅を取得しました"),
        ),
        FetchStatus::Error(detail) => (NotificationKind::Error, "取得失敗", detail.clone()),
    }
}

fn theme_badge_label(kind: CarbonThemeKind) -> &'static str {
    match kind {
        CarbonThemeKind::Gray100 => "Gray 100",
        CarbonThemeKind::Gray10 => "Gray 10",
    }
}

fn theme_toggle_label(kind: CarbonThemeKind) -> &'static str {
    match kind {
        CarbonThemeKind::Gray100 => "Gray 10 に切替",
        CarbonThemeKind::Gray10 => "Gray 100 に切替",
    }
}

fn fetch_button_label(loading: bool) -> &'static str {
    if loading { "取得中..." } else { "取得" }
}

fn config_row(label: &'static str, value: impl Into<SharedString>, theme: &CarbonTheme) -> Div {
    let c = &theme.colors;
    div()
        .flex()
        .flex_row()
        .items_center()
        .min_w_0()
        .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
        .child(
            div()
                .flex_shrink_0()
                .text_size(CarbonTypography::LABEL_01.font_size_px())
                .line_height(CarbonTypography::LABEL_01.line_height_px())
                .text_color(c.text_secondary)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(CarbonTypography::BODY_01.font_size_px())
                .line_height(CarbonTypography::BODY_01.line_height_px())
                .text_color(c.text_primary)
                .child(value.into()),
        )
}

struct FetchPoc {
    theme: CarbonTheme,
    connection: ConnectionInfo,
    status: FetchStatus,
    routes: Vec<(String, usize)>,
}

impl FetchPoc {
    fn new() -> Self {
        Self {
            theme: CarbonTheme::default(),
            connection: ConnectionInfo::from_env(),
            status: FetchStatus::Idle,
            routes: Vec::new(),
        }
    }

    fn start_fetch(&mut self, cx: &mut Context<Self>) {
        if self.status.is_loading() {
            return;
        }

        let connection = ConnectionInfo::from_env();
        self.connection = connection.clone();

        let endpoint = match connection.endpoint {
            Some(value) => value,
            None => {
                self.status = FetchStatus::Error(
                    "環境変数 MTR_ENDPOINT を設定してください（例: http://127.0.0.1:49182）"
                        .to_string(),
                );
                cx.notify();
                return;
            }
        };
        let dimension = connection.dimension;

        self.status = FetchStatus::Loading {
            endpoint: endpoint.clone(),
            dimension,
        };
        self.routes.clear();
        cx.notify();

        let (tx, rx) = oneshot::channel::<Result<MtrSnapshotResponse, String>>();
        std::thread::spawn(move || {
            let result = (|| -> Result<MtrSnapshotResponse, String> {
                let endpoint = MtrEndpoint::parse(&endpoint).map_err(|err| format!("{err:?}"))?;
                let client = ReqwestMtrApiClient::new().map_err(|err| format!("{err:?}"))?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;
                runtime
                    .block_on(client.fetch_snapshot(&endpoint, dimension))
                    .map_err(|err| format!("{err:?}"))
            })();
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            let result = rx.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(response)) => {
                        let routes: Vec<(String, usize)> = response
                            .snapshot
                            .routes
                            .iter()
                            .map(|route| (route.display_name.clone(), route.stops.len()))
                            .collect();
                        let (route_count, stop_count) = summarize_routes(&routes);
                        this.routes = routes;
                        this.status = FetchStatus::Success {
                            route_count,
                            stop_count,
                        };
                    }
                    Ok(Err(err)) => {
                        this.status = FetchStatus::Error(err);
                    }
                    Err(_) => {
                        this.status = FetchStatus::Error("取得タスクが終了しました".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for FetchPoc {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.theme.colors;
        let loading = self.status.is_loading();
        let (notif_kind, notif_title, notif_subtitle) = status_notification(&self.status);

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(CarbonTypography::HEADING_03.font_size_px())
                            .line_height(CarbonTypography::HEADING_03.line_height_px())
                            .text_color(c.text_primary)
                            .child("GPUI async bridge PoC"),
                    )
                    .child(carbon_tag(
                        theme_badge_label(self.theme.kind),
                        TagKind::CoolGray,
                        &self.theme,
                    )),
            )
            .child(carbon_button(
                "toggle-theme",
                theme_toggle_label(self.theme.kind),
                ButtonKind::Ghost,
                false,
                &self.theme,
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.theme.toggle();
                    cx.notify();
                }),
            ));

        let connection_panel = carbon_tile(TileLayer::Layer01, &self.theme)
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(CarbonTypography::HEADING_02.font_size_px())
                                    .line_height(CarbonTypography::HEADING_02.line_height_px())
                                    .text_color(c.text_primary)
                                    .child("接続設定"),
                            )
                            .child(config_row(
                                "MTR_ENDPOINT:",
                                self.connection.endpoint_display(),
                                &self.theme,
                            ))
                            .child(config_row(
                                "MTR_DIMENSION:",
                                self.connection.dimension.to_string(),
                                &self.theme,
                            )),
                    )
                    .child(carbon_button(
                        "fetch",
                        fetch_button_label(loading),
                        ButtonKind::Primary,
                        loading,
                        &self.theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_fetch(cx);
                        }),
                    )),
            );

        let notification =
            carbon_inline_notification(notif_kind, notif_title, notif_subtitle, &self.theme);

        let route_list_content = if self.routes.is_empty() {
            div()
                .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                .text_size(CarbonTypography::BODY_01.font_size_px())
                .line_height(CarbonTypography::BODY_01.line_height_px())
                .text_color(c.text_secondary)
                .child(if loading {
                    "路線データを取得しています..."
                } else {
                    "表示できる路線データはありません"
                })
                .into_any_element()
        } else {
            div()
                .id("routes")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                .children(
                    self.routes
                        .iter()
                        .enumerate()
                        .map(|(index, (name, stops))| {
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                                .px(CarbonSpacing::px(CarbonSpacing::SP_04))
                                .py(CarbonSpacing::px(CarbonSpacing::SP_03))
                                .bg(c.layer_02)
                                .border_1()
                                .border_color(c.border_subtle)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(CarbonTypography::BODY_01.font_size_px())
                                        .line_height(CarbonTypography::BODY_01.line_height_px())
                                        .text_color(c.text_primary)
                                        .child(format!("{}. {name}", index + 1)),
                                )
                                .child(carbon_tag(
                                    format!("{stops} 駅"),
                                    TagKind::Blue,
                                    &self.theme,
                                ))
                        }),
                )
                .into_any_element()
        };

        let routes_panel = carbon_tile(TileLayer::Layer01, &self.theme)
            .flex_1()
            .min_h_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(CarbonTypography::HEADING_02.font_size_px())
                            .line_height(CarbonTypography::HEADING_02.line_height_px())
                            .text_color(c.text_primary)
                            .child("路線一覧"),
                    )
                    .child(carbon_tag(
                        format!("{} 路線", self.routes.len()),
                        TagKind::CoolGray,
                        &self.theme,
                    )),
            )
            .child(route_list_content);

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .p(CarbonSpacing::px(CarbonSpacing::SP_05))
            .bg(c.background)
            .text_color(c.text_primary)
            .child(header)
            .child(connection_panel)
            .child(notification)
            .child(routes_panel)
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(720.0), px(560.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| FetchPoc::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dimension_parses_valid_uint_and_defaults_to_zero() {
        assert_eq!(parse_dimension(None), 0);
        assert_eq!(parse_dimension(Some("")), 0);
        assert_eq!(parse_dimension(Some("-1")), 0);
        assert_eq!(parse_dimension(Some("abc")), 0);
        assert_eq!(parse_dimension(Some("0")), 0);
        assert_eq!(parse_dimension(Some("42")), 42);
    }

    #[test]
    fn connection_info_formats_endpoint_display() {
        let unset = ConnectionInfo::from_env_values(None, None);
        assert_eq!(unset.dimension, 0);
        assert!(unset.endpoint_display().starts_with("未設定"));

        let blank = ConnectionInfo::from_env_values(Some("   ".to_string()), Some("2".to_string()));
        assert_eq!(blank.dimension, 2);
        assert!(blank.endpoint_display().starts_with("未設定"));

        let configured = ConnectionInfo::from_env_values(
            Some("http://127.0.0.1:49182".to_string()),
            Some("1".to_string()),
        );
        assert_eq!(configured.dimension, 1);
        assert_eq!(configured.endpoint_display(), "http://127.0.0.1:49182");
    }

    #[test]
    fn summarize_routes_counts_routes_and_total_stops() {
        assert_eq!(summarize_routes(&[]), (0, 0));
        let routes = vec![("山手線".to_string(), 30), ("中央線".to_string(), 24)];
        assert_eq!(summarize_routes(&routes), (2, 54));
    }

    #[test]
    fn status_notification_maps_all_fetch_states() {
        let idle = status_notification(&FetchStatus::Idle);
        assert_eq!(idle.0, NotificationKind::Info);
        assert_eq!(idle.1, "待機中");
        assert!(idle.2.contains("取得"));

        let loading = status_notification(&FetchStatus::Loading {
            endpoint: "http://127.0.0.1:49182".to_string(),
            dimension: 3,
        });
        assert_eq!(loading.0, NotificationKind::Info);
        assert_eq!(loading.1, "取得中");
        assert!(loading.2.contains("http://127.0.0.1:49182"));
        assert!(loading.2.contains("dimension=3"));

        let success = status_notification(&FetchStatus::Success {
            route_count: 4,
            stop_count: 52,
        });
        assert_eq!(success.0, NotificationKind::Success);
        assert_eq!(success.1, "取得完了");
        assert_eq!(success.2, "4 路線 / 52 駅を取得しました");

        let error = status_notification(&FetchStatus::Error("connection refused".to_string()));
        assert_eq!(error.0, NotificationKind::Error);
        assert_eq!(error.1, "取得失敗");
        assert_eq!(error.2, "connection refused");
    }

    #[test]
    fn fetch_status_is_loading_only_in_loading_variant() {
        assert!(!FetchStatus::Idle.is_loading());
        assert!(
            FetchStatus::Loading {
                endpoint: "http://127.0.0.1:49182".to_string(),
                dimension: 0,
            }
            .is_loading()
        );
        assert!(
            !FetchStatus::Success {
                route_count: 1,
                stop_count: 2,
            }
            .is_loading()
        );
        assert!(!FetchStatus::Error("err".to_string()).is_loading());
    }

    #[test]
    fn theme_and_button_labels_match_state() {
        assert_eq!(theme_badge_label(CarbonThemeKind::Gray100), "Gray 100");
        assert_eq!(theme_badge_label(CarbonThemeKind::Gray10), "Gray 10");
        assert_eq!(
            theme_toggle_label(CarbonThemeKind::Gray100),
            "Gray 10 に切替"
        );
        assert_eq!(
            theme_toggle_label(CarbonThemeKind::Gray10),
            "Gray 100 に切替"
        );
        assert_eq!(fetch_button_label(false), "取得");
        assert_eq!(fetch_button_label(true), "取得中...");
    }
}
