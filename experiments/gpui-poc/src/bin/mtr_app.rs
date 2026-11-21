//! Carbon Design System v11 ベースの 5 ステップ画面シェルと Step 1（MTR 接続）・Step 2（OuDia 選択）。
//!
//! バックエンド処理はすべて [`gpui_poc::backend::AppBackend`] 経由で実行する。

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, ClickEvent, Context, Div, FontWeight, IntoElement, ParentElement,
    Render, Rgba, SharedString, Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    size,
};
use gpui_poc::backend::{AppBackend, format_business_error};
use gpui_poc::carbon::{
    ButtonKind, CarbonSpacing, CarbonTheme, CarbonThemeKind, CarbonTypography, NotificationKind,
    TagKind, TileLayer, carbon_button, carbon_inline_notification, carbon_tag, carbon_tile,
};
use mtr_oudia_application::{
    BusinessError, EndpointDto, InspectionDto, MtrEndpoint, SessionId, SnapshotDto,
};

const TRANSPARENT: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

const SIDEBAR_WIDTH: f32 = 220.0;

/// ウィザード全体の 5 ステップを表す列挙型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppStep {
    Step1Mtr,
    Step2Oudia,
    Step3RouteMatch,
    Step4Preview,
    Step5Save,
}

impl AppStep {
    pub const ALL: [Self; 5] = [
        Self::Step1Mtr,
        Self::Step2Oudia,
        Self::Step3RouteMatch,
        Self::Step4Preview,
        Self::Step5Save,
    ];

    /// 1 始まりのステップ番号を返す。
    pub const fn number(self) -> u8 {
        match self {
            Self::Step1Mtr => 1,
            Self::Step2Oudia => 2,
            Self::Step3RouteMatch => 3,
            Self::Step4Preview => 4,
            Self::Step5Save => 5,
        }
    }

    /// サイドバーおよびヘッダーに表示するステップ名を返す。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Step1Mtr => "MTR 接続",
            Self::Step2Oudia => "OuDia 選択",
            Self::Step3RouteMatch => "路線・経路照合",
            Self::Step4Preview => "プレビュー",
            Self::Step5Save => "保存",
        }
    }

    /// 各ステップの説明文を返す。
    pub const fn description(self) -> &'static str {
        match self {
            Self::Step1Mtr => "MTR API の接続先を自動検出し、路線スナップショットを取得します。",
            Self::Step2Oudia => "反映先の OuDia (.oud2) ファイルを選択して解析します。",
            Self::Step3RouteMatch => {
                "MTR 路線と OuDia の列車テンプレートを照合し、駅対応を確認します。"
            }
            Self::Step4Preview => "生成される時刻表と Operation 設定のプレビューを確認します。",
            Self::Step5Save => "元の OuDia ファイルを検証付きで上書き保存します。",
        }
    }
}

/// Step 1・Step 2 の処理状態と通知内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppStatus {
    InitFailed {
        title: String,
        detail: String,
    },
    Idle,
    Detecting,
    DetectedSingle {
        endpoint: String,
    },
    DetectedMultiple {
        count: usize,
        selected: String,
    },
    DetectionError {
        title: String,
        detail: String,
    },
    Fetching {
        endpoint: String,
        dimension: u32,
    },
    FetchSuccess {
        route_count: usize,
        stop_count: usize,
    },
    FetchError {
        title: String,
        detail: String,
    },
    InspectingOudia {
        path: String,
    },
    InspectSuccess {
        file_type: String,
        station_count: usize,
        diagram_count: usize,
    },
    InspectError {
        title: String,
        detail: String,
    },
}

impl AppStatus {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Detecting | Self::Fetching { .. } | Self::InspectingOudia { .. }
        )
    }

    pub fn is_detecting(&self) -> bool {
        matches!(self, Self::Detecting)
    }

    pub fn is_fetching(&self) -> bool {
        matches!(self, Self::Fetching { .. })
    }

    pub fn is_inspecting(&self) -> bool {
        matches!(self, Self::InspectingOudia { .. })
    }

    pub fn is_init_failed(&self) -> bool {
        matches!(self, Self::InitFailed { .. })
    }
}

/// [`AppStatus`] を Carbon `InlineNotification` 用の `(種別, タイトル, 詳細)` に変換する。
pub fn status_notification(status: &AppStatus) -> (NotificationKind, String, String) {
    match status {
        AppStatus::InitFailed { title, detail } => {
            (NotificationKind::Error, title.clone(), detail.clone())
        }
        AppStatus::Idle => (
            NotificationKind::Info,
            "待機中".to_string(),
            "「自動検出」で MTR 接続先を探索し、「スナップショット取得」で路線を取得してください"
                .to_string(),
        ),
        AppStatus::Detecting => (
            NotificationKind::Info,
            "検出中".to_string(),
            "Minecraft ログと待受ポートから MTR API を探索しています...".to_string(),
        ),
        AppStatus::DetectedSingle { endpoint } => (
            NotificationKind::Success,
            "検出完了".to_string(),
            format!("MTR 接続先を検出しました: {endpoint}"),
        ),
        AppStatus::DetectedMultiple { count, selected } => (
            NotificationKind::Info,
            "複数候補を検出".to_string(),
            format!("{count} 件の候補が見つかりました。選択中: {selected}"),
        ),
        AppStatus::DetectionError { title, detail } => {
            (NotificationKind::Error, title.clone(), detail.clone())
        }
        AppStatus::Fetching {
            endpoint,
            dimension,
        } => (
            NotificationKind::Info,
            "取得中".to_string(),
            format!("{endpoint} (dimension={dimension}) から読み込んでいます..."),
        ),
        AppStatus::FetchSuccess {
            route_count,
            stop_count,
        } => (
            NotificationKind::Success,
            "取得完了".to_string(),
            format!("{route_count} 路線 / {stop_count} 駅を取得しました"),
        ),
        AppStatus::FetchError { title, detail } => {
            (NotificationKind::Error, title.clone(), detail.clone())
        }
        AppStatus::InspectingOudia { path } => (
            NotificationKind::Info,
            "解析中".to_string(),
            format!("{path} を読み込んでいます..."),
        ),
        AppStatus::InspectSuccess {
            file_type,
            station_count,
            diagram_count,
        } => (
            NotificationKind::Success,
            "解析完了".to_string(),
            format!("{file_type}: {station_count} 駅 / {diagram_count} ダイヤを読み込みました"),
        ),
        AppStatus::InspectError { title, detail } => {
            (NotificationKind::Error, title.clone(), detail.clone())
        }
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

fn discover_button_label(detecting: bool) -> &'static str {
    if detecting {
        "検出中..."
    } else {
        "自動検出"
    }
}

fn fetch_button_label(fetching: bool) -> &'static str {
    if fetching {
        "取得中..."
    } else {
        "スナップショット取得"
    }
}

fn inspect_button_label(inspecting: bool) -> &'static str {
    if inspecting {
        "解析中..."
    } else {
        "OuDia ファイルを選択"
    }
}

fn kijun_status_badge(kijun_status: &str) -> (&str, TagKind) {
    match kijun_status {
        "valid" => ("valid", TagKind::Green),
        "missing" => ("missing", TagKind::WarmGray),
        other => (other, TagKind::Red),
    }
}

fn format_dimension_label(index: usize, value: &impl std::fmt::Display) -> String {
    let raw = value.to_string();
    let display = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(&raw);
    format!("{index}: {display}")
}

/// UI から独立してテスト可能なアプリケーション状態。
struct MtrShellState {
    theme: CarbonTheme,
    current_step: AppStep,
    candidates: Vec<String>,
    selected_endpoint: Option<String>,
    dimension: u32,
    snapshot: Option<SnapshotDto>,
    oudia_path: Option<PathBuf>,
    inspection: Option<InspectionDto>,
    status: AppStatus,
}

impl MtrShellState {
    fn new(init_result: Result<(), String>) -> Self {
        let status = match init_result {
            Ok(()) => AppStatus::Idle,
            Err(detail) => AppStatus::InitFailed {
                title: "バックエンドの初期化に失敗しました".to_string(),
                detail,
            },
        };
        Self {
            theme: CarbonTheme::default(),
            current_step: AppStep::Step1Mtr,
            candidates: Vec::new(),
            selected_endpoint: None,
            dimension: 0,
            snapshot: None,
            oudia_path: None,
            inspection: None,
            status,
        }
    }

    fn is_step_completed(&self, step: AppStep) -> bool {
        match step {
            AppStep::Step1Mtr => self.snapshot.is_some(),
            AppStep::Step2Oudia => self.inspection.is_some(),
            AppStep::Step3RouteMatch | AppStep::Step4Preview | AppStep::Step5Save => false,
        }
    }

    fn select_step(&mut self, step: AppStep) {
        self.current_step = step;
    }

    /// 接続先を選択し、異なる接続先へ切り替わった場合は旧 `snapshot` をクリアする。
    fn select_endpoint(&mut self, endpoint: String) -> bool {
        if self.status.is_busy() {
            return false;
        }
        self.apply_selected_endpoint(endpoint.clone());
        self.status = if self.candidates.len() > 1 {
            AppStatus::DetectedMultiple {
                count: self.candidates.len(),
                selected: endpoint,
            }
        } else {
            AppStatus::DetectedSingle { endpoint }
        };
        true
    }

    fn apply_selected_endpoint(&mut self, endpoint: String) {
        if self.selected_endpoint.as_deref() != Some(endpoint.as_str()) {
            self.snapshot = None;
            self.dimension = 0;
        }
        self.selected_endpoint = Some(endpoint);
    }

    fn apply_detection_result(&mut self, result: Result<Vec<EndpointDto>, BusinessError>) {
        match result {
            Ok(endpoints) => {
                let urls: Vec<String> = endpoints.into_iter().map(|item| item.url).collect();
                if urls.is_empty() {
                    self.candidates.clear();
                    self.status = AppStatus::DetectionError {
                        title: "接続先が見つかりませんでした".to_string(),
                        detail: "MTR API の待受ポートを検出できませんでした。".to_string(),
                    };
                } else if urls.len() == 1 {
                    let endpoint = urls[0].clone();
                    self.candidates = urls;
                    self.apply_selected_endpoint(endpoint.clone());
                    self.status = AppStatus::DetectedSingle { endpoint };
                } else {
                    let selected = urls[0].clone();
                    let count = urls.len();
                    self.candidates = urls;
                    self.apply_selected_endpoint(selected.clone());
                    self.status = AppStatus::DetectedMultiple { count, selected };
                }
            }
            Err(err) => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::DetectionError { title, detail };
            }
        }
    }

    fn apply_snapshot_result(
        &mut self,
        dimension: u32,
        result: Result<SnapshotDto, BusinessError>,
    ) {
        match result {
            Ok(dto) => {
                let route_count = dto.routes.len();
                let stop_count: usize = dto.routes.iter().map(|route| route.station_count).sum();
                self.dimension = dimension;
                self.snapshot = Some(dto);
                self.status = AppStatus::FetchSuccess {
                    route_count,
                    stop_count,
                };
            }
            Err(err) => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::FetchError { title, detail };
            }
        }
    }

    fn apply_inspection_result(
        &mut self,
        path: PathBuf,
        result: Result<InspectionDto, BusinessError>,
    ) {
        match result {
            Ok(dto) => {
                let file_type = dto.file_type.clone();
                let station_count = dto.station_count;
                let diagram_count = dto.diagrams.len();
                self.oudia_path = Some(path);
                self.inspection = Some(dto);
                self.status = AppStatus::InspectSuccess {
                    file_type,
                    station_count,
                    diagram_count,
                };
            }
            Err(err) => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::InspectError { title, detail };
            }
        }
    }
}

struct MtrApp {
    backend: Option<AppBackend>,
    session_id: Option<SessionId>,
    state: MtrShellState,
}

impl MtrApp {
    fn new() -> Self {
        match AppBackend::new() {
            Ok(backend) => {
                let session_id = backend.create_session();
                Self {
                    backend: Some(backend),
                    session_id: Some(session_id),
                    state: MtrShellState::new(Ok(())),
                }
            }
            Err(err) => Self {
                backend: None,
                session_id: None,
                state: MtrShellState::new(Err(err)),
            },
        }
    }

    fn start_detect(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        self.state.status = AppStatus::Detecting;
        cx.notify();

        let rx = backend.detect_mtr_endpoint(session_id);
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                match received {
                    Ok(result) => this.state.apply_detection_result(result),
                    Err(_) => {
                        this.state.status = AppStatus::DetectionError {
                            title: "自動検出が中断されました".to_string(),
                            detail: "バックグラウンド処理との通信が終了しました。".to_string(),
                        };
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_fetch(&mut self, cx: &mut Context<Self>) {
        self.start_fetch_for_dimension(self.state.dimension, cx);
    }

    fn start_fetch_for_dimension(&mut self, dimension: u32, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let Some(endpoint_str) = self.state.selected_endpoint.clone() else {
            self.state.status = AppStatus::FetchError {
                title: "接続先が未選択です".to_string(),
                detail: "先に「自動検出」を実行して MTR 接続先を選択してください。".to_string(),
            };
            cx.notify();
            return;
        };

        let endpoint = match MtrEndpoint::parse(&endpoint_str) {
            Ok(parsed) => parsed,
            Err(err) => {
                self.state.status = AppStatus::FetchError {
                    title: "接続先 URL が不正です".to_string(),
                    detail: format!("{endpoint_str}: {err}"),
                };
                cx.notify();
                return;
            }
        };

        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        self.state.status = AppStatus::Fetching {
            endpoint: endpoint_str,
            dimension,
        };
        cx.notify();

        let rx = backend.fetch_mtr_snapshot(session_id, endpoint, dimension);
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                match received {
                    Ok(result) => this.state.apply_snapshot_result(dimension, result),
                    Err(_) => {
                        this.state.status = AppStatus::FetchError {
                            title: "スナップショット取得が中断されました".to_string(),
                            detail: "バックグラウンド処理との通信が終了しました。".to_string(),
                        };
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_dimension(&mut self, dimension: u32, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        if self.state.dimension == dimension {
            return;
        }
        if self.state.selected_endpoint.is_some() {
            self.start_fetch_for_dimension(dimension, cx);
        } else {
            self.state.dimension = dimension;
            cx.notify();
        }
    }

    fn start_select_oudia(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }

        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("OuDia ファイルを選択".into()),
        });

        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| match received {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.start_inspect_oudia(path, cx);
                    }
                }
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(err)) => {
                    this.state.status = AppStatus::InspectError {
                        title: "ファイル選択ダイアログを開けませんでした".to_string(),
                        detail: err.to_string(),
                    };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn start_inspect_oudia(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        self.state.status = AppStatus::InspectingOudia {
            path: path.display().to_string(),
        };
        cx.notify();

        let rx = backend.inspect_oudia(session_id, path.clone());
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                match received {
                    Ok(result) => this.state.apply_inspection_result(path, result),
                    Err(_) => {
                        this.state.status = AppStatus::InspectError {
                            title: "OuDia 解析が中断されました".to_string(),
                            detail: "バックグラウンド処理との通信が終了しました。".to_string(),
                        };
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .px(CarbonSpacing::px(CarbonSpacing::SP_05))
            .py(CarbonSpacing::px(CarbonSpacing::SP_03))
            .bg(c.layer_01)
            .border_b_1()
            .border_color(c.border_subtle)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .min_w_0()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(CarbonTypography::HEADING_03.font_size_px())
                            .line_height(CarbonTypography::HEADING_03.line_height_px())
                            .text_color(c.text_primary)
                            .child("MTR-Link-OuDia"),
                    )
                    .child(carbon_tag(
                        theme_badge_label(theme.kind),
                        TagKind::CoolGray,
                        theme,
                    )),
            )
            .child(carbon_button(
                "toggle-theme",
                theme_toggle_label(theme.kind),
                ButtonKind::Ghost,
                false,
                theme,
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.state.theme.toggle();
                    cx.notify();
                }),
            ))
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;

        let items = AppStep::ALL.into_iter().map(|step| {
            let active = self.state.current_step == step;
            let completed = self.state.is_step_completed(step);
            let accent = if active {
                c.button_primary
            } else {
                TRANSPARENT
            };
            let bg = if active { c.layer_02 } else { c.layer_01 };
            let hover_bg = c.layer_hover_01;
            let text_color = if active {
                c.text_primary
            } else {
                c.text_secondary
            };

            let mut row = div()
                .id(SharedString::from(format!(
                    "sidebar-step-{}",
                    step.number()
                )))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                .px(CarbonSpacing::px(CarbonSpacing::SP_04))
                .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                .bg(bg)
                .border_l_3()
                .border_color(accent)
                .cursor_pointer()
                .hover(move |style| style.bg(hover_bg))
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.state.select_step(step);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_size(CarbonTypography::BODY_COMPACT_01.font_size_px())
                        .line_height(CarbonTypography::BODY_COMPACT_01.line_height_px())
                        .text_color(text_color)
                        .child(format!("{}. {}", step.number(), step.label())),
                );

            if completed {
                row = row.child(carbon_tag("完了", TagKind::Green, theme));
            }
            row
        });

        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(c.layer_01)
            .border_r_1()
            .border_color(c.border_subtle)
            .py(CarbonSpacing::px(CarbonSpacing::SP_03))
            .children(items)
    }

    fn render_step1(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;
        let busy = self.state.status.is_busy();
        let disabled = busy || self.state.status.is_init_failed();
        let detecting = self.state.status.is_detecting();
        let fetching = self.state.status.is_fetching();

        let endpoint_display = self
            .state
            .selected_endpoint
            .as_deref()
            .unwrap_or("未選択（「自動検出」を実行してください）");
        let (endpoint_tag_label, endpoint_tag_kind) = if self.state.snapshot.is_some() {
            ("取得済み", TagKind::Green)
        } else if self.state.selected_endpoint.is_some() {
            ("選択中", TagKind::Blue)
        } else {
            ("未選択", TagKind::CoolGray)
        };

        let mut connection_tile = carbon_tile(TileLayer::Layer01, theme)
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
                                            .child("MTR 接続"),
                                    )
                                    .child(carbon_tag(
                                        endpoint_tag_label,
                                        endpoint_tag_kind,
                                        theme,
                                    )),
                            )
                            .child(info_row("接続先:", endpoint_display.to_string(), theme))
                            .child(info_row(
                                "dimension:",
                                self.state.dimension.to_string(),
                                theme,
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
                                "detect-mtr",
                                discover_button_label(detecting),
                                ButtonKind::Secondary,
                                disabled,
                                theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.start_detect(cx);
                                }),
                            ))
                            .child(carbon_button(
                                "fetch-snapshot",
                                fetch_button_label(fetching),
                                ButtonKind::Primary,
                                disabled,
                                theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.start_fetch(cx);
                                }),
                            )),
                    ),
            );

        if self.state.candidates.len() > 1 {
            let active = self.state.selected_endpoint.as_deref();
            let candidate_buttons =
                self.state
                    .candidates
                    .iter()
                    .enumerate()
                    .map(|(index, candidate)| {
                        let is_selected = active == Some(candidate.as_str());
                        let value = candidate.clone();
                        carbon_button(
                            SharedString::from(format!("endpoint-candidate-{index}")),
                            candidate.clone(),
                            if is_selected {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Secondary
                            },
                            disabled,
                            theme,
                            cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                if this.state.select_endpoint(value.clone()) {
                                    cx.notify();
                                }
                            }),
                        )
                    });

            connection_tile = connection_tile.child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .child(
                        div()
                            .text_size(CarbonTypography::LABEL_01.font_size_px())
                            .line_height(CarbonTypography::LABEL_01.line_height_px())
                            .text_color(c.text_secondary)
                            .child(
                                "検出候補（クリックで切替・旧スナップショットはクリアされます）:",
                            ),
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

        if let Some(snapshot) = &self.state.snapshot
            && !snapshot.dimensions.is_empty()
        {
            let active_dim = self.state.dimension;
            let dim_buttons =
                snapshot
                    .dimensions
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| {
                        let dim_u32 = u32::try_from(index).ok()?;
                        let is_selected = active_dim == dim_u32;
                        Some(carbon_button(
                            SharedString::from(format!("dimension-select-{index}")),
                            format_dimension_label(index, value),
                            if is_selected {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Secondary
                            },
                            disabled,
                            theme,
                            cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                this.select_dimension(dim_u32, cx);
                            }),
                        ))
                    });

            connection_tile = connection_tile.child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .child(
                        div()
                            .text_size(CarbonTypography::LABEL_01.font_size_px())
                            .line_height(CarbonTypography::LABEL_01.line_height_px())
                            .text_color(c.text_secondary)
                            .child("利用可能な dimension（クリックで切替・再取得）:"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .children(dim_buttons),
                    ),
            );
        }

        let (notif_kind, notif_title, notif_subtitle) = status_notification(&self.state.status);
        let notification =
            carbon_inline_notification(notif_kind, notif_title, notif_subtitle, theme);

        let routes = self
            .state
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.routes.as_slice())
            .unwrap_or(&[]);

        let route_list_content = if routes.is_empty() {
            div()
                .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                .text_size(CarbonTypography::BODY_01.font_size_px())
                .line_height(CarbonTypography::BODY_01.line_height_px())
                .text_color(c.text_secondary)
                .child(if fetching {
                    "路線データを取得しています..."
                } else if detecting {
                    "接続先を自動検出しています..."
                } else if self.state.snapshot.is_some() {
                    "変換可能な路線が見つかりませんでした"
                } else {
                    "表示できる路線データはありません"
                })
                .into_any_element()
        } else {
            div()
                .id("step1-routes")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                .children(routes.iter().enumerate().map(|(index, route)| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .min_w_0()
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
                                .child(format!("{}. {}", index + 1, route.name)),
                        )
                        .child(carbon_tag(
                            format!("{} 駅", route.station_count),
                            TagKind::Blue,
                            theme,
                        ))
                }))
                .into_any_element()
        };

        let routes_tile = carbon_tile(TileLayer::Layer01, theme)
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
                        format!("{} 路線", routes.len()),
                        TagKind::CoolGray,
                        theme,
                    )),
            )
            .child(route_list_content);

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(connection_tile)
            .child(notification)
            .child(routes_tile)
    }

    fn render_step2(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;
        let busy = self.state.status.is_busy();
        let disabled = busy || self.state.status.is_init_failed();
        let inspecting = self.state.status.is_inspecting();

        let path_display = self
            .state
            .oudia_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "未選択（「OuDia ファイルを選択」を押してください）".to_string());
        let (file_tag_label, file_tag_kind) = if self.state.inspection.is_some() {
            ("解析済み", TagKind::Green)
        } else {
            ("未選択", TagKind::CoolGray)
        };

        let file_tile = carbon_tile(TileLayer::Layer01, theme)
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
                                            .child("OuDia ファイル"),
                                    )
                                    .child(carbon_tag(file_tag_label, file_tag_kind, theme)),
                            )
                            .child(info_row("選択ファイル:", path_display, theme)),
                    )
                    .child(carbon_button(
                        "select-oudia-file",
                        inspect_button_label(inspecting),
                        ButtonKind::Primary,
                        disabled,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_select_oudia(cx);
                        }),
                    )),
            );

        let (notif_kind, notif_title, notif_subtitle) = status_notification(&self.state.status);
        let notification =
            carbon_inline_notification(notif_kind, notif_title, notif_subtitle, theme);

        let inspection_content = if let Some(inspection) = &self.state.inspection {
            let line_name = inspection
                .line_name
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or("記載なし")
                .to_string();
            let (kijun_label, kijun_kind) = kijun_status_badge(&inspection.kijun_status);

            let diagram_list = if inspection.diagrams.is_empty() {
                div()
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child("ダイヤが見つかりませんでした")
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .children(inspection.diagrams.iter().map(|diagram| {
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .min_w_0()
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
                                    .child(format!("ダイヤ #{}", diagram.index)),
                            )
                            .child(carbon_tag(
                                format!("{} 列車", diagram.train_count),
                                TagKind::Blue,
                                theme,
                            ))
                    }))
                    .into_any_element()
            };

            let train_type_list = if inspection.train_type_names.is_empty() {
                div()
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child("列車種別はありません")
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .children(inspection.train_type_names.iter().enumerate().map(
                        |(index, name)| {
                            let label = if name.is_empty() {
                                format!("種別 {index}")
                            } else {
                                name.clone()
                            };
                            carbon_tag(label, TagKind::WarmGray, theme)
                        },
                    ))
                    .into_any_element()
            };

            div()
                .id("step2-inspection-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                        .child(carbon_tag(
                            format!("形式: {}", inspection.file_type),
                            TagKind::Blue,
                            theme,
                        ))
                        .child(carbon_tag(
                            format!("{} 駅", inspection.station_count),
                            TagKind::CoolGray,
                            theme,
                        ))
                        .child(carbon_tag(
                            format!("基準ダイヤ: {kijun_label}"),
                            kijun_kind,
                            theme,
                        )),
                )
                .child(info_row("路線名:", line_name, theme))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                                        .text_color(c.text_secondary)
                                        .child("ダイヤ一覧"),
                                )
                                .child(carbon_tag(
                                    format!("{} ダイヤ", inspection.diagrams.len()),
                                    TagKind::CoolGray,
                                    theme,
                                )),
                        )
                        .child(diagram_list),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                                        .text_color(c.text_secondary)
                                        .child("列車種別一覧"),
                                )
                                .child(carbon_tag(
                                    format!("{} 種別", inspection.train_type_names.len()),
                                    TagKind::CoolGray,
                                    theme,
                                )),
                        )
                        .child(train_type_list),
                )
                .into_any_element()
        } else {
            div()
                .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                .text_size(CarbonTypography::BODY_01.font_size_px())
                .line_height(CarbonTypography::BODY_01.line_height_px())
                .text_color(c.text_secondary)
                .child(if inspecting {
                    "OuDia ファイルを解析しています..."
                } else {
                    "OuDia ファイルを選択すると解析結果が表示されます"
                })
                .into_any_element()
        };

        let inspection_tile = carbon_tile(TileLayer::Layer01, theme)
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
                            .child("解析結果"),
                    ),
            )
            .child(inspection_content);

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(file_tile)
            .child(notification)
            .child(inspection_tile)
    }

    fn render_placeholder_step(&self, step: AppStep) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;

        carbon_tile(TileLayer::Layer01, theme)
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(CarbonTypography::HEADING_02.font_size_px())
                            .line_height(CarbonTypography::HEADING_02.line_height_px())
                            .text_color(c.text_primary)
                            .child(format!("Step {}: {}", step.number(), step.label())),
                    )
                    .child(carbon_tag(
                        "今後のタスクで移植予定",
                        TagKind::WarmGray,
                        theme,
                    )),
            )
            .child(
                div()
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(format!(
                        "{} 今後のタスクで移植予定です。",
                        step.description()
                    )),
            )
    }

    fn render_main(&self, cx: &mut Context<Self>) -> Div {
        let step = self.state.current_step;
        let theme = &self.state.theme;
        let c = &theme.colors;

        let step_header = div()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_01))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(CarbonTypography::HEADING_03.font_size_px())
                    .line_height(CarbonTypography::HEADING_03.line_height_px())
                    .text_color(c.text_primary)
                    .child(format!("Step {}: {}", step.number(), step.label())),
            )
            .child(
                div()
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(step.description()),
            );

        let body = match step {
            AppStep::Step1Mtr => self.render_step1(cx),
            AppStep::Step2Oudia => self.render_step2(cx),
            AppStep::Step3RouteMatch | AppStep::Step4Preview | AppStep::Step5Save => {
                self.render_placeholder_step(step)
            }
        };

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .p(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(step_header)
            .child(body)
    }
}

impl Render for MtrApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.state.theme.colors;

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.background)
            .text_color(c.text_primary)
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(self.render_sidebar(cx))
                    .child(self.render_main(cx)),
            )
    }
}

fn info_row(label: &'static str, value: impl Into<SharedString>, theme: &CarbonTheme) -> Div {
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

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.0), px(680.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| MtrApp::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtr_oudia_application::{BusinessErrorKind, DiagramDto, RouteDto};

    fn dummy_snapshot(routes: Vec<(&str, usize)>) -> SnapshotDto {
        SnapshotDto {
            routes: routes
                .into_iter()
                .enumerate()
                .map(|(idx, (name, station_count))| RouteDto {
                    id: format!("route-{idx}"),
                    name: name.to_string(),
                    stations: Vec::new(),
                    station_count,
                    total_run_millis: 0,
                    total_dwell_millis: 0,
                })
                .collect(),
            dimensions: Vec::new(),
            api_current_time_millis: 1_000,
        }
    }

    fn dummy_inspection(
        file_type: &str,
        line_name: Option<&str>,
        station_count: usize,
    ) -> InspectionDto {
        InspectionDto {
            file_type: file_type.to_string(),
            line_name: line_name.map(str::to_string),
            station_count,
            kijun_status: "valid".to_string(),
            diagrams: vec![
                DiagramDto {
                    index: 0,
                    train_count: 12,
                },
                DiagramDto {
                    index: 1,
                    train_count: 8,
                },
            ],
            train_types: vec![0, 1],
            train_type_names: vec!["普通".to_string(), "急行".to_string()],
            templates: Vec::new(),
        }
    }

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
    fn select_endpoint_clears_old_snapshot_only_when_endpoint_changes() {
        let mut state = MtrShellState::new(Ok(()));
        state.candidates = vec![
            "http://127.0.0.1:49182/".to_string(),
            "http://127.0.0.1:49183/".to_string(),
        ];
        assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
        state.dimension = 2;
        state.apply_snapshot_result(2, Ok(dummy_snapshot(vec![("山手線", 30)])));
        assert!(state.snapshot.is_some());
        assert!(state.is_step_completed(AppStep::Step1Mtr));

        // 同一接続先の再選択では既存のスナップショットを保持する。
        assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
        assert!(state.snapshot.is_some());
        assert_eq!(state.dimension, 2);

        // 別の接続先へ切り替えると旧スナップショットと dimension をクリアし、完了状態を戻す。
        assert!(state.select_endpoint("http://127.0.0.1:49183/".to_string()));
        assert!(state.snapshot.is_none());
        assert_eq!(state.dimension, 0);
        assert!(!state.is_step_completed(AppStep::Step1Mtr));
        assert_eq!(
            state.status,
            AppStatus::DetectedMultiple {
                count: 2,
                selected: "http://127.0.0.1:49183/".to_string(),
            }
        );
    }

    #[test]
    fn apply_detection_result_handles_single_multiple_and_error() {
        let mut state = MtrShellState::new(Ok(()));

        state.apply_detection_result(Ok(vec![EndpointDto {
            url: "http://127.0.0.1:49182/".to_string(),
        }]));
        assert_eq!(
            state.selected_endpoint.as_deref(),
            Some("http://127.0.0.1:49182/")
        );
        assert_eq!(
            state.status,
            AppStatus::DetectedSingle {
                endpoint: "http://127.0.0.1:49182/".to_string(),
            }
        );

        state.apply_snapshot_result(0, Ok(dummy_snapshot(vec![("中央線", 24)])));
        assert!(state.snapshot.is_some());

        // 異なる接続先が検出された場合は旧スナップショットをクリアする。
        state.apply_detection_result(Ok(vec![
            EndpointDto {
                url: "http://127.0.0.1:49183/".to_string(),
            },
            EndpointDto {
                url: "http://127.0.0.1:49184/".to_string(),
            },
        ]));
        assert!(state.snapshot.is_none());
        assert_eq!(state.candidates.len(), 2);
        assert_eq!(
            state.status,
            AppStatus::DetectedMultiple {
                count: 2,
                selected: "http://127.0.0.1:49183/".to_string(),
            }
        );

        state.apply_detection_result(Err(BusinessError {
            kind: BusinessErrorKind::NoEndpoints,
            message: "待受ポートが見つかりません".to_string(),
            detail: Some("Minecraft ログが見つかりません".to_string()),
        }));
        assert_eq!(
            state.status,
            AppStatus::DetectionError {
                title: "待受ポートが見つかりません".to_string(),
                detail: "Minecraft ログが見つかりません".to_string(),
            }
        );
    }

    #[test]
    fn status_notification_maps_statuses_and_business_errors() {
        let init_err = MtrShellState::new(Err("runtime init error".to_string()));
        assert!(init_err.status.is_init_failed());
        assert_eq!(
            status_notification(&init_err.status),
            (
                NotificationKind::Error,
                "バックエンドの初期化に失敗しました".to_string(),
                "runtime init error".to_string(),
            )
        );

        let idle = status_notification(&AppStatus::Idle);
        assert_eq!(idle.0, NotificationKind::Info);
        assert_eq!(idle.1, "待機中");

        let detecting = status_notification(&AppStatus::Detecting);
        assert_eq!(detecting.0, NotificationKind::Info);
        assert_eq!(detecting.1, "検出中");

        let fetching = status_notification(&AppStatus::Fetching {
            endpoint: "http://127.0.0.1:49182/".to_string(),
            dimension: 1,
        });
        assert_eq!(fetching.0, NotificationKind::Info);
        assert_eq!(fetching.1, "取得中");
        assert!(fetching.2.contains("dimension=1"));

        let success = status_notification(&AppStatus::FetchSuccess {
            route_count: 3,
            stop_count: 45,
        });
        assert_eq!(
            success,
            (
                NotificationKind::Success,
                "取得完了".to_string(),
                "3 路線 / 45 駅を取得しました".to_string(),
            )
        );

        let mut state = MtrShellState::new(Ok(()));
        state.apply_snapshot_result(
            1,
            Err(BusinessError {
                kind: BusinessErrorKind::Connection,
                message: "MTR API へ接続できません".to_string(),
                detail: None,
            }),
        );
        assert_eq!(
            status_notification(&state.status),
            (
                NotificationKind::Error,
                "MTR API へ接続できません".to_string(),
                String::new(),
            )
        );
    }

    #[test]
    fn format_dimension_label_handles_string_and_number_values() {
        assert_eq!(format_dimension_label(0, &"\"overworld\""), "0: overworld");
        assert_eq!(format_dimension_label(1, &1), "1: 1");
    }

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
}
