//! GPUI フロントエンドから `ConversionService` を安全に呼び出すバックエンドブリッジ層。
//!
//! `reqwest` ベースの通信が tokio リアクタを必要とするため、専用の常駐 current-thread
//! tokio スレッドで `ConversionService` を保持・実行し、結果を `oneshot::Receiver` で返す。

use std::{ffi::OsString, path::PathBuf, sync::Arc, thread::JoinHandle};

use futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
    future::{Either, LocalBoxFuture},
    stream::FuturesUnordered,
};
use mtr_oudia_application::{
    BusinessError, ConversionService, ConversionSessionStore, EndpointDto, InspectionDto,
    MtrEndpoint, RouteCandidateDto, SessionId, SnapshotDto,
};
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
                    worker_store,
                    &ports,
                    &client,
                    &repository,
                    &settings,
                    &saver,
                )
                .with_minecraft_log(&minecraft_log);
                run_worker_loop(runtime, &service, rx);
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

fn run_worker_loop(
    runtime: tokio::runtime::Runtime,
    service: &Service<'_>,
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
}
