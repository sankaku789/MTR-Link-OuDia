//! Carbon Design System v11 ベースの 5 ステップ画面シェルと Step 1〜5 の実装。
//!
//! バックエンド処理はすべて [`gpui_poc::backend::AppBackend`] 経由で実行する。

use std::path::{Path, PathBuf};

use gpui::{
    App, Application, Bounds, ClickEvent, Context, Div, Entity, FontWeight, Hsla, IntoElement,
    ParentElement, Render, Rgba, ScrollHandle, ScrollWheelEvent, SharedString, Styled,
    Subscription, Window, WindowAppearance, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_poc::backend::{AppBackend, RefreshSnapshotOutcome, format_business_error};
use gpui_poc::carbon::{
    ButtonKind, CarbonSpacing, CarbonTheme, CarbonThemeKind, CarbonTypography, NotificationKind,
    TagKind, TileLayer, carbon_button, carbon_inline_notification, carbon_tag, carbon_tile,
};
use gpui_poc::text_input::{TextInput, TextInputStyle, bind_keys as bind_text_input_keys};
use mtr_oudia_application::{
    BusinessError, CandidateId, EndpointDto, InspectionDto, ManualMappingInput, MtrEndpoint,
    PreviewDto, PreviewId, RouteCandidateDto, RouteDto, SaveReceipt, SessionId, SnapshotDto,
    StationMappingDto, StationSlotDto, TemplateDto,
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

    /// 0 始まりのインデックスを返す。
    pub const fn index(self) -> usize {
        match self {
            Self::Step1Mtr => 0,
            Self::Step2Oudia => 1,
            Self::Step3RouteMatch => 2,
            Self::Step4Preview => 3,
            Self::Step5Save => 4,
        }
    }

    /// 0 始まりのインデックスから対応するステップを返す。
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Step1Mtr),
            1 => Some(Self::Step2Oudia),
            2 => Some(Self::Step3RouteMatch),
            3 => Some(Self::Step4Preview),
            4 => Some(Self::Step5Save),
            _ => None,
        }
    }

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

/// Step 1〜5 の処理状態と通知内容。
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
    RefreshingCache {
        endpoint: String,
        dimension: u32,
    },
    RefreshUpdated {
        route_count: usize,
        stop_count: usize,
    },
    RefreshUnchanged,
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
    FindingCandidates,
    CandidatesFound {
        count: usize,
        auto_selected: bool,
    },
    CandidatesError {
        title: String,
        detail: String,
    },
    Previewing,
    PreviewSuccess {
        stop_count: usize,
        warning_count: usize,
    },
    PreviewError {
        title: String,
        detail: String,
    },
    Saving {
        path: String,
    },
    SaveSuccess {
        output_path: String,
        bytes: u64,
    },
    SaveError {
        title: String,
        detail: String,
    },
}

impl AppStatus {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Detecting
                | Self::Fetching { .. }
                | Self::RefreshingCache { .. }
                | Self::InspectingOudia { .. }
                | Self::FindingCandidates
                | Self::Previewing
                | Self::Saving { .. }
        )
    }

    pub fn is_detecting(&self) -> bool {
        matches!(self, Self::Detecting)
    }

    pub fn is_fetching(&self) -> bool {
        matches!(self, Self::Fetching { .. })
    }

    pub fn is_refreshing(&self) -> bool {
        matches!(self, Self::RefreshingCache { .. })
    }

    pub fn is_inspecting(&self) -> bool {
        matches!(self, Self::InspectingOudia { .. })
    }

    pub fn is_finding_candidates(&self) -> bool {
        matches!(self, Self::FindingCandidates)
    }

    pub fn is_previewing(&self) -> bool {
        matches!(self, Self::Previewing)
    }

    pub fn is_saving(&self) -> bool {
        matches!(self, Self::Saving { .. })
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
        AppStatus::RefreshingCache {
            endpoint,
            dimension,
        } => (
            NotificationKind::Info,
            "キャッシュ待機中".to_string(),
            format!(
                "{endpoint} (dimension={dimension}) の MTR API キャッシュ更新を待っています（最大 30 秒）..."
            ),
        ),
        AppStatus::RefreshUpdated {
            route_count,
            stop_count,
        } => (
            NotificationKind::Success,
            "更新完了".to_string(),
            format!("駅・路線データの変化を検出し、{route_count} 路線 / {stop_count} 駅に更新しました"),
        ),
        AppStatus::RefreshUnchanged => (
            NotificationKind::Info,
            "変化なし（キャッシュ未更新）".to_string(),
            "最大 30 秒待機しましたが駅・路線データに変化はありませんでした。既存の取得結果と作業状態を維持しています。"
                .to_string(),
        ),
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
        AppStatus::FindingCandidates => (
            NotificationKind::Info,
            "照合中".to_string(),
            "MTR 路線と OuDia の列車テンプレート候補を検索しています...".to_string(),
        ),
        AppStatus::CandidatesFound {
            count,
            auto_selected,
        } => {
            if *count == 0 {
                (
                    NotificationKind::Warning,
                    "候補なし".to_string(),
                    "条件に一致する候補が見つかりませんでした。ダイヤや列車種別を変更してください。"
                        .to_string(),
                )
            } else if *auto_selected {
                (
                    NotificationKind::Success,
                    "照合完了".to_string(),
                    format!("{count} 件の候補が見つかり、一致候補を自動選択しました"),
                )
            } else {
                (
                    NotificationKind::Info,
                    "照合完了".to_string(),
                    format!("{count} 件の候補が見つかりました。一覧から候補を選択してください"),
                )
            }
        }
        AppStatus::CandidatesError { title, detail } => {
            (NotificationKind::Error, title.clone(), detail.clone())
        }
        AppStatus::Previewing => (
            NotificationKind::Info,
            "プレビュー生成中".to_string(),
            "選択された照合候補から時刻表プレビューを生成しています...".to_string(),
        ),
        AppStatus::PreviewSuccess {
            stop_count,
            warning_count,
        } => {
            if *warning_count > 0 {
                (
                    NotificationKind::Warning,
                    "プレビュー生成完了（警告あり）".to_string(),
                    format!(
                        "{stop_count} 駅の時刻表を生成しましたが、{warning_count} 件の警告があります"
                    ),
                )
            } else {
                (
                    NotificationKind::Success,
                    "プレビュー生成完了".to_string(),
                    format!("{stop_count} 駅の時刻表プレビューを生成しました"),
                )
            }
        }
        AppStatus::PreviewError { title, detail } => {
            (NotificationKind::Error, title.clone(), detail.clone())
        }
        AppStatus::Saving { path } => (
            NotificationKind::Info,
            "保存中".to_string(),
            format!("{path} に検証付きで保存しています..."),
        ),
        AppStatus::SaveSuccess { output_path, bytes } => (
            NotificationKind::Success,
            "保存完了".to_string(),
            format!("{output_path} に保存しました ({bytes} bytes)"),
        ),
        AppStatus::SaveError { title, detail } => {
            (NotificationKind::Error, title.clone(), detail.clone())
        }
    }
}

/// アプリケーションのテーマ設定モード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreference {
    /// OS の外観設定に自動で追従する。
    #[default]
    System,
    /// ライトモードに固定する。
    Light,
    /// ダークモードに固定する。
    Dark,
}

fn is_dark_appearance(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

fn resolve_theme_kind(preference: ThemePreference, os_dark: bool) -> CarbonThemeKind {
    match preference {
        ThemePreference::System => {
            if os_dark {
                CarbonThemeKind::Gray100
            } else {
                CarbonThemeKind::Gray10
            }
        }
        ThemePreference::Light => CarbonThemeKind::Gray10,
        ThemePreference::Dark => CarbonThemeKind::Gray100,
    }
}

fn theme_badge_label(preference: ThemePreference, kind: CarbonThemeKind) -> &'static str {
    match (preference, kind) {
        (ThemePreference::System, CarbonThemeKind::Gray100) => "OS 連動・ダーク",
        (ThemePreference::System, CarbonThemeKind::Gray10) => "OS 連動・ライト",
        (ThemePreference::Dark, _) | (_, CarbonThemeKind::Gray100) => "ダーク",
        (ThemePreference::Light, _) => "ライト",
    }
}

fn dark_mode_button_label(kind: CarbonThemeKind) -> &'static str {
    match kind {
        CarbonThemeKind::Gray100 => "ダークモード: ON",
        CarbonThemeKind::Gray10 => "ダークモード: OFF",
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

fn refresh_button_label(refreshing: bool) -> &'static str {
    if refreshing {
        "キャッシュ待機中..."
    } else {
        "キャッシュ待機して再取得"
    }
}

fn inspect_button_label(inspecting: bool) -> &'static str {
    if inspecting {
        "解析中..."
    } else {
        "OuDia ファイルを選択"
    }
}

fn find_candidates_button_label(finding: bool) -> &'static str {
    if finding {
        "検索中..."
    } else {
        "候補を検索"
    }
}

fn preview_button_label(previewing: bool) -> &'static str {
    if previewing {
        "生成中..."
    } else {
        "プレビュー生成"
    }
}

fn save_button_label(saving: bool) -> &'static str {
    if saving {
        "保存中..."
    } else {
        "保存先を選んで保存"
    }
}

/// 元 OuDia パスから保存先ダイアログの初期ディレクトリと既定ファイル名を生成する。
pub fn suggest_save_location(oudia_path: Option<&Path>) -> (PathBuf, Option<String>) {
    let Some(path) = oudia_path else {
        return (PathBuf::from("."), None);
    };
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let suggested_name = path.file_stem().map(|stem| {
        let stem = stem.to_string_lossy();
        match path.extension() {
            Some(ext) if !ext.is_empty() => format!("{stem}_converted.{}", ext.to_string_lossy()),
            _ => format!("{stem}_converted"),
        }
    });
    (dir, suggested_name)
}

fn format_arrival_departure(arrival: Option<&str>, departure: Option<&str>) -> String {
    match (arrival, departure) {
        (Some(arr), Some(dep)) => format!("{arr} / {dep}"),
        (Some(arr), None) => format!("{arr} / -"),
        (None, Some(dep)) => format!("- / {dep}"),
        (None, None) => "-".to_string(),
    }
}

fn format_dwell_seconds(dwell_millis: i64) -> String {
    if dwell_millis % 1000 == 0 {
        format!("{} 秒", dwell_millis / 1000)
    } else {
        let sign = if dwell_millis < 0 { "-" } else { "" };
        let abs = dwell_millis.unsigned_abs();
        let whole = abs / 1000;
        let frac = abs % 1000;
        let formatted = format!("{sign}{whole}.{frac:03}");
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        format!("{trimmed} 秒")
    }
}

fn direction_label(direction: &str) -> &str {
    match direction {
        "kudari" => "下り",
        "nobori" => "上り",
        other => other,
    }
}

fn candidate_rank_badge(rank: &str) -> (&str, TagKind) {
    match rank {
        "PrimaryExact" => ("完全一致", TagKind::Green),
        "MultilingualExact" => ("多言語一致", TagKind::Green),
        "AliasExact" => ("別名一致", TagKind::Blue),
        "CollapsedDuplicateExact" => ("重複統合一致", TagKind::Blue),
        "Partial" => ("部分一致", TagKind::WarmGray),
        "manual" => ("手動対応", TagKind::CoolGray),
        other => (other, TagKind::CoolGray),
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

/// 列車番号入力文字列をトリムし、空文字なら `None` に正規化する。
pub fn normalize_train_number_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn with_alpha_hsla(color: Rgba, alpha: f32) -> Hsla {
    let mut hsla: Hsla = color.into();
    hsla.a = alpha;
    hsla
}

fn carbon_text_input_style(theme: &CarbonTheme) -> TextInputStyle {
    let c = &theme.colors;
    TextInputStyle {
        bg: c.layer_02.into(),
        outer_bg: TRANSPARENT.into(),
        border_color: Some(c.border_strong.into()),
        focus_border_color: Some(c.focus.into()),
        text_color: c.text_primary.into(),
        placeholder_color: with_alpha_hsla(c.text_secondary, 0.7),
        cursor_color: c.button_primary.into(),
        text_size: CarbonTypography::BODY_01.font_size_px(),
        line_height: CarbonTypography::BODY_01.line_height_px(),
        height: px(40.0),
        padding_x: CarbonSpacing::px(CarbonSpacing::SP_04),
        padding_y: px(10.0),
    }
}

fn format_station_slot_label(slot: &StationSlotDto) -> String {
    let name = if slot.name.is_empty() {
        "名称なし"
    } else {
        slot.name.as_str()
    };
    let kind = match slot.handling_code {
        Some(1) => "停車",
        Some(2) => "通過",
        Some(3) => "経由なし",
        _ => "未指定",
    };
    format!("#{} {name} ({kind})", slot.index)
}

fn station_mapping_issues(
    route: &RouteDto,
    template: &TemplateDto,
    mappings: &[StationMappingDto],
) -> Vec<String> {
    let mut issues = Vec::new();
    let mut slot_to_stations: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();

    for (mtr_idx, station) in route.stations.iter().enumerate() {
        let assigned: Vec<usize> = mappings
            .iter()
            .filter(|m| m.mtr_station_index == mtr_idx)
            .map(|m| m.oudia_station_slot)
            .collect();
        if assigned.is_empty() {
            issues.push(format!("MTR 駅「{}」が未割当です", station.station_name));
        }
        for slot in assigned {
            slot_to_stations.entry(slot).or_default().push(mtr_idx);
        }
    }

    for slot in &template.active_station_slots {
        match slot_to_stations.get(&slot.index) {
            None => {
                let name = if slot.name.is_empty() {
                    "名称なし"
                } else {
                    slot.name.as_str()
                };
                issues.push(format!(
                    "OuDia 駅スロット #{}「{name}」が未割当です",
                    slot.index
                ));
            }
            Some(stations) if stations.len() > 1 => {
                issues.push(format!(
                    "OuDia 駅スロット #{} が複数の MTR 駅に重複割当されています",
                    slot.index
                ));
            }
            _ => {}
        }
    }

    issues
}

/// スクロール位置から現在表示中のステップを判定する。
///
/// 末尾付近までスクロールされている場合は最後のステップ (`Step5Save`) を優先する。
pub fn step_for_scroll_position(top_item: usize, offset_y: f32, max_offset_height: f32) -> AppStep {
    if max_offset_height > 0.0 && -offset_y >= max_offset_height - 4.0 {
        return AppStep::Step5Save;
    }
    AppStep::from_index(top_item).unwrap_or(AppStep::Step1Mtr)
}

/// UI から独立してテスト可能なアプリケーション状態。
struct MtrShellState {
    theme_preference: ThemePreference,
    os_dark: bool,
    theme: CarbonTheme,
    current_step: AppStep,
    candidates: Vec<String>,
    selected_endpoint: Option<String>,
    dimension: u32,
    snapshot: Option<SnapshotDto>,
    oudia_path: Option<PathBuf>,
    inspection: Option<InspectionDto>,
    selected_route_id: Option<String>,
    selected_diagram_index: Option<usize>,
    selected_train_type: Option<usize>,
    route_candidates: Vec<RouteCandidateDto>,
    selected_candidate_id: Option<CandidateId>,
    manual_station_mappings: Option<Vec<StationMappingDto>>,
    preview: Option<PreviewDto>,
    save_receipt: Option<SaveReceipt>,
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
        let theme_preference = ThemePreference::default();
        let os_dark = true;
        Self {
            theme_preference,
            os_dark,
            theme: CarbonTheme::from_kind(resolve_theme_kind(theme_preference, os_dark)),
            current_step: AppStep::Step1Mtr,
            candidates: Vec::new(),
            selected_endpoint: None,
            dimension: 0,
            snapshot: None,
            oudia_path: None,
            inspection: None,
            selected_route_id: None,
            selected_diagram_index: None,
            selected_train_type: None,
            route_candidates: Vec::new(),
            selected_candidate_id: None,
            manual_station_mappings: None,
            preview: None,
            save_receipt: None,
            status,
        }
    }

    fn sync_theme(&mut self) {
        self.theme =
            CarbonTheme::from_kind(resolve_theme_kind(self.theme_preference, self.os_dark));
    }

    fn set_os_dark(&mut self, os_dark: bool) {
        self.os_dark = os_dark;
        self.sync_theme();
    }

    fn set_theme_preference(&mut self, preference: ThemePreference) {
        self.theme_preference = preference;
        self.sync_theme();
    }

    fn toggle_dark_mode(&mut self) {
        let next = match self.theme.kind {
            CarbonThemeKind::Gray100 => ThemePreference::Light,
            CarbonThemeKind::Gray10 => ThemePreference::Dark,
        };
        self.set_theme_preference(next);
    }

    fn is_step_completed(&self, step: AppStep) -> bool {
        match step {
            AppStep::Step1Mtr => self.snapshot.is_some(),
            AppStep::Step2Oudia => self.inspection.is_some(),
            AppStep::Step3RouteMatch => self.selected_candidate_id.is_some(),
            AppStep::Step4Preview => self.preview.is_some(),
            AppStep::Step5Save => self.save_receipt.is_some(),
        }
    }

    fn select_step(&mut self, step: AppStep) {
        self.current_step = step;
    }

    fn clear_candidate_selection(&mut self) {
        self.route_candidates.clear();
        self.selected_candidate_id = None;
        self.manual_station_mappings = None;
        self.preview = None;
        self.save_receipt = None;
    }

    fn clear_step3_for_snapshot_change(&mut self) {
        self.selected_route_id = None;
        self.clear_candidate_selection();
    }

    fn clear_step3_for_inspection_change(&mut self) {
        self.selected_diagram_index = None;
        self.selected_train_type = None;
        self.clear_candidate_selection();
    }

    /// 接続先を選択し、異なる接続先へ切り替わった場合は旧 `snapshot` と下流状態をクリアする。
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
            self.clear_step3_for_snapshot_change();
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
                    self.clear_candidate_selection();
                    self.status = AppStatus::DetectedSingle { endpoint };
                } else {
                    let selected = urls[0].clone();
                    let count = urls.len();
                    self.candidates = urls;
                    self.apply_selected_endpoint(selected.clone());
                    self.clear_candidate_selection();
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
                self.clear_step3_for_snapshot_change();
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

    fn apply_refresh_snapshot_result(
        &mut self,
        dimension: u32,
        result: Result<RefreshSnapshotOutcome, BusinessError>,
    ) {
        match result {
            Ok(RefreshSnapshotOutcome::Updated(dto)) => {
                // 本セッション側は commit 時点で invalidate 済みのため、UI 側も必ず同期して下流をクリアする。
                let route_count = dto.routes.len();
                let stop_count: usize = dto.routes.iter().map(|route| route.station_count).sum();
                self.dimension = dimension;
                self.snapshot = Some(dto);
                self.clear_step3_for_snapshot_change();
                self.status = AppStatus::RefreshUpdated {
                    route_count,
                    stop_count,
                };
            }
            Ok(RefreshSnapshotOutcome::Unchanged) => {
                self.status = AppStatus::RefreshUnchanged;
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
                self.clear_step3_for_inspection_change();
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

    fn select_route(&mut self, route_id: String) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.selected_route_id.as_deref() != Some(route_id.as_str()) {
            self.selected_route_id = Some(route_id);
            self.clear_candidate_selection();
        }
        true
    }

    fn select_diagram_index(&mut self, diagram_index: Option<usize>) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.selected_diagram_index != diagram_index {
            self.selected_diagram_index = diagram_index;
            self.clear_candidate_selection();
        }
        true
    }

    fn select_train_type(&mut self, train_type: Option<usize>) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.selected_train_type != train_type {
            self.selected_train_type = train_type;
            self.clear_candidate_selection();
        }
        true
    }

    fn select_candidate(&mut self, candidate_id: CandidateId) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.route_candidates.iter().any(|c| c.id == candidate_id) {
            if self.selected_candidate_id.as_ref() != Some(&candidate_id) {
                self.selected_candidate_id = Some(candidate_id);
                self.manual_station_mappings = None;
                self.preview = None;
                self.save_receipt = None;
            }
            true
        } else {
            false
        }
    }

    fn selected_route(&self) -> Option<&RouteDto> {
        let route_id = self.selected_route_id.as_deref()?;
        self.snapshot
            .as_ref()?
            .routes
            .iter()
            .find(|route| route.id == route_id)
    }

    fn selected_candidate(&self) -> Option<&RouteCandidateDto> {
        let candidate_id = self.selected_candidate_id.as_ref()?;
        self.route_candidates
            .iter()
            .find(|candidate| &candidate.id == candidate_id)
    }

    fn selected_template(&self) -> Option<&TemplateDto> {
        let candidate = self.selected_candidate()?;
        self.inspection.as_ref()?.templates.iter().find(|template| {
            template.diagram_index == candidate.diagram_index
                && template.train_index == candidate.train_index
                && template.direction == candidate.direction
        })
    }

    fn effective_station_mappings(&self) -> Option<&[StationMappingDto]> {
        if let Some(manual) = self.manual_station_mappings.as_deref() {
            Some(manual)
        } else {
            self.selected_candidate()
                .map(|candidate| candidate.station_mappings.as_slice())
        }
    }

    fn set_manual_station_slot(
        &mut self,
        mtr_station_index: usize,
        oudia_station_slot: usize,
    ) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        let Some(candidate) = self.selected_candidate() else {
            return false;
        };
        let mut mappings = self
            .manual_station_mappings
            .clone()
            .unwrap_or_else(|| candidate.station_mappings.clone());

        let already_assigned = mappings.iter().any(|m| {
            m.mtr_station_index == mtr_station_index && m.oudia_station_slot == oudia_station_slot
        });
        if already_assigned {
            mappings.retain(|m| {
                !(m.mtr_station_index == mtr_station_index
                    && m.oudia_station_slot == oudia_station_slot)
            });
        } else {
            mappings.retain(|m| m.oudia_station_slot != oudia_station_slot);
            mappings.push(StationMappingDto {
                mtr_station_index,
                oudia_station_slot,
            });
        }
        mappings.sort_by_key(|m| (m.mtr_station_index, m.oudia_station_slot));

        self.manual_station_mappings = Some(mappings);
        self.preview = None;
        self.save_receipt = None;
        true
    }

    fn reset_manual_station_mappings(&mut self) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.manual_station_mappings.take().is_some() {
            self.preview = None;
            self.save_receipt = None;
            true
        } else {
            false
        }
    }

    fn apply_candidates_result(&mut self, result: Result<Vec<RouteCandidateDto>, BusinessError>) {
        match result {
            Ok(candidates) => {
                let count = candidates.len();
                let auto_selected_id = candidates
                    .iter()
                    .find(|candidate| candidate.auto_selected)
                    .map(|candidate| candidate.id.clone());
                let auto_selected = auto_selected_id.is_some();
                self.route_candidates = candidates;
                self.selected_candidate_id = auto_selected_id;
                self.manual_station_mappings = None;
                self.preview = None;
                self.save_receipt = None;
                self.status = AppStatus::CandidatesFound {
                    count,
                    auto_selected,
                };
            }
            Err(err) => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::CandidatesError { title, detail };
            }
        }
    }

    fn apply_preview_result(&mut self, result: Result<PreviewDto, BusinessError>) {
        match result {
            Ok(dto) => {
                let stop_count = dto.stops.len();
                let warning_count = dto.warnings.len();
                self.preview = Some(dto);
                self.save_receipt = None;
                self.status = AppStatus::PreviewSuccess {
                    stop_count,
                    warning_count,
                };
            }
            Err(err) => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::PreviewError { title, detail };
            }
        }
    }

    fn apply_save_result(&mut self, result: Result<SaveReceipt, BusinessError>) {
        match result {
            Ok(receipt) => {
                let output_path = receipt.output_path.clone();
                let bytes = receipt.bytes;
                self.save_receipt = Some(receipt);
                self.status = AppStatus::SaveSuccess { output_path, bytes };
            }
            Err(err) => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::SaveError { title, detail };
            }
        }
    }
}

struct MtrApp {
    backend: Option<AppBackend>,
    session_id: Option<SessionId>,
    state: MtrShellState,
    train_number_input: Entity<TextInput>,
    steps_scroll_handle: ScrollHandle,
    sync_step_from_scroll: bool,
    _appearance_subscription: Subscription,
}

impl MtrApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let os_dark = is_dark_appearance(window.appearance());
        let appearance_subscription = cx.observe_window_appearance(window, |this, window, cx| {
            this.state
                .set_os_dark(is_dark_appearance(window.appearance()));
            this.sync_input_theme(cx);
            cx.notify();
        });

        let init_result = AppBackend::new();
        let (backend, session_id, mut state) = match init_result {
            Ok(backend) => {
                let session_id = backend.create_session();
                (Some(backend), Some(session_id), MtrShellState::new(Ok(())))
            }
            Err(err) => (None, None, MtrShellState::new(Err(err))),
        };
        state.set_os_dark(os_dark);

        let input_style = carbon_text_input_style(&state.theme);
        let train_number_input = cx.new(|cx| {
            TextInput::new("列車番号（任意・指定時は全ダイヤ横断）", cx).with_style(input_style)
        });

        Self {
            backend,
            session_id,
            state,
            train_number_input,
            steps_scroll_handle: ScrollHandle::new(),
            sync_step_from_scroll: false,
            _appearance_subscription: appearance_subscription,
        }
    }

    fn sync_input_theme(&self, cx: &mut Context<Self>) {
        let style = carbon_text_input_style(&self.state.theme);
        self.train_number_input
            .update(cx, |input, cx| input.set_style(style, cx));
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

    fn start_refresh_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let Some(previous) = self.state.snapshot.clone() else {
            return;
        };
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

        let dimension = self.state.dimension;
        self.state.status = AppStatus::RefreshingCache {
            endpoint: endpoint_str,
            dimension,
        };
        cx.notify();

        let rx = backend.refresh_mtr_snapshot(session_id, endpoint, dimension, previous);
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                match received {
                    Ok(result) => this.state.apply_refresh_snapshot_result(dimension, result),
                    Err(_) => {
                        this.state.status = AppStatus::FetchError {
                            title: "キャッシュ待機再取得が中断されました".to_string(),
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

    fn start_find_candidates(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let Some(route_id) = self.state.selected_route_id.clone() else {
            self.state.status = AppStatus::CandidatesError {
                title: "MTR 路線が未選択です".to_string(),
                detail: "左の一覧から照合対象の MTR 路線を選択してください。".to_string(),
            };
            cx.notify();
            return;
        };
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        let diagram_index = self.state.selected_diagram_index;
        let train_type = self.state.selected_train_type;
        let train_number = normalize_train_number_input(self.train_number_input.read(cx).text());
        self.state.status = AppStatus::FindingCandidates;
        cx.notify();

        let rx = backend.find_route_candidates(
            session_id,
            route_id,
            diagram_index,
            train_type,
            train_number,
        );
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                match received {
                    Ok(result) => this.state.apply_candidates_result(result),
                    Err(_) => {
                        this.state.status = AppStatus::CandidatesError {
                            title: "候補検索が中断されました".to_string(),
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

    fn start_preview(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let Some(candidate_id) = self.state.selected_candidate_id.clone() else {
            self.state.status = AppStatus::PreviewError {
                title: "照合候補が未選択です".to_string(),
                detail: "Step 3 で照合候補を選択してください。".to_string(),
            };
            cx.notify();
            return;
        };
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        let manual = self
            .state
            .manual_station_mappings
            .clone()
            .map(|station_mappings| ManualMappingInput { station_mappings });
        self.state.status = AppStatus::Previewing;
        cx.notify();

        let rx = backend.preview_conversion(session_id, candidate_id, manual);
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                match received {
                    Ok(result) => this.state.apply_preview_result(result),
                    Err(_) => {
                        this.state.status = AppStatus::PreviewError {
                            title: "プレビュー生成が中断されました".to_string(),
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

    fn start_select_save_path(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let Some(preview_id) = self.state.preview.as_ref().map(|p| p.id.clone()) else {
            self.state.status = AppStatus::SaveError {
                title: "プレビューが未生成です".to_string(),
                detail: "Step 4 で時刻表プレビューを生成してから保存してください。".to_string(),
            };
            cx.notify();
            return;
        };

        let (dir, suggested_name) = suggest_save_location(self.state.oudia_path.as_deref());
        let rx = cx.prompt_for_new_path(&dir, suggested_name.as_deref());

        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| match received {
                Ok(Ok(Some(path))) => {
                    this.start_save(preview_id, path, cx);
                }
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(err)) => {
                    this.state.status = AppStatus::SaveError {
                        title: "保存先選択ダイアログを開けませんでした".to_string(),
                        detail: err.to_string(),
                    };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn start_save(&mut self, preview_id: PreviewId, output_path: PathBuf, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        if self.state.preview.as_ref().map(|p| &p.id) != Some(&preview_id) {
            return;
        }
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        self.state.status = AppStatus::Saving {
            path: output_path.display().to_string(),
        };
        cx.notify();

        let rx = backend.save_preview(session_id, preview_id, output_path);
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                match received {
                    Ok(result) => this.state.apply_save_result(result),
                    Err(_) => {
                        this.state.status = AppStatus::SaveError {
                            title: "保存処理が中断されました".to_string(),
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
                        theme_badge_label(self.state.theme_preference, theme.kind),
                        TagKind::CoolGray,
                        theme,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .child(carbon_button(
                        "toggle-dark-mode",
                        dark_mode_button_label(theme.kind),
                        if self.state.theme_preference == ThemePreference::System {
                            ButtonKind::Ghost
                        } else {
                            ButtonKind::Secondary
                        },
                        false,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.state.toggle_dark_mode();
                            this.sync_input_theme(cx);
                            cx.notify();
                        }),
                    ))
                    .child(carbon_button(
                        "follow-os-theme",
                        "OS に合わせる",
                        if self.state.theme_preference == ThemePreference::System {
                            ButtonKind::Secondary
                        } else {
                            ButtonKind::Ghost
                        },
                        false,
                        theme,
                        cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.state
                                .set_os_dark(is_dark_appearance(window.appearance()));
                            this.state.set_theme_preference(ThemePreference::System);
                            this.sync_input_theme(cx);
                            cx.notify();
                        }),
                    )),
            )
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
                    this.sync_step_from_scroll = false;
                    this.state.select_step(step);
                    this.steps_scroll_handle.scroll_to_top_of_item(step.index());
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
        let refreshing = self.state.status.is_refreshing();
        let refresh_disabled = disabled || self.state.snapshot.is_none();

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
                    .flex_wrap()
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
                            .flex_wrap()
                            .items_center()
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
                            ))
                            .child(carbon_button(
                                "refresh-snapshot",
                                refresh_button_label(refreshing),
                                ButtonKind::Secondary,
                                refresh_disabled,
                                theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.start_refresh_snapshot(cx);
                                }),
                            )),
                    ),
            )
            .child(
                div()
                    .text_size(CarbonTypography::LABEL_01.font_size_px())
                    .line_height(CarbonTypography::LABEL_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(
                        "MTR API は駅・路線情報を最大 30 秒キャッシュします。Minecraft 側で路線や駅を変更した直後に反映されない場合は「キャッシュ待機して再取得」を使ってください。",
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
                } else if refreshing {
                    "MTR API キャッシュの更新を待機しています..."
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
            .min_w_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(connection_tile)
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
            .min_w_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(file_tile)
            .child(inspection_tile)
    }

    fn render_step3(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;
        let busy = self.state.status.is_busy();
        let disabled = busy || self.state.status.is_init_failed();
        let finding = self.state.status.is_finding_candidates();

        let (Some(snapshot), Some(inspection)) = (&self.state.snapshot, &self.state.inspection)
        else {
            let (title, subtitle) = match (&self.state.snapshot, &self.state.inspection) {
                (None, None) => (
                    "Step 1・Step 2 が未完了です",
                    "Step 1（MTR 接続）でスナップショットを取得し、Step 2（OuDia 選択）で OuDia ファイルを解析してください。",
                ),
                (None, Some(_)) => (
                    "Step 1（MTR 接続）が未完了です",
                    "Step 1 で MTR 接続先からスナップショットを取得してから路線を照合してください。",
                ),
                (Some(_), None) => (
                    "Step 2（OuDia 選択）が未完了です",
                    "Step 2 で OuDia (.oud2) ファイルを選択・解析してから路線を照合してください。",
                ),
                (Some(_), Some(_)) => unreachable!(),
            };
            return div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
                .child(carbon_inline_notification(
                    NotificationKind::Info,
                    title,
                    subtitle,
                    theme,
                ));
        };

        let route_list_content = if snapshot.routes.is_empty() {
            div()
                .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                .text_size(CarbonTypography::BODY_01.font_size_px())
                .line_height(CarbonTypography::BODY_01.line_height_px())
                .text_color(c.text_secondary)
                .child("取得されたスナップショットに路線が含まれていません")
                .into_any_element()
        } else {
            let active_route_id = self.state.selected_route_id.as_deref();
            let hover_bg = c.layer_hover_01;
            div()
                .id("step3-routes")
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                .children(snapshot.routes.iter().enumerate().map(|(index, route)| {
                    let is_selected = active_route_id == Some(route.id.as_str());
                    let route_id = route.id.clone();
                    let bg = if is_selected {
                        c.layer_hover_01
                    } else {
                        c.layer_02
                    };
                    let border_color = if is_selected {
                        c.button_primary
                    } else {
                        c.border_subtle
                    };

                    let mut row = div()
                        .id(SharedString::from(format!("step3-route-{index}")))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .min_w_0()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                        .px(CarbonSpacing::px(CarbonSpacing::SP_04))
                        .py(CarbonSpacing::px(CarbonSpacing::SP_03))
                        .bg(bg)
                        .border_1()
                        .border_color(border_color)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_weight(if is_selected {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_size(CarbonTypography::BODY_01.font_size_px())
                                .line_height(CarbonTypography::BODY_01.line_height_px())
                                .text_color(c.text_primary)
                                .child(format!("{}. {}", index + 1, route.name)),
                        );

                    let mut tags = div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_02));
                    if is_selected {
                        tags = tags.child(carbon_tag("選択中", TagKind::Blue, theme));
                    }
                    tags = tags.child(carbon_tag(
                        format!("{} 駅", route.station_count),
                        TagKind::CoolGray,
                        theme,
                    ));
                    row = row.child(tags);

                    if disabled {
                        row.cursor_not_allowed()
                    } else {
                        row.cursor_pointer()
                            .hover(move |style| style.bg(hover_bg))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                if this.state.select_route(route_id.clone()) {
                                    cx.notify();
                                }
                            }))
                    }
                }))
                .into_any_element()
        };

        let routes_tile = carbon_tile(TileLayer::Layer01, theme)
            .flex_1()
            .min_w_0()
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
                            .child("MTR 路線選択"),
                    )
                    .child(carbon_tag(
                        format!("{} 路線", snapshot.routes.len()),
                        TagKind::CoolGray,
                        theme,
                    )),
            )
            .child(route_list_content);

        let diagram_buttons = std::iter::once(carbon_button(
            "step3-diagram-kijun",
            "基準ダイヤ",
            if self.state.selected_diagram_index.is_none() {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            },
            disabled,
            theme,
            cx.listener(|this, _: &ClickEvent, _window, cx| {
                if this.state.select_diagram_index(None) {
                    cx.notify();
                }
            }),
        ))
        .chain(inspection.diagrams.iter().map(|diagram| {
            let diagram_idx = diagram.index;
            let is_selected = self.state.selected_diagram_index == Some(diagram_idx);
            carbon_button(
                SharedString::from(format!("step3-diagram-{diagram_idx}")),
                format!("ダイヤ #{diagram_idx} ({} 本)", diagram.train_count),
                if is_selected {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Secondary
                },
                disabled,
                theme,
                cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    if this.state.select_diagram_index(Some(diagram_idx)) {
                        cx.notify();
                    }
                }),
            )
        }));

        let train_type_buttons =
            std::iter::once(carbon_button(
                "step3-train-type-all",
                "すべて",
                if self.state.selected_train_type.is_none() {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Secondary
                },
                disabled,
                theme,
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    if this.state.select_train_type(None) {
                        cx.notify();
                    }
                }),
            ))
            .chain(inspection.train_type_names.iter().enumerate().map(
                |(index, name)| {
                    let is_selected = self.state.selected_train_type == Some(index);
                    let label = if name.is_empty() {
                        format!("種別 {index}")
                    } else {
                        name.clone()
                    };
                    carbon_button(
                        SharedString::from(format!("step3-train-type-{index}")),
                        label,
                        if is_selected {
                            ButtonKind::Primary
                        } else {
                            ButtonKind::Secondary
                        },
                        disabled,
                        theme,
                        cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            if this.state.select_train_type(Some(index)) {
                                cx.notify();
                            }
                        }),
                    )
                },
            ));

        let search_disabled = disabled || self.state.selected_route_id.is_none();
        let filter_tile = carbon_tile(TileLayer::Layer01, theme)
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
                            .child("ダイヤ・種別フィルタ"),
                    )
                    .child(carbon_button(
                        "step3-find-candidates",
                        find_candidates_button_label(finding),
                        ButtonKind::Primary,
                        search_disabled,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_find_candidates(cx);
                        }),
                    )),
            )
            .child(
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
                            .child("ダイヤ"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .children(diagram_buttons),
                    ),
            )
            .child(
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
                            .child("列車種別"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .children(train_type_buttons),
                    ),
            )
            .child(
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
                            .child("列車番号"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .min_w_0()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.train_number_input.clone()),
                            )
                            .child(carbon_button(
                                "step3-clear-train-number",
                                "クリア",
                                ButtonKind::Ghost,
                                disabled || self.train_number_input.read(cx).text().is_empty(),
                                theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.train_number_input
                                        .update(cx, |input, cx| input.clear(cx));
                                    cx.notify();
                                }),
                            )),
                    ),
            );

        let candidate_list_content = if self.state.route_candidates.is_empty() {
            div()
                .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                .text_size(CarbonTypography::BODY_01.font_size_px())
                .line_height(CarbonTypography::BODY_01.line_height_px())
                .text_color(c.text_secondary)
                .child(if finding {
                    "候補を検索しています..."
                } else if self.state.selected_route_id.is_none() {
                    "左の一覧から MTR 路線を選択し、「候補を検索」を押してください"
                } else if matches!(
                    self.state.status,
                    AppStatus::CandidatesFound { count: 0, .. }
                ) {
                    "条件に一致する候補はありませんでした"
                } else {
                    "「候補を検索」を押すと照合候補が表示されます"
                })
                .into_any_element()
        } else {
            let active_candidate_id = self.state.selected_candidate_id.as_ref();
            let hover_bg = c.layer_hover_01;
            div()
                .id("step3-candidates")
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                .children(self.state.route_candidates.iter().enumerate().map(
                    |(index, candidate)| {
                        let is_selected = active_candidate_id == Some(&candidate.id);
                        let candidate_id = candidate.id.clone();
                        let (rank_label, rank_kind) = candidate_rank_badge(&candidate.rank);
                        let direction = direction_label(&candidate.direction);
                        let reason = candidate
                            .reasons
                            .first()
                            .map(String::as_str)
                            .unwrap_or("照合詳細なし")
                            .to_string();
                        let bg = if is_selected {
                            c.layer_hover_01
                        } else {
                            c.layer_02
                        };
                        let border_color = if is_selected {
                            c.button_primary
                        } else {
                            c.border_subtle
                        };

                        let mut header_tags = div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(carbon_tag(rank_label.to_string(), rank_kind, theme));
                        if candidate.auto_selected {
                            header_tags =
                                header_tags.child(carbon_tag("自動選択", TagKind::Blue, theme));
                        }
                        if is_selected {
                            header_tags =
                                header_tags.child(carbon_tag("選択中", TagKind::Green, theme));
                        }
                        header_tags = header_tags.child(carbon_tag(
                            format!("{} 駅一致", candidate.station_mappings.len()),
                            TagKind::CoolGray,
                            theme,
                        ));

                        let card = div()
                            .id(SharedString::from(format!("step3-candidate-{index}")))
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .px(CarbonSpacing::px(CarbonSpacing::SP_04))
                            .py(CarbonSpacing::px(CarbonSpacing::SP_03))
                            .bg(bg)
                            .border_1()
                            .border_color(border_color)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .min_w_0()
                                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(CarbonTypography::BODY_01.font_size_px())
                                            .line_height(CarbonTypography::BODY_01.line_height_px())
                                            .text_color(c.text_primary)
                                            .child(format!(
                                                "{direction} / ダイヤ #{} ・ 列車 #{}",
                                                candidate.diagram_index, candidate.train_index
                                            )),
                                    )
                                    .child(header_tags),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(CarbonTypography::LABEL_01.font_size_px())
                                    .line_height(CarbonTypography::LABEL_01.line_height_px())
                                    .text_color(c.text_secondary)
                                    .child(reason),
                            );

                        if disabled {
                            card.cursor_not_allowed()
                        } else {
                            card.cursor_pointer()
                                .hover(move |style| style.bg(hover_bg))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                    if this.state.select_candidate(candidate_id.clone()) {
                                        cx.notify();
                                    }
                                }))
                        }
                    },
                ))
                .into_any_element()
        };

        let candidates_tile = carbon_tile(TileLayer::Layer01, theme)
            .min_w_0()
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
                            .child("照合候補一覧"),
                    )
                    .child(carbon_tag(
                        format!("{} 件", self.state.route_candidates.len()),
                        TagKind::CoolGray,
                        theme,
                    )),
            )
            .child(candidate_list_content);

        let mapping_tile = self.render_step3_station_mapping_tile(disabled, cx);

        div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
                    .child(routes_tile)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
                            .child(filter_tile)
                            .child(candidates_tile),
                    ),
            )
            .child(mapping_tile)
    }

    fn render_step3_station_mapping_tile(&self, disabled: bool, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;
        let has_manual_edits = self.state.manual_station_mappings.is_some();

        let (status_label, status_kind) = if let Some(candidate) = self.state.selected_candidate() {
            if has_manual_edits {
                ("手動編集あり", TagKind::Blue)
            } else if candidate.manual_only {
                ("手動対応が必要", TagKind::WarmGray)
            } else {
                ("自動マッピング", TagKind::Green)
            }
        } else {
            ("候補未選択", TagKind::CoolGray)
        };

        let mut tile = carbon_tile(TileLayer::Layer01, theme)
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
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
                                    .line_height(CarbonTypography::HEADING_02.line_height_px())
                                    .text_color(c.text_primary)
                                    .child("駅マッピング編集"),
                            )
                            .child(carbon_tag(status_label, status_kind, theme)),
                    )
                    .child(carbon_button(
                        "step3-reset-manual-mapping",
                        "自動マッピングに戻す",
                        ButtonKind::Ghost,
                        disabled || !has_manual_edits,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            if this.state.reset_manual_station_mappings() {
                                cx.notify();
                            }
                        }),
                    )),
            );

        let (Some(candidate), Some(route)) =
            (self.state.selected_candidate(), self.state.selected_route())
        else {
            return tile.child(
                div()
                    .py(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(
                        "照合候補を選択すると、MTR 駅と OuDia 駅スロットの対応を確認・編集できます",
                    ),
            );
        };

        let Some(template) = self.state.selected_template() else {
            return tile.child(
                div()
                    .py(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(format!(
                        "対象テンプレート（{} / ダイヤ #{} ・ 列車 #{}）の駅スロット情報が見つかりません",
                        direction_label(&candidate.direction),
                        candidate.diagram_index,
                        candidate.train_index
                    )),
            );
        };

        let effective = self.state.effective_station_mappings().unwrap_or(&[]);
        tile = tile.child(info_row(
            "対象テンプレート:",
            format!(
                "{} / ダイヤ #{} ・ 列車 #{}（有効スロット {} 件）",
                direction_label(&candidate.direction),
                candidate.diagram_index,
                candidate.train_index,
                template.active_station_slots.len()
            ),
            theme,
        ));

        let issues = station_mapping_issues(route, template, effective);
        if !issues.is_empty() {
            tile = tile.child(carbon_inline_notification(
                NotificationKind::Warning,
                "駅対応の確認が必要です",
                issues.join(" / "),
                theme,
            ));
        }

        if route.stations.is_empty() || template.active_station_slots.is_empty() {
            return tile.child(
                div()
                    .py(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child("対応付け可能な駅または OuDia 駅スロットがありません"),
            );
        }

        let rows = route.stations.iter().enumerate().map(|(mtr_idx, station)| {
            let mapped_slots: Vec<usize> = effective
                .iter()
                .filter(|m| m.mtr_station_index == mtr_idx)
                .map(|m| m.oudia_station_slot)
                .collect();

            let mut header_tags = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_02));
            if !station.platform_name.is_empty() {
                header_tags = header_tags.child(carbon_tag(
                    format!("ホーム: {}", station.platform_name),
                    TagKind::CoolGray,
                    theme,
                ));
            }
            if mapped_slots.is_empty() {
                header_tags = header_tags.child(carbon_tag("未割当", TagKind::Red, theme));
            } else {
                for slot_idx in &mapped_slots {
                    let slot_label = template
                        .active_station_slots
                        .iter()
                        .find(|slot| slot.index == *slot_idx)
                        .map(format_station_slot_label)
                        .unwrap_or_else(|| format!("スロット #{slot_idx}"));
                    header_tags = header_tags.child(carbon_tag(
                        format!("→ {slot_label}"),
                        TagKind::Blue,
                        theme,
                    ));
                }
            }

            let slot_buttons = template.active_station_slots.iter().map(|slot| {
                let slot_idx = slot.index;
                let is_assigned = mapped_slots.contains(&slot_idx);
                carbon_button(
                    SharedString::from(format!("step3-map-{mtr_idx}-slot-{slot_idx}")),
                    format_station_slot_label(slot),
                    if is_assigned {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Secondary
                    },
                    disabled,
                    theme,
                    cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        if this.state.set_manual_station_slot(mtr_idx, slot_idx) {
                            cx.notify();
                        }
                    }),
                )
            });

            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                .px(CarbonSpacing::px(CarbonSpacing::SP_04))
                .py(CarbonSpacing::px(CarbonSpacing::SP_03))
                .bg(c.layer_02)
                .border_1()
                .border_color(c.border_subtle)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(CarbonTypography::BODY_01.font_size_px())
                                .line_height(CarbonTypography::BODY_01.line_height_px())
                                .text_color(c.text_primary)
                                .child(format!("{}. {}", mtr_idx + 1, station.station_name)),
                        )
                        .child(header_tags),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                        .children(slot_buttons),
                )
        });

        tile.child(
            div()
                .id("step3-station-mappings")
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                .children(rows),
        )
    }

    fn render_step4(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;
        let busy = self.state.status.is_busy();
        let disabled = busy || self.state.status.is_init_failed();
        let previewing = self.state.status.is_previewing();

        let Some(candidate_id) = &self.state.selected_candidate_id else {
            return div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
                .child(carbon_inline_notification(
                    NotificationKind::Info,
                    "Step 3（路線・経路照合）が未完了です",
                    "Step 3 で MTR 路線と OuDia の照合候補を選択してからプレビューを生成してください。",
                    theme,
                ));
        };

        let (preview_tag_label, preview_tag_kind) = if self.state.preview.is_some() {
            ("生成済み", TagKind::Green)
        } else {
            ("未生成", TagKind::CoolGray)
        };

        let mut action_info = div()
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
                            .line_height(CarbonTypography::HEADING_02.line_height_px())
                            .text_color(c.text_primary)
                            .child("時刻表プレビュー生成"),
                    )
                    .child(carbon_tag(preview_tag_label, preview_tag_kind, theme)),
            )
            .child(info_row("選択中候補 ID:", candidate_id.0.clone(), theme));

        if let Some(candidate) = self
            .state
            .route_candidates
            .iter()
            .find(|item| &item.id == candidate_id)
        {
            action_info = action_info.child(info_row(
                "対象列車:",
                format!(
                    "{} / ダイヤ #{} ・ 列車 #{}",
                    direction_label(&candidate.direction),
                    candidate.diagram_index,
                    candidate.train_index
                ),
                theme,
            ));
        }

        let action_tile = carbon_tile(TileLayer::Layer01, theme)
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                    .child(action_info)
                    .child(carbon_button(
                        "generate-preview",
                        preview_button_label(previewing),
                        ButtonKind::Primary,
                        disabled,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_preview(cx);
                        }),
                    )),
            );

        let mut result_tile = carbon_tile(TileLayer::Layer01, theme)
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04));

        if let Some(preview) = &self.state.preview {
            let base_time = if preview.fixed_base_time.is_empty() {
                "-".to_string()
            } else {
                format!(
                    "{} ({})",
                    preview.fixed_base_time,
                    if preview.precision_mode {
                        "精密入力"
                    } else {
                        "10:00 固定"
                    }
                )
            };
            let (midnight_label, midnight_kind) = if preview.crosses_midnight {
                ("日跨ぎあり", TagKind::Red)
            } else {
                ("日跨ぎなし", TagKind::Green)
            };
            let (operation_label, operation_kind) = if preview.operation_present {
                ("運用あり", TagKind::WarmGray)
            } else {
                ("運用なし", TagKind::CoolGray)
            };

            result_tile = result_tile.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
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
                                    .line_height(CarbonTypography::HEADING_02.line_height_px())
                                    .text_color(c.text_primary)
                                    .child("プレビュー結果"),
                            )
                            .child(carbon_tag(
                                format!("基準時刻: {base_time}"),
                                TagKind::CoolGray,
                                theme,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(carbon_tag(
                                format!("{} 駅", preview.stops.len()),
                                TagKind::Blue,
                                theme,
                            ))
                            .child(carbon_tag(midnight_label, midnight_kind, theme))
                            .child(carbon_tag(operation_label, operation_kind, theme)),
                    ),
            );

            if !preview.warnings.is_empty() {
                result_tile = result_tile.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                        .children(preview.warnings.iter().map(|warning| {
                            carbon_inline_notification(
                                NotificationKind::Warning,
                                "警告",
                                warning.clone(),
                                theme,
                            )
                        })),
                );
            }

            let column_header = div()
                .flex()
                .flex_row()
                .items_center()
                .min_w_0()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                .px(CarbonSpacing::px(CarbonSpacing::SP_04))
                .py(CarbonSpacing::px(CarbonSpacing::SP_02))
                .border_b_1()
                .border_color(c.border_subtle)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child("駅名"),
                )
                .child(
                    div()
                        .w(px(156.0))
                        .flex_shrink_0()
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child("既存着発"),
                )
                .child(
                    div()
                        .w(px(156.0))
                        .flex_shrink_0()
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child("生成着発"),
                )
                .child(
                    div()
                        .w(px(72.0))
                        .flex_shrink_0()
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child("停車秒数"),
                );

            let stops_list = if preview.stops.is_empty() {
                div()
                    .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                    .text_size(CarbonTypography::BODY_01.font_size_px())
                    .line_height(CarbonTypography::BODY_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child("停車駅データがありません")
                    .into_any_element()
            } else {
                div()
                    .id("step4-preview-stops")
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .children(preview.stops.iter().enumerate().map(|(index, stop)| {
                        let existing = format_arrival_departure(
                            stop.existing_arrival.as_deref(),
                            stop.existing_departure.as_deref(),
                        );
                        let generated = format_arrival_departure(
                            stop.rounded_arrival.as_deref(),
                            stop.rounded_departure.as_deref(),
                        );
                        let dwell = format_dwell_seconds(stop.dwell_millis);

                        div()
                            .flex()
                            .flex_row()
                            .items_center()
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
                                    .child(format!("{}. {}", index + 1, stop.station)),
                            )
                            .child(
                                div()
                                    .w(px(156.0))
                                    .flex_shrink_0()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(CarbonTypography::BODY_COMPACT_01.font_size_px())
                                    .line_height(CarbonTypography::BODY_COMPACT_01.line_height_px())
                                    .text_color(c.text_secondary)
                                    .child(existing),
                            )
                            .child(
                                div()
                                    .w(px(156.0))
                                    .flex_shrink_0()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(CarbonTypography::BODY_COMPACT_01.font_size_px())
                                    .line_height(CarbonTypography::BODY_COMPACT_01.line_height_px())
                                    .text_color(c.text_primary)
                                    .child(generated),
                            )
                            .child(
                                div()
                                    .w(px(72.0))
                                    .flex_shrink_0()
                                    .text_size(CarbonTypography::BODY_COMPACT_01.font_size_px())
                                    .line_height(CarbonTypography::BODY_COMPACT_01.line_height_px())
                                    .text_color(c.text_secondary)
                                    .child(dwell),
                            )
                    }))
                    .into_any_element()
            };

            result_tile = result_tile.child(column_header).child(stops_list);
        } else {
            result_tile = result_tile
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(CarbonTypography::HEADING_02.font_size_px())
                        .line_height(CarbonTypography::HEADING_02.line_height_px())
                        .text_color(c.text_primary)
                        .child("プレビュー結果"),
                )
                .child(
                    div()
                        .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                        .text_size(CarbonTypography::BODY_01.font_size_px())
                        .line_height(CarbonTypography::BODY_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child(if previewing {
                            "時刻表プレビューを生成しています..."
                        } else {
                            "「プレビュー生成」を押すと時刻表プレビューが表示されます"
                        }),
                );
        }

        div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(action_tile)
            .child(result_tile)
    }

    fn render_step5(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let c = &theme.colors;
        let busy = self.state.status.is_busy();
        let disabled = busy || self.state.status.is_init_failed();
        let saving = self.state.status.is_saving();

        let Some(preview) = &self.state.preview else {
            return div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
                .child(carbon_inline_notification(
                    NotificationKind::Info,
                    "Step 4（プレビュー）が未完了です",
                    "Step 4 で時刻表プレビューを生成してから安全保存を実行してください。",
                    theme,
                ));
        };

        let (save_tag_label, save_tag_kind) = if self.state.save_receipt.is_some() {
            ("保存済み", TagKind::Green)
        } else {
            ("未保存", TagKind::CoolGray)
        };

        let oudia_path_display = self
            .state
            .oudia_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "未選択".to_string());

        let action_tile = carbon_tile(TileLayer::Layer01, theme)
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
                                            .child("安全保存"),
                                    )
                                    .child(carbon_tag(save_tag_label, save_tag_kind, theme)),
                            )
                            .child(info_row("プレビュー ID:", preview.id.0.clone(), theme))
                            .child(info_row("元 OuDia パス:", oudia_path_display, theme)),
                    )
                    .child(carbon_button(
                        "select-save-path",
                        save_button_label(saving),
                        ButtonKind::Primary,
                        disabled,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_select_save_path(cx);
                        }),
                    )),
            );

        let result_tile = if let Some(receipt) = &self.state.save_receipt {
            carbon_tile(TileLayer::Layer01, theme)
                .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
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
                                        .line_height(CarbonTypography::HEADING_02.line_height_px())
                                        .text_color(c.text_primary)
                                        .child("保存結果"),
                                )
                                .child(carbon_tag("検証済み", TagKind::Green, theme)),
                        )
                        .child(carbon_tag(
                            format!("{} bytes", receipt.bytes),
                            TagKind::Blue,
                            theme,
                        )),
                )
                .child(info_row("出力パス:", receipt.output_path.clone(), theme))
                .child(info_row("SHA-256:", receipt.sha256.clone(), theme))
        } else {
            carbon_tile(TileLayer::Layer01, theme)
                .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(CarbonTypography::HEADING_02.font_size_px())
                        .line_height(CarbonTypography::HEADING_02.line_height_px())
                        .text_color(c.text_primary)
                        .child("保存結果"),
                )
                .child(
                    div()
                        .py(CarbonSpacing::px(CarbonSpacing::SP_04))
                        .text_size(CarbonTypography::BODY_01.font_size_px())
                        .line_height(CarbonTypography::BODY_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child(if saving {
                            "変換結果を検証付きで保存しています..."
                        } else {
                            "「保存先を選んで保存」を押すと検証付きで保存し、結果が表示されます"
                        }),
                )
        };

        div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(action_tile)
            .child(result_tile)
    }

    fn render_step_section(&self, step: AppStep, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.state.theme;
        let c = &theme.colors;
        let completed = self.state.is_step_completed(step);

        let mut header_title_row = div()
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
                    .child(format!("Step {}: {}", step.number(), step.label())),
            );
        if completed {
            header_title_row = header_title_row.child(carbon_tag("完了", TagKind::Green, theme));
        }

        let step_header = div()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_01))
            .child(header_title_row)
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
            AppStep::Step3RouteMatch => self.render_step3(cx),
            AppStep::Step4Preview => self.render_step4(cx),
            AppStep::Step5Save => self.render_step5(cx),
        };

        let mut section = div()
            .id(SharedString::from(format!(
                "section-step-{}",
                step.number()
            )))
            .min_w_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04));

        if step != AppStep::Step1Mtr {
            section = section
                .pt(CarbonSpacing::px(CarbonSpacing::SP_06))
                .border_t_1()
                .border_color(c.border_subtle);
        }

        section.child(step_header).child(body)
    }

    fn render_main(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = &self.state.theme;
        let (notif_kind, notif_title, notif_subtitle) = status_notification(&self.state.status);
        let notification =
            carbon_inline_notification(notif_kind, notif_title, notif_subtitle, theme);
        let bottom_spacer = (f32::from(window.viewport_size().height) - 180.0).max(320.0);

        let mut steps_scroll = div()
            .id("main-steps")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.steps_scroll_handle)
            .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _window, _cx| {
                this.sync_step_from_scroll = true;
            }))
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_06))
            .pr(CarbonSpacing::px(CarbonSpacing::SP_02))
            .pb(px(bottom_spacer));

        for step in AppStep::ALL {
            steps_scroll = steps_scroll.child(self.render_step_section(step, cx));
        }

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_05))
            .p(CarbonSpacing::px(CarbonSpacing::SP_05))
            .child(notification)
            .child(steps_scroll)
    }
}

impl Render for MtrApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.sync_step_from_scroll {
            let offset_y = f32::from(self.steps_scroll_handle.offset().y);
            let max_offset_height = f32::from(self.steps_scroll_handle.max_offset().height);
            let scrolled_step = step_for_scroll_position(
                self.steps_scroll_handle.top_item(),
                offset_y,
                max_offset_height,
            );
            self.state.select_step(scrolled_step);
        }
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
                    .child(self.render_main(window, cx)),
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
        bind_text_input_keys(cx);
        let bounds = Bounds::centered(None, size(px(960.0), px(680.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| MtrApp::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_poc::backend::snapshot_routes_changed;
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

    fn dummy_candidate(
        id: &str,
        diagram_index: usize,
        train_index: usize,
        rank: &str,
        auto_selected: bool,
        matched_stations: usize,
    ) -> RouteCandidateDto {
        use mtr_oudia_application::StationMappingDto;
        RouteCandidateDto {
            id: CandidateId(id.to_string()),
            diagram_index,
            train_index,
            direction: "kudari".to_string(),
            station_mappings: (0..matched_stations)
                .map(|idx| StationMappingDto {
                    mtr_station_index: idx,
                    oudia_station_slot: idx,
                })
                .collect(),
            rank: rank.to_string(),
            reasons: vec!["station sequence matched".to_string()],
            auto_selected,
            manual_only: rank == "manual",
        }
    }

    #[test]
    fn apply_candidates_result_handles_auto_manual_empty_and_error() {
        let mut state = MtrShellState::new(Ok(()));
        assert!(!state.is_step_completed(AppStep::Step3RouteMatch));

        // 1. 自動選択あり
        state.apply_candidates_result(Ok(vec![
            dummy_candidate("cand-1", 0, 2, "PrimaryExact", true, 10),
            dummy_candidate("cand-2", 0, 5, "Partial", false, 6),
        ]));
        assert_eq!(state.route_candidates.len(), 2);
        assert_eq!(
            state.selected_candidate_id,
            Some(CandidateId("cand-1".to_string()))
        );
        assert!(state.is_step_completed(AppStep::Step3RouteMatch));
        assert_eq!(
            state.status,
            AppStatus::CandidatesFound {
                count: 2,
                auto_selected: true,
            }
        );

        // 2. 複数手動候補（auto_selected == false のみ）
        state.apply_candidates_result(Ok(vec![
            dummy_candidate("cand-3", 1, 0, "Partial", false, 5),
            dummy_candidate("cand-4", 1, 1, "Partial", false, 4),
        ]));
        assert_eq!(state.route_candidates.len(), 2);
        assert!(state.selected_candidate_id.is_none());
        assert!(!state.is_step_completed(AppStep::Step3RouteMatch));
        assert_eq!(
            state.status,
            AppStatus::CandidatesFound {
                count: 2,
                auto_selected: false,
            }
        );

        // 手動選択で Step3RouteMatch が完了になる
        assert!(state.select_candidate(CandidateId("cand-4".to_string())));
        assert!(state.is_step_completed(AppStep::Step3RouteMatch));

        // 3. 0件
        state.apply_candidates_result(Ok(Vec::new()));
        assert!(state.route_candidates.is_empty());
        assert!(state.selected_candidate_id.is_none());
        assert!(!state.is_step_completed(AppStep::Step3RouteMatch));
        assert_eq!(
            state.status,
            AppStatus::CandidatesFound {
                count: 0,
                auto_selected: false,
            }
        );

        // 4. エラー
        state.apply_candidates_result(Err(BusinessError {
            kind: BusinessErrorKind::Validation,
            message: "路線が見つかりません".to_string(),
            detail: Some("route-missing".to_string()),
        }));
        assert_eq!(
            state.status,
            AppStatus::CandidatesError {
                title: "路線が見つかりません".to_string(),
                detail: "route-missing".to_string(),
            }
        );
    }

    #[test]
    fn upstream_updates_clear_downstream_step3_state() {
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
        assert!(state.select_diagram_index(Some(1)));
        assert!(state.select_train_type(Some(0)));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-1",
            1,
            0,
            "PrimaryExact",
            true,
            30,
        )]));
        assert!(state.selected_route_id.is_some());
        assert_eq!(state.selected_diagram_index, Some(1));
        assert_eq!(state.selected_train_type, Some(0));
        assert!(!state.route_candidates.is_empty());
        assert!(state.selected_candidate_id.is_some());

        // Step 1 スナップショット更新で selected_route_id / route_candidates / selected_candidate_id がクリアされる
        state.apply_snapshot_result(1, Ok(dummy_snapshot(vec![("京浜東北線", 35)])));
        assert!(state.selected_route_id.is_none());
        assert!(state.route_candidates.is_empty());
        assert!(state.selected_candidate_id.is_none());
        assert_eq!(state.selected_diagram_index, Some(1));
        assert_eq!(state.selected_train_type, Some(0));

        // 再度選択して、Step 1 接続先変更時もクリアされることを確認
        assert!(state.select_route("route-0".to_string()));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-2",
            1,
            2,
            "PrimaryExact",
            true,
            35,
        )]));
        assert!(state.select_endpoint("http://127.0.0.1:49183/".to_string()));
        assert!(state.selected_route_id.is_none());
        assert!(state.route_candidates.is_empty());
        assert!(state.selected_candidate_id.is_none());

        // Step 2 OuDia 更新で selected_diagram_index / selected_train_type / route_candidates / selected_candidate_id がクリアされる
        state.apply_snapshot_result(0, Ok(dummy_snapshot(vec![("山手線", 30)])));
        assert!(state.select_route("route-0".to_string()));
        assert!(state.select_diagram_index(Some(0)));
        assert!(state.select_train_type(Some(1)));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-3",
            0,
            1,
            "PrimaryExact",
            true,
            30,
        )]));
        state.apply_inspection_result(
            PathBuf::from("/tmp/chuo.oud2"),
            Ok(dummy_inspection("OuDiaSecond.1.04", Some("中央線"), 24)),
        );
        assert_eq!(state.selected_route_id.as_deref(), Some("route-0"));
        assert!(state.selected_diagram_index.is_none());
        assert!(state.selected_train_type.is_none());
        assert!(state.route_candidates.is_empty());
        assert!(state.selected_candidate_id.is_none());
    }

    #[test]
    fn status_notification_maps_step3_statuses() {
        let finding = AppStatus::FindingCandidates;
        assert!(finding.is_busy());
        assert!(finding.is_finding_candidates());
        assert_eq!(status_notification(&finding).0, NotificationKind::Info);

        let auto = AppStatus::CandidatesFound {
            count: 1,
            auto_selected: true,
        };
        assert!(!auto.is_busy());
        assert_eq!(status_notification(&auto).0, NotificationKind::Success);

        let manual = AppStatus::CandidatesFound {
            count: 3,
            auto_selected: false,
        };
        assert_eq!(status_notification(&manual).0, NotificationKind::Info);

        let empty = AppStatus::CandidatesFound {
            count: 0,
            auto_selected: false,
        };
        assert_eq!(status_notification(&empty).0, NotificationKind::Warning);

        let err = AppStatus::CandidatesError {
            title: "路線が見つかりません".to_string(),
            detail: "route-0".to_string(),
        };
        assert_eq!(
            status_notification(&err),
            (
                NotificationKind::Error,
                "路線が見つかりません".to_string(),
                "route-0".to_string(),
            )
        );
    }

    fn dummy_preview(
        id: &str,
        stop_count: usize,
        crosses_midnight: bool,
        operation_present: bool,
        warnings: Vec<&str>,
    ) -> PreviewDto {
        use mtr_oudia_application::{PreviewId, PreviewStopDto};
        PreviewDto {
            precision_mode: false,
            outbound: None,
            id: PreviewId(id.to_string()),
            fixed_base_time: "10:00:00".to_string(),
            stops: (0..stop_count)
                .map(|idx| PreviewStopDto {
                    updated_platform: None,
                    station: format!("駅{idx}"),
                    existing_arrival: (idx > 0).then(|| format!("09:{:02}:00", idx * 2)),
                    existing_departure: (idx + 1 < stop_count)
                        .then(|| format!("09:{:02}:30", idx * 2)),
                    raw_arrival_millis: (idx > 0).then_some(36_000_000 + (idx as i64) * 120_000),
                    raw_departure_millis: (idx + 1 < stop_count)
                        .then_some(36_030_000 + (idx as i64) * 120_000),
                    rounded_arrival: (idx > 0).then(|| format!("10:{:02}:00", idx * 2)),
                    rounded_departure: (idx + 1 < stop_count)
                        .then(|| format!("10:{:02}:30", idx * 2)),
                    run_millis: (idx + 1 < stop_count).then_some(90_000),
                    dwell_millis: 30_000,
                })
                .collect(),
            warnings: warnings.into_iter().map(str::to_string).collect(),
            crosses_midnight,
            operation_present,
            policy_choices: if operation_present {
                vec!["preserve".to_string(), "remove_target_train".to_string()]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn apply_preview_result_updates_state_and_preserves_consistency_on_error() {
        let mut state = MtrShellState::new(Ok(()));
        assert!(state.preview.is_none());
        assert!(!state.is_step_completed(AppStep::Step4Preview));

        // 初期状態での失敗時は preview 未設定のままエラー状態になる。
        state.apply_preview_result(Err(BusinessError {
            kind: BusinessErrorKind::StaleState,
            message: "候補が現在のセッションにありません".to_string(),
            detail: None,
        }));
        assert!(state.preview.is_none());
        assert!(!state.is_step_completed(AppStep::Step4Preview));
        assert_eq!(
            state.status,
            AppStatus::PreviewError {
                title: "候補が現在のセッションにありません".to_string(),
                detail: String::new(),
            }
        );

        // 成功時は preview が確定し、Step4Preview が完了になる。
        state.apply_preview_result(Ok(dummy_preview("preview-1", 4, false, false, Vec::new())));
        assert_eq!(
            state.preview.as_ref().map(|dto| dto.id.0.as_str()),
            Some("preview-1")
        );
        assert!(state.is_step_completed(AppStep::Step4Preview));
        assert_eq!(
            state.status,
            AppStatus::PreviewSuccess {
                stop_count: 4,
                warning_count: 0,
            }
        );

        // 以降の生成失敗時もエラー状態になりつつ、既存の preview と矛盾しない。
        state.apply_preview_result(Err(BusinessError {
            kind: BusinessErrorKind::Validation,
            message: "時刻表を生成できません".to_string(),
            detail: Some("station mismatch".to_string()),
        }));
        assert_eq!(
            state.preview.as_ref().map(|dto| dto.id.0.as_str()),
            Some("preview-1")
        );
        assert!(state.is_step_completed(AppStep::Step4Preview));
        assert_eq!(
            state.status,
            AppStatus::PreviewError {
                title: "時刻表を生成できません".to_string(),
                detail: "station mismatch".to_string(),
            }
        );
    }

    #[test]
    fn upstream_updates_clear_downstream_step4_preview() {
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
        assert!(state.preview.is_some());
        assert!(state.is_step_completed(AppStep::Step4Preview));

        // Step 3 同一候補の再選択では preview を保持する。
        assert!(state.select_candidate(CandidateId("cand-1".to_string())));
        assert!(state.preview.is_some());

        // Step 3 別候補への変更で preview がクリアされる。
        assert!(state.select_candidate(CandidateId("cand-2".to_string())));
        assert!(state.preview.is_none());
        assert!(!state.is_step_completed(AppStep::Step4Preview));

        // Step 3 候補再検索で preview がクリアされる。
        state.apply_preview_result(Ok(dummy_preview("preview-2", 30, false, false, Vec::new())));
        assert!(state.preview.is_some());
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-3",
            0,
            2,
            "PrimaryExact",
            true,
            30,
        )]));
        assert!(state.preview.is_none());

        // Step 2 OuDia 更新で preview がクリアされる。
        state.apply_preview_result(Ok(dummy_preview("preview-3", 30, false, false, Vec::new())));
        assert!(state.preview.is_some());
        state.apply_inspection_result(
            PathBuf::from("/tmp/chuo.oud2"),
            Ok(dummy_inspection("OuDiaSecond.1.04", Some("中央線"), 24)),
        );
        assert!(state.preview.is_none());

        // Step 1 スナップショット変更で preview がクリアされる。
        assert!(state.select_route("route-0".to_string()));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-4",
            0,
            0,
            "PrimaryExact",
            true,
            24,
        )]));
        state.apply_preview_result(Ok(dummy_preview("preview-4", 24, false, false, Vec::new())));
        assert!(state.preview.is_some());
        state.apply_snapshot_result(1, Ok(dummy_snapshot(vec![("京浜東北線", 35)])));
        assert!(state.preview.is_none());

        // Step 1 検出成功で preview がクリアされる。
        assert!(state.select_route("route-0".to_string()));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-5",
            0,
            0,
            "PrimaryExact",
            true,
            35,
        )]));
        state.apply_preview_result(Ok(dummy_preview("preview-5", 35, false, false, Vec::new())));
        assert!(state.preview.is_some());
        state.apply_detection_result(Ok(vec![EndpointDto {
            url: "http://127.0.0.1:49182/".to_string(),
        }]));
        assert!(state.preview.is_none());
    }

    #[test]
    fn status_notification_maps_step4_statuses() {
        let previewing = AppStatus::Previewing;
        assert!(previewing.is_busy());
        assert!(previewing.is_previewing());
        assert_eq!(status_notification(&previewing).0, NotificationKind::Info);

        let success = AppStatus::PreviewSuccess {
            stop_count: 10,
            warning_count: 0,
        };
        assert!(!success.is_busy());
        assert_eq!(
            status_notification(&success),
            (
                NotificationKind::Success,
                "プレビュー生成完了".to_string(),
                "10 駅の時刻表プレビューを生成しました".to_string(),
            )
        );

        let warned = AppStatus::PreviewSuccess {
            stop_count: 10,
            warning_count: 1,
        };
        assert!(!warned.is_busy());
        assert_eq!(
            status_notification(&warned),
            (
                NotificationKind::Warning,
                "プレビュー生成完了（警告あり）".to_string(),
                "10 駅の時刻表を生成しましたが、1 件の警告があります".to_string(),
            )
        );

        let err = AppStatus::PreviewError {
            title: "時刻表を生成できません".to_string(),
            detail: "candidate expired".to_string(),
        };
        assert!(!err.is_busy());
        assert_eq!(
            status_notification(&err),
            (
                NotificationKind::Error,
                "時刻表を生成できません".to_string(),
                "candidate expired".to_string(),
            )
        );
    }

    #[test]
    fn preview_formatting_helpers_format_times_and_dwell_seconds() {
        assert_eq!(
            format_arrival_departure(Some("10:02:00"), Some("10:02:30")),
            "10:02:00 / 10:02:30"
        );
        assert_eq!(
            format_arrival_departure(None, Some("10:00:00")),
            "- / 10:00:00"
        );
        assert_eq!(
            format_arrival_departure(Some("10:05:00"), None),
            "10:05:00 / -"
        );
        assert_eq!(format_arrival_departure(None, None), "-");

        assert_eq!(format_dwell_seconds(0), "0 秒");
        assert_eq!(format_dwell_seconds(30_000), "30 秒");
        assert_eq!(format_dwell_seconds(15_500), "15.5 秒");
        assert_eq!(format_dwell_seconds(12_340), "12.34 秒");
    }

    fn dummy_save_receipt(output_path: &str, bytes: u64, sha256: &str) -> SaveReceipt {
        SaveReceipt {
            output_path: output_path.to_string(),
            bytes,
            sha256: sha256.to_string(),
        }
    }

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

    fn dummy_detailed_snapshot(
        routes: Vec<(&str, &str, Vec<(&str, &str, i64, Option<i64>)>)>,
        api_current_time_millis: i64,
    ) -> SnapshotDto {
        use mtr_oudia_application::RouteStationDto;
        SnapshotDto {
            routes: routes
                .into_iter()
                .map(|(id, name, stations)| {
                    let station_count = stations.len();
                    let stations = stations
                        .into_iter()
                        .map(
                            |(station_name, platform_name, dwell_millis, run_millis_to_next)| {
                                RouteStationDto {
                                    station_name: station_name.to_string(),
                                    platform_name: platform_name.to_string(),
                                    dwell_millis,
                                    run_millis_to_next,
                                }
                            },
                        )
                        .collect();
                    RouteDto {
                        id: id.to_string(),
                        name: name.to_string(),
                        stations,
                        station_count,
                        total_run_millis: 120_000,
                        total_dwell_millis: 15_000,
                    }
                })
                .collect(),
            dimensions: Vec::new(),
            api_current_time_millis,
        }
    }

    #[test]
    fn snapshot_routes_changed_detects_route_and_station_differences() {
        let base = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線",
                vec![
                    ("東京", "1番線", 15_000, Some(120_000)),
                    ("神田", "2番線", 10_000, None),
                ],
            )],
            1_000,
        );

        // api_current_time_millis のみが異なる同一データは false
        let same_routes_later = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線",
                vec![
                    ("東京", "1番線", 15_000, Some(120_000)),
                    ("神田", "2番線", 10_000, None),
                ],
            )],
            99_999,
        );
        assert!(!snapshot_routes_changed(&base, &same_routes_later));

        // 路線数の変化で true
        let added_route = dummy_detailed_snapshot(
            vec![
                (
                    "r1",
                    "山手線",
                    vec![
                        ("東京", "1番線", 15_000, Some(120_000)),
                        ("神田", "2番線", 10_000, None),
                    ],
                ),
                ("r2", "京浜東北線", vec![("東京", "3番線", 10_000, None)]),
            ],
            2_000,
        );
        assert!(snapshot_routes_changed(&base, &added_route));

        // 路線名の変化で true
        let renamed_route = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線（内回り）",
                vec![
                    ("東京", "1番線", 15_000, Some(120_000)),
                    ("神田", "2番線", 10_000, None),
                ],
            )],
            2_000,
        );
        assert!(snapshot_routes_changed(&base, &renamed_route));

        // 駅構成（駅名・ホーム・駅数・時分）の変化で true
        let changed_station = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線",
                vec![
                    ("東京", "1番線", 15_000, Some(120_000)),
                    ("秋葉原", "2番線", 10_000, None),
                ],
            )],
            2_000,
        );
        assert!(snapshot_routes_changed(&base, &changed_station));

        let changed_station_count = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線",
                vec![
                    ("東京", "1番線", 15_000, Some(120_000)),
                    ("神田", "2番線", 10_000, Some(90_000)),
                    ("秋葉原", "1番線", 10_000, None),
                ],
            )],
            2_000,
        );
        assert!(snapshot_routes_changed(&base, &changed_station_count));
    }

    #[test]
    fn apply_refresh_snapshot_result_preserves_downstream_when_unchanged_and_clears_when_updated() {
        let mut state = MtrShellState::new(Ok(()));
        state.candidates = vec!["http://127.0.0.1:49182/".to_string()];
        assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
        let initial_snapshot = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線",
                vec![
                    ("東京", "1", 10_000, Some(120_000)),
                    ("神田", "2", 10_000, None),
                ],
            )],
            1_000,
        );
        state.apply_snapshot_result(0, Ok(initial_snapshot));
        state.apply_inspection_result(
            PathBuf::from("/tmp/yamanote.oud2"),
            Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 30)),
        );
        assert!(state.select_route("r1".to_string()));
        assert!(state.select_diagram_index(Some(1)));
        assert!(state.select_train_type(Some(0)));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-1",
            1,
            0,
            "PrimaryExact",
            true,
            2,
        )]));
        state.apply_preview_result(Ok(dummy_preview("preview-1", 2, false, false, Vec::new())));
        state.apply_save_result(Ok(dummy_save_receipt(
            "/tmp/yamanote_converted.oud2",
            2048,
            "sha-1",
        )));

        // 1. 変化なし適用時は既存スナップショットおよび Step 3/4/5 の状態がすべて維持される
        state.apply_refresh_snapshot_result(0, Ok(RefreshSnapshotOutcome::Unchanged));
        assert_eq!(state.status, AppStatus::RefreshUnchanged);
        assert_eq!(
            state.snapshot.as_ref().map(|s| s.api_current_time_millis),
            Some(1_000)
        );
        assert_eq!(state.selected_route_id.as_deref(), Some("r1"));
        assert_eq!(state.selected_diagram_index, Some(1));
        assert_eq!(state.selected_train_type, Some(0));
        assert_eq!(state.route_candidates.len(), 1);
        assert_eq!(
            state.selected_candidate_id,
            Some(CandidateId("cand-1".to_string()))
        );
        assert!(state.preview.is_some());
        assert!(state.save_receipt.is_some());

        // 2. エラー時も既存スナップショットおよび Step 3/4/5 の状態は維持される
        state.apply_refresh_snapshot_result(
            0,
            Err(BusinessError {
                kind: BusinessErrorKind::Connection,
                message: "MTR API へ接続できません".to_string(),
                detail: None,
            }),
        );
        assert!(matches!(state.status, AppStatus::FetchError { .. }));
        assert_eq!(state.selected_route_id.as_deref(), Some("r1"));
        assert!(state.preview.is_some());
        assert!(state.save_receipt.is_some());

        // 3. 変化ありの更新時は snapshot を更新し、Step 3〜5 の下流状態をクリアする
        let updated_snapshot = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線（外回り）",
                vec![
                    ("東京", "1", 10_000, Some(120_000)),
                    ("神田", "2", 10_000, Some(90_000)),
                    ("秋葉原", "1", 10_000, None),
                ],
            )],
            32_000,
        );
        state.apply_refresh_snapshot_result(
            0,
            Ok(RefreshSnapshotOutcome::Updated(updated_snapshot)),
        );
        assert_eq!(
            state.status,
            AppStatus::RefreshUpdated {
                route_count: 1,
                stop_count: 3,
            }
        );
        assert_eq!(
            state.snapshot.as_ref().map(|s| s.routes[0].name.as_str()),
            Some("山手線（外回り）")
        );
        assert!(state.selected_route_id.is_none());
        assert!(state.route_candidates.is_empty());
        assert!(state.selected_candidate_id.is_none());
        assert!(state.preview.is_none());
        assert!(state.save_receipt.is_none());
        assert_eq!(state.selected_diagram_index, Some(1));
        assert_eq!(state.selected_train_type, Some(0));
    }

    #[test]
    fn status_notification_and_busy_cover_refresh_statuses() {
        let refreshing = AppStatus::RefreshingCache {
            endpoint: "http://127.0.0.1:49182/".to_string(),
            dimension: 0,
        };
        assert!(refreshing.is_busy());
        assert!(refreshing.is_refreshing());
        assert_eq!(refresh_button_label(true), "キャッシュ待機中...");
        assert_eq!(refresh_button_label(false), "キャッシュ待機して再取得");
        let notif_refreshing = status_notification(&refreshing);
        assert_eq!(notif_refreshing.0, NotificationKind::Info);
        assert_eq!(notif_refreshing.1, "キャッシュ待機中");
        assert!(notif_refreshing.2.contains("最大 30 秒"));

        let updated = AppStatus::RefreshUpdated {
            route_count: 2,
            stop_count: 18,
        };
        assert!(!updated.is_busy());
        assert!(!updated.is_refreshing());
        assert_eq!(
            status_notification(&updated),
            (
                NotificationKind::Success,
                "更新完了".to_string(),
                "駅・路線データの変化を検出し、2 路線 / 18 駅に更新しました".to_string(),
            )
        );

        let unchanged = AppStatus::RefreshUnchanged;
        assert!(!unchanged.is_busy());
        assert!(!unchanged.is_refreshing());
        let notif_unchanged = status_notification(&unchanged);
        assert_eq!(notif_unchanged.0, NotificationKind::Info);
        assert_eq!(notif_unchanged.1, "変化なし（キャッシュ未更新）");
    }

    #[test]
    fn normalize_train_number_input_trims_and_handles_empty() {
        assert_eq!(normalize_train_number_input(""), None);
        assert_eq!(normalize_train_number_input("   \t\n "), None);
        assert_eq!(
            normalize_train_number_input("101M"),
            Some("101M".to_string())
        );
        assert_eq!(
            normalize_train_number_input("  101M \t"),
            Some("101M".to_string())
        );
    }

    fn mapping_pairs(mappings: Option<&[StationMappingDto]>) -> Vec<(usize, usize)> {
        mappings
            .unwrap_or(&[])
            .iter()
            .map(|m| (m.mtr_station_index, m.oudia_station_slot))
            .collect()
    }

    #[test]
    fn manual_station_mapping_updates_resets_and_clears_downstream() {
        let mut state = MtrShellState::new(Ok(()));
        // 候補未選択時は編集・リセットとも無効
        assert!(!state.set_manual_station_slot(0, 1));
        assert!(!state.reset_manual_station_mappings());
        assert!(state.manual_station_mappings.is_none());

        state.apply_snapshot_result(
            0,
            Ok(dummy_detailed_snapshot(
                vec![(
                    "r1",
                    "山手線",
                    vec![
                        ("東京", "1", 10_000, Some(120_000)),
                        ("神田", "2", 10_000, None),
                    ],
                )],
                1_000,
            )),
        );
        state.apply_inspection_result(
            PathBuf::from("/tmp/yamanote.oud2"),
            Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 3)),
        );
        assert!(state.select_route("r1".to_string()));
        state.apply_candidates_result(Ok(vec![
            dummy_candidate("cand-1", 0, 0, "PrimaryExact", true, 2),
            dummy_candidate("cand-2", 0, 1, "manual", false, 0),
        ]));
        assert!(state.manual_station_mappings.is_none());
        assert_eq!(
            mapping_pairs(state.effective_station_mappings()),
            vec![(0, 0), (1, 1)]
        );

        // Step 4 / Step 5 完了状態を構築
        state.apply_preview_result(Ok(dummy_preview("preview-1", 2, false, false, Vec::new())));
        state.apply_save_result(Ok(dummy_save_receipt(
            "/tmp/yamanote_converted.oud2",
            1024,
            "sha-1",
        )));

        // 同一スロットの再設定は no-op として扱い下流をクリアしない
        assert!(!state.set_manual_station_slot(0, 0));
        assert!(state.manual_station_mappings.is_none());
        assert!(state.preview.is_some());
        assert!(state.save_receipt.is_some());

        // 別スロットへ変更すると手動マッピングが保持され、Step 4/5 がクリアされる
        assert!(state.set_manual_station_slot(0, 2));
        assert_eq!(
            mapping_pairs(state.manual_station_mappings.as_deref()),
            vec![(0, 2), (1, 1)]
        );
        assert_eq!(
            mapping_pairs(state.effective_station_mappings()),
            vec![(0, 2), (1, 1)]
        );
        assert!(state.preview.is_none());
        assert!(state.save_receipt.is_none());

        // Step 4/5 を再度埋めた後、「自動マッピングに戻す」で手動編集と下流がクリアされる
        state.apply_preview_result(Ok(dummy_preview("preview-2", 2, false, false, Vec::new())));
        state.apply_save_result(Ok(dummy_save_receipt(
            "/tmp/yamanote_converted.oud2",
            1024,
            "sha-2",
        )));
        assert!(state.reset_manual_station_mappings());
        assert!(state.manual_station_mappings.is_none());
        assert_eq!(
            mapping_pairs(state.effective_station_mappings()),
            vec![(0, 0), (1, 1)]
        );
        assert!(state.preview.is_none());
        assert!(state.save_receipt.is_none());
        assert!(!state.reset_manual_station_mappings());

        // manual_only 候補（初期マッピング空）への手動割当も機能する
        assert!(state.select_candidate(CandidateId("cand-2".to_string())));
        assert!(mapping_pairs(state.effective_station_mappings()).is_empty());
        assert!(state.set_manual_station_slot(1, 2));
        assert_eq!(
            mapping_pairs(state.manual_station_mappings.as_deref()),
            vec![(1, 2)]
        );
    }

    #[test]
    fn manual_station_mapping_clears_on_candidate_and_upstream_changes() {
        let mut state = MtrShellState::new(Ok(()));
        state.candidates = vec![
            "http://127.0.0.1:49182/".to_string(),
            "http://127.0.0.1:49183/".to_string(),
        ];
        assert!(state.select_endpoint("http://127.0.0.1:49182/".to_string()));
        state.apply_snapshot_result(0, Ok(dummy_snapshot(vec![("山手線", 2), ("中央線", 2)])));
        state.apply_inspection_result(
            PathBuf::from("/tmp/yamanote.oud2"),
            Ok(dummy_inspection("OuDiaSecond.1.04", Some("山手線"), 3)),
        );
        assert!(state.select_route("route-0".to_string()));
        state.apply_candidates_result(Ok(vec![
            dummy_candidate("cand-1", 0, 0, "PrimaryExact", true, 2),
            dummy_candidate("cand-2", 0, 1, "Partial", false, 2),
        ]));

        // 1. 同一候補再選択は維持、別候補選択でクリア
        assert!(state.set_manual_station_slot(0, 2));
        assert!(state.select_candidate(CandidateId("cand-1".to_string())));
        assert!(state.manual_station_mappings.is_some());
        assert!(state.select_candidate(CandidateId("cand-2".to_string())));
        assert!(state.manual_station_mappings.is_none());

        // 2. 候補再検索でクリア
        assert!(state.set_manual_station_slot(0, 2));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-3",
            0,
            2,
            "PrimaryExact",
            true,
            2,
        )]));
        assert!(state.manual_station_mappings.is_none());

        // 3. 路線・ダイヤ・種別変更でクリア
        assert!(state.set_manual_station_slot(0, 2));
        assert!(state.select_diagram_index(Some(1)));
        assert!(state.manual_station_mappings.is_none());

        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-4",
            1,
            0,
            "PrimaryExact",
            true,
            2,
        )]));
        assert!(state.set_manual_station_slot(0, 2));
        assert!(state.select_train_type(Some(1)));
        assert!(state.manual_station_mappings.is_none());

        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-5",
            1,
            1,
            "PrimaryExact",
            true,
            2,
        )]));
        assert!(state.set_manual_station_slot(0, 2));
        assert!(state.select_route("route-1".to_string()));
        assert!(state.manual_station_mappings.is_none());

        // 4. Step 2 OuDia 再解析・Step 1 スナップショット更新・接続先変更でクリア
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-6",
            1,
            2,
            "PrimaryExact",
            true,
            2,
        )]));
        assert!(state.set_manual_station_slot(0, 2));
        state.apply_inspection_result(
            PathBuf::from("/tmp/chuo.oud2"),
            Ok(dummy_inspection("OuDiaSecond.1.04", Some("中央線"), 2)),
        );
        assert!(state.manual_station_mappings.is_none());

        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-7",
            0,
            0,
            "PrimaryExact",
            true,
            2,
        )]));
        assert!(state.set_manual_station_slot(0, 2));
        state.apply_snapshot_result(1, Ok(dummy_snapshot(vec![("京浜東北線", 2)])));
        assert!(state.manual_station_mappings.is_none());

        assert!(state.select_route("route-0".to_string()));
        state.apply_candidates_result(Ok(vec![dummy_candidate(
            "cand-8",
            0,
            0,
            "PrimaryExact",
            true,
            2,
        )]));
        assert!(state.set_manual_station_slot(0, 2));
        assert!(state.select_endpoint("http://127.0.0.1:49183/".to_string()));
        assert!(state.manual_station_mappings.is_none());
    }

    #[test]
    fn station_mapping_issues_detects_missing_and_duplicate_slots() {
        let snapshot = dummy_detailed_snapshot(
            vec![(
                "r1",
                "山手線",
                vec![
                    ("東京", "1", 10_000, Some(120_000)),
                    ("神田", "2", 10_000, None),
                ],
            )],
            1_000,
        );
        let route = &snapshot.routes[0];
        let template = TemplateDto {
            diagram_index: 0,
            direction: "kudari".to_string(),
            train_index: 0,
            train_type_index: Some(0),
            active_station_slots: vec![
                StationSlotDto {
                    index: 0,
                    name: "東京".to_string(),
                    handling_code: Some(1),
                    previous_name: None,
                    next_name: Some("神田".to_string()),
                },
                StationSlotDto {
                    index: 1,
                    name: "神田".to_string(),
                    handling_code: Some(1),
                    previous_name: Some("東京".to_string()),
                    next_name: None,
                },
            ],
            route_station_slots: Vec::new(),
        };

        let valid = vec![
            StationMappingDto {
                mtr_station_index: 0,
                oudia_station_slot: 0,
            },
            StationMappingDto {
                mtr_station_index: 1,
                oudia_station_slot: 1,
            },
        ];
        assert!(station_mapping_issues(route, &template, &valid).is_empty());

        let invalid = vec![
            StationMappingDto {
                mtr_station_index: 0,
                oudia_station_slot: 0,
            },
            StationMappingDto {
                mtr_station_index: 1,
                oudia_station_slot: 0,
            },
        ];
        let issues = station_mapping_issues(route, &template, &invalid);
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|issue| issue.contains("未割当")));
        assert!(issues.iter().any(|issue| issue.contains("重複割当")));
    }
}
