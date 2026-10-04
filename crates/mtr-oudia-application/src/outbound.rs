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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObaArrivalDto {
    pub route_id: MtrId,
    pub platform_id: MtrId,
    pub stop_sequence: u32,
    pub block_trip_sequence: u32,
    pub arrival: i64,
    pub departure: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObaArrivalsDto {
    pub current_time_millis: i64,
    pub arrivals: Vec<ObaArrivalDto>,
}

/// 車庫発はAPI現在時刻の端末日付に属する。翌日同時刻や折返しを採用しない。
pub fn measure_oba_arrivals(
    dimension: u32,
    route: &MtrRouteSnapshot,
    platform: MtrId,
    depot_clock: &str,
    utc_offset: &str,
    response: &ObaArrivalsDto,
) -> Result<OutboundRuntimeSetting, BusinessError> {
    let clock = parse_clock(depot_clock)?;
    let offset = parse_utc_offset(utc_offset)?;
    let local_now = response
        .current_time_millis
        .checked_add(offset)
        .ok_or_else(|| input_error("API現在時刻が範囲外です"))?;
    let depot = local_now
        .div_euclid(86_400_000)
        .checked_mul(86_400_000)
        .and_then(|day| day.checked_add(clock))
        .and_then(|local| local.checked_sub(offset))
        .ok_or_else(|| input_error("車庫発時刻が範囲外です"))?;
    let end = depot
        .checked_add(86_400_000)
        .ok_or_else(|| input_error("車庫発時刻が範囲外です"))?;
    let route_id = MtrId::from_hex(&route.route_id).map_err(|_| input_error("路線IDが不正です"))?;
    let first = route
        .stops
        .first()
        .ok_or_else(|| input_error("始発駅がありません"))?;
    let arrivals = response
        .arrivals
        .iter()
        .filter(|a| {
            a.route_id == route_id
                && a.platform_id == platform
                && a.stop_sequence == 0
                && a.block_trip_sequence == 0
                && a.arrival >= depot
                && a.arrival < end
        })
        .map(|a| ArrivalDto {
            route_id: a.route_id,
            platform_id: a.platform_id,
            platform_name: first.platform_name.clone(),
            arrival: a.arrival,
            departure: a.departure,
            deviation: 0,
            realtime: false,
            departure_index: 0,
        })
        .collect();
    measure_arrivals(
        dimension,
        route,
        depot_clock,
        utc_offset,
        &ArrivalsDto {
            current_time_millis: response.current_time_millis,
            arrivals,
        },
    )
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
    let arrival = ServiceTimeMillis::new(candidate.arrival.rem_euclid(86_400_000))
        .map_err(|_| input_error("API到着時刻が不正です"))?;
    let depot = ServiceTimeMillis::new((clock - offset).rem_euclid(86_400_000))
        .map_err(|_| input_error("車庫発時刻が不正です"))?;
    let runtime = OutboundRuntime::between_daily_times(depot, arrival)
        .map_err(|_| input_error("出庫時分を計算できません"))?;
    Ok(OutboundRuntimeSetting {
        dimension,
        route_id: id.to_hex(),
        first_station_id: station_id.to_hex(),
        first_platform_name: first.platform_name.clone(),
        runtime,
        measured_at: response.current_time_millis,
        source: OutboundRuntimeSource::Measured,
        runtime_basis: OutboundRuntimeBasis::FirstArrival,
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
    let route_id = MtrId::from_hex(&route.route_id).map_err(|_| input_error("路線IDが不正です"))?;
    let mut platforms = response
        .arrivals
        .iter()
        .filter(|a| a.route_id == route_id && a.platform_name == first.platform_name)
        .map(|a| a.platform_id);
    let platform = platforms
        .next()
        .ok_or_else(|| input_error("対象路線の始発ホームが見つかりません"))?;
    if platforms.any(|other| other != platform) {
        return Err(input_error("始発ホームを一意に特定できません"));
    }
    let oba = client
        .fetch_oba_arrivals(endpoint, dimension, &route.route_id, &platform.to_hex())
        .await
        .map_err(crate::map_application_error)?;
    measure_oba_arrivals(dimension, route, platform, depot_clock, utc_offset, &oba)
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
    /// 省略された旧設定は始発駅の停車時間込み。
    #[serde(default)]
    pub runtime_basis: OutboundRuntimeBasis,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundRuntimeBasis {
    #[default]
    FirstDeparture,
    FirstArrival,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutboundStatusDto {
    pub setting: Option<OutboundRuntimeSetting>,
    pub valid: bool,
    pub message: Option<String>,
    pub duration_label: String,
    pub first_station_name: String,
    pub first_platform_name: String,
}

pub(crate) fn status_for_route(
    dimension: u32,
    route: &MtrRouteSnapshot,
    settings: &crate::SettingsSnapshot,
) -> Result<OutboundStatusDto, BusinessError> {
    let first = route
        .stops
        .first()
        .ok_or_else(|| input_error("始発駅がありません"))?;
    let setting = settings
        .outbound_runtimes
        .iter()
        .find(|s| s.dimension == dimension && ids_equal(&s.route_id, &route.route_id))
        .cloned();
    let valid = setting.as_ref().is_some_and(|s| {
        s.runtime_basis == OutboundRuntimeBasis::FirstArrival
            && ids_equal(&s.first_station_id, &first.station_id)
            && s.first_platform_name == first.platform_name
    });
    let duration_label = setting
        .as_ref()
        .map(|s| {
            let seconds = s.runtime.millis() as f64 / 1000.0;
            let whole_seconds = s.runtime.millis() / 1000;
            format!(
                "{seconds}秒（{}分{}秒）",
                whole_seconds / 60,
                (s.runtime.millis() % 60_000) as f64 / 1000.0
            )
        })
        .unwrap_or_else(|| "未測定".into());
    let message = if setting
        .as_ref()
        .is_some_and(|s| s.runtime_basis != OutboundRuntimeBasis::FirstArrival)
    {
        Some("以前の保存値は始発駅の停車時間を含むため、再測定または再入力が必要".into())
    } else if setting.is_some() && !valid {
        Some("現在の路線構成と一致しないため再測定が必要".into())
    } else {
        None
    };
    Ok(OutboundStatusDto {
        setting,
        valid,
        message,
        duration_label,
        first_station_name: first.station_name.clone(),
        first_platform_name: first.platform_name.clone(),
    })
}

fn ids_equal(left: &str, right: &str) -> bool {
    match (MtrId::from_hex(left), MtrId::from_hex(right)) {
        (Ok(a), Ok(b)) => a == b,
        _ => left == right,
    }
}

impl<
    'a,
    P: crate::ListeningPortProvider + ?Sized,
    C: crate::MtrApiClient + ?Sized,
    R: crate::OudiaRepository + ?Sized,
    S: crate::SettingsRepository + ?Sized,
    V: crate::ValidatedSavePort + ?Sized,
> crate::ConversionService<'a, P, C, R, S, V>
{
    fn outbound_context(
        &self,
        id: &crate::SessionId,
        route_id: &str,
    ) -> Result<(u64, crate::MtrEndpoint, u32, MtrRouteSnapshot), BusinessError> {
        self.store.with(id, |s| {
            let snapshot = s
                .snapshot
                .as_ref()
                .ok_or_else(|| input_error("MTRへ接続してください"))?;
            let route = snapshot
                .snapshot
                .routes
                .iter()
                .find(|r| r.route_id == route_id)
                .ok_or_else(|| input_error("路線を選択してください"))?;
            let endpoint = s
                .endpoint
                .clone()
                .ok_or_else(|| input_error("MTRへ接続してください"))?;
            Ok((
                s.revision,
                endpoint,
                snapshot.snapshot.dimension,
                route.clone(),
            ))
        })
    }

    pub fn outbound_status(
        &self,
        id: &crate::SessionId,
        route_id: &str,
    ) -> Result<OutboundStatusDto, BusinessError> {
        let (_, _, dimension, route) = self.outbound_context(id, route_id)?;
        status_for_route(dimension, &route, &self.settings.load()?)
    }

    pub fn save_manual_outbound(
        &self,
        id: &crate::SessionId,
        route_id: &str,
        seconds: i64,
    ) -> Result<OutboundStatusDto, BusinessError> {
        let runtime = OutboundRuntime::from_seconds(seconds)
            .map_err(|_| input_error("出庫時分は非負の秒数で入力してください"))?;
        self.save_manual_outbound_millis(id, route_id, runtime)
    }

    pub fn save_manual_outbound_decimal(
        &self,
        id: &crate::SessionId,
        route_id: &str,
        seconds: f64,
    ) -> Result<OutboundStatusDto, BusinessError> {
        let millis = (seconds * 1000.0).round();
        if !seconds.is_finite() || seconds < 0.0 || millis > 9_007_199_254_740_991.0 {
            return Err(input_error("出庫時分は非負の有限な秒数で入力してください"));
        }
        let runtime =
            OutboundRuntime::new(millis as i64).map_err(|_| input_error("出庫時分が範囲外です"))?;
        self.save_manual_outbound_millis(id, route_id, runtime)
    }

    fn save_manual_outbound_millis(
        &self,
        id: &crate::SessionId,
        route_id: &str,
        runtime: OutboundRuntime,
    ) -> Result<OutboundStatusDto, BusinessError> {
        let (revision, _, dimension, route) = self.outbound_context(id, route_id)?;
        let first = route
            .stops
            .first()
            .ok_or_else(|| input_error("始発駅がありません"))?;
        let measured_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .ok_or_else(|| input_error("現在時刻を取得できません"))?;
        let setting = OutboundRuntimeSetting {
            dimension,
            route_id: MtrId::from_hex(&route.route_id)
                .map_err(|_| input_error("路線IDが不正です"))?
                .to_hex(),
            first_station_id: MtrId::from_hex(&first.station_id)
                .map_err(|_| input_error("始発駅IDが不正です"))?
                .to_hex(),
            first_platform_name: first.platform_name.clone(),
            runtime,
            measured_at,
            source: OutboundRuntimeSource::Manual,
            runtime_basis: OutboundRuntimeBasis::FirstArrival,
        };
        self.persist_outbound(id, revision, &setting)?;
        self.outbound_status(id, route_id)
    }

    pub async fn measure_and_save_outbound(
        &self,
        id: &crate::SessionId,
        route_id: &str,
        depot_clock: &str,
        utc_offset: &str,
    ) -> Result<OutboundStatusDto, BusinessError> {
        let (revision, endpoint, dimension, route) = self.outbound_context(id, route_id)?;
        let setting = measure_outbound_runtime(
            self.client,
            &endpoint,
            dimension,
            &route,
            depot_clock,
            utc_offset,
        )
        .await?;
        self.persist_outbound(id, revision, &setting)?;
        self.outbound_status(id, route_id)
    }

    fn persist_outbound(
        &self,
        id: &crate::SessionId,
        revision: u64,
        setting: &OutboundRuntimeSetting,
    ) -> Result<(), BusinessError> {
        self.store.update(id, |s| {
            if s.revision != revision {
                return Err(BusinessError::new(
                    BusinessErrorKind::StaleState,
                    "測定中に入力が変更されました。再測定してください",
                ));
            }
            self.settings.save_outbound_runtime(setting)?;
            s.previews.clear();
            Ok(())
        })
    }
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
