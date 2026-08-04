//! Domain のユースケース境界と Port を置く Application 層。

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use futures_util::{StreamExt, stream};
use mtr_oudia_domain::{
    GeneratedTimetable, KijunDiaIndex, MtrNetworkSnapshot, MtrRouteSnapshot, OperationPolicy,
    OudiaDirection, OudiaPatch, OudiaRouteTemplate, OudiaSource, ReferenceDiagramSelection,
    RouteMatchCandidate, RouteMatchOutcome, build_eki_jikoku_patch_with_groups,
    build_oudia_route_templates, generate_timetable, match_mtr_route,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{Host, Url};

pub use async_trait::async_trait;

/// P01 から維持する依存方向確認用の最小 Port。
pub trait DomainPort {
    fn domain_layer(&self) -> mtr_oudia_domain::DomainLayer;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationError {
    InvalidEndpoint { reason: &'static str },
    Timeout,
    Transport { message: String },
    ResponseTooLarge,
    InvalidResponse { reason: String },
    ListeningPortEnumeration { message: String },
    ListeningPortProviderUnsupported,
}
impl std::fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MTR API の処理に失敗しました")
    }
}
impl std::error::Error for ApplicationError {}

/// localhost 上の MTR API のベース URL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtrEndpoint(Url);
impl MtrEndpoint {
    pub fn parse(input: &str) -> Result<Self, ApplicationError> {
        let mut url = Url::parse(input).map_err(|_| ApplicationError::InvalidEndpoint {
            reason: "URL として解析できません",
        })?;
        if url.scheme() != "http"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || (!url.path().is_empty() && url.path() != "/")
            || url.port_or_known_default().is_none()
        {
            return Err(ApplicationError::InvalidEndpoint {
                reason: "許可されない URL です",
            });
        }
        match url.host() {
            Some(Host::Ipv4(a)) if a.octets()[0] == 127 => {}
            Some(Host::Ipv6(a)) if a.is_loopback() => {}
            _ => {
                return Err(ApplicationError::InvalidEndpoint {
                    reason: "literal loopback address のみ許可されます",
                });
            }
        }
        url.set_path("/");
        Ok(Self(url))
    }
    pub fn stations_and_routes_url(&self, dimension: u32) -> Url {
        let mut u = self.0.clone();
        u.set_path("/mtr/api/map/stations-and-routes");
        u.set_query(Some(&format!("dimension={dimension}")));
        u
    }
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MtrSnapshotResponse {
    pub snapshot: MtrNetworkSnapshot,
    pub available_dimensions: Vec<serde_json::Value>,
}
#[async_trait]
pub trait MtrApiClient: Send + Sync {
    /// MTR API の共通包絡だけを確認する接続先探索用 probe。
    async fn probe_endpoint(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<(), ApplicationError> {
        self.fetch_snapshot(endpoint, dimension).await.map(|_| ())
    }

    /// 通常探索で timeout した localhost API だけに使う長時間 probe。
    async fn probe_endpoint_extended(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<(), ApplicationError> {
        self.fetch_snapshot_extended(endpoint, dimension)
            .await
            .map(|_| ())
    }

    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError>;

    /// 通常探索で timeout した localhost API だけに使う長時間取得。
    async fn fetch_snapshot_extended(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        self.fetch_snapshot(endpoint, dimension).await
    }
}
pub trait ListeningPortProvider: Send + Sync {
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinecraftLogRead {
    NotFound,
    ReadFailed,
    Contents(String),
}

pub trait MinecraftLogProvider: Send + Sync {
    fn read_latest_log(&self) -> MinecraftLogRead;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinecraftLogStatus {
    NotConfigured,
    NotFound,
    NoEndpoint,
    InvalidEndpoint,
    ReadFailed,
    CandidateRejected,
}

/// Minecraftログの対象メッセージから、手動入力では許可しないlocalhostをliteral loopbackへ変換する。
pub fn endpoint_from_minecraft_log(log: &str) -> Result<Option<MtrEndpoint>, ApplicationError> {
    const MARKER: &str = "Open the Transport System Map at ";
    let mut latest = None;
    let mut matched = false;
    for line in log.lines() {
        let Some((_, suffix)) = line.split_once(MARKER) else {
            continue;
        };
        matched = true;
        let Some(candidate) = suffix.split_whitespace().next() else {
            continue;
        };
        let Ok(url) = Url::parse(candidate) else {
            continue;
        };
        if url.scheme() != "http"
            || url.host_str() != Some("localhost")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || (!url.path().is_empty() && url.path() != "/")
        {
            continue;
        }
        let Some(port) = url.port_or_known_default() else {
            continue;
        };
        latest = Some(MtrEndpoint::parse(&format!("http://127.0.0.1:{port}/"))?);
    }
    if matched && latest.is_none() {
        return Err(ApplicationError::InvalidEndpoint {
            reason: "MinecraftログのMTR API URLが不正です",
        });
    }
    Ok(latest)
}
pub trait OudiaRepository: Send + Sync {
    fn read(&self, location: &Path) -> Result<OudiaSource, BusinessError>;
}
pub trait ValidatedSavePort: Send + Sync {
    fn save(
        &self,
        input: &Path,
        output: &Path,
        expected_hash: [u8; 32],
        patch: &OudiaPatch,
    ) -> Result<SaveReceipt, BusinessError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveReceipt {
    pub output_path: String,
    pub bytes: u64,
    pub sha256: String,
}
pub trait SettingsRepository: Send + Sync {
    fn load(&self) -> Result<SettingsSnapshot, BusinessError>;
    fn save_last_successful_endpoint(&self, endpoint: &MtrEndpoint) -> Result<(), BusinessError>;
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub last_endpoint: Option<String>,
    pub station_aliases: BTreeMap<String, String>,
    pub route_mappings: Vec<RouteMappingSetting>,
    pub last_train_type: Option<usize>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMappingSetting {
    pub mtr_route_id: String,
    pub mtr_signature: Vec<String>,
    pub oudia_signature: Vec<String>,
    pub version: u32,
    pub station_slots: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessErrorKind {
    Validation,
    Connection,
    NoEndpoints,
    MultipleEndpoints,
    ParseUnsupported,
    SelectionRequired,
    StaleState,
    SourceChanged,
    OutputExists,
    SaveVerification,
    Permission,
    Io,
    Internal,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessError {
    pub kind: BusinessErrorKind,
    pub message: String,
    pub detail: Option<String>,
}
impl BusinessError {
    fn new(kind: BusinessErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.into(),
            detail: None,
        }
    }
}

/// 不透明なセッション、候補、プレビュー識別子。値を推測しても所有セッション照合を通過できない。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CandidateId(pub String);
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PreviewId(pub String);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDto {
    pub url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDto {
    pub routes: Vec<RouteDto>,
    pub dimensions: Vec<serde_json::Value>,
    pub api_current_time_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDto {
    pub id: String,
    pub name: String,
    pub stations: Vec<RouteStationDto>,
    pub station_count: usize,
    pub total_run_millis: i64,
    pub total_dwell_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStationDto {
    pub station_name: String,
    pub platform_name: String,
    pub dwell_millis: i64,
    pub run_millis_to_next: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionDto {
    pub file_type: String,
    pub line_name: Option<String>,
    pub station_count: usize,
    pub kijun_status: String,
    pub diagrams: Vec<DiagramDto>,
    pub train_types: Vec<usize>,
    pub train_type_names: Vec<String>,
    pub templates: Vec<TemplateDto>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramDto {
    pub index: usize,
    pub train_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateDto {
    pub diagram_index: usize,
    pub direction: String,
    pub train_index: usize,
    pub train_type_index: Option<usize>,
    pub active_station_slots: Vec<StationSlotDto>,
    pub route_station_slots: Vec<StationSlotDto>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationSlotDto {
    pub index: usize,
    pub name: String,
    pub handling_code: Option<u8>,
    pub previous_name: Option<String>,
    pub next_name: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCandidateDto {
    pub id: CandidateId,
    pub diagram_index: usize,
    pub train_index: usize,
    pub direction: String,
    pub station_mappings: Vec<StationMappingDto>,
    pub rank: String,
    pub reasons: Vec<String>,
    pub auto_selected: bool,
    pub manual_only: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationMappingDto {
    pub mtr_station_index: usize,
    pub oudia_station_slot: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewDto {
    pub id: PreviewId,
    pub fixed_base_time: String,
    pub stops: Vec<PreviewStopDto>,
    pub warnings: Vec<String>,
    pub crosses_midnight: bool,
    pub operation_present: bool,
    pub policy_choices: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewStopDto {
    pub station: String,
    pub existing_arrival: Option<String>,
    pub existing_departure: Option<String>,
    pub raw_arrival_millis: Option<i64>,
    pub raw_departure_millis: Option<i64>,
    pub rounded_arrival: Option<String>,
    pub rounded_departure: Option<String>,
    pub run_millis: Option<i64>,
    pub dwell_millis: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualMappingInput {
    pub station_mappings: Vec<StationMappingDto>,
}

#[derive(Clone)]
pub struct ConversionSessionStore {
    inner: Arc<Mutex<Store>>,
    max: usize,
    next: Arc<AtomicU64>,
}
struct Store {
    values: HashMap<SessionId, Session>,
    order: VecDeque<SessionId>,
}
#[derive(Clone)]
struct Session {
    revision: u64,
    endpoint: Option<MtrEndpoint>,
    snapshot: Option<MtrSnapshotResponse>,
    source: Option<(PathBuf, [u8; 32], OudiaSource)>,
    selected_route: Option<String>,
    candidates: HashMap<CandidateId, Candidate>,
    previews: HashMap<PreviewId, Preview>,
}
#[derive(Clone)]
struct Candidate {
    revision: u64,
    template: OudiaRouteTemplate,
    mapping: Vec<StationMappingDto>,
}
#[derive(Clone)]
struct Preview {
    revision: u64,
    template: OudiaRouteTemplate,
    timetable: GeneratedTimetable,
    station_slot_groups: Vec<Vec<usize>>,
}
impl ConversionSessionStore {
    pub fn new(max: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store {
                values: HashMap::new(),
                order: VecDeque::new(),
            })),
            max: max.max(1),
            next: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn create(&self, prefix: String) -> SessionId {
        let id = SessionId(format!(
            "{prefix}-{}",
            self.next.fetch_add(1, Ordering::Relaxed)
        ));
        let mut s = self.inner.lock().expect("session store poisoned");
        s.values.insert(
            id.clone(),
            Session {
                revision: 0,
                endpoint: None,
                snapshot: None,
                source: None,
                selected_route: None,
                candidates: HashMap::new(),
                previews: HashMap::new(),
            },
        );
        s.order.push_back(id.clone());
        while s.values.len() > self.max {
            if let Some(old) = s.order.pop_front() {
                s.values.remove(&old);
            }
        }
        id
    }
    pub fn remove(&self, id: &SessionId) -> bool {
        let mut s = self.inner.lock().expect("session store poisoned");
        s.order.retain(|v| v != id);
        s.values.remove(id).is_some()
    }
    /// 現在の revision を診断・テスト向けに返す。内容はセッション外へ露出しない。
    pub fn require(&self, id: &SessionId) -> Result<ConversionSession, BusinessError> {
        self.with(id, |session| {
            Ok(ConversionSession {
                revision: session.revision,
            })
        })
    }
    fn with<R>(
        &self,
        id: &SessionId,
        f: impl FnOnce(&Session) -> Result<R, BusinessError>,
    ) -> Result<R, BusinessError> {
        let s = self.inner.lock().expect("session store poisoned");
        f(s.values.get(id).ok_or_else(|| {
            BusinessError::new(BusinessErrorKind::StaleState, "セッションが見つかりません")
        })?)
    }
    fn update<R>(
        &self,
        id: &SessionId,
        f: impl FnOnce(&mut Session) -> Result<R, BusinessError>,
    ) -> Result<R, BusinessError> {
        let mut s = self.inner.lock().expect("session store poisoned");
        f(s.values.get_mut(id).ok_or_else(|| {
            BusinessError::new(BusinessErrorKind::StaleState, "セッションが見つかりません")
        })?)
    }
}

/// セッションの公開可能な最小状態。候補・原本・パッチは保持しない。
#[derive(Debug, Clone)]
pub struct ConversionSession {
    pub revision: u64,
}
fn invalidate(session: &mut Session) {
    session.revision += 1;
    session.candidates.clear();
    session.previews.clear();
}

/// P09 の最小縦切り。セッションごとに上流入力と下流生成物を一貫して保持する。
pub struct ConversionService<'a, P: ?Sized, C: ?Sized, R: ?Sized, S: ?Sized, V: ?Sized> {
    store: ConversionSessionStore,
    ports: &'a P,
    client: &'a C,
    repository: &'a R,
    settings: &'a S,
    saver: &'a V,
    minecraft_log: Option<&'a dyn MinecraftLogProvider>,
}
impl<
    'a,
    P: ListeningPortProvider + ?Sized,
    C: MtrApiClient + ?Sized,
    R: OudiaRepository + ?Sized,
    S: SettingsRepository + ?Sized,
    V: ValidatedSavePort + ?Sized,
> ConversionService<'a, P, C, R, S, V>
{
    pub fn new(
        store: ConversionSessionStore,
        ports: &'a P,
        client: &'a C,
        repository: &'a R,
        settings: &'a S,
        saver: &'a V,
    ) -> Self {
        Self {
            store,
            ports,
            client,
            repository,
            settings,
            saver,
            minecraft_log: None,
        }
    }
    pub fn with_minecraft_log(mut self, provider: &'a dyn MinecraftLogProvider) -> Self {
        self.minecraft_log = Some(provider);
        self
    }
    pub fn create_session(&self) -> SessionId {
        self.store.create("session".into())
    }
    pub fn remove_session(&self, id: &SessionId) -> bool {
        self.store.remove(id)
    }
    pub async fn detect_mtr_endpoint(
        &self,
        id: &SessionId,
    ) -> Result<Vec<EndpointDto>, BusinessError> {
        let previous = self
            .settings
            .load()?
            .last_endpoint
            .and_then(|v| MtrEndpoint::parse(&v).ok());
        let mut discovery = MtrEndpointDiscoveryService::new(self.ports, self.client);
        if let Some(provider) = self.minecraft_log {
            discovery = discovery.with_minecraft_log(provider);
        }
        let result = discovery.detect(previous).await;
        let endpoints = match result {
            MtrEndpointDiscovery::Single(e) => vec![e],
            MtrEndpointDiscovery::Multiple(v) => v,
            MtrEndpointDiscovery::NoListeningPorts { log_status } => {
                return Err(discovery_error(
                    "待受ポートが見つかりません",
                    log_status,
                    None,
                ));
            }
            MtrEndpointDiscovery::NoValidEndpoints {
                attempted,
                unreachable,
                invalid_responses,
                timeouts,
                log_status,
            } => {
                let message = if invalid_responses > 0 && unreachable == 0 {
                    "MTR APIの応答形式が一致しません"
                } else if timeouts > 0 && unreachable == timeouts {
                    "MTR APIの応答がタイムアウトしました"
                } else {
                    "MTR候補へ接続できません"
                };
                return Err(discovery_error(
                    message,
                    log_status,
                    Some(format!(
                        "試行 {attempted} 件、到達不能 {unreachable} 件（タイムアウト {timeouts} 件）、応答不正 {invalid_responses} 件"
                    )),
                ));
            }
            MtrEndpointDiscovery::PortEnumerationFailed { log_status, .. } => {
                return Err(discovery_error(
                    "待受ポートを取得できません",
                    log_status,
                    None,
                ));
            }
        };
        if endpoints.len() == 1 {
            self.settings.save_last_successful_endpoint(&endpoints[0])?;
        }
        self.store.update(id, |s| {
            invalidate(s);
            s.endpoint = endpoints.first().cloned();
            Ok(())
        })?;
        Ok(endpoints
            .into_iter()
            .map(|e| EndpointDto {
                url: e.as_url().to_string(),
            })
            .collect())
    }
    pub async fn fetch_mtr_snapshot(
        &self,
        id: &SessionId,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<SnapshotDto, BusinessError> {
        let response = self
            .client
            .fetch_snapshot(endpoint, dimension)
            .await
            .map_err(map_application_error)?;
        self.settings.save_last_successful_endpoint(endpoint)?;
        let dto = snapshot_dto(&response)?;
        self.store.update(id, |s| {
            invalidate(s);
            s.endpoint = Some(endpoint.clone());
            s.snapshot = Some(response);
            s.selected_route = None;
            Ok(())
        })?;
        Ok(dto)
    }
    pub fn inspect_oudia(
        &self,
        id: &SessionId,
        location: &Path,
    ) -> Result<InspectionDto, BusinessError> {
        let source = self.repository.read(location)?;
        let inspection = inspection_dto(&source);
        let hash: [u8; 32] = Sha256::digest(&source.bytes).into();
        self.store.update(id, |s| {
            invalidate(s);
            s.source = Some((location.to_path_buf(), hash, source));
            Ok(())
        })?;
        Ok(inspection)
    }
    pub fn find_route_candidates(
        &self,
        id: &SessionId,
        route_id: &str,
        diagram_index: Option<usize>,
        train_type: Option<usize>,
    ) -> Result<Vec<RouteCandidateDto>, BusinessError> {
        let aliases = self.settings.load()?.station_aliases;
        self.store.update(id, |s| {
            let snapshot = s.snapshot.as_ref().ok_or_else(|| {
                BusinessError::new(
                    BusinessErrorKind::SelectionRequired,
                    "MTR スナップショットを取得してください",
                )
            })?;
            let route = snapshot
                .snapshot
                .routes
                .iter()
                .find(|v| v.route_id == route_id)
                .ok_or_else(|| {
                    BusinessError::new(BusinessErrorKind::Validation, "路線が見つかりません")
                })?;
            let source = s.source.as_ref().ok_or_else(|| {
                BusinessError::new(
                    BusinessErrorKind::SelectionRequired,
                    "OuDia を読み込んでください",
                )
            })?;
            let mut templates = templates_for(&source.2, diagram_index)?;
            if let Some(train_type) = train_type {
                templates.retain(|template| template.train_type_index == Some(train_type));
            }
            let outcome = match_mtr_route(
                &route
                    .stops
                    .iter()
                    .map(|v| v.station_name.clone())
                    .collect::<Vec<_>>(),
                &templates,
                &aliases,
                train_type,
            );
            invalidate(s);
            s.selected_route = Some(route_id.into());
            let candidates = match outcome {
                RouteMatchOutcome::NoCandidate { .. } => Vec::new(),
                RouteMatchOutcome::Automatic { candidate } => vec![(candidate, true)],
                RouteMatchOutcome::Manual { candidates, .. } => {
                    candidates.into_iter().map(|v| (v, false)).collect()
                }
            };
            let mut result = Vec::new();
            for (candidate, auto_selected) in candidates {
                let template = templates
                    .iter()
                    .find(|t| {
                        t.diagram_index == candidate.id.diagram_index
                            && t.direction == candidate.id.direction
                            && t.train_index == candidate.id.train_index
                    })
                    .expect("candidate template")
                    .clone();
                let mapping: Vec<StationMappingDto> = candidate
                    .station_mapping
                    .iter()
                    .map(|v| StationMappingDto {
                        mtr_station_index: v.mtr_station_index,
                        oudia_station_slot: v.oudia_slot_index,
                    })
                    .collect();
                let cid = CandidateId(format!("candidate-{}-{}", s.revision, s.candidates.len()));
                s.candidates.insert(
                    cid.clone(),
                    Candidate {
                        revision: s.revision,
                        template,
                        mapping: mapping.clone(),
                    },
                );
                result.push(candidate_dto(cid, candidate, mapping, auto_selected));
            }
            if result.is_empty() {
                // 自動照合が不成立でも、既存列車を指定して手動駅対応を安全に確定できる。
                for template in templates {
                    let cid =
                        CandidateId(format!("candidate-{}-{}", s.revision, s.candidates.len()));
                    s.candidates.insert(
                        cid.clone(),
                        Candidate {
                            revision: s.revision,
                            template: template.clone(),
                            mapping: Vec::new(),
                        },
                    );
                    result.push(RouteCandidateDto {
                        id: cid,
                        diagram_index: template.diagram_index,
                        train_index: template.train_index,
                        direction: direction(template.direction).into(),
                        station_mappings: Vec::new(),
                        rank: "manual".into(),
                        reasons: vec!["自動照合候補なし".into()],
                        auto_selected: false,
                        manual_only: true,
                    });
                }
            }
            Ok(result)
        })
    }
    pub fn build_preview(
        &self,
        id: &SessionId,
        candidate_id: Option<&CandidateId>,
        manual: Option<ManualMappingInput>,
    ) -> Result<PreviewDto, BusinessError> {
        self.store.update(id, |s| {
            let candidate = candidate_id
                .and_then(|v| s.candidates.get(v))
                .cloned()
                .ok_or_else(|| {
                    BusinessError::new(
                        BusinessErrorKind::StaleState,
                        "候補が現在のセッションにありません",
                    )
                })?;
            if candidate.revision != s.revision {
                return Err(BusinessError::new(
                    BusinessErrorKind::StaleState,
                    "候補が古くなっています",
                ));
            }
            let mapping = manual
                .map(|v| v.station_mappings)
                .unwrap_or(candidate.mapping.clone());
            let route = selected_route(s)?;
            let mut station_slot_groups = vec![Vec::new(); route.stops.len()];
            for item in mapping {
                let Some(group) = station_slot_groups.get_mut(item.mtr_station_index) else {
                    return Err(BusinessError::new(
                        BusinessErrorKind::SelectionRequired,
                        "全駅の対応を確定してください",
                    ));
                };
                group.push(item.oudia_station_slot);
            }
            let expected_slots = candidate.template.active_station_slots.clone();
            let mut selected_slots = station_slot_groups
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            let mut sorted_expected = expected_slots.clone();
            selected_slots.sort_unstable();
            sorted_expected.sort_unstable();
            if station_slot_groups.iter().any(Vec::is_empty)
                || selected_slots.windows(2).any(|pair| pair[0] == pair[1])
                || selected_slots != sorted_expected
            {
                return Err(BusinessError::new(
                    BusinessErrorKind::SelectionRequired,
                    "全駅の対応を確定してください",
                ));
            }
            let slot_positions = expected_slots
                .iter()
                .enumerate()
                .map(|(position, slot)| (*slot, position))
                .collect::<HashMap<_, _>>();
            for group in &mut station_slot_groups {
                group.sort_by_key(|slot| slot_positions.get(slot).copied().unwrap_or(usize::MAX));
            }
            let timetable = generate_timetable(route).map_err(domain_error)?;
            let source = &s
                .source
                .as_ref()
                .ok_or_else(|| {
                    BusinessError::new(
                        BusinessErrorKind::SelectionRequired,
                        "OuDia を読み込んでください",
                    )
                })?
                .2;
            let operation_present = source
                .document
                .properties
                .iter()
                .any(|v| v.key.starts_with("Operation"));
            let pid = PreviewId(format!("preview-{}-{}", s.revision, s.previews.len()));
            let dto = preview_dto(pid.clone(), route, &timetable, operation_present);
            s.previews.insert(
                pid,
                Preview {
                    revision: s.revision,
                    template: candidate.template,
                    timetable,
                    station_slot_groups,
                },
            );
            Ok(dto)
        })
    }
    pub fn save_conversion(
        &self,
        id: &SessionId,
        preview_id: &PreviewId,
        output: &Path,
        policy: OperationPolicy,
    ) -> Result<SaveReceipt, BusinessError> {
        let (input, hash, source, preview) = self.store.with(id, |s| {
            let (_, h, source) = s.source.as_ref().ok_or_else(|| {
                BusinessError::new(
                    BusinessErrorKind::SelectionRequired,
                    "OuDia を読み込んでください",
                )
            })?;
            let preview = s.previews.get(preview_id).cloned().ok_or_else(|| {
                BusinessError::new(
                    BusinessErrorKind::StaleState,
                    "プレビューが現在のセッションにありません",
                )
            })?;
            if preview.revision != s.revision {
                return Err(BusinessError::new(
                    BusinessErrorKind::StaleState,
                    "プレビューが古くなっています",
                ));
            }
            Ok((
                s.source.as_ref().unwrap().0.clone(),
                *h,
                source.clone(),
                preview,
            ))
        })?;
        if preview.timetable.crosses_midnight {
            return Err(BusinessError::new(
                BusinessErrorKind::Validation,
                "24 時を超える時刻は保存できません",
            ));
        }
        let patch = build_eki_jikoku_patch_with_groups(
            &source,
            &preview.template,
            &preview.timetable,
            &preview.station_slot_groups,
            policy,
        )
        .map_err(|error| {
            #[cfg(debug_assertions)]
            eprintln!("[OuDia] 保存計画作成失敗: {error:?}");
            BusinessError {
                kind: BusinessErrorKind::SaveVerification,
                message: "安全な保存計画を作成できません".into(),
                detail: Some(error.to_string()),
            }
        })?;
        self.saver.save(&input, output, hash, &patch)
    }
}

fn selected_route(s: &Session) -> Result<&MtrRouteSnapshot, BusinessError> {
    let snapshot = s.snapshot.as_ref().ok_or_else(|| {
        BusinessError::new(
            BusinessErrorKind::SelectionRequired,
            "MTR スナップショットを取得してください",
        )
    })?;
    snapshot
        .snapshot
        .routes
        .iter()
        .find(|r| Some(&r.route_id) == s.selected_route.as_ref())
        .ok_or_else(|| {
            BusinessError::new(
                BusinessErrorKind::SelectionRequired,
                "路線を選択してください",
            )
        })
}
fn templates_for(
    source: &OudiaSource,
    diagram: Option<usize>,
) -> Result<Vec<OudiaRouteTemplate>, BusinessError> {
    match build_oudia_route_templates(&source.document) {
        ReferenceDiagramSelection::Selected(v)
            if diagram.is_none() || diagram == Some(v.diagram_index) =>
        {
            Ok(v.templates)
        }
        ReferenceDiagramSelection::Selected(_) => Err(BusinessError::new(
            BusinessErrorKind::Validation,
            "ダイヤ選択が一致しません",
        )),
        ReferenceDiagramSelection::NeedsSelection { .. } => {
            let index = diagram.ok_or_else(|| {
                BusinessError::new(
                    BusinessErrorKind::SelectionRequired,
                    "基準ダイヤを選択してください",
                )
            })?;
            let mut copy = source.document.clone();
            copy.kijun_dia_index = KijunDiaIndex::Valid(index);
            match build_oudia_route_templates(&copy) {
                ReferenceDiagramSelection::Selected(v) => Ok(v.templates),
                _ => Err(BusinessError::new(
                    BusinessErrorKind::Validation,
                    "ダイヤ選択が範囲外です",
                )),
            }
        }
    }
}
fn snapshot_dto(response: &MtrSnapshotResponse) -> Result<SnapshotDto, BusinessError> {
    Ok(SnapshotDto {
        routes: response
            .snapshot
            .routes
            .iter()
            .map(|r| {
                let total_run_millis = r
                    .stops
                    .iter()
                    .try_fold(0_i64, |total, stop| {
                        total.checked_add(stop.run_millis_to_next.map_or(0, |value| value.millis()))
                    })
                    .ok_or_else(|| {
                        BusinessError::new(
                            BusinessErrorKind::Internal,
                            "運転時分の合計が範囲外です",
                        )
                    })?;
                let total_dwell_millis = r
                    .stops
                    .iter()
                    .try_fold(0_i64, |total, stop| {
                        total.checked_add(stop.dwell_millis.millis())
                    })
                    .ok_or_else(|| {
                        BusinessError::new(
                            BusinessErrorKind::Internal,
                            "停車時分の合計が範囲外です",
                        )
                    })?;
                Ok(RouteDto {
                    id: r.route_id.clone(),
                    name: r.display_name.clone(),
                    station_count: r.stops.len(),
                    total_run_millis,
                    total_dwell_millis,
                    stations: r
                        .stops
                        .iter()
                        .map(|stop| RouteStationDto {
                            station_name: stop.station_name.clone(),
                            platform_name: stop.platform_name.clone(),
                            dwell_millis: stop.dwell_millis.millis(),
                            run_millis_to_next: stop.run_millis_to_next.map(|value| value.millis()),
                        })
                        .collect(),
                })
            })
            .collect::<Result<Vec<_>, BusinessError>>()?,
        dimensions: response.available_dimensions.clone(),
        api_current_time_millis: response.snapshot.api_current_time_millis,
    })
}
fn inspection_dto(source: &OudiaSource) -> InspectionDto {
    let templates = match build_oudia_route_templates(&source.document) {
        ReferenceDiagramSelection::Selected(v) => v.templates,
        _ => Vec::new(),
    };
    InspectionDto {
        file_type: source.document.file_type.clone(),
        line_name: source
            .document
            .properties
            .iter()
            .find(|property| property.key == "Rosenmei")
            .map(|property| property.value.clone()),
        station_count: source.document.station_slots.len(),
        kijun_status: match source.document.kijun_dia_index {
            KijunDiaIndex::Valid(_) => "valid",
            KijunDiaIndex::Missing => "missing",
            KijunDiaIndex::Invalid => "invalid",
            KijunDiaIndex::OutOfRange { .. } => "out_of_range",
        }
        .into(),
        diagrams: source
            .document
            .diagrams
            .iter()
            .enumerate()
            .map(|(index, d)| DiagramDto {
                index,
                train_count: d.trains.len(),
            })
            .collect(),
        train_types: source
            .document
            .diagrams
            .iter()
            .flat_map(|d| d.trains.iter().filter_map(|t| t.train_type_index))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        train_type_names: source
            .document
            .properties
            .iter()
            .filter(|property| property.key == "Syubetsumei")
            .map(|property| property.value.clone())
            .collect(),
        templates: templates
            .into_iter()
            .map(|t| TemplateDto {
                diagram_index: t.diagram_index,
                direction: direction(t.direction).into(),
                train_index: t.train_index,
                train_type_index: t.train_type_index,
                active_station_slots: t
                    .active_station_slots
                    .iter()
                    .zip(&t.stop_pattern)
                    .map(|(&index, stop)| {
                        station_slot_dto(&source.document.station_slots, index, stop.handling_code)
                    })
                    .collect(),
                route_station_slots: t
                    .route_station_slots
                    .iter()
                    .zip(&t.route_stop_pattern)
                    .map(|(&index, stop)| {
                        station_slot_dto(&source.document.station_slots, index, stop.handling_code)
                    })
                    .collect(),
            })
            .collect(),
    }
}
fn candidate_dto(
    id: CandidateId,
    c: RouteMatchCandidate,
    mapping: Vec<StationMappingDto>,
    auto_selected: bool,
) -> RouteCandidateDto {
    RouteCandidateDto {
        id,
        diagram_index: c.id.diagram_index,
        train_index: c.id.train_index,
        direction: direction(c.direction).into(),
        station_mappings: mapping,
        rank: format!("{:?}", c.rank),
        reasons: c
            .diagnostics
            .into_iter()
            .map(|v| format!("{:?}", v))
            .collect(),
        auto_selected,
        manual_only: false,
    }
}
fn station_slot_dto(
    slots: &[mtr_oudia_domain::OudiaStationSlot],
    index: usize,
    handling_code: Option<u8>,
) -> StationSlotDto {
    StationSlotDto {
        index,
        name: slots
            .get(index)
            .map(|slot| slot.name.clone())
            .unwrap_or_default(),
        handling_code,
        previous_name: index
            .checked_sub(1)
            .and_then(|previous| slots.get(previous))
            .map(|slot| slot.name.clone()),
        next_name: slots.get(index + 1).map(|slot| slot.name.clone()),
    }
}
fn preview_dto(
    id: PreviewId,
    route: &MtrRouteSnapshot,
    timetable: &GeneratedTimetable,
    operation_present: bool,
) -> PreviewDto {
    PreviewDto {
        id,
        fixed_base_time: "10:00:00".into(),
        stops: timetable
            .stops
            .iter()
            .zip(&route.stops)
            .map(|(t, r)| PreviewStopDto {
                station: r.station_name.clone(),
                existing_arrival: None,
                existing_departure: None,
                raw_arrival_millis: t.arrival.map(|v| v.millis()),
                raw_departure_millis: t.departure.map(|v| v.millis()),
                rounded_arrival: t.rounded_arrival_display.clone(),
                rounded_departure: t.rounded_departure_display.clone(),
                run_millis: r.run_millis_to_next.map(|v| v.millis()),
                dwell_millis: r.dwell_millis.millis(),
            })
            .collect(),
        warnings: if timetable.crosses_midnight {
            vec!["24 時を超える時刻は保存できません".into()]
        } else {
            Vec::new()
        },
        crosses_midnight: timetable.crosses_midnight,
        operation_present,
        policy_choices: if operation_present {
            vec!["preserve".into(), "remove_target_train".into()]
        } else {
            Vec::new()
        },
    }
}
fn direction(v: OudiaDirection) -> &'static str {
    match v {
        OudiaDirection::Kudari => "kudari",
        OudiaDirection::Nobori => "nobori",
    }
}
fn domain_error(_: impl std::fmt::Display) -> BusinessError {
    BusinessError::new(BusinessErrorKind::Validation, "時刻表を生成できません")
}
fn map_application_error(e: ApplicationError) -> BusinessError {
    let (kind, message) = match e {
        ApplicationError::Timeout => (
            BusinessErrorKind::Connection,
            "MTR APIの応答がタイムアウトしました",
        ),
        ApplicationError::Transport { .. } => {
            (BusinessErrorKind::Connection, "MTR APIへ接続できません")
        }
        ApplicationError::InvalidEndpoint { .. } => {
            (BusinessErrorKind::Validation, "接続先URLが不正です")
        }
        _ => (
            BusinessErrorKind::ParseUnsupported,
            "MTR APIの応答形式が一致しません",
        ),
    };
    BusinessError::new(kind, message)
}

fn discovery_error(
    message: &str,
    log_status: MinecraftLogStatus,
    counts: Option<String>,
) -> BusinessError {
    let log = match log_status {
        MinecraftLogStatus::NotConfigured => None,
        MinecraftLogStatus::NotFound => Some("Minecraftログが見つかりません"),
        MinecraftLogStatus::NoEndpoint => Some("ログ内にMTR API URLがありません"),
        MinecraftLogStatus::InvalidEndpoint => Some("ログ内の接続先URLが不正です"),
        MinecraftLogStatus::ReadFailed => Some("Minecraftログを読み取れません"),
        MinecraftLogStatus::CandidateRejected => Some("ログ内のMTR APIへ接続できません"),
    };
    let detail = match (log, counts) {
        (Some(log), Some(counts)) => Some(format!("{log}。{counts}")),
        (Some(log), None) => Some(log.to_owned()),
        (None, counts) => counts,
    };
    BusinessError {
        kind: BusinessErrorKind::NoEndpoints,
        message: message.to_owned(),
        detail,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MtrEndpointDiscovery {
    NoListeningPorts {
        log_status: MinecraftLogStatus,
    },
    PortEnumerationFailed {
        error: ApplicationError,
        log_status: MinecraftLogStatus,
    },
    NoValidEndpoints {
        attempted: usize,
        unreachable: usize,
        invalid_responses: usize,
        timeouts: usize,
        log_status: MinecraftLogStatus,
    },
    Single(MtrEndpoint),
    Multiple(Vec<MtrEndpoint>),
}
pub struct MtrEndpointDiscoveryService<'a, P: ?Sized, C: ?Sized> {
    ports: &'a P,
    client: &'a C,
    minecraft_log: Option<&'a dyn MinecraftLogProvider>,
}
impl<'a, P: ListeningPortProvider + ?Sized, C: MtrApiClient + ?Sized>
    MtrEndpointDiscoveryService<'a, P, C>
{
    pub fn new(ports: &'a P, client: &'a C) -> Self {
        Self {
            ports,
            client,
            minecraft_log: None,
        }
    }
    pub fn with_minecraft_log(mut self, provider: &'a dyn MinecraftLogProvider) -> Self {
        self.minecraft_log = Some(provider);
        self
    }
    pub async fn detect(&self, previous: Option<MtrEndpoint>) -> MtrEndpointDiscovery {
        let mut attempted = 0;
        let mut unreachable = 0;
        let mut invalid = 0;
        let mut timeouts = 0;
        let mut seen = std::collections::HashSet::new();
        let mut previous_timeout = None;
        if let Some(e) = previous {
            attempted += 1;
            match self.client.probe_endpoint(&e, 0).await {
                Ok(_) => return MtrEndpointDiscovery::Single(e),
                Err(ApplicationError::Timeout) => {
                    timeouts += 1;
                    previous_timeout = Some(e.clone());
                }
                Err(ApplicationError::Transport { .. }) => unreachable += 1,
                Err(_) => invalid += 1,
            }
            seen.insert(e.as_url().to_string());
        }
        let mut log_status = MinecraftLogStatus::NotConfigured;
        let mut log_timeout = None;
        if let Some(provider) = self.minecraft_log {
            match provider.read_latest_log() {
                MinecraftLogRead::NotFound => log_status = MinecraftLogStatus::NotFound,
                MinecraftLogRead::ReadFailed => log_status = MinecraftLogStatus::ReadFailed,
                MinecraftLogRead::Contents(log) => match endpoint_from_minecraft_log(&log) {
                    Ok(Some(endpoint)) => {
                        if seen.insert(endpoint.as_url().to_string()) {
                            attempted += 1;
                            match self.client.probe_endpoint(&endpoint, 0).await {
                                Ok(()) => return MtrEndpointDiscovery::Single(endpoint),
                                Err(ApplicationError::Timeout) => {
                                    timeouts += 1;
                                    log_timeout = Some(endpoint);
                                }
                                Err(ApplicationError::Transport { .. }) => unreachable += 1,
                                Err(_) => invalid += 1,
                            }
                        }
                        log_status = MinecraftLogStatus::CandidateRejected;
                    }
                    Ok(None) => log_status = MinecraftLogStatus::NoEndpoint,
                    Err(_) => log_status = MinecraftLogStatus::InvalidEndpoint,
                },
            }
        }
        let ports = match self.ports.listening_tcp_ports() {
            Ok(v) => v,
            Err(error) => {
                return MtrEndpointDiscovery::PortEnumerationFailed { error, log_status };
            }
        };
        let endpoints: Vec<_> = ports
            .into_iter()
            .filter(|p| *p != 0)
            .flat_map(|p| {
                [
                    format!("http://127.0.0.1:{p}/"),
                    format!("http://[::1]:{p}/"),
                ]
            })
            .filter_map(|v| MtrEndpoint::parse(&v).ok())
            .filter(|v| seen.insert(v.as_url().to_string()))
            .collect();
        if endpoints.is_empty() && attempted == 0 {
            return MtrEndpointDiscovery::NoListeningPorts { log_status };
        }
        let result = stream::iter(endpoints)
            .map(|e| async move {
                let r = self.client.probe_endpoint(&e, 0).await;
                (e, r)
            })
            .buffer_unordered(16)
            .collect::<Vec<_>>()
            .await;
        let mut valid = Vec::new();
        let mut timed_out = Vec::new();
        for (e, r) in result {
            attempted += 1;
            match r {
                Ok(_) => valid.push(e),
                Err(ApplicationError::Timeout) => {
                    timeouts += 1;
                    timed_out.push(e);
                }
                Err(ApplicationError::Transport { .. }) => unreachable += 1,
                Err(_) => invalid += 1,
            }
        }
        timed_out.sort_by(|a, b| a.as_url().as_str().cmp(b.as_url().as_str()));
        if let Some(e) = previous_timeout {
            timed_out.insert(0, e);
        }
        if let Some(e) = log_timeout {
            timed_out.insert(0, e);
        }
        let mut extended = timed_out.into_iter();
        let retry = extended.by_ref().take(16).collect::<Vec<_>>();
        unreachable += extended.count();
        let result = stream::iter(retry)
            .map(|e| async move {
                let r = self.client.probe_endpoint_extended(&e, 0).await;
                (e, r)
            })
            .buffer_unordered(16)
            .collect::<Vec<_>>()
            .await;
        for (e, r) in result {
            match r {
                Ok(_) => valid.push(e),
                Err(ApplicationError::Timeout | ApplicationError::Transport { .. }) => {
                    unreachable += 1
                }
                Err(_) => invalid += 1,
            }
        }
        valid.sort_by(|a, b| a.as_url().as_str().cmp(b.as_url().as_str()));
        valid.dedup_by(|a, b| a.as_url() == b.as_url());
        match valid.len() {
            0 => MtrEndpointDiscovery::NoValidEndpoints {
                attempted,
                unreachable,
                invalid_responses: invalid,
                timeouts,
                log_status,
            },
            1 => MtrEndpointDiscovery::Single(valid.pop().unwrap()),
            _ => MtrEndpointDiscovery::Multiple(valid),
        }
    }
}
pub struct DetectMtrEndpoint<'a, P: ?Sized, C: ?Sized>(pub MtrEndpointDiscoveryService<'a, P, C>);
impl<'a, P: ListeningPortProvider + ?Sized, C: MtrApiClient + ?Sized> DetectMtrEndpoint<'a, P, C> {
    pub fn new(p: &'a P, c: &'a C) -> Self {
        Self(MtrEndpointDiscoveryService::new(p, c))
    }
    pub async fn execute(&self, previous: Option<MtrEndpoint>) -> MtrEndpointDiscovery {
        self.0.detect(previous).await
    }
}

/// セッション非使用の既存呼出し互換用 snapshot 取得ユースケース。
pub struct FetchMtrSnapshot<'a, C: ?Sized, S: ?Sized> {
    client: &'a C,
    settings: &'a S,
}
impl<'a, C: MtrApiClient + ?Sized, S: SettingsRepository + ?Sized> FetchMtrSnapshot<'a, C, S> {
    pub fn new(client: &'a C, settings: &'a S) -> Self {
        Self { client, settings }
    }
    pub async fn execute(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<SnapshotDto, BusinessError> {
        let response = self
            .client
            .fetch_snapshot(endpoint, dimension)
            .await
            .map_err(map_application_error)?;
        self.settings.save_last_successful_endpoint(endpoint)?;
        snapshot_dto(&response)
    }
}

/// セッション非使用の既存呼出し互換用 OuDia 検査ユースケース。
pub struct InspectOudia<'a, R: ?Sized> {
    repository: &'a R,
}
impl<'a, R: OudiaRepository + ?Sized> InspectOudia<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }
    pub fn execute(&self, path: &Path) -> Result<(OudiaSource, InspectionDto), BusinessError> {
        let source = self.repository.read(path)?;
        Ok((source.clone(), inspection_dto(&source)))
    }
}
