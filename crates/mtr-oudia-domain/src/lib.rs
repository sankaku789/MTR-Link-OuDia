//! 外部システムに依存しない業務規則を置く Domain 層。

use std::error::Error;
use std::fmt;

pub mod mtr_id;
pub mod oudia;
pub mod outbound_operation;
pub mod outbound_runtime;
pub mod patch;
pub mod route_matching;
pub mod timetable;

pub use mtr_id::*;
pub use oudia::*;
pub use outbound_operation::*;
pub use outbound_runtime::*;
pub use patch::*;
pub use route_matching::*;
pub use timetable::*;

/// Domain 層で検出した不正な値や演算結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// サービス時刻に負数が指定された。
    NegativeServiceTime { millis: i64 },
    /// サービス時刻の演算が `i64` の範囲を超えた。
    TimeOverflow,
    /// 範囲の終端が始端より前である。
    InvalidRange { start: usize, end: usize },
    /// 範囲が元データの境界を超えている。
    RangeOutOfBounds {
        start: usize,
        end: usize,
        bounds: usize,
    },
    /// インデックスが対象コレクションの範囲外である。
    IndexOutOfRange {
        index_name: &'static str,
        index: usize,
        count: usize,
    },
    /// 空文字列など、値オブジェクトとして無効な値が指定された。
    InvalidValue { value_name: &'static str },
    /// MTR 路線の駅列または運転時分が整合していない。
    InvalidRoute { reason: &'static str },
    /// 時刻表生成に必要な駅数または時分数が不正である。
    InvalidTimetable { reason: &'static str },
    /// 入力の文字コードは正式対応外である。
    UnsupportedEncoding { encoding: &'static str },
    /// 入力を厳格に文字列へ変換できない。
    DecodeError { encoding: &'static str },
    /// LF と CRLF が混在している。
    MixedLineEnding,
    /// 正式対応外の OuDia FileType が指定された。
    UnsupportedFileType { file_type: String },
    /// OuDia のセクション構造が破損している。
    InvalidOudiaStructure { reason: &'static str },
    /// 書換え安全性を確認できない EkiJikoku セルが存在する。
    UnknownEkiJikoku { raw: String },
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativeServiceTime { millis } => {
                write!(formatter, "負のサービス時刻は使用できません: {millis}")
            }
            Self::TimeOverflow => formatter.write_str("サービス時刻の演算がオーバーフローしました"),
            Self::InvalidRange { start, end } => {
                write!(formatter, "不正な範囲です: {start}..{end}")
            }
            Self::RangeOutOfBounds { start, end, bounds } => {
                write!(
                    formatter,
                    "範囲 {start}..{end} は境界 {bounds} を超えています"
                )
            }
            Self::IndexOutOfRange {
                index_name,
                index,
                count,
            } => write!(
                formatter,
                "{index_name}={index} は要素数 {count} の範囲外です"
            ),
            Self::InvalidValue { value_name } => write!(formatter, "{value_name} が不正です"),
            Self::InvalidRoute { reason } => write!(formatter, "不正な路線データです: {reason}"),
            Self::InvalidTimetable { reason } => write!(formatter, "不正な時刻表です: {reason}"),
            Self::UnsupportedEncoding { encoding } => {
                write!(formatter, "未対応の文字コードです: {encoding}")
            }
            Self::DecodeError { encoding } => {
                write!(formatter, "{encoding} として厳格に復号できません")
            }
            Self::MixedLineEnding => formatter.write_str("改行コードが混在しています"),
            Self::UnsupportedFileType { file_type } => {
                write!(formatter, "未対応の FileType です: {file_type}")
            }
            Self::InvalidOudiaStructure { reason } => {
                write!(formatter, "不正な OuDia 構造です: {reason}")
            }
            Self::UnknownEkiJikoku { raw } => {
                write!(formatter, "安全に解析できない EkiJikoku です: {raw}")
            }
        }
    }
}

impl Error for DomainError {}

/// サービス日開始からの非負ミリ秒。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServiceTimeMillis(i64);

impl ServiceTimeMillis {
    /// 基準始発駅の固定発時刻（10:00:00）。
    pub const TEN_OCLOCK: Self = Self(36_000_000);

    /// 非負のミリ秒からサービス時刻を作成する。
    pub fn new(millis: i64) -> Result<Self, DomainError> {
        if millis < 0 {
            return Err(DomainError::NegativeServiceTime { millis });
        }

        Ok(Self(millis))
    }

    /// 保持しているミリ秒値を返す。
    pub const fn millis(self) -> i64 {
        self.0
    }

    /// 別のサービス時刻を加算する。
    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DomainError::TimeOverflow)
    }

    /// OuDia 表示直前に秒へ half-up 丸めする。
    pub fn rounded_seconds(self) -> Result<i64, DomainError> {
        self.0
            .checked_add(500)
            .map(|millis| millis / 1_000)
            .ok_or(DomainError::TimeOverflow)
    }
}

/// 元バイト列内の半開区間 `[start, end)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    start: usize,
    end: usize,
}

impl SourceRange {
    /// 半開区間を作成する。
    pub fn new(start: usize, end: usize) -> Result<Self, DomainError> {
        if start > end {
            return Err(DomainError::InvalidRange { start, end });
        }

        Ok(Self { start, end })
    }

    /// 区間の始端を返す。
    pub const fn start(self) -> usize {
        self.start
    }

    /// 区間の終端を返す。
    pub const fn end(self) -> usize {
        self.end
    }

    /// 区間の長さを返す。
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// 区間が空かを返す。
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// 指定したバイト位置が区間に含まれるかを返す。
    pub const fn contains(self, offset: usize) -> bool {
        self.start <= offset && offset < self.end
    }

    /// 指定した元データ長の範囲内にあることを検証する。
    pub const fn validate_bounds(self, bounds: usize) -> Result<(), DomainError> {
        if self.end > bounds {
            return Err(DomainError::RangeOutOfBounds {
                start: self.start,
                end: self.end,
                bounds,
            });
        }

        Ok(())
    }

    /// 他の区間と共通するバイト位置があるかを返す。
    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// 他の非空区間と端点だけを共有するかを返す。
    pub const fn touches(self, other: Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && (self.end == other.start || other.end == self.start)
    }
}

/// OuDia の列車が使用する駅スロットを特定する識別子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OudiaStationSlotId {
    diagram_index: usize,
    direction: OudiaDirection,
    train_index: usize,
    station_slot_index: usize,
}

impl OudiaStationSlotId {
    /// 各コレクションの要素数で検証した駅スロット識別子を作成する。
    pub fn new(
        diagram_index: usize,
        direction: OudiaDirection,
        train_index: usize,
        station_slot_index: usize,
        diagram_count: usize,
        train_count: usize,
        station_slot_count: usize,
    ) -> Result<Self, DomainError> {
        validate_index("diagram_index", diagram_index, diagram_count)?;
        validate_index("train_index", train_index, train_count)?;
        validate_index("station_slot_index", station_slot_index, station_slot_count)?;

        Ok(Self {
            diagram_index,
            direction,
            train_index,
            station_slot_index,
        })
    }

    /// ダイヤのインデックスを返す。
    pub const fn diagram_index(self) -> usize {
        self.diagram_index
    }

    /// 列車の方向を返す。
    pub const fn direction(self) -> OudiaDirection {
        self.direction
    }

    /// 方向内の列車インデックスを返す。
    pub const fn train_index(self) -> usize {
        self.train_index
    }

    /// 路線内の駅スロットインデックスを返す。
    pub const fn station_slot_index(self) -> usize {
        self.station_slot_index
    }
}

/// OuDia の列車方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OudiaDirection {
    /// 下り列車。
    Kudari,
    /// 上り列車。
    Nobori,
}

/// MTR API から正規化したネットワーク情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtrNetworkSnapshot {
    pub base_url: String,
    pub dimension: u32,
    pub api_current_time_millis: i64,
    pub retrieved_at_unix_millis: i64,
    pub routes: Vec<MtrRouteSnapshot>,
}

impl MtrNetworkSnapshot {
    /// 必須の接続先情報を検証してスナップショットを作成する。
    pub fn new(
        base_url: impl Into<String>,
        dimension: u32,
        api_current_time_millis: i64,
        retrieved_at_unix_millis: i64,
        routes: Vec<MtrRouteSnapshot>,
    ) -> Result<Self, DomainError> {
        let base_url = base_url.into();
        validate_non_empty("base_url", &base_url)?;

        Ok(Self {
            base_url,
            dimension,
            api_current_time_millis,
            retrieved_at_unix_millis,
            routes,
        })
    }
}

/// MTR 路線を駅順に正規化した情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtrRouteSnapshot {
    pub route_id: String,
    pub display_name: String,
    pub stations_signature: Vec<String>,
    pub stops: Vec<MtrStopSnapshot>,
}

impl MtrRouteSnapshot {
    /// 駅数と駅間運転時分の対応を検証して路線を作成する。
    pub fn new(
        route_id: impl Into<String>,
        display_name: impl Into<String>,
        stations_signature: Vec<String>,
        stops: Vec<MtrStopSnapshot>,
    ) -> Result<Self, DomainError> {
        let route_id = route_id.into();
        let display_name = display_name.into();
        validate_non_empty("route_id", &route_id)?;
        validate_non_empty("display_name", &display_name)?;

        if stops.len() < 2 {
            return Err(DomainError::InvalidRoute {
                reason: "駅数は2以上必要です",
            });
        }
        if stations_signature.len() != stops.len() {
            return Err(DomainError::InvalidRoute {
                reason: "駅署名数が駅数と一致しません",
            });
        }
        if stops[..stops.len() - 1]
            .iter()
            .any(|stop| stop.run_millis_to_next.is_none())
            || stops
                .last()
                .is_some_and(|stop| stop.run_millis_to_next.is_some())
        {
            return Err(DomainError::InvalidRoute {
                reason: "駅間運転時分が駅列と一致しません",
            });
        }

        Ok(Self {
            route_id,
            display_name,
            stations_signature,
            stops,
        })
    }
}

/// MTR 路線上の駅とホームの情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtrStopSnapshot {
    pub station_id: String,
    pub station_name: String,
    pub platform_name: String,
    pub dwell_millis: ServiceTimeMillis,
    pub run_millis_to_next: Option<ServiceTimeMillis>,
}

impl MtrStopSnapshot {
    /// 必須の駅識別情報を検証して駅情報を作成する。
    pub fn new(
        station_id: impl Into<String>,
        station_name: impl Into<String>,
        platform_name: impl Into<String>,
        dwell_millis: ServiceTimeMillis,
        run_millis_to_next: Option<ServiceTimeMillis>,
    ) -> Result<Self, DomainError> {
        let station_id = station_id.into();
        let station_name = station_name.into();
        validate_non_empty("station_id", &station_id)?;
        validate_non_empty("station_name", &station_name)?;

        Ok(Self {
            station_id,
            station_name,
            platform_name: platform_name.into(),
            dwell_millis,
            run_millis_to_next,
        })
    }
}

/// Domain 層を表す最小の型。
///
/// P01 の依存方向テストで使用する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainLayer;

fn validate_non_empty(value_name: &'static str, value: &str) -> Result<(), DomainError> {
    if value.trim().is_empty() {
        return Err(DomainError::InvalidValue { value_name });
    }

    Ok(())
}

fn validate_index(index_name: &'static str, index: usize, count: usize) -> Result<(), DomainError> {
    if index >= count {
        return Err(DomainError::IndexOutOfRange {
            index_name,
            index,
            count,
        });
    }

    Ok(())
}
