//! UI 非依存のウィザード状態機械と表示用の純粋ヘルパ。

use std::path::{Path, PathBuf};

use gpui::WindowAppearance;
use mtr_oudia_application::{
    BusinessError, CandidateId, EndpointDto, InspectionDto, OutboundRuntimeSource,
    OutboundStatusDto, PreviewDto, RouteCandidateDto, RouteDto, SaveReceipt, SnapshotDto,
    StationMappingDto, StationSlotDto, TemplateDto,
};
use mtr_oudia_domain::OperationPolicy;

use crate::backend::{RefreshSnapshotOutcome, format_business_error};
use crate::carbon::{CarbonTheme, CarbonThemeKind, NotificationKind, TagKind};

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
    MeasuringOutbound,
    OutboundMeasured {
        duration_label: String,
        is_manual: bool,
        valid: bool,
    },
    OutboundError {
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
                | Self::MeasuringOutbound
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

    pub fn is_measuring_outbound(&self) -> bool {
        matches!(self, Self::MeasuringOutbound)
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
        AppStatus::MeasuringOutbound => (
            NotificationKind::Info,
            "出庫時分を計測中".to_string(),
            "MTR API から試験列車の出庫時分を取得・保存しています...".to_string(),
        ),
        AppStatus::OutboundMeasured {
            duration_label,
            is_manual,
            valid,
        } => {
            if *valid {
                let title = if *is_manual {
                    "出庫時分を手動保存しました"
                } else {
                    "出庫時分を計測しました"
                };
                (
                    NotificationKind::Success,
                    title.to_string(),
                    format!("出庫 → 始発駅所要時間: {duration_label}"),
                )
            } else {
                (
                    NotificationKind::Warning,
                    "出庫時分を保存しました（要確認）".to_string(),
                    format!(
                        "出庫 → 始発駅所要時間: {duration_label}（現在の路線構成と一致するか確認してください）"
                    ),
                )
            }
        }
        AppStatus::OutboundError { title, detail } => {
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

pub fn is_dark_appearance(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

pub fn resolve_theme_kind(preference: ThemePreference, os_dark: bool) -> CarbonThemeKind {
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

pub fn theme_badge_label(preference: ThemePreference, kind: CarbonThemeKind) -> &'static str {
    match (preference, kind) {
        (ThemePreference::System, CarbonThemeKind::Gray100) => "OS 連動・ダーク",
        (ThemePreference::System, CarbonThemeKind::Gray10) => "OS 連動・ライト",
        (ThemePreference::Dark, _) | (_, CarbonThemeKind::Gray100) => "ダーク",
        (ThemePreference::Light, _) => "ライト",
    }
}

pub fn dark_mode_button_label(kind: CarbonThemeKind) -> &'static str {
    match kind {
        CarbonThemeKind::Gray100 => "ダークモード: ON",
        CarbonThemeKind::Gray10 => "ダークモード: OFF",
    }
}

pub fn discover_button_label(detecting: bool) -> &'static str {
    if detecting {
        "検出中..."
    } else {
        "自動検出"
    }
}

pub fn fetch_button_label(fetching: bool) -> &'static str {
    if fetching {
        "取得中..."
    } else {
        "スナップショット取得"
    }
}

pub fn refresh_button_label(refreshing: bool) -> &'static str {
    if refreshing {
        "キャッシュ待機中..."
    } else {
        "キャッシュ待機して再取得"
    }
}

pub fn inspect_button_label(inspecting: bool) -> &'static str {
    if inspecting {
        "解析中..."
    } else {
        "OuDia ファイルを選択"
    }
}

pub fn find_candidates_button_label(finding: bool) -> &'static str {
    if finding {
        "検索中..."
    } else {
        "候補を検索"
    }
}

pub fn preview_button_label(previewing: bool) -> &'static str {
    if previewing {
        "生成中..."
    } else {
        "プレビュー生成"
    }
}

pub fn outbound_measure_button_label(measuring: bool) -> &'static str {
    if measuring {
        "計測中..."
    } else {
        "出庫時分を計測"
    }
}

pub fn generate_outbound_button_label(enabled: bool) -> &'static str {
    if enabled {
        "出庫時分を含める: ON"
    } else {
        "出庫時分を含める: OFF"
    }
}

/// `PreviewDto.policy_choices` の文字列を [`OperationPolicy`] へ変換する。
pub fn operation_policy_from_choice(choice: &str) -> Option<OperationPolicy> {
    match choice {
        "preserve" => Some(OperationPolicy::Preserve),
        "remove_target_train" => Some(OperationPolicy::RemoveTargetTrain),
        _ => None,
    }
}

/// [`OperationPolicy`] の画面表示ラベルを返す。
pub fn operation_policy_label(policy: OperationPolicy) -> &'static str {
    match policy {
        OperationPolicy::Preserve => "元のまま保持 (Preserve)",
        OperationPolicy::RemoveTargetTrain => "対象列車から削除 (RemoveTargetTrain)",
        OperationPolicy::Unselected => "未選択 (Unselected)",
    }
}

pub fn operation_policy_short_label(policy: OperationPolicy) -> &'static str {
    match policy {
        OperationPolicy::Preserve => "Preserve",
        OperationPolicy::RemoveTargetTrain => "RemoveTargetTrain",
        OperationPolicy::Unselected => "Unselected",
    }
}

pub fn outbound_source_label(source: OutboundRuntimeSource) -> &'static str {
    match source {
        OutboundRuntimeSource::Measured => "API 計測",
        OutboundRuntimeSource::Manual => "手動入力",
    }
}

/// 手動入力の出庫時分（秒・小数対応）を検証して `f64` へ変換する。
pub fn parse_manual_outbound_seconds(raw: &str) -> Result<f64, BusinessError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(BusinessError {
            kind: mtr_oudia_application::BusinessErrorKind::Validation,
            message: "出庫時分（秒数）を入力してください".to_string(),
            detail: None,
        });
    }
    let seconds: f64 = trimmed.parse().map_err(|_| BusinessError {
        kind: mtr_oudia_application::BusinessErrorKind::Validation,
        message: "出庫時分は 0 以上の数値（秒）で入力してください".to_string(),
        detail: Some(format!("入力値: {trimmed}")),
    })?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(BusinessError {
            kind: mtr_oudia_application::BusinessErrorKind::Validation,
            message: "出庫時分は 0 以上の有限な数値（秒）で入力してください".to_string(),
            detail: Some(format!("入力値: {trimmed}")),
        });
    }
    Ok(seconds)
}

pub fn normalize_utc_offset_input(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "+09:00".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn save_button_label(saving: bool) -> &'static str {
    if saving {
        "保存中..."
    } else {
        "上書き保存"
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

pub fn format_arrival_departure(arrival: Option<&str>, departure: Option<&str>) -> String {
    match (arrival, departure) {
        (Some(arr), Some(dep)) => format!("{arr} / {dep}"),
        (Some(arr), None) => format!("{arr} / -"),
        (None, Some(dep)) => format!("- / {dep}"),
        (None, None) => "-".to_string(),
    }
}

pub fn format_dwell_seconds(dwell_millis: i64) -> String {
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

pub fn direction_label(direction: &str) -> &str {
    match direction {
        "kudari" => "下り",
        "nobori" => "上り",
        other => other,
    }
}

pub fn candidate_rank_badge(rank: &str) -> (&str, TagKind) {
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

pub fn kijun_status_badge(kijun_status: &str) -> (&str, TagKind) {
    match kijun_status {
        "valid" => ("valid", TagKind::Green),
        "missing" => ("missing", TagKind::WarmGray),
        other => (other, TagKind::Red),
    }
}

pub fn format_dimension_label(index: usize, value: &impl std::fmt::Display) -> String {
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

pub fn format_station_slot_label(slot: &StationSlotDto) -> String {
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

pub fn station_mapping_issues(
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
pub struct MtrShellState {
    pub theme_preference: ThemePreference,
    pub os_dark: bool,
    pub theme: CarbonTheme,
    pub current_step: AppStep,
    pub candidates: Vec<String>,
    pub selected_endpoint: Option<String>,
    pub dimension: u32,
    pub snapshot: Option<SnapshotDto>,
    pub oudia_path: Option<PathBuf>,
    pub inspection: Option<InspectionDto>,
    pub selected_route_id: Option<String>,
    pub selected_diagram_index: Option<usize>,
    pub selected_train_type: Option<usize>,
    pub route_candidates: Vec<RouteCandidateDto>,
    pub selected_candidate_id: Option<CandidateId>,
    pub searched_train_number: Option<String>,
    pub manual_station_mappings: Option<Vec<StationMappingDto>>,
    pub outbound_status: Option<OutboundStatusDto>,
    pub generate_outbound: bool,
    pub operation_policy: OperationPolicy,
    pub preview: Option<PreviewDto>,
    pub save_receipt: Option<SaveReceipt>,
    pub status: AppStatus,
}

impl MtrShellState {
    pub fn new(init_result: Result<(), String>) -> Self {
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
            searched_train_number: None,
            manual_station_mappings: None,
            outbound_status: None,
            generate_outbound: false,
            operation_policy: OperationPolicy::Preserve,
            preview: None,
            save_receipt: None,
            status,
        }
    }

    pub fn sync_theme(&mut self) {
        self.theme =
            CarbonTheme::from_kind(resolve_theme_kind(self.theme_preference, self.os_dark));
    }

    pub fn set_os_dark(&mut self, os_dark: bool) {
        self.os_dark = os_dark;
        self.sync_theme();
    }

    pub fn set_theme_preference(&mut self, preference: ThemePreference) {
        self.theme_preference = preference;
        self.sync_theme();
    }

    pub fn toggle_dark_mode(&mut self) {
        let next = match self.theme.kind {
            CarbonThemeKind::Gray100 => ThemePreference::Light,
            CarbonThemeKind::Gray10 => ThemePreference::Dark,
        };
        self.set_theme_preference(next);
    }

    pub fn is_step_completed(&self, step: AppStep) -> bool {
        match step {
            AppStep::Step1Mtr => self.snapshot.is_some(),
            AppStep::Step2Oudia => self.inspection.is_some(),
            AppStep::Step3RouteMatch => self.selected_candidate_id.is_some(),
            AppStep::Step4Preview => self.preview.is_some(),
            AppStep::Step5Save => self.save_receipt.is_some(),
        }
    }

    pub fn select_step(&mut self, step: AppStep) {
        self.current_step = step;
    }

    pub fn clear_candidate_selection(&mut self) {
        self.route_candidates.clear();
        self.selected_candidate_id = None;
        self.searched_train_number = None;
        self.manual_station_mappings = None;
        self.preview = None;
        self.save_receipt = None;
    }

    pub fn clear_step3_for_snapshot_change(&mut self) {
        self.selected_route_id = None;
        self.outbound_status = None;
        self.generate_outbound = false;
        self.operation_policy = OperationPolicy::Preserve;
        self.clear_candidate_selection();
    }

    pub fn clear_step3_for_inspection_change(&mut self) {
        self.selected_diagram_index = None;
        self.selected_train_type = None;
        self.operation_policy = OperationPolicy::Preserve;
        self.clear_candidate_selection();
    }

    /// 接続先を選択し、異なる接続先へ切り替わった場合は旧 `snapshot` と下流状態をクリアする。
    pub fn select_endpoint(&mut self, endpoint: String) -> bool {
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

    pub fn apply_selected_endpoint(&mut self, endpoint: String) {
        if self.selected_endpoint.as_deref() != Some(endpoint.as_str()) {
            self.snapshot = None;
            self.dimension = 0;
            self.clear_step3_for_snapshot_change();
        }
        self.selected_endpoint = Some(endpoint);
    }

    pub fn apply_detection_result(&mut self, result: Result<Vec<EndpointDto>, BusinessError>) {
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

    pub fn apply_snapshot_result(
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

    pub fn apply_refresh_snapshot_result(
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

    pub fn apply_inspection_result(
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

    pub fn select_route(&mut self, route_id: String) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.selected_route_id.as_deref() != Some(route_id.as_str()) {
            self.selected_route_id = Some(route_id);
            self.outbound_status = None;
            self.generate_outbound = false;
            self.operation_policy = OperationPolicy::Preserve;
            self.clear_candidate_selection();
        }
        true
    }

    pub fn select_diagram_index(&mut self, diagram_index: Option<usize>) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.selected_diagram_index != diagram_index {
            self.selected_diagram_index = diagram_index;
            self.clear_candidate_selection();
        }
        true
    }

    pub fn select_train_type(&mut self, train_type: Option<usize>) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.selected_train_type != train_type {
            self.selected_train_type = train_type;
            self.clear_candidate_selection();
        }
        true
    }

    pub fn select_candidate(&mut self, candidate_id: CandidateId) -> bool {
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

    pub fn selected_route(&self) -> Option<&RouteDto> {
        let route_id = self.selected_route_id.as_deref()?;
        self.snapshot
            .as_ref()?
            .routes
            .iter()
            .find(|route| route.id == route_id)
    }

    pub fn selected_candidate(&self) -> Option<&RouteCandidateDto> {
        let candidate_id = self.selected_candidate_id.as_ref()?;
        self.route_candidates
            .iter()
            .find(|candidate| &candidate.id == candidate_id)
    }

    pub fn selected_template(&self) -> Option<&TemplateDto> {
        let candidate = self.selected_candidate()?;
        self.inspection.as_ref()?.templates.iter().find(|template| {
            template.diagram_index == candidate.diagram_index
                && template.train_index == candidate.train_index
                && template.direction == candidate.direction
        })
    }

    pub fn effective_station_mappings(&self) -> Option<&[StationMappingDto]> {
        if let Some(manual) = self.manual_station_mappings.as_deref() {
            Some(manual)
        } else {
            self.selected_candidate()
                .map(|candidate| candidate.station_mappings.as_slice())
        }
    }

    pub fn set_manual_station_slot(
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

    pub fn reset_manual_station_mappings(&mut self) -> bool {
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

    pub fn apply_candidates_result_with_train_number(
        &mut self,
        train_number: Option<String>,
        result: Result<Vec<RouteCandidateDto>, BusinessError>,
    ) {
        let succeeded = result.is_ok();
        self.apply_candidates_result(result);
        if succeeded {
            self.searched_train_number = train_number;
        }
    }

    pub fn apply_candidates_result(
        &mut self,
        result: Result<Vec<RouteCandidateDto>, BusinessError>,
    ) {
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

    pub fn set_operation_policy(&mut self, policy: OperationPolicy) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.operation_policy != policy {
            self.operation_policy = policy;
            self.preview = None;
            self.save_receipt = None;
            true
        } else {
            false
        }
    }

    pub fn set_generate_outbound(&mut self, enabled: bool) -> bool {
        if self.status.is_busy() || self.status.is_init_failed() {
            return false;
        }
        if self.generate_outbound != enabled {
            self.generate_outbound = enabled;
            self.preview = None;
            self.save_receipt = None;
            true
        } else {
            false
        }
    }

    pub fn toggle_generate_outbound(&mut self) -> bool {
        self.set_generate_outbound(!self.generate_outbound)
    }

    pub fn apply_outbound_status_loaded(
        &mut self,
        result: Result<OutboundStatusDto, BusinessError>,
        notify_error: bool,
    ) {
        match result {
            Ok(dto) => {
                let changed = self.outbound_status.as_ref().is_some_and(|prev| {
                    prev.valid != dto.valid
                        || prev.setting != dto.setting
                        || prev.duration_label != dto.duration_label
                        || prev.message != dto.message
                }) || (!dto.valid && self.generate_outbound);
                if !dto.valid {
                    self.generate_outbound = false;
                }
                if changed {
                    self.preview = None;
                    self.save_receipt = None;
                }
                self.outbound_status = Some(dto);
            }
            Err(err) if notify_error => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::OutboundError { title, detail };
            }
            Err(_) => {}
        }
    }

    pub fn apply_outbound_result(
        &mut self,
        result: Result<OutboundStatusDto, BusinessError>,
        is_manual: bool,
    ) {
        match result {
            Ok(dto) => {
                let duration_label = dto.duration_label.clone();
                let valid = dto.valid;
                if !valid {
                    self.generate_outbound = false;
                }
                self.outbound_status = Some(dto);
                self.preview = None;
                self.save_receipt = None;
                self.status = AppStatus::OutboundMeasured {
                    duration_label,
                    is_manual,
                    valid,
                };
            }
            Err(err) => {
                let (title, detail) = format_business_error(&err);
                self.status = AppStatus::OutboundError { title, detail };
            }
        }
    }

    pub fn apply_preview_result(&mut self, result: Result<PreviewDto, BusinessError>) {
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

    pub fn apply_save_result(&mut self, result: Result<SaveReceipt, BusinessError>) {
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
