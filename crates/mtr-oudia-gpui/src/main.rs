#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Carbon Design System v11 ベースの 5 ステップ画面シェルと Step 1〜5 の実装。
//!
//! バックエンド処理はすべて [`mtr_oudia_gpui::backend::AppBackend`] 経由で実行する。

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, ClickEvent, Context, Div, Entity, FontWeight, Hsla, IntoElement,
    ParentElement, Render, Rgba, ScrollHandle, ScrollWheelEvent, SharedString, Styled,
    Subscription, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use mtr_oudia_application::{ManualMappingInput, MtrEndpoint, PreviewId, SessionId};
use mtr_oudia_domain::OperationPolicy;
use mtr_oudia_gpui::backend::{AppBackend, format_business_error};
use mtr_oudia_gpui::carbon::{
    ButtonKind, CarbonSpacing, CarbonTheme, CarbonTypography, NotificationKind, TagKind, TileLayer,
    carbon_button, carbon_inline_notification, carbon_tag, carbon_tile,
};
use mtr_oudia_gpui::shell_state::{
    AppStatus, AppStep, MtrShellState, ThemePreference, candidate_rank_badge,
    dark_mode_button_label, direction_label, discover_button_label, fetch_button_label,
    find_candidates_button_label, format_arrival_departure, format_dimension_label,
    format_dwell_seconds, format_station_slot_label, generate_outbound_button_label,
    inspect_button_label, is_dark_appearance, kijun_status_badge, normalize_train_number_input,
    normalize_utc_offset_input, operation_policy_from_choice, operation_policy_label,
    operation_policy_short_label, outbound_measure_button_label, outbound_source_label,
    parse_manual_outbound_seconds, preview_button_label, refresh_button_label, save_button_label,
    station_mapping_issues, status_notification, step_for_scroll_position, suggest_save_location,
    theme_badge_label,
};
use mtr_oudia_gpui::text_input::{TextInput, TextInputStyle, bind_keys as bind_text_input_keys};

const TRANSPARENT: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

const SIDEBAR_WIDTH: f32 = 220.0;

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

struct MtrApp {
    backend: Option<AppBackend>,
    session_id: Option<SessionId>,
    state: MtrShellState,
    train_number_input: Entity<TextInput>,
    step4_train_number_input: Entity<TextInput>,
    depot_clock_input: Entity<TextInput>,
    utc_offset_input: Entity<TextInput>,
    manual_outbound_seconds_input: Entity<TextInput>,
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
        let step4_train_number_input = cx.new(|cx| {
            TextInput::new("列車番号（精密入力用・空欄で 10:00 固定）", cx).with_style(input_style)
        });
        let depot_clock_input = cx.new(|cx| {
            TextInput::new("車庫発時刻 (HH:mm:ss 例: 12:00:00)", cx).with_style(input_style)
        });
        let utc_offset_input = cx.new(|cx| {
            TextInput::new("UTCオフセット (例: +09:00)", cx)
                .with_text("+09:00")
                .with_style(input_style)
        });
        let manual_outbound_seconds_input =
            cx.new(|cx| TextInput::new("手動入力秒数 (例: 13.5)", cx).with_style(input_style));

        Self {
            backend,
            session_id,
            state,
            train_number_input,
            step4_train_number_input,
            depot_clock_input,
            utc_offset_input,
            manual_outbound_seconds_input,
            steps_scroll_handle: ScrollHandle::new(),
            sync_step_from_scroll: false,
            _appearance_subscription: appearance_subscription,
        }
    }

    fn sync_input_theme(&self, cx: &mut Context<Self>) {
        let style = carbon_text_input_style(&self.state.theme);
        for input in [
            &self.train_number_input,
            &self.step4_train_number_input,
            &self.depot_clock_input,
            &self.utc_offset_input,
            &self.manual_outbound_seconds_input,
        ] {
            input.update(cx, |field, cx| field.set_style(style, cx));
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
        let train_number = normalize_train_number_input(self.train_number_input.read(cx).text());
        let synced_text = train_number.clone().unwrap_or_default();
        self.step4_train_number_input
            .update(cx, |input, cx| input.set_text(synced_text, cx));
        self.start_find_candidates_with_train_number(train_number, cx);
    }

    fn start_find_candidates_from_step4(&mut self, cx: &mut Context<Self>) {
        let train_number =
            normalize_train_number_input(self.step4_train_number_input.read(cx).text());
        let synced_text = train_number.clone().unwrap_or_default();
        self.train_number_input
            .update(cx, |input, cx| input.set_text(synced_text, cx));
        self.start_find_candidates_with_train_number(train_number, cx);
    }

    fn switch_to_normal_mode_from_step4(&mut self, cx: &mut Context<Self>) {
        self.train_number_input
            .update(cx, |input, cx| input.clear(cx));
        self.step4_train_number_input
            .update(cx, |input, cx| input.clear(cx));
        self.start_find_candidates_with_train_number(None, cx);
    }

    fn start_find_candidates_with_train_number(
        &mut self,
        train_number: Option<String>,
        cx: &mut Context<Self>,
    ) {
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
        let searched_number = train_number.clone();
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
                    Ok(result) => this
                        .state
                        .apply_candidates_result_with_train_number(searched_number, result),
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

    fn start_fetch_outbound_status(&mut self, notify_error: bool, cx: &mut Context<Self>) {
        if self.state.status.is_init_failed() {
            return;
        }
        let Some(route_id) = self.state.selected_route_id.clone() else {
            return;
        };
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        let rx = backend.get_outbound_status(session_id, route_id.clone());
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                if this.state.selected_route_id.as_deref() != Some(route_id.as_str()) {
                    return;
                }
                match received {
                    Ok(result) => this
                        .state
                        .apply_outbound_status_loaded(result, notify_error),
                    Err(_) if notify_error => {
                        this.state.status = AppStatus::OutboundError {
                            title: "出庫時分の取得が中断されました".to_string(),
                            detail: "バックグラウンド処理との通信が終了しました。".to_string(),
                        };
                    }
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_measure_outbound(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let Some(route_id) = self.state.selected_route_id.clone() else {
            self.state.status = AppStatus::OutboundError {
                title: "MTR 路線が未選択です".to_string(),
                detail: "Step 3 で対象の MTR 路線を選択してください。".to_string(),
            };
            cx.notify();
            return;
        };
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        let depot_clock = self.depot_clock_input.read(cx).text().trim().to_string();
        let utc_offset = normalize_utc_offset_input(self.utc_offset_input.read(cx).text());
        self.state.status = AppStatus::MeasuringOutbound;
        cx.notify();

        let rx =
            backend.measure_outbound_runtime(session_id, route_id.clone(), depot_clock, utc_offset);
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                if this.state.selected_route_id.as_deref() != Some(route_id.as_str()) {
                    this.state.status = AppStatus::Idle;
                    cx.notify();
                    return;
                }
                match received {
                    Ok(result) => this.state.apply_outbound_result(result, false),
                    Err(_) => {
                        this.state.status = AppStatus::OutboundError {
                            title: "出庫時分の計測が中断されました".to_string(),
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

    fn start_save_manual_outbound(&mut self, cx: &mut Context<Self>) {
        if self.state.status.is_busy() || self.state.status.is_init_failed() {
            return;
        }
        let Some(route_id) = self.state.selected_route_id.clone() else {
            self.state.status = AppStatus::OutboundError {
                title: "MTR 路線が未選択です".to_string(),
                detail: "Step 3 で対象の MTR 路線を選択してください。".to_string(),
            };
            cx.notify();
            return;
        };
        let seconds =
            match parse_manual_outbound_seconds(self.manual_outbound_seconds_input.read(cx).text())
            {
                Ok(seconds) => seconds,
                Err(err) => {
                    let (title, detail) = format_business_error(&err);
                    self.state.status = AppStatus::OutboundError { title, detail };
                    cx.notify();
                    return;
                }
            };
        let (Some(backend), Some(session_id)) = (&self.backend, self.session_id.clone()) else {
            return;
        };

        self.state.status = AppStatus::MeasuringOutbound;
        cx.notify();

        let rx = backend.save_manual_outbound(session_id, route_id.clone(), seconds);
        cx.spawn(async move |this, cx| {
            let received = rx.await;
            this.update(cx, |this, cx| {
                if this.state.selected_route_id.as_deref() != Some(route_id.as_str()) {
                    this.state.status = AppStatus::Idle;
                    cx.notify();
                    return;
                }
                match received {
                    Ok(result) => this.state.apply_outbound_result(result, true),
                    Err(_) => {
                        this.state.status = AppStatus::OutboundError {
                            title: "出庫時分の手動保存が中断されました".to_string(),
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
        let operation_policy = self.state.operation_policy;
        let generate_outbound = self.state.generate_outbound;
        self.state.status = AppStatus::Previewing;
        cx.notify();

        let rx = backend.preview_conversion(
            session_id,
            candidate_id,
            manual,
            operation_policy,
            generate_outbound,
        );
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

    fn start_overwrite_save(&mut self, cx: &mut Context<Self>) {
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
        let Some(oudia_path) = self.state.oudia_path.clone() else {
            self.state.status = AppStatus::SaveError {
                title: "元の OuDia ファイルが未選択です".to_string(),
                detail: "Step 2 で OuDia ファイルを読み込んでから保存してください。".to_string(),
            };
            cx.notify();
            return;
        };
        self.start_save(preview_id, oudia_path, cx);
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

        let operation_policy = self.state.operation_policy;
        self.state.status = AppStatus::Saving {
            path: output_path.display().to_string(),
        };
        cx.notify();

        let rx = backend.save_preview(session_id, preview_id, output_path, operation_policy);
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
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
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
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
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
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
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
                    ),
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
            .min_w_0()
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
                                    this.start_fetch_outbound_status(false, cx);
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
            .overflow_hidden()
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
                            .flex_wrap()
                            .items_center()
                            .min_w_0()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(160.0))
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
                            .overflow_hidden()
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
        let measuring_outbound = self.state.status.is_measuring_outbound();

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
        let is_precision_active = self
            .state
            .preview
            .as_ref()
            .map(|p| p.precision_mode)
            .unwrap_or_else(|| self.state.searched_train_number.is_some());

        let mut action_info = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(CarbonTypography::HEADING_02.font_size_px())
                            .line_height(CarbonTypography::HEADING_02.line_height_px())
                            .text_color(c.text_primary)
                            .child("時刻表プレビュー生成"),
                    )
                    .child(carbon_tag(preview_tag_label, preview_tag_kind, theme))
                    .child(carbon_tag(
                        if is_precision_active {
                            "精密入力"
                        } else {
                            "10:00 固定"
                        },
                        if is_precision_active {
                            TagKind::Blue
                        } else {
                            TagKind::CoolGray
                        },
                        theme,
                    ))
                    .child(carbon_tag(
                        format!(
                            "Policy: {}",
                            operation_policy_short_label(self.state.operation_policy)
                        ),
                        TagKind::CoolGray,
                        theme,
                    ))
                    .child(carbon_tag(
                        if self.state.generate_outbound {
                            "出区: ON"
                        } else {
                            "出区: OFF"
                        },
                        if self.state.generate_outbound {
                            TagKind::Green
                        } else {
                            TagKind::CoolGray
                        },
                        theme,
                    )),
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
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                    .child(action_info)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
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
                    ),
            );

        // 精密入力モード設定パネル
        let (precision_badge_label, precision_badge_kind) =
            if let Some(number) = &self.state.searched_train_number {
                (format!("有効 (列車番号: {number})"), TagKind::Blue)
            } else if self
                .state
                .preview
                .as_ref()
                .is_some_and(|preview| preview.precision_mode)
            {
                ("有効 (精密入力)".to_string(), TagKind::Blue)
            } else {
                ("通常モード (10:00 固定)".to_string(), TagKind::CoolGray)
            };
        let step4_train_empty = self
            .step4_train_number_input
            .read(cx)
            .text()
            .trim()
            .is_empty();
        let normal_reset_disabled =
            disabled || (self.state.searched_train_number.is_none() && step4_train_empty);

        let precision_tile = carbon_tile(TileLayer::Layer01, theme)
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
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
                            .flex_wrap()
                            .items_center()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(CarbonTypography::HEADING_02.font_size_px())
                                    .line_height(CarbonTypography::HEADING_02.line_height_px())
                                    .text_color(c.text_primary)
                                    .child("精密入力モード（元発時刻・番線更新）"),
                            )
                            .child(carbon_tag(
                                precision_badge_label,
                                precision_badge_kind,
                                theme,
                            )),
                    ),
            )
            .child(
                div()
                    .text_size(CarbonTypography::LABEL_01.font_size_px())
                    .line_height(CarbonTypography::LABEL_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(
                        "列車番号を指定して照合すると、元ファイルの始発駅発時刻を基準時刻として採用し、MTR ホーム名と駅番線略称の一致で番線も更新します。空欄に戻すと 10:00 固定・番線保持になります。",
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .min_w_0()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(220.0))
                            .child(self.step4_train_number_input.clone()),
                    )
                    .child(carbon_button(
                        "step4-apply-precision",
                        "列車番号で再照合",
                        ButtonKind::Secondary,
                        disabled || self.state.selected_route_id.is_none(),
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_find_candidates_from_step4(cx);
                        }),
                    ))
                    .child(carbon_button(
                        "step4-reset-normal-mode",
                        "10:00 固定に戻す",
                        ButtonKind::Ghost,
                        normal_reset_disabled || self.state.selected_route_id.is_none(),
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.switch_to_normal_mode_from_step4(cx);
                        }),
                    )),
            );

        // OperationPolicy 切替パネル
        let mut policy_variants: Vec<OperationPolicy> = self
            .state
            .preview
            .as_ref()
            .map(|preview| {
                preview
                    .policy_choices
                    .iter()
                    .filter_map(|choice| operation_policy_from_choice(choice))
                    .collect()
            })
            .unwrap_or_default();
        if policy_variants.is_empty() {
            policy_variants = vec![
                OperationPolicy::Preserve,
                OperationPolicy::RemoveTargetTrain,
            ];
        }
        let policy_buttons = policy_variants.into_iter().map(|policy| {
            let is_selected = self.state.operation_policy == policy;
            carbon_button(
                SharedString::from(format!(
                    "step4-policy-{}",
                    operation_policy_short_label(policy)
                )),
                operation_policy_label(policy),
                if is_selected {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Secondary
                },
                disabled,
                theme,
                cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    if this.state.set_operation_policy(policy) {
                        cx.notify();
                    }
                }),
            )
        });

        let operation_note = if self
            .state
            .preview
            .as_ref()
            .is_some_and(|preview| !preview.operation_present)
        {
            "この OuDia ファイルには Operation 定義がありません（選択内容は保存時の検証・計画に引き継がれます）。方針切替時は既存の保存状態がクリアされます。"
        } else {
            "既存 Operation の扱いを切り替えます。方針を切り替えると Step 5 の保存状態とプレビューがクリアされるため、再度「プレビュー生成」を実行してください。"
        };

        let operation_tile = carbon_tile(TileLayer::Layer01, theme)
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
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
                            .flex_wrap()
                            .items_center()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(CarbonTypography::HEADING_02.font_size_px())
                                    .line_height(CarbonTypography::HEADING_02.line_height_px())
                                    .text_color(c.text_primary)
                                    .child("Operation 方針"),
                            )
                            .child(carbon_tag(
                                operation_policy_short_label(self.state.operation_policy),
                                TagKind::Blue,
                                theme,
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .children(policy_buttons),
            )
            .child(
                div()
                    .text_size(CarbonTypography::LABEL_01.font_size_px())
                    .line_height(CarbonTypography::LABEL_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(operation_note),
            );

        // 出庫時分計測・出区設定パネル
        let selected_route = self.state.selected_route();
        let outbound_valid = self
            .state
            .outbound_status
            .as_ref()
            .is_some_and(|status| status.valid);
        let (outbound_tag_label, outbound_tag_kind) = match &self.state.outbound_status {
            Some(status) if status.valid => {
                (format!("有効: {}", status.duration_label), TagKind::Green)
            }
            Some(_) => ("要再計測".to_string(), TagKind::WarmGray),
            None => ("未計測".to_string(), TagKind::CoolGray),
        };
        let first_station_name = self
            .state
            .outbound_status
            .as_ref()
            .map(|status| status.first_station_name.as_str())
            .or_else(|| {
                selected_route
                    .and_then(|route| route.stations.first())
                    .map(|stop| stop.station_name.as_str())
            })
            .unwrap_or("未取得");
        let first_platform_name = self
            .state
            .outbound_status
            .as_ref()
            .map(|status| status.first_platform_name.as_str())
            .or_else(|| {
                selected_route
                    .and_then(|route| route.stations.first())
                    .map(|stop| stop.platform_name.as_str())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or("記載なし");
        let first_dwell_display = selected_route
            .and_then(|route| route.stations.first())
            .map(|stop| format_dwell_seconds(stop.dwell_millis))
            .unwrap_or_else(|| "-".to_string());
        let outbound_duration_display = self
            .state
            .outbound_status
            .as_ref()
            .map(|status| status.duration_label.clone())
            .unwrap_or_else(|| "未測定".to_string());
        let outbound_source_display = self
            .state
            .outbound_status
            .as_ref()
            .and_then(|status| status.setting.as_ref())
            .map(|setting| outbound_source_label(setting.source).to_string())
            .unwrap_or_else(|| "-".to_string());

        let toggle_outbound_disabled =
            disabled || (!outbound_valid && !self.state.generate_outbound);
        let route_missing = self.state.selected_route_id.is_none();

        let mut outbound_tile = carbon_tile(TileLayer::Layer01, theme)
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
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
                                            .child("出庫時分計測・出区設定"),
                                    )
                                    .child(carbon_tag(outbound_tag_label, outbound_tag_kind, theme)),
                            )
                            .child(info_row(
                                "始発駅 / ホーム:",
                                format!("{first_station_name} / {first_platform_name}"),
                                theme,
                            ))
                            .child(info_row(
                                "出庫 → 始発駅所要時間:",
                                format!(
                                    "{outbound_duration_display}（始発駅停車: {first_dwell_display} / 方式: {outbound_source_display}）"
                                ),
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
                                "step4-toggle-generate-outbound",
                                generate_outbound_button_label(self.state.generate_outbound),
                                if self.state.generate_outbound {
                                    ButtonKind::Primary
                                } else {
                                    ButtonKind::Secondary
                                },
                                toggle_outbound_disabled,
                                theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    if this.state.toggle_generate_outbound() {
                                        cx.notify();
                                    }
                                }),
                            )),
                    ),
            );

        if let Some(message) = self
            .state
            .outbound_status
            .as_ref()
            .and_then(|status| status.message.as_ref())
        {
            outbound_tile = outbound_tile.child(carbon_inline_notification(
                NotificationKind::Warning,
                "出庫時分の確認が必要です",
                message.clone(),
                theme,
            ));
        }

        outbound_tile = outbound_tile
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .min_w_0()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.0))
                            .child(self.depot_clock_input.clone()),
                    )
                    .child(
                        div()
                            .w(px(140.0))
                            .flex_shrink_0()
                            .child(self.utc_offset_input.clone()),
                    )
                    .child(carbon_button(
                        "step4-measure-outbound",
                        outbound_measure_button_label(measuring_outbound),
                        ButtonKind::Secondary,
                        disabled || route_missing,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_measure_outbound(cx);
                        }),
                    ))
                    .child(carbon_button(
                        "step4-refresh-outbound-status",
                        "保存済み状態を取得",
                        ButtonKind::Ghost,
                        disabled || route_missing,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_fetch_outbound_status(true, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .min_w_0()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_03))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.0))
                            .child(self.manual_outbound_seconds_input.clone()),
                    )
                    .child(carbon_button(
                        "step4-save-manual-outbound",
                        "手動秒数を保存",
                        ButtonKind::Secondary,
                        disabled || route_missing,
                        theme,
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.start_save_manual_outbound(cx);
                        }),
                    )),
            )
            .child(
                div()
                    .text_size(CarbonTypography::LABEL_01.font_size_px())
                    .line_height(CarbonTypography::LABEL_01.line_height_px())
                    .text_color(c.text_secondary)
                    .child(if outbound_valid {
                        "出区時刻 = 始発駅発 − 始発駅停車時間 − 出庫時分。「出庫時分を含める」を切り替えた後はプレビューを再生成してください。"
                    } else {
                        "車庫発時刻 (HH:mm:ss) から API 計測するか、秒数を手動保存すると「出庫時分を含める」を ON にできます。"
                    }),
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

            if let Some(outbound_preview) = &preview.outbound {
                let first_dwell = preview
                    .stops
                    .first()
                    .map(|stop| format_dwell_seconds(stop.dwell_millis))
                    .unwrap_or_else(|| "-".to_string());
                result_tile = result_tile.child(
                    carbon_tile(TileLayer::Layer02, theme)
                        .min_w_0()
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
                                        .text_size(CarbonTypography::BODY_01.font_size_px())
                                        .line_height(CarbonTypography::BODY_01.line_height_px())
                                        .text_color(c.text_primary)
                                        .child("出区プレビュー"),
                                )
                                .child(carbon_tag("出区時刻生成あり", TagKind::Green, theme)),
                        )
                        .child(info_row(
                            "出庫時刻:",
                            outbound_preview.outbound_time.clone(),
                            theme,
                        ))
                        .child(info_row(
                            "初発（始発駅発）:",
                            outbound_preview.first_departure.clone(),
                            theme,
                        ))
                        .child(info_row(
                            "所要時間:",
                            format!(
                                "{}（始発駅停車: {first_dwell}）",
                                outbound_preview.duration_label
                            ),
                            theme,
                        )),
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
                        .w(px(144.0))
                        .flex_shrink_0()
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child("既存着発"),
                )
                .child(
                    div()
                        .w(px(144.0))
                        .flex_shrink_0()
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child("生成着発"),
                )
                .child(
                    div()
                        .w(px(88.0))
                        .flex_shrink_0()
                        .text_size(CarbonTypography::LABEL_01.font_size_px())
                        .line_height(CarbonTypography::LABEL_01.line_height_px())
                        .text_color(c.text_secondary)
                        .child("番線"),
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
                        let platform = stop
                            .updated_platform
                            .as_deref()
                            .filter(|name| !name.is_empty())
                            .unwrap_or("保持")
                            .to_string();
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
                                    .w(px(144.0))
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
                                    .w(px(144.0))
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
                                    .w(px(88.0))
                                    .flex_shrink_0()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(CarbonTypography::BODY_COMPACT_01.font_size_px())
                                    .line_height(CarbonTypography::BODY_COMPACT_01.line_height_px())
                                    .text_color(if stop.updated_platform.is_some() {
                                        c.text_primary
                                    } else {
                                        c.text_secondary
                                    })
                                    .child(platform),
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
            .child(precision_tile)
            .child(operation_tile)
            .child(outbound_tile)
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
            .min_w_0()
            .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(CarbonSpacing::px(CarbonSpacing::SP_04))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(CarbonSpacing::px(CarbonSpacing::SP_02))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
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
                            .child(info_row("元 OuDia パス:", oudia_path_display, theme))
                            .child(info_row(
                                "Operation 方針:",
                                operation_policy_label(self.state.operation_policy),
                                theme,
                            ))
                            .child(info_row(
                                "出区生成:",
                                if preview.outbound.is_some() {
                                    "有効（出区時刻を含める）"
                                } else {
                                    "無効"
                                },
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
                                "select-save-path",
                                if saving {
                                    "保存中..."
                                } else {
                                    "別名で保存..."
                                },
                                ButtonKind::Secondary,
                                disabled,
                                theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.start_select_save_path(cx);
                                }),
                            ))
                            .child(carbon_button(
                                "overwrite-save",
                                save_button_label(saving),
                                ButtonKind::Primary,
                                disabled || self.state.oudia_path.is_none(),
                                theme,
                                cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.start_overwrite_save(cx);
                                }),
                            )),
                    ),
            );

        let result_tile = if let Some(receipt) = &self.state.save_receipt {
            carbon_tile(TileLayer::Layer01, theme)
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
                window_min_size: Some(size(px(800.0), px(560.0))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| MtrApp::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
