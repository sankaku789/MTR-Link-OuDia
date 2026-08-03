//! Domain のユースケース境界と Port を置く Application 層。

use futures_util::{StreamExt, stream};
use mtr_oudia_domain::DomainLayer;
use mtr_oudia_domain::MtrNetworkSnapshot;
use std::error::Error;
use std::fmt;
use url::{Host, Url};

pub use async_trait::async_trait;

/// GUI が接続失敗の種類を表示するための Application 境界のエラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationError {
    /// localhost 以外など、許可されない手動接続先である。
    InvalidEndpoint { reason: &'static str },
    /// 接続または応答が制限時間内に完了しなかった。
    Timeout,
    /// HTTP 通信自体に失敗した。
    Transport { message: String },
    /// 応答本文が許可サイズを超えた。
    ResponseTooLarge,
    /// MTR API として必要な構造または値を満たさない。
    InvalidResponse { reason: String },
    /// OS の待受 TCP ポートを列挙できなかった。
    ListeningPortEnumeration { message: String },
    /// 現在の OS では待受 TCP ポート列挙を提供していない。
    ListeningPortProviderUnsupported,
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint { reason } => {
                write!(formatter, "許可されない API 接続先です: {reason}")
            }
            Self::Timeout => formatter.write_str("MTR API の応答がタイムアウトしました"),
            Self::Transport { message } => {
                write!(formatter, "MTR API への接続に失敗しました: {message}")
            }
            Self::ResponseTooLarge => formatter.write_str("MTR API の応答が大きすぎます"),
            Self::InvalidResponse { reason } => {
                write!(formatter, "MTR API の応答が不正です: {reason}")
            }
            Self::ListeningPortEnumeration { message } => {
                write!(formatter, "待受 TCP ポートを列挙できません: {message}")
            }
            Self::ListeningPortProviderUnsupported => {
                formatter.write_str("この OS では待受 TCP ポート列挙を利用できません")
            }
        }
    }
}

impl Error for ApplicationError {}

/// localhost 上の MTR API のベース URL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtrEndpoint(Url);

impl MtrEndpoint {
    /// 手動入力を検証し、固定 API パスだけを後から組み立てられる形に正規化する。
    pub fn parse(input: &str) -> Result<Self, ApplicationError> {
        let mut url = Url::parse(input).map_err(|_| ApplicationError::InvalidEndpoint {
            reason: "URL として解析できません",
        })?;
        if url.scheme() != "http" {
            return Err(ApplicationError::InvalidEndpoint {
                reason: "http のみ許可されます",
            });
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ApplicationError::InvalidEndpoint {
                reason: "userinfo は許可されません",
            });
        }
        if url.fragment().is_some() {
            return Err(ApplicationError::InvalidEndpoint {
                reason: "fragment は許可されません",
            });
        }
        if url.query().is_some() {
            return Err(ApplicationError::InvalidEndpoint {
                reason: "query は許可されません",
            });
        }
        if !url.path().is_empty() && url.path() != "/" {
            return Err(ApplicationError::InvalidEndpoint {
                reason: "path は許可されません",
            });
        }
        if url.port().is_none() {
            return Err(ApplicationError::InvalidEndpoint {
                reason: "port が必要です",
            });
        }
        match url.host() {
            Some(Host::Ipv4(address)) if address.octets()[0] == 127 => {}
            Some(Host::Ipv6(address)) if address.is_loopback() => {}
            _ => {
                return Err(ApplicationError::InvalidEndpoint {
                    reason: "literal loopback address のみ許可されます",
                });
            }
        }
        url.set_path("/");
        Ok(Self(url))
    }

    /// dimension を含む固定 MTR API URL を URL API で構築する。
    pub fn stations_and_routes_url(&self, dimension: u32) -> Url {
        let mut url = self.0.clone();
        url.set_path("/mtr/api/map/stations-and-routes");
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair("dimension", &dimension.to_string());
        url
    }

    /// 正規化済みのベース URL を返す。
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

/// API 応答から得たスナップショットと dimension 選択肢。
#[derive(Debug, Clone, PartialEq)]
pub struct MtrSnapshotResponse {
    pub snapshot: MtrNetworkSnapshot,
    /// API 固有形式を変更せず GUI へ渡す dimension 一覧。
    pub available_dimensions: Vec<serde_json::Value>,
}

/// MTR localhost API を取得する Infrastructure Port。
#[async_trait]
pub trait MtrApiClient: Send + Sync {
    /// 指定 dimension の正規化済みスナップショットを取得する。
    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError>;
}

/// OS が提供する localhost 到達可能な TCP 待受ポートの列挙境界。
pub trait ListeningPortProvider: Send + Sync {
    /// port 0 を含まない、重複しない待受 TCP ポートを返す。
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError>;
}

/// API エンドポイント自動検出の結果。GUI の選択と手動入力は呼出側が担当する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MtrEndpointDiscovery {
    /// OS に localhost 到達可能な待受ポートがなかった。
    NoListeningPorts,
    /// OS の待受ポート列挙に失敗した。
    PortEnumerationFailed(ApplicationError),
    /// 応答不能または MTR API 応答として不正な候補だけだった。
    NoValidEndpoints {
        attempted: usize,
        unreachable: usize,
        invalid_responses: usize,
    },
    /// 一意の MTR API エンドポイントを検出した。
    Single(MtrEndpoint),
    /// 複数の MTR API エンドポイントを検出したため自動確定しない。
    Multiple(Vec<MtrEndpoint>),
}

/// 前回 URL と OS 列挙ポートを用いて MTR API を検出する Application service。
pub struct MtrEndpointDiscoveryService<'a, P: ?Sized, C: ?Sized> {
    ports: &'a P,
    client: &'a C,
}

impl<'a, P: ListeningPortProvider + ?Sized, C: MtrApiClient + ?Sized>
    MtrEndpointDiscoveryService<'a, P, C>
{
    /// Port 実装と HTTP client を借用して service を作る。
    pub fn new(ports: &'a P, client: &'a C) -> Self {
        Self { ports, client }
    }

    /// 前回成功 URL を先に検証し、失敗時だけ列挙済みポートを最大16並列で検証する。
    pub async fn detect(&self, previous: Option<MtrEndpoint>) -> MtrEndpointDiscovery {
        let mut attempted = 0;
        let mut unreachable = 0;
        let mut invalid_responses = 0;
        let mut valid = Vec::new();
        let mut seen = std::collections::HashSet::new();

        if let Some(endpoint) = previous {
            attempted += 1;
            match self.client.fetch_snapshot(&endpoint, 0).await {
                Ok(_) => return MtrEndpointDiscovery::Single(endpoint),
                Err(error) => count_probe_failure(error, &mut unreachable, &mut invalid_responses),
            }
            seen.insert(endpoint.as_url().as_str().to_owned());
        }

        let ports = match self.ports.listening_tcp_ports() {
            Ok(ports) => ports,
            Err(error) => return MtrEndpointDiscovery::PortEnumerationFailed(error),
        };
        let mut endpoints = Vec::new();
        for port in ports.into_iter().filter(|port| *port != 0) {
            for input in [
                format!("http://127.0.0.1:{port}/"),
                format!("http://[::1]:{port}/"),
            ] {
                // 定数の loopback URL だけを組み立てるので parser 失敗は不変条件違反である。
                let endpoint = MtrEndpoint::parse(&input).expect("generated loopback endpoint");
                if seen.insert(endpoint.as_url().as_str().to_owned()) {
                    endpoints.push(endpoint);
                }
            }
        }
        if endpoints.is_empty() {
            return if attempted == 0 {
                MtrEndpointDiscovery::NoListeningPorts
            } else {
                MtrEndpointDiscovery::NoValidEndpoints {
                    attempted,
                    unreachable,
                    invalid_responses,
                }
            };
        }

        let outcomes = stream::iter(endpoints)
            .map(|endpoint| async move {
                let result = self.client.fetch_snapshot(&endpoint, 0).await;
                (endpoint, result)
            })
            .buffer_unordered(16)
            .collect::<Vec<_>>()
            .await;
        for (endpoint, result) in outcomes {
            attempted += 1;
            match result {
                Ok(_) => valid.push(endpoint),
                Err(error) => count_probe_failure(error, &mut unreachable, &mut invalid_responses),
            }
        }
        valid.sort_by(|left, right| left.as_url().as_str().cmp(right.as_url().as_str()));
        match valid.len() {
            0 => MtrEndpointDiscovery::NoValidEndpoints {
                attempted,
                unreachable,
                invalid_responses,
            },
            1 => MtrEndpointDiscovery::Single(valid.pop().expect("one endpoint")),
            _ => MtrEndpointDiscovery::Multiple(valid),
        }
    }
}

fn count_probe_failure(
    error: ApplicationError,
    unreachable: &mut usize,
    invalid_responses: &mut usize,
) {
    match error {
        ApplicationError::Timeout | ApplicationError::Transport { .. } => *unreachable += 1,
        _ => *invalid_responses += 1,
    }
}

/// Infrastructure が実装する、Domain 型を返す最小の Port。
pub trait DomainPort {
    /// Domain 層を返す。
    fn domain_layer(&self) -> DomainLayer;
}
