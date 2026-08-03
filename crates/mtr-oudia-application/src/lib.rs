//! Domain のユースケース境界と Port を置く Application 層。

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

/// Infrastructure が実装する、Domain 型を返す最小の Port。
pub trait DomainPort {
    /// Domain 層を返す。
    fn domain_layer(&self) -> DomainLayer;
}
