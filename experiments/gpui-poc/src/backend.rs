//! GPUI フロントエンドから `ConversionService` を安全に呼び出すバックエンドブリッジ層。
//!
//! `reqwest` ベースの通信が tokio リアクタを必要とするため、専用の常駐 current-thread
//! tokio スレッドで `ConversionService` を保持・実行し、結果を `oneshot::Receiver` で返す。

use std::{
    ffi::OsString, future::Future, path::PathBuf, sync::Arc, thread::JoinHandle, time::Duration,
};

use futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
    future::{Either, LocalBoxFuture},
    stream::FuturesUnordered,
};
use mtr_oudia_application::{
    BusinessError, CandidateId, ConversionService, ConversionSessionStore, EndpointDto,
    InspectionDto, ManualMappingInput, MtrEndpoint, OutboundStatusDto, PreviewDto, PreviewId,
    RouteCandidateDto, RouteDto, RouteStationDto, SaveReceipt, SessionId, SnapshotDto,
};
use mtr_oudia_domain::OperationPolicy;
use mtr_oudia_infrastructure::{
    FileMinecraftLogProvider, FileOudiaRepository, JsonSettingsRepository, ReqwestMtrApiClient,
    SafeOudiaWriter, WindowsListeningPortProvider,
};

type Service<'a> = ConversionService<
    'a,
    WindowsListeningPortProvider,
    ReqwestMtrApiClient,
    FileOudiaRepository,
    JsonSettingsRepository,
    SafeOudiaWriter,
>;

/// 環境変数の値から設定ファイル (`MtrOudiaConverter/settings.json`) のパスを解決する。
///
/// 優先順位は本体 (`src-tauri/src/lib.rs`) と同じ `APPDATA` → `XDG_CONFIG_HOME` → `HOME/.config` → `.`。
pub fn settings_path_from_env(
    appdata: Option<OsString>,
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> PathBuf {
    let base = appdata
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            xdg_config_home
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("MtrOudiaConverter").join("settings.json")
}

fn settings_path() -> PathBuf {
    settings_path_from_env(
        std::env::var_os("APPDATA"),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

/// 環境変数の値から Minecraft ログ候補のパス一覧を構築する。
///
/// 空値は除外し、`MTR_OUDIA_MINECRAFT_LOG` → `APPDATA/.minecraft/logs/latest.log` の順に並べる。
pub fn minecraft_log_paths_from_env(
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

/// `BusinessError` を Carbon 通知バナー向けの `(タイトル, 詳細)` ペアへ整形する。
pub fn format_business_error(err: &BusinessError) -> (String, String) {
    (err.message.clone(), err.detail.clone().unwrap_or_default())
}

/// MTR API キャッシュ待機再取得のポーリング間隔。
pub const REFRESH_CACHE_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// MTR API キャッシュ待機再取得の最大待機時間（30 秒キャッシュ + 境界余裕 1 秒）。
pub const REFRESH_CACHE_MAX_WAIT: Duration = Duration::from_secs(31);

/// キャッシュ待機再取得の結果。
#[derive(Debug, Clone)]
pub enum RefreshSnapshotOutcome {
    /// 駅・路線データに変化があり、セッションとスナップショットを更新した。
    Updated(SnapshotDto),
    /// 最大待機時間内に駅・路線データの変化がなかった（既存セッション・画面状態を維持）。
    Unchanged,
}

fn route_station_eq(a: &RouteStationDto, b: &RouteStationDto) -> bool {
    a.station_name == b.station_name
        && a.platform_name == b.platform_name
        && a.dwell_millis == b.dwell_millis
        && a.run_millis_to_next == b.run_millis_to_next
}

fn route_eq(a: &RouteDto, b: &RouteDto) -> bool {
    a.id == b.id
        && a.name == b.name
        && a.station_count == b.station_count
        && a.total_run_millis == b.total_run_millis
        && a.total_dwell_millis == b.total_dwell_millis
        && a.stations.len() == b.stations.len()
        && a.stations
            .iter()
            .zip(b.stations.iter())
            .all(|(sa, sb)| route_station_eq(sa, sb))
}

/// `api_current_time_millis` を除外し、駅・路線データ（および dimensions）に変化があるかを判定する。
pub fn snapshot_routes_changed(old: &SnapshotDto, new: &SnapshotDto) -> bool {
    if old.dimensions != new.dimensions || old.routes.len() != new.routes.len() {
        return true;
    }
    !old.routes
        .iter()
        .zip(new.routes.iter())
        .all(|(a, b)| route_eq(a, b))
}

enum BackendCommand {
    DetectMtrEndpoint {
        session_id: SessionId,
        reply: oneshot::Sender<Result<Vec<EndpointDto>, BusinessError>>,
    },
    FetchMtrSnapshot {
        session_id: SessionId,
        endpoint: MtrEndpoint,
        dimension: u32,
        reply: oneshot::Sender<Result<SnapshotDto, BusinessError>>,
    },
    RefreshMtrSnapshot {
        session_id: SessionId,
        endpoint: MtrEndpoint,
        dimension: u32,
        previous: SnapshotDto,
        reply: oneshot::Sender<Result<RefreshSnapshotOutcome, BusinessError>>,
    },
    InspectOudia {
        session_id: SessionId,
        location: PathBuf,
        reply: oneshot::Sender<Result<InspectionDto, BusinessError>>,
    },
    FindRouteCandidates {
        session_id: SessionId,
        route_id: String,
        diagram_index: Option<usize>,
        train_type: Option<usize>,
        train_number: Option<String>,
        reply: oneshot::Sender<Result<Vec<RouteCandidateDto>, BusinessError>>,
    },
    GetOutboundStatus {
        session_id: SessionId,
        route_id: String,
        reply: oneshot::Sender<Result<OutboundStatusDto, BusinessError>>,
    },
    MeasureOutboundRuntime {
        session_id: SessionId,
        route_id: String,
        depot_clock: String,
        utc_offset: String,
        reply: oneshot::Sender<Result<OutboundStatusDto, BusinessError>>,
    },
    SaveManualOutbound {
        session_id: SessionId,
        route_id: String,
        seconds: f64,
        reply: oneshot::Sender<Result<OutboundStatusDto, BusinessError>>,
    },
    PreviewConversion {
        session_id: SessionId,
        candidate_id: CandidateId,
        manual: Option<ManualMappingInput>,
        operation_policy: OperationPolicy,
        generate_outbound: bool,
        reply: oneshot::Sender<Result<PreviewDto, BusinessError>>,
    },
    SavePreview {
        session_id: SessionId,
        preview_id: PreviewId,
        output: PathBuf,
        operation_policy: OperationPolicy,
        reply: oneshot::Sender<Result<SaveReceipt, BusinessError>>,
    },
}

struct BackendInner {
    store: ConversionSessionStore,
    tx: Option<mpsc::UnboundedSender<BackendCommand>>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for BackendInner {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            tx.close_channel();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// GPUI ビュー間で共有できる `ConversionService` バックエンドブリッジ。
#[derive(Clone)]
pub struct AppBackend {
    inner: Arc<BackendInner>,
}

impl AppBackend {
    /// 本体 (`src-tauri`) と同じ adapter 構成で `ConversionService` と常駐 tokio スレッドを起動する。
    pub fn new() -> Result<Self, String> {
        let client = ReqwestMtrApiClient::new().map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;

        let ports = WindowsListeningPortProvider;
        let minecraft_log =
            FileMinecraftLogProvider::new(minecraft_log_paths()).with_running_minecraft();
        let repository = FileOudiaRepository;
        let settings = JsonSettingsRepository::new(settings_path());
        let saver = SafeOudiaWriter::new();

        let store = ConversionSessionStore::new(16);
        let worker_store = store.clone();
        let (tx, rx) = mpsc::unbounded();

        let worker = std::thread::Builder::new()
            .name("gpui-backend-tokio".into())
            .spawn(move || {
                let service = ConversionService::new(
                    worker_store.clone(),
                    &ports,
                    &client,
                    &repository,
                    &settings,
                    &saver,
                )
                .with_minecraft_log(&minecraft_log);
                run_worker_loop(runtime, &service, worker_store, rx);
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            inner: Arc::new(BackendInner {
                store,
                tx: Some(tx),
                worker: Some(worker),
            }),
        })
    }

    /// 新しい変換セッションを発行する。
    pub fn create_session(&self) -> SessionId {
        self.inner.store.create("session".into())
    }

    /// MTR エンドポイントの自動検出を常駐 tokio スレッド上で実行し、結果を `oneshot::Receiver` で返す。
    pub fn detect_mtr_endpoint(
        &self,
        session_id: SessionId,
    ) -> oneshot::Receiver<Result<Vec<EndpointDto>, BusinessError>> {
        self.dispatch(|reply| BackendCommand::DetectMtrEndpoint { session_id, reply })
    }

    /// 指定エンドポイント・ディメンションの MTR スナップショットを取得する。
    pub fn fetch_mtr_snapshot(
        &self,
        session_id: SessionId,
        endpoint: MtrEndpoint,
        dimension: u32,
    ) -> oneshot::Receiver<Result<SnapshotDto, BusinessError>> {
        self.dispatch(|reply| BackendCommand::FetchMtrSnapshot {
            session_id,
            endpoint,
            dimension,
            reply,
        })
    }

    /// 既存の `previous` と駅・路線データを比較しながら最大 30 秒間ポーリングし、変化時のみセッションを更新する。
    pub fn refresh_mtr_snapshot(
        &self,
        session_id: SessionId,
        endpoint: MtrEndpoint,
        dimension: u32,
        previous: SnapshotDto,
    ) -> oneshot::Receiver<Result<RefreshSnapshotOutcome, BusinessError>> {
        self.dispatch(|reply| BackendCommand::RefreshMtrSnapshot {
            session_id,
            endpoint,
            dimension,
            previous,
            reply,
        })
    }

    /// 指定された OuDia (.oud2) ファイルを読み込み、解析結果を返す。
    pub fn inspect_oudia(
        &self,
        session_id: SessionId,
        location: PathBuf,
    ) -> oneshot::Receiver<Result<InspectionDto, BusinessError>> {
        self.dispatch(|reply| BackendCommand::InspectOudia {
            session_id,
            location,
            reply,
        })
    }

    /// 選択された MTR 路線とフィルタ条件から OuDia 列車テンプレート候補を検索する。
    pub fn find_route_candidates(
        &self,
        session_id: SessionId,
        route_id: String,
        diagram_index: Option<usize>,
        train_type: Option<usize>,
        train_number: Option<String>,
    ) -> oneshot::Receiver<Result<Vec<RouteCandidateDto>, BusinessError>> {
        self.dispatch(|reply| BackendCommand::FindRouteCandidates {
            session_id,
            route_id,
            diagram_index,
            train_type,
            train_number,
            reply,
        })
    }

    /// 選択された路線の保存済み出庫時分状態を取得する。
    pub fn get_outbound_status(
        &self,
        session_id: SessionId,
        route_id: String,
    ) -> oneshot::Receiver<Result<OutboundStatusDto, BusinessError>> {
        self.dispatch(|reply| BackendCommand::GetOutboundStatus {
            session_id,
            route_id,
            reply,
        })
    }

    /// 試験列車の車庫発時刻と UTC オフセットから出庫時分を計測して保存する。
    pub fn measure_outbound_runtime(
        &self,
        session_id: SessionId,
        route_id: String,
        depot_clock: String,
        utc_offset: String,
    ) -> oneshot::Receiver<Result<OutboundStatusDto, BusinessError>> {
        self.dispatch(|reply| BackendCommand::MeasureOutboundRuntime {
            session_id,
            route_id,
            depot_clock,
            utc_offset,
            reply,
        })
    }

    /// 出庫時分を手動入力の秒数（小数対応）で保存する。
    pub fn save_manual_outbound(
        &self,
        session_id: SessionId,
        route_id: String,
        seconds: f64,
    ) -> oneshot::Receiver<Result<OutboundStatusDto, BusinessError>> {
        self.dispatch(|reply| BackendCommand::SaveManualOutbound {
            session_id,
            route_id,
            seconds,
            reply,
        })
    }

    /// 選択された照合候補から時刻表プレビューを生成する。
    pub fn preview_conversion(
        &self,
        session_id: SessionId,
        candidate_id: CandidateId,
        manual: Option<ManualMappingInput>,
        operation_policy: OperationPolicy,
        generate_outbound: bool,
    ) -> oneshot::Receiver<Result<PreviewDto, BusinessError>> {
        self.dispatch(|reply| BackendCommand::PreviewConversion {
            session_id,
            candidate_id,
            manual,
            operation_policy,
            generate_outbound,
            reply,
        })
    }

    /// 生成済みプレビューを指定パスへ検証付きで保存する。
    pub fn save_preview(
        &self,
        session_id: SessionId,
        preview_id: PreviewId,
        output: PathBuf,
        operation_policy: OperationPolicy,
    ) -> oneshot::Receiver<Result<SaveReceipt, BusinessError>> {
        self.dispatch(|reply| BackendCommand::SavePreview {
            session_id,
            preview_id,
            output,
            operation_policy,
            reply,
        })
    }

    /// 待受ポート列挙による MTR 自動検出に対応している環境かどうかを返す。
    pub fn auto_detection_supported() -> bool {
        cfg!(target_os = "windows")
    }

    fn dispatch<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<T>) -> BackendCommand,
    ) -> oneshot::Receiver<T> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if let Some(tx) = &self.inner.tx {
            let _ = tx.unbounded_send(build(reply_tx));
        }
        reply_rx
    }
}

struct ProbeSessionGuard {
    store: ConversionSessionStore,
    session_id: SessionId,
}

impl Drop for ProbeSessionGuard {
    fn drop(&mut self) {
        self.store.remove(&self.session_id);
    }
}

/// キャッシュ更新を待つ間は仮セッションでポーリングし、変化を検出したときのみ本セッションへ反映する。
async fn poll_snapshot_until_changed<F, Fut, S, SFut>(
    previous: &SnapshotDto,
    poll_interval: Duration,
    max_wait: Duration,
    mut fetch_probe: F,
    mut commit_session: S,
    is_canceled: impl Fn() -> bool,
) -> Result<RefreshSnapshotOutcome, BusinessError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<SnapshotDto, BusinessError>>,
    S: FnMut() -> SFut,
    SFut: Future<Output = Result<SnapshotDto, BusinessError>>,
{
    let started_at = tokio::time::Instant::now();
    loop {
        if is_canceled() {
            return Ok(RefreshSnapshotOutcome::Unchanged);
        }
        let candidate = fetch_probe().await?;
        if snapshot_routes_changed(previous, &candidate) {
            if is_canceled() {
                return Ok(RefreshSnapshotOutcome::Unchanged);
            }
            let committed = commit_session().await?;
            return Ok(RefreshSnapshotOutcome::Updated(committed));
        }
        let elapsed = started_at.elapsed();
        if elapsed >= max_wait {
            return Ok(RefreshSnapshotOutcome::Unchanged);
        }
        let wait = poll_interval.min(max_wait.saturating_sub(elapsed));
        tokio::time::sleep(wait).await;
    }
}

fn run_worker_loop(
    runtime: tokio::runtime::Runtime,
    service: &Service<'_>,
    store: ConversionSessionStore,
    mut rx: mpsc::UnboundedReceiver<BackendCommand>,
) {
    runtime.block_on(async move {
        let mut in_flight: FuturesUnordered<LocalBoxFuture<'_, ()>> = FuturesUnordered::new();
        loop {
            let next_cmd = if in_flight.is_empty() {
                rx.next().await
            } else {
                match futures::future::select(rx.next(), in_flight.next()).await {
                    Either::Left((cmd, _)) => cmd,
                    Either::Right((_, _)) => continue,
                }
            };

            let Some(cmd) = next_cmd else {
                // チャネルが閉じられたら進行中タスクを破棄して速やかにスレッドを終了する。
                break;
            };

            match cmd {
                BackendCommand::DetectMtrEndpoint { session_id, reply } => {
                    in_flight.push(
                        async move {
                            let result = service.detect_mtr_endpoint(&session_id).await;
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::FetchMtrSnapshot {
                    session_id,
                    endpoint,
                    dimension,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service
                                .fetch_mtr_snapshot(&session_id, &endpoint, dimension)
                                .await;
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::RefreshMtrSnapshot {
                    session_id,
                    endpoint,
                    dimension,
                    previous,
                    reply,
                } => {
                    let store = store.clone();
                    in_flight.push(
                        async move {
                            // 変化がない間は本セッションの下流状態（候補・プレビュー）を無効化しないよう仮セッションで探索する。
                            let probe_session = store.create("refresh-probe".into());
                            let _guard = ProbeSessionGuard {
                                store,
                                session_id: probe_session.clone(),
                            };
                            let probe_ref = &probe_session;
                            let session_ref = &session_id;
                            let endpoint_ref = &endpoint;
                            let reply_ref = &reply;
                            let result = poll_snapshot_until_changed(
                                &previous,
                                REFRESH_CACHE_POLL_INTERVAL,
                                REFRESH_CACHE_MAX_WAIT,
                                || service.fetch_mtr_snapshot(probe_ref, endpoint_ref, dimension),
                                || service.fetch_mtr_snapshot(session_ref, endpoint_ref, dimension),
                                || reply_ref.is_canceled(),
                            )
                            .await;
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::InspectOudia {
                    session_id,
                    location,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service.inspect_oudia(&session_id, &location);
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::FindRouteCandidates {
                    session_id,
                    route_id,
                    diagram_index,
                    train_type,
                    train_number,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service.find_route_candidates_by_number(
                                &session_id,
                                &route_id,
                                diagram_index,
                                train_type,
                                train_number.as_deref(),
                            );
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::GetOutboundStatus {
                    session_id,
                    route_id,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service.outbound_status(&session_id, &route_id);
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::MeasureOutboundRuntime {
                    session_id,
                    route_id,
                    depot_clock,
                    utc_offset,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service
                                .measure_and_save_outbound(
                                    &session_id,
                                    &route_id,
                                    &depot_clock,
                                    &utc_offset,
                                )
                                .await;
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::SaveManualOutbound {
                    session_id,
                    route_id,
                    seconds,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service.save_manual_outbound_decimal(
                                &session_id,
                                &route_id,
                                seconds,
                            );
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::PreviewConversion {
                    session_id,
                    candidate_id,
                    manual,
                    operation_policy,
                    generate_outbound,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service.build_preview_with_outbound(
                                &session_id,
                                Some(&candidate_id),
                                manual,
                                operation_policy,
                                generate_outbound,
                            );
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
                BackendCommand::SavePreview {
                    session_id,
                    preview_id,
                    output,
                    operation_policy,
                    reply,
                } => {
                    in_flight.push(
                        async move {
                            let result = service.save_conversion(
                                &session_id,
                                &preview_id,
                                &output,
                                operation_policy,
                            );
                            let _ = reply.send(result);
                        }
                        .boxed_local(),
                    );
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtr_oudia_application::BusinessErrorKind;

    #[test]
    fn settings_path_from_env_resolves_priority_order() {
        let from_appdata = settings_path_from_env(
            Some(OsString::from("C:\\Users\\test\\AppData\\Roaming")),
            Some(OsString::from("/xdg/config")),
            Some(OsString::from("/home/test")),
        );
        assert_eq!(
            from_appdata,
            PathBuf::from("C:\\Users\\test\\AppData\\Roaming")
                .join("MtrOudiaConverter")
                .join("settings.json")
        );

        let from_xdg = settings_path_from_env(
            None,
            Some(OsString::from("/xdg/config")),
            Some(OsString::from("/home/test")),
        );
        assert_eq!(
            from_xdg,
            PathBuf::from("/xdg/config")
                .join("MtrOudiaConverter")
                .join("settings.json")
        );

        let from_home = settings_path_from_env(
            Some(OsString::from("")),
            Some(OsString::from("")),
            Some(OsString::from("/home/test")),
        );
        assert_eq!(
            from_home,
            PathBuf::from("/home/test")
                .join(".config")
                .join("MtrOudiaConverter")
                .join("settings.json")
        );

        let fallback = settings_path_from_env(None, None, None);
        assert_eq!(
            fallback,
            PathBuf::from(".")
                .join("MtrOudiaConverter")
                .join("settings.json")
        );
    }

    #[test]
    fn minecraft_log_paths_from_env_handles_both_single_and_empty() {
        let both = minecraft_log_paths_from_env(
            Some(OsString::from("/custom/latest.log")),
            Some(OsString::from("/appdata")),
        );
        assert_eq!(
            both,
            vec![
                PathBuf::from("/custom/latest.log"),
                PathBuf::from("/appdata")
                    .join(".minecraft")
                    .join("logs")
                    .join("latest.log"),
            ]
        );

        let custom_only =
            minecraft_log_paths_from_env(Some(OsString::from("/custom/latest.log")), None);
        assert_eq!(custom_only, vec![PathBuf::from("/custom/latest.log")]);

        let appdata_only = minecraft_log_paths_from_env(None, Some(OsString::from("/appdata")));
        assert_eq!(
            appdata_only,
            vec![
                PathBuf::from("/appdata")
                    .join(".minecraft")
                    .join("logs")
                    .join("latest.log"),
            ]
        );

        let empty_filtered =
            minecraft_log_paths_from_env(Some(OsString::from("")), Some(OsString::from("")));
        assert!(empty_filtered.is_empty());
    }

    #[test]
    fn format_business_error_handles_with_and_without_detail() {
        let with_detail = BusinessError {
            kind: BusinessErrorKind::NoEndpoints,
            message: "待受ポートが見つかりません".into(),
            detail: Some("Minecraftログが見つかりません".into()),
        };
        assert_eq!(
            format_business_error(&with_detail),
            (
                "待受ポートが見つかりません".to_string(),
                "Minecraftログが見つかりません".to_string(),
            )
        );

        let without_detail = BusinessError {
            kind: BusinessErrorKind::Connection,
            message: "MTR APIへ接続できません".into(),
            detail: None,
        };
        assert_eq!(
            format_business_error(&without_detail),
            ("MTR APIへ接続できません".to_string(), String::new())
        );
    }

    fn test_snapshot(
        routes: Vec<(&str, &str, Vec<(&str, &str, i64, Option<i64>)>)>,
        api_current_time_millis: i64,
    ) -> SnapshotDto {
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
                        total_run_millis: 0,
                        total_dwell_millis: 0,
                    }
                })
                .collect(),
            dimensions: Vec::new(),
            api_current_time_millis,
        }
    }

    #[test]
    fn poll_snapshot_until_changed_commits_only_when_routes_change() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("tokio runtime");

        let previous = test_snapshot(
            vec![("r1", "山手線", vec![("東京", "1", 10_000, Some(120_000))])],
            1_000,
        );

        // 2 回目は同一路線データ（時刻のみ更新）、3 回目で路線名変化 → 更新される。
        let mut probe_calls = 0usize;
        let mut commit_calls = 0usize;
        let updated_outcome = rt
            .block_on(poll_snapshot_until_changed(
                &previous,
                Duration::from_millis(5),
                Duration::from_millis(200),
                || {
                    probe_calls += 1;
                    let snapshot = if probe_calls < 3 {
                        test_snapshot(
                            vec![("r1", "山手線", vec![("東京", "1", 10_000, Some(120_000))])],
                            1_000 + (probe_calls as i64) * 2_000,
                        )
                    } else {
                        test_snapshot(
                            vec![(
                                "r1",
                                "山手線（外回り）",
                                vec![("東京", "1", 10_000, Some(120_000))],
                            )],
                            9_000,
                        )
                    };
                    async move { Ok(snapshot) }
                },
                || {
                    commit_calls += 1;
                    let committed = test_snapshot(
                        vec![(
                            "r1",
                            "山手線（外回り）",
                            vec![("東京", "1", 10_000, Some(120_000))],
                        )],
                        9_100,
                    );
                    async move { Ok(committed) }
                },
                || false,
            ))
            .expect("polling should succeed");
        assert_eq!(probe_calls, 3);
        assert_eq!(commit_calls, 1);
        assert!(matches!(
            updated_outcome,
            RefreshSnapshotOutcome::Updated(dto) if dto.routes[0].name == "山手線（外回り）"
        ));

        // 最大待機時間まで変化なしの場合は Unchanged を返し、本セッションはコミットしない。
        let mut unchanged_probes = 0usize;
        let mut unchanged_commits = 0usize;
        let unchanged_outcome = rt
            .block_on(poll_snapshot_until_changed(
                &previous,
                Duration::from_millis(10),
                Duration::from_millis(35),
                || {
                    unchanged_probes += 1;
                    let snapshot = test_snapshot(
                        vec![("r1", "山手線", vec![("東京", "1", 10_000, Some(120_000))])],
                        2_000 + (unchanged_probes as i64) * 1_000,
                    );
                    async move { Ok(snapshot) }
                },
                || {
                    unchanged_commits += 1;
                    async move { unreachable!("commit must not run when unchanged") }
                },
                || false,
            ))
            .expect("unchanged polling should succeed");
        assert!(unchanged_probes >= 2);
        assert_eq!(unchanged_commits, 0);
        assert!(matches!(
            unchanged_outcome,
            RefreshSnapshotOutcome::Unchanged
        ));
    }
}
