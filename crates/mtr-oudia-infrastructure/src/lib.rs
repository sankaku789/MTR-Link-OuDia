//! Application の Port 実装を置く Infrastructure 層。

use futures_util::StreamExt;
use mtr_oudia_application::DomainPort;
#[cfg(not(target_os = "windows"))]
use mtr_oudia_application::ListeningPortProvider;
use mtr_oudia_application::{
    ApplicationError, BusinessError, BusinessErrorKind, MinecraftLogProvider, MinecraftLogRead,
    MtrApiClient, MtrEndpoint, MtrSnapshotResponse, OudiaRepository, SaveReceipt,
    SettingsRepository, SettingsSnapshot, ValidatedSavePort, async_trait,
};
use mtr_oudia_domain::{
    DomainLayer, MtrNetworkSnapshot, MtrRouteSnapshot, MtrStopSnapshot, ServiceTimeMillis,
};
use serde_json::{Map, Value};
use sha2::Digest;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod safe_save;
pub use safe_save::*;

/// 設定専用の小さな JSON adapter。壊れた設定は既定値と診断へ退避する。
pub struct JsonSettingsRepository {
    path: std::path::PathBuf,
}
impl JsonSettingsRepository {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }
}
impl SettingsRepository for JsonSettingsRepository {
    fn load(&self) -> Result<SettingsSnapshot, BusinessError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SettingsSnapshot::default());
            }
            Err(e) => return Err(io_business_error(e)),
        };
        match serde_json::from_slice(&bytes) {
            Ok(v) => Ok(v),
            Err(_) => Ok(SettingsSnapshot {
                diagnostics: vec!["設定ファイルが不正なため既定値を使用します".into()],
                ..SettingsSnapshot::default()
            }),
        }
    }
    fn save_last_successful_endpoint(&self, endpoint: &MtrEndpoint) -> Result<(), BusinessError> {
        let mut settings = self.load()?;
        settings.last_endpoint = Some(endpoint.as_url().to_string());
        let bytes = serde_json::to_vec_pretty(&settings).map_err(|_| BusinessError {
            kind: BusinessErrorKind::Internal,
            message: "設定を保存できません".into(),
            detail: None,
        })?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(io_business_error)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes).map_err(io_business_error)?;
        std::fs::rename(temporary, &self.path).map_err(io_business_error)
    }
}

/// filesystem の OuDia 原本を Application の読込 Port へ接続する adapter。
pub struct FileOudiaRepository;
impl OudiaRepository for FileOudiaRepository {
    fn read(
        &self,
        location: &std::path::Path,
    ) -> Result<mtr_oudia_domain::OudiaSource, BusinessError> {
        let bytes = std::fs::read(location).map_err(io_business_error)?;
        mtr_oudia_domain::parse_oudia(bytes).map_err(|_| BusinessError {
            kind: BusinessErrorKind::ParseUnsupported,
            message: "OuDia を解析できません".into(),
            detail: None,
        })
    }
}
impl ValidatedSavePort for SafeOudiaWriter {
    fn save(
        &self,
        input: &std::path::Path,
        output: &std::path::Path,
        expected_hash: [u8; 32],
        patch: &mtr_oudia_domain::OudiaPatch,
    ) -> Result<SaveReceipt, BusinessError> {
        SafeOudiaWriter::save(self, input, output, expected_hash, patch)
            .map_err(safe_save_business_error)?;
        let bytes = std::fs::read(output).map_err(io_business_error)?;
        Ok(SaveReceipt {
            output_path: output.display().to_string(),
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", sha2::Sha256::digest(bytes)),
        })
    }
}
fn io_business_error(error: std::io::Error) -> BusinessError {
    BusinessError {
        kind: if error.kind() == std::io::ErrorKind::PermissionDenied {
            BusinessErrorKind::Permission
        } else {
            BusinessErrorKind::Io
        },
        message: "ファイル操作に失敗しました".into(),
        detail: None,
    }
}
fn safe_save_business_error(error: SafeSaveError) -> BusinessError {
    let kind = match error {
        SafeSaveError::InputChanged => BusinessErrorKind::SourceChanged,
        SafeSaveError::OutputAlreadyExists
        | SafeSaveError::SamePath
        | SafeSaveError::FinalizeRace => BusinessErrorKind::OutputExists,
        SafeSaveError::Patch(_) | SafeSaveError::Reparse(_) => BusinessErrorKind::SaveVerification,
        SafeSaveError::Io(_) => BusinessErrorKind::Io,
    };
    BusinessError {
        kind,
        message: "安全な保存に失敗しました".into(),
        detail: None,
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_500);
const EXTENDED_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[cfg(target_os = "windows")]
mod windows;

/// Windows IP Helper API で待受ポートを得る Provider。
pub struct WindowsListeningPortProvider;

/// Minecraftログ候補を順に確認するadapter。カスタムランチャーのパスを先頭へ追加できる。
pub struct FileMinecraftLogProvider {
    paths: Vec<PathBuf>,
    include_running_minecraft: bool,
}

impl FileMinecraftLogProvider {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            paths: paths.into_iter().collect(),
            include_running_minecraft: false,
        }
    }

    /// Windowsで実行中Minecraftの--gameDirを、固定候補より先に確認する。
    pub fn with_running_minecraft(mut self) -> Self {
        self.include_running_minecraft = true;
        self
    }
}

impl MinecraftLogProvider for FileMinecraftLogProvider {
    fn read_latest_log(&self) -> MinecraftLogRead {
        let mut paths = if self.include_running_minecraft {
            running_minecraft_log_paths()
        } else {
            Vec::new()
        };
        paths.extend(self.paths.iter().cloned());
        for path in paths {
            match std::fs::read_to_string(path) {
                Ok(contents) => return MinecraftLogRead::Contents(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return MinecraftLogRead::ReadFailed,
            }
        }
        MinecraftLogRead::NotFound
    }
}

/// Minecraftの起動引数から--gameDirを抽出する。値を別引数にする形式と`=`形式に対応する。
pub fn game_dir_from_command_line(arguments: &[std::ffi::OsString]) -> Option<PathBuf> {
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--gameDir" {
            return arguments
                .next()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from);
        }
        let argument = argument.to_string_lossy();
        if let Some(path) = argument.strip_prefix("--gameDir=")
            && !path.is_empty()
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn running_minecraft_log_paths() -> Vec<PathBuf> {
    let system = sysinfo::System::new_all();
    let mut paths = system
        .processes()
        .values()
        .filter(|process| {
            let name = process.name().to_string_lossy().to_ascii_lowercase();
            matches!(name.strip_suffix(".exe").unwrap_or(&name), "java" | "javaw")
        })
        .filter_map(|process| game_dir_from_command_line(process.cmd()))
        .map(|game_dir| game_dir.join("logs").join("latest.log"))
        .filter_map(|path| {
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| right.0.cmp(&left.0));
    paths.dedup_by(|left, right| left.1 == right.1);
    paths.into_iter().map(|(_, path)| path).collect()
}

#[cfg(not(target_os = "windows"))]
fn running_minecraft_log_paths() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(not(target_os = "windows"))]
impl ListeningPortProvider for WindowsListeningPortProvider {
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError> {
        Err(ApplicationError::ListeningPortProviderUnsupported)
    }
}

/// P01 の Application Port 実装。
pub struct StaticDomainPort;

impl DomainPort for StaticDomainPort {
    fn domain_layer(&self) -> DomainLayer {
        DomainLayer
    }
}

/// reqwest を用いる MTR localhost API client。
pub struct ReqwestMtrApiClient {
    fast_client: reqwest::Client,
    extended_client: reqwest::Client,
}

impl ReqwestMtrApiClient {
    /// redirect を拒否し、MTR API 用の通信上限を設定した client を作る。
    pub fn new() -> Result<Self, ApplicationError> {
        Ok(Self {
            fast_client: build_client(RESPONSE_TIMEOUT)?,
            extended_client: build_client(EXTENDED_RESPONSE_TIMEOUT)?,
        })
    }
}

fn build_client(response_timeout: Duration) -> Result<reqwest::Client, ApplicationError> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(response_timeout)
        .redirect(reqwest::redirect::Policy::none())
        // localhost API を環境・OSの HTTP proxy に転送しない。
        .no_proxy()
        .build()
        .map_err(|error| ApplicationError::Transport {
            message: error.to_string(),
        })
}

#[async_trait]
impl MtrApiClient for ReqwestMtrApiClient {
    async fn probe_endpoint(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<(), ApplicationError> {
        probe_endpoint(&self.fast_client, endpoint, dimension).await
    }

    async fn probe_endpoint_extended(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<(), ApplicationError> {
        probe_endpoint(&self.extended_client, endpoint, dimension).await
    }

    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        fetch_snapshot(&self.fast_client, endpoint, dimension).await
    }

    async fn fetch_snapshot_extended(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        fetch_snapshot(&self.extended_client, endpoint, dimension).await
    }
}

async fn probe_endpoint(
    client: &reqwest::Client,
    endpoint: &MtrEndpoint,
    dimension: u32,
) -> Result<(), ApplicationError> {
    let (_, body) = fetch_response(client, endpoint, dimension).await?;
    parse_mtr_probe(&body)
}

async fn fetch_snapshot(
    client: &reqwest::Client,
    endpoint: &MtrEndpoint,
    dimension: u32,
) -> Result<MtrSnapshotResponse, ApplicationError> {
    let (status, body) = fetch_response(client, endpoint, dimension).await?;
    if !status {
        return Err(ApplicationError::InvalidResponse {
            reason: "HTTP status was not successful".to_owned(),
        });
    }
    parse_mtr_response(&body, endpoint, dimension, current_unix_millis()?)
}

async fn fetch_response(
    client: &reqwest::Client,
    endpoint: &MtrEndpoint,
    dimension: u32,
) -> Result<(bool, String), ApplicationError> {
    let response = client
        .get(endpoint.stations_and_routes_url(dimension))
        .send()
        .await
        .map_err(map_reqwest_error)?;
    let status = response.status().is_success();
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ApplicationError::ResponseTooLarge);
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_reqwest_error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(ApplicationError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(bytes).map_err(|_| ApplicationError::InvalidResponse {
        reason: "UTF-8 JSON ではありません".to_owned(),
    })?;
    Ok((status, body))
}

/// 接続先探索ではMTR共通包絡だけを確認し、路線の厳密な正規化は行わない。
pub fn parse_mtr_probe(body: &str) -> Result<(), ApplicationError> {
    let value: Value =
        serde_json::from_str(body).map_err(|error| invalid_response(error.to_string()))?;
    let envelope = object(&value, "response")?;
    if !required(envelope, "status")?.is_number() {
        return Err(invalid_response("status must be a number"));
    }
    let data = object(required(envelope, "data")?, "data")?;
    array(required(data, "stations")?, "data.stations")?;
    array(required(data, "routes")?, "data.routes")?;
    Ok(())
}

/// JSON fixture と HTTP 応答を同じ規則で正規化する。
pub fn parse_mtr_response(
    body: &str,
    endpoint: &MtrEndpoint,
    dimension: u32,
    retrieved_at_unix_millis: i64,
) -> Result<MtrSnapshotResponse, ApplicationError> {
    let value: Value =
        serde_json::from_str(body).map_err(|error| invalid_response(error.to_string()))?;
    let envelope = object(&value, "response")?;
    if integer(required(envelope, "status")?, "status")? != 200 {
        return Err(invalid_response("status must be 200"));
    }
    let current_time = integer(required(envelope, "currentTime")?, "currentTime")?;
    let data = object(required(envelope, "data")?, "data")?;
    let stations = array(required(data, "stations")?, "data.stations")?;
    let routes = array(required(data, "routes")?, "data.routes")?;
    let dimensions = array(required(data, "dimensions")?, "data.dimensions")?.clone();

    let mut station_names = HashMap::new();
    for station in stations {
        let station = object(station, "station")?;
        let id = string(required(station, "id")?, "station.id")?;
        let name = string(required(station, "name")?, "station.name")?;
        if station_names
            .insert(id.to_owned(), name.to_owned())
            .is_some()
        {
            return Err(invalid_response("duplicate station id"));
        }
    }

    let mut normalized_routes = Vec::with_capacity(routes.len());
    let mut route_ids = std::collections::HashSet::new();
    for route in routes {
        let route = object(route, "route")?;
        let route_id = string(required(route, "id")?, "route.id")?;
        if !route_ids.insert(route_id) {
            return Err(invalid_response("duplicate route id"));
        }
        let route_name = string(required(route, "name")?, "route.name")?;
        let route_stations = array(required(route, "stations")?, "route.stations")?;
        let durations = array(required(route, "durations")?, "route.durations")?;
        if route_stations.len() < 2 || durations.len() != route_stations.len() - 1 {
            return Err(invalid_response(
                "durations length must equal stations length minus one",
            ));
        }
        let mut stops = Vec::with_capacity(route_stations.len());
        for (index, route_station) in route_stations.iter().enumerate() {
            let route_station = object(route_station, "route.station")?;
            let station_id = string(required(route_station, "id")?, "route.station.id")?;
            let platform_name = string(required(route_station, "name")?, "route.station.name")?;
            let dwell = ServiceTimeMillis::new(integer(
                required(route_station, "dwellTime")?,
                "route.station.dwellTime",
            )?)
            .map_err(|_| invalid_response("dwellTime must not be negative"))?;
            let station_name = station_names
                .get(station_id)
                .ok_or_else(|| invalid_response("route references an unknown station id"))?;
            let run = durations
                .get(index)
                .map(|duration| {
                    ServiceTimeMillis::new(integer(duration, "route.duration")?)
                        .map_err(|_| invalid_response("duration must not be negative"))
                })
                .transpose()?;
            stops.push(
                MtrStopSnapshot::new(station_id, station_name, platform_name, dwell, run)
                    .map_err(|error| invalid_response(error.to_string()))?,
            );
        }
        let signature = stops.iter().map(|stop| stop.station_id.clone()).collect();
        normalized_routes.push(
            MtrRouteSnapshot::new(route_id, route_name, signature, stops)
                .map_err(|error| invalid_response(error.to_string()))?,
        );
    }
    let snapshot = MtrNetworkSnapshot::new(
        endpoint.as_url().as_str(),
        dimension,
        current_time,
        retrieved_at_unix_millis,
        normalized_routes,
    )
    .map_err(|error| invalid_response(error.to_string()))?;
    Ok(MtrSnapshotResponse {
        snapshot,
        available_dimensions: dimensions,
    })
}

fn current_unix_millis() -> Result<i64, ApplicationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid_response("system clock is before Unix epoch"))
        .and_then(|duration| {
            i64::try_from(duration.as_millis())
                .map_err(|_| invalid_response("system clock is out of range"))
        })
}

fn map_reqwest_error(error: reqwest::Error) -> ApplicationError {
    if error.is_timeout() {
        ApplicationError::Timeout
    } else {
        ApplicationError::Transport {
            message: error.to_string(),
        }
    }
}
fn invalid_response(reason: impl Into<String>) -> ApplicationError {
    ApplicationError::InvalidResponse {
        reason: reason.into(),
    }
}
fn required<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Value, ApplicationError> {
    object
        .get(key)
        .ok_or_else(|| invalid_response(format!("missing {key}")))
}
fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, ApplicationError> {
    value
        .as_object()
        .ok_or_else(|| invalid_response(format!("{name} must be an object")))
}
fn array<'a>(value: &'a Value, name: &str) -> Result<&'a Vec<Value>, ApplicationError> {
    value
        .as_array()
        .ok_or_else(|| invalid_response(format!("{name} must be an array")))
}
fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str, ApplicationError> {
    value
        .as_str()
        .ok_or_else(|| invalid_response(format!("{name} must be a string")))
}
fn integer(value: &Value, name: &str) -> Result<i64, ApplicationError> {
    value
        .as_i64()
        .ok_or_else(|| invalid_response(format!("{name} must be an integer")))
}
