use mtr_oudia_application::{ApplicationError, MtrEndpoint, ObaArrivalDto, ObaArrivalsDto};
use mtr_oudia_domain::MtrId;
use serde::Deserialize;

#[derive(Deserialize)]
struct MapResponse {
    code: u32,
    data: MapData,
}
#[derive(Deserialize)]
struct MapData {
    routes: Vec<MapRoute>,
}
#[derive(Deserialize)]
struct MapRoute {
    id: String,
    name: String,
    number: String,
    color: u32,
    stations: Vec<MapStop>,
}
#[derive(Deserialize)]
struct MapStop {
    id: String,
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObaResponse {
    code: u32,
    current_time: i64,
    data: ObaData,
}
#[derive(Deserialize)]
struct ObaData {
    entry: ObaEntry,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObaEntry {
    stop_id: String,
    arrivals_and_departures: Vec<ObaArrival>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObaArrival {
    route_id: String,
    route_long_name: String,
    route_short_name: String,
    stop_id: String,
    stop_sequence: u32,
    block_trip_sequence: u32,
    scheduled_arrival_time: i64,
    scheduled_departure_time: i64,
}

fn invalid(reason: &str) -> ApplicationError {
    ApplicationError::InvalidResponse {
        reason: reason.into(),
    }
}
fn format_name(name: &str) -> String {
    name.split("||")
        .next()
        .unwrap_or_default()
        .replace('|', " ")
}
fn safe_timestamp(value: i64) -> bool {
    (0..=9_007_199_254_740_991).contains(&value)
}

/// OBAの色・変換後の名前に加え、始発駅・ホームでMTR路線を一意に照合する。
pub fn parse_oba_arrivals_response(
    map_body: &str,
    oba_body: &str,
    route_id: &str,
    platform_id: &str,
) -> Result<ObaArrivalsDto, ApplicationError> {
    let map: MapResponse =
        serde_json::from_str(map_body).map_err(|_| invalid("MTR路線情報の形式が不正です"))?;
    let oba: ObaResponse =
        serde_json::from_str(oba_body).map_err(|_| invalid("OBA予定の形式が不正です"))?;
    if map.code != 200 || oba.code != 200 || !safe_timestamp(oba.current_time) {
        return Err(invalid("MTR/OBA API応答が不正です"));
    }
    let target_id = MtrId::from_hex(route_id).map_err(|_| invalid("路線IDが不正です"))?;
    let platform = MtrId::from_hex(platform_id).map_err(|_| invalid("ホームIDが不正です"))?;
    let mut targets = map
        .data
        .routes
        .iter()
        .filter(|r| MtrId::from_hex(&r.id).ok() == Some(target_id));
    let target = targets
        .next()
        .ok_or_else(|| invalid("対象MTR路線が見つかりません"))?;
    if targets.next().is_some() || target.color > 0xFFFFFF {
        return Err(invalid("対象MTR路線情報が曖昧です"));
    }
    let color = format!("{:06X}", target.color);
    let long_name = format_name(&target.name);
    let short_name = format_name(&target.number);
    let first = target
        .stations
        .first()
        .ok_or_else(|| invalid("対象路線の始発駅がありません"))?;
    let first_station = MtrId::from_hex(&first.id).map_err(|_| invalid("始発駅IDが不正です"))?;
    if first.name.is_empty() {
        return Err(invalid("対象路線の始発ホーム情報がありません"));
    }
    let mut matches = 0;
    for r in &map.data.routes {
        if r.color != target.color
            || format_name(&r.name) != long_name
            || format_name(&r.number) != short_name
        {
            continue;
        }
        if let Some(other_first) = r.stations.first() {
            let station = MtrId::from_hex(&other_first.id)
                .map_err(|_| invalid("照合対象路線の始発駅IDが不正です"))?;
            if station == first_station && other_first.name == first.name {
                matches += 1;
            }
        }
    }
    if matches != 1 {
        return Err(invalid(
            "OBAの色・路線名・種別名・始発駅・ホームが同じ別路線があるため、対象路線を一意に識別できません",
        ));
    }
    if MtrId::from_hex(&oba.data.entry.stop_id).ok() != Some(platform) {
        return Err(invalid("OBA応答のホームが要求と一致しません"));
    }
    let mut arrivals = Vec::new();
    for a in oba.data.entry.arrivals_and_departures {
        let stop = MtrId::from_hex(&a.stop_id).map_err(|_| invalid("OBAホームIDが不正です"))?;
        if stop != platform
            || !safe_timestamp(a.scheduled_arrival_time)
            || !safe_timestamp(a.scheduled_departure_time)
            || a.scheduled_departure_time < a.scheduled_arrival_time
        {
            return Err(invalid("OBA予定時刻・ホーム情報が不正です"));
        }
        if a.route_id == color
            && a.route_long_name == long_name
            && a.route_short_name == short_name
            && a.stop_sequence == 0
        {
            arrivals.push(ObaArrivalDto {
                route_id: target_id,
                platform_id: stop,
                stop_sequence: a.stop_sequence,
                block_trip_sequence: a.block_trip_sequence,
                arrival: a.scheduled_arrival_time,
                departure: a.scheduled_departure_time,
            });
        }
    }
    Ok(ObaArrivalsDto {
        current_time_millis: oba.current_time,
        arrivals,
    })
}

pub(crate) async fn fetch_oba_arrivals(
    client: &reqwest::Client,
    endpoint: &MtrEndpoint,
    dimension: u32,
    route_id: &str,
    platform_id: &str,
) -> Result<ObaArrivalsDto, ApplicationError> {
    let platform = MtrId::from_hex(platform_id).map_err(|_| invalid("ホームIDが不正です"))?;
    let (status, map_body) = super::fetch_response(client, endpoint, dimension).await?;
    if !status.is_success() {
        return Err(invalid("MTR路線情報のHTTP取得に失敗しました"));
    }
    let mut url = endpoint.as_url().clone();
    url.set_path(&format!(
        "/oba/api/where/arrivals-and-departures-for-stop/{}",
        platform.to_hex()
    ));
    // 過ぎた車庫発は翌日へ送る。翌日の遅い時刻の日跨ぎ到着も漏らさないよう未来2日を取得。
    url.query_pairs_mut()
        .append_pair("dimension", &dimension.to_string())
        .append_pair("minutesBefore", "1440")
        .append_pair("minutesAfter", "2880");
    let response = client
        .get(url)
        .send()
        .await
        .map_err(super::map_reqwest_error)?;
    let (status, body) = super::read_http_response(response).await?;
    if !status.is_success() {
        return Err(invalid("OBA予定のHTTP取得に失敗しました"));
    }
    parse_oba_arrivals_response(&map_body, &body, route_id, platform_id)
}
