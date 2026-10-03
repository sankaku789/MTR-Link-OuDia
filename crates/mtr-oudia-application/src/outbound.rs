use crate::{BusinessError, BusinessErrorKind};
use mtr_oudia_domain::OutboundRuntime;
use mtr_oudia_domain::{MtrId, MtrRouteSnapshot, ServiceTimeMillis};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivalDto {
    pub route_id: MtrId,
    pub platform_id: MtrId,
    pub platform_name: String,
    pub arrival: i64,
    pub departure: i64,
    pub deviation: i64,
    pub realtime: bool,
    pub departure_index: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivalsDto {
    pub current_time_millis: i64,
    pub arrivals: Vec<ArrivalDto>,
}

/// JSON/HTTPやOuDiaに依存しない測定。一意でない候補は選ばない。
pub fn measure_arrivals(
    dimension: u32,
    route: &MtrRouteSnapshot,
    depot_clock: &str,
    utc_offset: &str,
    response: &ArrivalsDto,
) -> Result<OutboundRuntimeSetting, BusinessError> {
    let first = route
        .stops
        .first()
        .ok_or_else(|| input_error("始発駅がありません"))?;
    if first.platform_name.is_empty() {
        return Err(input_error("始発ホーム情報がないため手動入力してください"));
    }
    let id = MtrId::from_hex(&route.route_id).map_err(|_| input_error("路線IDが不正です"))?;
    let station_id =
        MtrId::from_hex(&first.station_id).map_err(|_| input_error("始発駅IDが不正です"))?;
    let mut candidates = response
        .arrivals
        .iter()
        .filter(|a| a.route_id == id && a.platform_name == first.platform_name);
    let candidate = candidates
        .next()
        .ok_or_else(|| input_error("対象路線・始発ホームの発車候補がありません"))?;
    if candidates.next().is_some() {
        return Err(input_error(
            "発車候補が複数あるため一意に測定できません。試験列車を1本にするか手動入力してください",
        ));
    }
    let clock = parse_clock(depot_clock)?;
    let offset = parse_utc_offset(utc_offset)?;
    let departure = ServiceTimeMillis::new(candidate.departure.rem_euclid(86_400_000))
        .map_err(|_| input_error("API発車時刻が不正です"))?;
    let depot = ServiceTimeMillis::new((clock - offset).rem_euclid(86_400_000))
        .map_err(|_| input_error("車庫発時刻が不正です"))?;
    let runtime = OutboundRuntime::between_daily_times(depot, departure)
        .map_err(|_| input_error("出庫時分を計算できません"))?;
    Ok(OutboundRuntimeSetting {
        dimension,
        route_id: id.to_hex(),
        first_station_id: station_id.to_hex(),
        first_platform_name: first.platform_name.clone(),
        runtime,
        measured_at: response.current_time_millis,
        source: OutboundRuntimeSource::Measured,
    })
}

fn parse_clock(value: &str) -> Result<i64, BusinessError> {
    let bytes = value.as_bytes();
    if bytes.len() != 8
        || bytes[2] != b':'
        || bytes[5] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 2 || i == 5 || b.is_ascii_digit())
    {
        return Err(input_error("車庫発はHH:mm:ssで入力してください"));
    }
    let h: i64 = value[0..2]
        .parse()
        .map_err(|_| input_error("時刻が不正です"))?;
    let m: i64 = value[3..5]
        .parse()
        .map_err(|_| input_error("時刻が不正です"))?;
    let s: i64 = value[6..8]
        .parse()
        .map_err(|_| input_error("時刻が不正です"))?;
    if h >= 24 || m >= 60 || s >= 60 {
        return Err(input_error("時刻が範囲外です"));
    }
    Ok((h * 3600 + m * 60 + s) * 1000)
}

fn parse_utc_offset(value: &str) -> Result<i64, BusinessError> {
    let bytes = value.as_bytes();
    if bytes.len() != 6
        || !matches!(bytes[0], b'+' | b'-')
        || bytes[3] != b':'
        || ![bytes[1], bytes[2], bytes[4], bytes[5]]
            .iter()
            .all(u8::is_ascii_digit)
    {
        return Err(input_error(
            "Minecraft端末のUTCオフセットを+09:00等で入力してください",
        ));
    }
    let clock = parse_clock(&format!("{}:00", &value[1..]))?;
    Ok(if bytes[0] == b'-' { -clock } else { clock })
}

fn input_error(message: &str) -> BusinessError {
    BusinessError::new(BusinessErrorKind::Validation, message)
}

pub async fn measure_outbound_runtime<C: crate::MtrApiClient + ?Sized>(
    client: &C,
    endpoint: &crate::MtrEndpoint,
    dimension: u32,
    route: &MtrRouteSnapshot,
    depot_clock: &str,
    utc_offset: &str,
) -> Result<OutboundRuntimeSetting, BusinessError> {
    // Reject invalid input before issuing HTTP.
    parse_clock(depot_clock)?;
    parse_utc_offset(utc_offset)?;
    let first = route
        .stops
        .first()
        .ok_or_else(|| input_error("始発駅がありません"))?;
    let station_id = MtrId::from_hex(&first.station_id)
        .map_err(|_| input_error("始発駅IDが不正です"))?
        .to_hex();
    let response = client
        .fetch_arrivals(endpoint, dimension, &station_id)
        .await
        .map_err(crate::map_application_error)?;
    measure_arrivals(dimension, route, depot_clock, utc_offset, &response)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundRuntimeSource {
    Measured,
    Manual,
}

/// 1 dimension・1路線につき1値。構成識別情報は再測定判定に使う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundRuntimeSetting {
    pub dimension: u32,
    pub route_id: String,
    pub first_station_id: String,
    pub first_platform_name: String,
    #[serde(
        rename = "outbound_millis",
        serialize_with = "serialize_runtime",
        deserialize_with = "deserialize_runtime"
    )]
    pub runtime: OutboundRuntime,
    /// 測定/入力時のepoch millis。
    pub measured_at: i64,
    pub source: OutboundRuntimeSource,
}

fn serialize_runtime<S: Serializer>(
    value: &OutboundRuntime,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_i64(value.millis())
}

fn deserialize_runtime<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<OutboundRuntime, D::Error> {
    OutboundRuntime::new(i64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}
