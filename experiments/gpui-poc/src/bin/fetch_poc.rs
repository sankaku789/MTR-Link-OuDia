//! 既存の reqwest ベース infrastructure と接続先自動検出を GPUI から呼べるかの最小検証。
//!
//! - 接続先: 「自動検出」ボタンで `MtrEndpointDiscoveryService` により探索する
//!   （`MTR_ENDPOINT` が設定されている場合は初期値の手動オーバーライドとして使用）
//! - Minecraft ログ候補: `MTR_OUDIA_MINECRAFT_LOG` / `APPDATA/.minecraft/logs/latest.log` /
//!   実行中 Minecraft の `--gameDir`
//! - dimension: 環境変数 `MTR_DIMENSION`（未設定なら 0）
//!
//! reqwest は tokio のリアクタを必要とするため、専用スレッドで current-thread
//! ランタイムを回し、結果を oneshot で GPUI の executor に返す。

use std::{ffi::OsString, path::PathBuf};

use futures::channel::oneshot;
use gpui::{
    App, Application, Bounds, ClickEvent, Context, Div, FontWeight, IntoElement, ParentElement,
    Render, SharedString, Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_poc::carbon::{
    ButtonKind, CarbonSpacing, CarbonTheme, CarbonThemeKind, CarbonTypography, NotificationKind,
    TagKind, TileLayer, carbon_button, carbon_inline_notification, carbon_tag, carbon_tile,
};
use mtr_oudia_application::{
    ApplicationError, MinecraftLogStatus, MtrApiClient, MtrEndpoint, MtrEndpointDiscovery,
    MtrEndpointDiscoveryService, MtrSnapshotResponse,
};
use mtr_oudia_infrastructure::{
    FileMinecraftLogProvider, ReqwestMtrApiClient, WindowsListeningPortProvider,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointSource {
    Unset,
    EnvOverride,
    AutoDetected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PocStatus {
    Idle,
    Discovering,
    DiscoverySuccess {
        endpoint: String,
    },
    DiscoveryMultiple {
        count: usize,
        selected: String,
    },
    DiscoveryError(String),
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

impl PocStatus {
    fn is_busy(&self) -> bool {
        matches!(self, Self::Discovering | Self::Loading { .. })
    }

    fn is_discovering(&self) -> bool {
        matches!(self, Self::Discovering)
    }

    fn is_fetching(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectionInfo {
    endpoint: Option<String>,
    source: EndpointSource,
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
        let normalized_endpoint = endpoint
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let source = if normalized_endpoint.is_some() {
            EndpointSource::EnvOverride
        } else {
            EndpointSource::Unset
        };
        Self {
            endpoint: normalized_endpoint,
            source,
            dimension: parse_dimension(dimension.as_deref()),
        }
    }

    fn apply_detected_endpoint(&mut self, endpoint: String) {
        self.endpoint = Some(endpoint);
        self.source = EndpointSource::AutoDetected;
    }

    fn endpoint_display(&self) -> String {
        match &self.endpoint {
            Some(value) if !value.trim().is_empty() => value.clone(),
            _ => "未設定（「自動検出」または MTR_ENDPOINT で指定）".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveryOutcome {
    candidates: Vec<String>,
    selected: Option<String>,
    status: PocStatus,
}

struct AutoFetchOutcome {
    discovery: DiscoveryOutcome,
    fetch: Option<Result<MtrSnapshotResponse, String>>,
}

fn parse_dimension(raw: Option<&str>) -> u32 {
    raw.and_then(|value| value.parse::<u32>().ok()).unwrap_or(0)
}

fn minecraft_log_paths_from_env(
    custom_log: Option<OsString>,
    appdata: Option<OsString>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = custom_log.filter(|value| !value.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    if let Some(appdata) = appdata.filter(|value| !value.is_empty()) {
        paths.push(
            PathBuf::from(appdata)
                .join(".minecraft")
                .join("logs")
                .join("latest.log"),
        );
    }
    paths
}

fn minecraft_log_paths() -> Vec<PathBuf> {
    minecraft_log_paths_from_env(
        std::env::var_os("MTR_OUDIA_MINECRAFT_LOG"),
        std::env::var_os("APPDATA"),
    )
}

fn minecraft_log_status_note(status: MinecraftLogStatus) -> Option<&'static str> {
    match status {
        MinecraftLogStatus::NotConfigured => None,
        MinecraftLogStatus::NotFound => Some("Minecraft ログが見つかりません"),
        MinecraftLogStatus::NoEndpoint => Some("ログ内に MTR API URL がありません"),
        MinecraftLogStatus::InvalidEndpoint => Some("ログ内の接続先 URL が不正です"),
        MinecraftLogStatus::ReadFailed => Some("Minecraft ログを読み取れません"),
        MinecraftLogStatus::CandidateRejected => Some("ログ内の MTR API へ接続できません"),
    }
}

fn format_discovery_error(
    headline: &str,
    log_status: MinecraftLogStatus,
    counts: Option<String>,
) -> String {
    let parts: Vec<String> = [
        minecraft_log_status_note(log_status).map(str::to_string),
        counts,
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        headline.to_string()
    } else {
        format!("{headline}（{}）", parts.join("。"))
    }
}

fn map_discovery_result(result: MtrEndpointDiscovery) -> DiscoveryOutcome {
    match result {
        MtrEndpointDiscovery::Single(endpoint) => {
            let url = endpoint.as_url().to_string();
            DiscoveryOutcome {
                candidates: vec![url.clone()],
                selected: Some(url.clone()),
                status: PocStatus::DiscoverySuccess { endpoint: url },
            }
        }
        MtrEndpointDiscovery::Multiple(endpoints) => {
            let candidates: Vec<String> = endpoints
                .into_iter()
                .map(|endpoint| endpoint.as_url().to_string())
                .collect();
            let count = candidates.len();
            if let Some(selected) = candidates.first().cloned() {
                DiscoveryOutcome {
                    candidates,
                    selected: Some(selected.clone()),
                    status: PocStatus::DiscoveryMultiple { count, selected },
                }
            } else {
                DiscoveryOutcome {
                    candidates,
                    selected: None,
                    status: PocStatus::DiscoveryError("検出結果が空でした".to_string()),
                }
            }
        }
        MtrEndpointDiscovery::NoListeningPorts { log_status } => DiscoveryOutcome {
            candidates: Vec::new(),
            selected: None,
            status: PocStatus::DiscoveryError(format_discovery_error(
                "待受ポートが見つかりません",
                log_status,
                None,
            )),
        },
        MtrEndpointDiscovery::PortEnumerationFailed { error, log_status } => {
            let headline = match error {
                ApplicationError::ListeningPortProviderUnsupported => {
                    "この環境では待受ポート自動列挙に対応していません（MTR_ENDPOINT を設定してください）"
                        .to_string()
                }
                ApplicationError::ListeningPortEnumeration { message } => {
                    format!("待受ポートの取得に失敗しました: {message}")
                }
                other => format!("待受ポートの取得に失敗しました: {other:?}"),
            };
            DiscoveryOutcome {
                candidates: Vec::new(),
                selected: None,
                status: PocStatus::DiscoveryError(format_discovery_error(
                    &headline, log_status, None,
                )),
            }
        }
        MtrEndpointDiscovery::NoValidEndpoints {
            attempted,
            unreachable,
            invalid_responses,
            timeouts,
            log_status,
        } => {
            let headline = if invalid_responses > 0 && unreachable == 0 {
                "MTR API の応答形式が一致しません"
            } else if timeouts > 0 && unreachable == timeouts {
                "MTR API の応答がタイムアウトしました"
            } else {
                "MTR 候補へ接続できませんでした"
            };
            let counts = format!(
                "試行 {attempted} 件、到達不能 {unreachable} 件、初回タイムアウト {timeouts} 件、応答不正 {invalid_responses} 件"
            );
            DiscoveryOutcome {
                candidates: Vec::new(),
                selected: None,
                status: PocStatus::DiscoveryError(format_discovery_error(
                    headline,
                    log_status,
                    Some(counts),
                )),
            }
        }
    }
}

fn summarize_routes(routes: &[(String, usize)]) -> (usize, usize) {
    let stop_count = routes.iter().map(|(_, stops)| *stops).sum();
    (routes.len(), stop_count)
}

fn status_notification(status: &PocStatus) -> (NotificationKind, &'static str, String) {
    match status {
        PocStatus::Idle => (
            NotificationKind::Info,
            "待機中",
            "「自動検出」で接続先を探索するか、「取得」で MTR スナップショットを取得してください"
                .to_string(),
        ),
        PocStatus::Discovering => (
            NotificationKind::Info,
            "検出中",
            "Minecraft ログと待受ポートから MTR API を探索しています...".to_string(),
        ),
        PocStatus::DiscoverySuccess { endpoint } => (
            NotificationKind::Success,
            "検出完了",
            format!("MTR 接続先を検出しました: {endpoint}"),
        ),
        PocStatus::DiscoveryMultiple { count, selected } => (
            NotificationKind::Info,
            "複数候補を検出",
            format!("{count} 件の候補が見つかりました。選択中: {selected}"),
        ),
        PocStatus::DiscoveryError(detail) => {
            (NotificationKind::Error, "自動検出失敗", detail.clone())
        }
        PocStatus::Loading {
            endpoint,
            dimension,
        } => (
            NotificationKind::Info,
            "取得中",
            format!("{endpoint} (dimension={dimension}) から読み込んでいます..."),
        ),
        PocStatus::Success {
            route_count,
            stop_count,
        } => (
            NotificationKind::Success,
            "取得完了",
            format!("{route_count} 路線 / {stop_count} 駅を取得しました"),
        ),
        PocStatus::Error(detail) => (NotificationKind::Error, "取得失敗", detail.clone()),
    }
}

fn endpoint_source_tag(source: EndpointSource) -> (&'static str, TagKind) {
    match source {
        EndpointSource::Unset => ("未検出", TagKind::CoolGray),
        EndpointSource::EnvOverride => ("MTR_ENDPOINT", TagKind::WarmGray),
        EndpointSource::AutoDetected => ("自動検出", TagKind::Green),
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

fn discover_button_label(discovering: bool) -> &'static str {
    if discovering {
        "検出中..."
    } else {
        "自動検出"
    }
}

fn fetch_button_label(fetching: bool) -> &'static str {
    if fetching { "取得中..." } else { "取得" }
}

fn run_discovery_sync(
    runtime: &tokio::runtime::Runtime,
    client: &ReqwestMtrApiClient,
    previous: Option<MtrEndpoint>,
) -> MtrEndpointDiscovery {
    let ports = WindowsListeningPortProvider;
    let log_provider =
        FileMinecraftLogProvider::new(minecraft_log_paths()).with_running_minecraft();
    let service =
        MtrEndpointDiscoveryService::new(&ports, client).with_minecraft_log(&log_provider);
    runtime.block_on(service.detect(previous))
}

fn fetch_snapshot_sync(
    runtime: &tokio::runtime::Runtime,
    client: &ReqwestMtrApiClient,
    endpoint_str: &str,
    dimension: u32,
) -> Result<MtrSnapshotResponse, String> {
    let endpoint = MtrEndpoint::parse(endpoint_str).map_err(|err| format!("{err:?}"))?;
    runtime
        .block_on(client.fetch_snapshot(&endpoint, dimension))
        .map_err(|err| format!("{err:?}"))
}

fn spawn_tokio_bridge<T, F>(
    cx: &mut Context<FetchPoc>,
    work: F,
    on_complete: impl FnOnce(&mut FetchPoc, Result<T, String>) + 'static,
) where
    T: Send + 'static,
    F: FnOnce(&tokio::runtime::Runtime, &ReqwestMtrApiClient) -> Result<T, String> + Send + 'static,
{
    let (tx, rx) = oneshot::channel::<Result<T, String>>();
    std::thread::spawn(move || {
        let result = (|| -> Result<T, String> {
            let client = ReqwestMtrApiClient::new().map_err(|err| format!("{err:?}"))?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| err.to_string())?;
            work(&runtime, &client)
        })();
        let _ = tx.send(result);
    });

    cx.spawn(async move |this, cx| {
        let result = rx.await;
        this.update(cx, |this, cx| {
            let mapped = match result {
                Ok(inner) => inner,
                Err(_) => Err("バックグラウンドタスクが終了しました".to_string()),
            };
            on_complete(this, mapped);
            cx.notify();
        })
        .ok();
    })
    .detach();
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
    discovered_candidates: Vec<String>,
    status: PocStatus,
    routes: Vec<(String, usize)>,
}

impl FetchPoc {
    fn new() -> Self {
        Self {
            theme: CarbonTheme::default(),
            connection: ConnectionInfo::from_env(),
            discovered_candidates: Vec::new(),
            status: PocStatus::Idle,
            routes: Vec::new(),
        }
    }

    fn apply_discovery_outcome(&mut self, outcome: DiscoveryOutcome) {
        self.discovered_candidates = outcome.candidates;
        if let Some(selected) = outcome.selected {
            if self.connection.endpoint.as_deref() != Some(selected.as_str()) {
                self.routes.clear();
            }
            self.connection.apply_detected_endpoint(selected);
        }
        self.status = outcome.status;
    }

    fn apply_fetch_result(&mut self, result: Result<MtrSnapshotResponse, String>) {
        match result {
            Ok(response) => {
                let routes: Vec<(String, usize)> = response
                    .snapshot
                    .routes
                    .iter()
                    .map(|route| (route.display_name.clone(), route.stops.len()))
                    .collect();
                let (route_count, stop_count) = summarize_routes(&routes);
                self.routes = routes;
                self.status = PocStatus::Success {
                    route_count,
                    stop_count,
                };
            }
            Err(err) => {
                self.status = PocStatus::Error(err);
            }
        }
    }

    fn select_candidate(&mut self, candidate: String, cx: &mut Context<Self>) {
        if self.status.is_busy() {
            return;
        }
        if self.connection.endpoint.as_deref() != Some(candidate.as_str()) {
            self.routes.clear();
        }
        self.connection.apply_detected_endpoint(candidate.clone());
        self.status = if self.discovered_candidates.len() > 1 {
            PocStatus::DiscoveryMultiple {
                count: self.discovered_candidates.len(),
                selected: candidate,
            }
        } else {
            PocStatus::DiscoverySuccess {
                endpoint: candidate,
            }
        };
        cx.notify();
    }

    fn start_discover(&mut self, cx: &mut Context<Self>) {
        if self.status.is_busy() {
            return;
        }

        let previous = self
            .connection
            .endpoint
            .as_deref()
            .and_then(|raw| MtrEndpoint::parse(raw).ok());
        self.status = PocStatus::Discovering;
        cx.notify();

        spawn_tokio_bridge(
            cx,
            move |runtime, client| {
                let result = run_discovery_sync(runtime, client, previous);
                Ok(map_discovery_result(result))
            },
            |this, result| match result {
                Ok(outcome) => this.apply_discovery_outcome(outcome),
                Err(err) => this.status = PocStatus::DiscoveryError(err),
            },
        );
    }

    fn start_fetch(&mut self, cx: &mut Context<Self>) {
        if self.status.is_busy() {
            return;
        }

        let dimension = parse_dimension(std::env::var("MTR_DIMENSION").ok().as_deref());
        self.connection.dimension = dimension;
        self.routes.clear();

        if let Some(endpoint) = self.connection.endpoint.clone() {
            self.status = PocStatus::Loading {
                endpoint: endpoint.clone(),
                dimension,
            };
            cx.notify();

            spawn_tokio_bridge(
                cx,
                move |runtime, client| fetch_snapshot_sync(runtime, client, &endpoint, dimension),
                |this, result| this.apply_fetch_result(result),
            );
            return;
        }

        self.status = PocStatus::Discovering;
        cx.notify();

        spawn_tokio_bridge(
            cx,
            move |runtime, client| {
                let outcome = map_discovery_result(run_discovery_sync(runtime, client, None));
                let fetch = match (&outcome.status, outcome.selected.as_deref()) {
                    (PocStatus::DiscoverySuccess { .. }, Some(endpoint)) => {
                        Some(fetch_snapshot_sync(runtime, client, endpoint, dimension))
                    }
                    _ => None,
                };
                Ok(AutoFetchOutcome {
                    discovery: outcome,
                    fetch,
                })
            },
            |this, result| match result {
                Ok(outcome) => {
                    this.apply_discovery_outcome(outcome.discovery);
                    if let Some(fetch_result) = outcome.fetch {
                        this.apply_fetch_result(fetch_result);
                    }
                }
                Err(err) => this.status = PocStatus::Error(err),
            },
        );
    }
}

impl Render for FetchPoc {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.theme.colors;
        let busy = self.status.is_busy();
        let discovering = self.status.is_discovering();
        let fetching = self.status.is_fetching();
        let (notif_kind, notif_title, notif_subtitle) = status_notification(&self.status);
        let (source_label, source_tag_kind) = endpoint_source_tag(self.connection.source);

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

        let mut connection_panel = carbon_tile(TileLayer::Layer01, &self.theme)
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
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(CarbonTypography::HEADING_02.font_size_px())
                                            .line_height(
                                                CarbonTypography::HEADING_02.line_height_px(),
                                            )
                                            .text_color(c.text_primary)
                                            .child("接続設定"),
                                    )
                                    .child(carbon_tag(source_label, source_tag_kind, &self.theme)),
                            )
                            .child(config_row(
                                "接続先:",
                                self.connection.endpoint_display(),
                                &self.theme,
                            ))
                            .child(config_row(
                                "MTR_DIMENSION:",
                                self.connection.dimension.to_string(),
                                &self.theme,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .flex_shrink_0()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                            .child(carbon_button(
                                "discover",
                                discover_button_label(discovering),
                                ButtonKind::Secondary,
                                busy,
                                &self.theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.start_discover(cx);
                                }),
                            ))
                            .child(carbon_button(
                                "fetch",
                                fetch_button_label(fetching),
                                ButtonKind::Primary,
                                busy,
                                &self.theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.start_fetch(cx);
                                }),
                            )),
                    ),
            );

        if self.discovered_candidates.len() > 1 {
            let active_endpoint = self.connection.endpoint.as_deref();
            let candidate_buttons =
                self.discovered_candidates
                    .iter()
                    .enumerate()
                    .map(|(index, candidate)| {
                        let is_selected = active_endpoint == Some(candidate.as_str());
                        let candidate_value = candidate.clone();
                        carbon_button(
                            SharedString::from(format!("candidate-{index}")),
                            candidate.clone(),
                            if is_selected {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Secondary
                            },
                            busy,
                            &self.theme,
                            cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                this.select_candidate(candidate_value.clone(), cx);
                            }),
                        )
                    });

            connection_panel = connection_panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .child(
                        div()
                            .text_size(CarbonTypography::LABEL_01.font_size_px())
                            .line_height(CarbonTypography::LABEL_01.line_height_px())
                            .text_color(c.text_secondary)
                            .child("検出候補（クリックで切替）:"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .children(candidate_buttons),
                    ),
            );
        }

        let notification =
            carbon_inline_notification(notif_kind, notif_title, notif_subtitle, &self.theme);

        let route_list_content = if self.routes.is_empty() {
            div()
                .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                .text_size(CarbonTypography::BODY_01.font_size_px())
                .line_height(CarbonTypography::BODY_01.line_height_px())
                .text_color(c.text_secondary)
                .child(if fetching {
                    "路線データを取得しています..."
                } else if discovering {
                    "接続先を自動検出しています..."
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
    fn minecraft_log_paths_include_custom_and_appdata_in_priority_order() {
        assert!(minecraft_log_paths_from_env(None, None).is_empty());
        assert!(
            minecraft_log_paths_from_env(Some(OsString::from("")), Some(OsString::from("")))
                .is_empty()
        );

        let paths = minecraft_log_paths_from_env(
            Some(OsString::from("/custom/latest.log")),
            Some(OsString::from("/appdata")),
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/custom/latest.log"),
                PathBuf::from("/appdata")
                    .join(".minecraft")
                    .join("logs")
                    .join("latest.log"),
            ]
        );
    }

    #[test]
    fn connection_info_formats_endpoint_display_and_source() {
        let unset = ConnectionInfo::from_env_values(None, None);
        assert_eq!(unset.endpoint, None);
        assert_eq!(unset.source, EndpointSource::Unset);
        assert_eq!(unset.dimension, 0);
        assert!(unset.endpoint_display().starts_with("未設定"));

        let blank = ConnectionInfo::from_env_values(Some("   ".to_string()), Some("2".to_string()));
        assert_eq!(blank.endpoint, None);
        assert_eq!(blank.source, EndpointSource::Unset);
        assert_eq!(blank.dimension, 2);
        assert!(blank.endpoint_display().starts_with("未設定"));

        let mut configured = ConnectionInfo::from_env_values(
            Some(" http://127.0.0.1:49182 ".to_string()),
            Some("1".to_string()),
        );
        assert_eq!(
            configured.endpoint.as_deref(),
            Some("http://127.0.0.1:49182")
        );
        assert_eq!(configured.source, EndpointSource::EnvOverride);
        assert_eq!(configured.dimension, 1);
        assert_eq!(configured.endpoint_display(), "http://127.0.0.1:49182");

        configured.apply_detected_endpoint("http://127.0.0.1:8888/".to_string());
        assert_eq!(configured.source, EndpointSource::AutoDetected);
        assert_eq!(configured.endpoint_display(), "http://127.0.0.1:8888/");
    }

    #[test]
    fn map_discovery_result_handles_single_and_multiple_candidates() {
        let single = map_discovery_result(MtrEndpointDiscovery::Single(
            MtrEndpoint::parse("http://127.0.0.1:49182").unwrap(),
        ));
        assert_eq!(single.candidates, vec!["http://127.0.0.1:49182/"]);
        assert_eq!(single.selected.as_deref(), Some("http://127.0.0.1:49182/"));
        assert_eq!(
            single.status,
            PocStatus::DiscoverySuccess {
                endpoint: "http://127.0.0.1:49182/".to_string(),
            }
        );

        let multiple = map_discovery_result(MtrEndpointDiscovery::Multiple(vec![
            MtrEndpoint::parse("http://127.0.0.1:49182").unwrap(),
            MtrEndpoint::parse("http://[::1]:49183").unwrap(),
        ]));
        assert_eq!(
            multiple.candidates,
            vec!["http://127.0.0.1:49182/", "http://[::1]:49183/"]
        );
        assert_eq!(
            multiple.selected.as_deref(),
            Some("http://127.0.0.1:49182/")
        );
        assert_eq!(
            multiple.status,
            PocStatus::DiscoveryMultiple {
                count: 2,
                selected: "http://127.0.0.1:49182/".to_string(),
            }
        );
    }

    #[test]
    fn map_discovery_result_formats_all_error_variants() {
        let no_ports = map_discovery_result(MtrEndpointDiscovery::NoListeningPorts {
            log_status: MinecraftLogStatus::NotFound,
        });
        assert!(no_ports.candidates.is_empty());
        assert_eq!(no_ports.selected, None);
        match no_ports.status {
            PocStatus::DiscoveryError(msg) => {
                assert!(msg.contains("待受ポートが見つかりません"));
                assert!(msg.contains("Minecraft ログが見つかりません"));
            }
            other => panic!("unexpected status: {other:?}"),
        }

        let unsupported = map_discovery_result(MtrEndpointDiscovery::PortEnumerationFailed {
            error: ApplicationError::ListeningPortProviderUnsupported,
            log_status: MinecraftLogStatus::NoEndpoint,
        });
        match unsupported.status {
            PocStatus::DiscoveryError(msg) => {
                assert!(msg.contains("非対応") || msg.contains("対応していません"));
                assert!(msg.contains("ログ内に MTR API URL がありません"));
            }
            other => panic!("unexpected status: {other:?}"),
        }

        let no_valid = map_discovery_result(MtrEndpointDiscovery::NoValidEndpoints {
            attempted: 3,
            unreachable: 2,
            invalid_responses: 1,
            timeouts: 1,
            log_status: MinecraftLogStatus::CandidateRejected,
        });
        match no_valid.status {
            PocStatus::DiscoveryError(msg) => {
                assert!(msg.contains("MTR 候補へ接続できませんでした"));
                assert!(msg.contains("試行 3 件"));
                assert!(msg.contains("ログ内の MTR API へ接続できません"));
            }
            other => panic!("unexpected status: {other:?}"),
        }

        let invalid_only = map_discovery_result(MtrEndpointDiscovery::NoValidEndpoints {
            attempted: 1,
            unreachable: 0,
            invalid_responses: 1,
            timeouts: 0,
            log_status: MinecraftLogStatus::NotConfigured,
        });
        match invalid_only.status {
            PocStatus::DiscoveryError(msg) => {
                assert!(msg.contains("MTR API の応答形式が一致しません"));
            }
            other => panic!("unexpected status: {other:?}"),
        }
    }

    #[test]
    fn summarize_routes_counts_routes_and_total_stops() {
        assert_eq!(summarize_routes(&[]), (0, 0));
        let routes = vec![("山手線".to_string(), 30), ("中央線".to_string(), 24)];
        assert_eq!(summarize_routes(&routes), (2, 54));
    }

    #[test]
    fn status_notification_maps_all_poc_states() {
        let idle = status_notification(&PocStatus::Idle);
        assert_eq!(idle.0, NotificationKind::Info);
        assert_eq!(idle.1, "待機中");
        assert!(idle.2.contains("自動検出"));

        let discovering = status_notification(&PocStatus::Discovering);
        assert_eq!(discovering.0, NotificationKind::Info);
        assert_eq!(discovering.1, "検出中");

        let detected = status_notification(&PocStatus::DiscoverySuccess {
            endpoint: "http://127.0.0.1:49182/".to_string(),
        });
        assert_eq!(detected.0, NotificationKind::Success);
        assert_eq!(detected.1, "検出完了");
        assert!(detected.2.contains("http://127.0.0.1:49182/"));

        let multi = status_notification(&PocStatus::DiscoveryMultiple {
            count: 2,
            selected: "http://127.0.0.1:49182/".to_string(),
        });
        assert_eq!(multi.0, NotificationKind::Info);
        assert_eq!(multi.1, "複数候補を検出");
        assert!(multi.2.contains("2 件"));

        let discovery_err = status_notification(&PocStatus::DiscoveryError("err".to_string()));
        assert_eq!(discovery_err.0, NotificationKind::Error);
        assert_eq!(discovery_err.1, "自動検出失敗");

        let loading = status_notification(&PocStatus::Loading {
            endpoint: "http://127.0.0.1:49182".to_string(),
            dimension: 3,
        });
        assert_eq!(loading.0, NotificationKind::Info);
        assert_eq!(loading.1, "取得中");
        assert!(loading.2.contains("http://127.0.0.1:49182"));
        assert!(loading.2.contains("dimension=3"));

        let success = status_notification(&PocStatus::Success {
            route_count: 4,
            stop_count: 52,
        });
        assert_eq!(success.0, NotificationKind::Success);
        assert_eq!(success.1, "取得完了");
        assert_eq!(success.2, "4 路線 / 52 駅を取得しました");

        let error = status_notification(&PocStatus::Error("connection refused".to_string()));
        assert_eq!(error.0, NotificationKind::Error);
        assert_eq!(error.1, "取得失敗");
        assert_eq!(error.2, "connection refused");
    }

    #[test]
    fn poc_status_busy_flags_match_state() {
        assert!(!PocStatus::Idle.is_busy());
        assert!(PocStatus::Discovering.is_busy());
        assert!(PocStatus::Discovering.is_discovering());
        assert!(!PocStatus::Discovering.is_fetching());

        let loading = PocStatus::Loading {
            endpoint: "http://127.0.0.1:49182".to_string(),
            dimension: 0,
        };
        assert!(loading.is_busy());
        assert!(!loading.is_discovering());
        assert!(loading.is_fetching());

        assert!(
            !PocStatus::Success {
                route_count: 1,
                stop_count: 2,
            }
            .is_busy()
        );
        assert!(!PocStatus::DiscoveryError("err".to_string()).is_busy());
        assert!(!PocStatus::Error("err".to_string()).is_busy());
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
        assert_eq!(discover_button_label(false), "自動検出");
        assert_eq!(discover_button_label(true), "検出中...");
        assert_eq!(fetch_button_label(false), "取得");
        assert_eq!(fetch_button_label(true), "取得中...");
    }
}
