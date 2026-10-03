use mtr_oudia_application::{ApplicationError, ArrivalDto, ArrivalsDto, MtrEndpoint};
use mtr_oudia_domain::MtrId;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireArrivals {
    current_time: i64,
    #[serde(default)]
    arrivals: Vec<WireArrival>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireArrival {
    route_id: i64,
    platform_id: i64,
    platform_name: String,
    arrival: i64,
    departure: i64,
    deviation: i64,
    realtime: bool,
    departure_index: i64,
}

pub fn parse_arrivals_response(body: &str) -> Result<ArrivalsDto, ApplicationError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| invalid("arrivals JSONが不正です"))?;
    if value
        .get("status")
        .is_some_and(|status| status.as_i64() != Some(200))
    {
        return Err(invalid("arrivals API statusが200ではありません"));
    }
    let data = value.get("data").unwrap_or(&value);
    let wire: WireArrivals =
        serde_json::from_value(data.clone()).map_err(|_| invalid("arrivals schemaが不正です"))?;
    if wire.current_time < 0 {
        return Err(invalid("currentTimeが負数です"));
    }
    let arrivals = wire
        .arrivals
        .into_iter()
        .map(|a| {
            if a.arrival < 0 || a.departure < a.arrival || a.departure_index < 0 {
                return Err(invalid("arrivals時刻・識別情報が不正です"));
            }
            Ok(ArrivalDto {
                route_id: MtrId::from_java_long(a.route_id),
                platform_id: MtrId::from_java_long(a.platform_id),
                platform_name: a.platform_name,
                arrival: a.arrival,
                departure: a.departure,
                deviation: a.deviation,
                realtime: a.realtime,
                departure_index: a.departure_index,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ArrivalsDto {
        current_time_millis: wire.current_time,
        arrivals,
    })
}

pub(crate) async fn fetch_arrivals(
    client: &reqwest::Client,
    endpoint: &MtrEndpoint,
    dimension: u32,
    station_id: &str,
) -> Result<ArrivalsDto, ApplicationError> {
    let station = MtrId::from_hex(station_id).map_err(|_| invalid("station hex IDが不正です"))?;
    let mut url = endpoint.as_url().clone();
    url.set_path("/mtr/api/map/arrivals");
    url.query_pairs_mut()
        .append_pair("dimension", &dimension.to_string());
    // Java integerの最大値を指定して候補を任意の小さい件数で切り捨てない。
    // 全体上限0はCore仕様の「全件」。HTTP byte上限/timeoutは既存制約を共有する。
    let body = serde_json::json!({"stationIdsHex": [station.to_hex()], "maxCountPerPlatform": i32::MAX, "maxCountTotal": 0});
    let response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(super::map_reqwest_error)?;
    let (status, body) = super::read_http_response(response).await?;
    if !status.is_success() {
        return Err(invalid(&format!("arrivals HTTP status: {status}")));
    }
    parse_arrivals_response(&body)
}

fn invalid(reason: &str) -> ApplicationError {
    ApplicationError::InvalidResponse {
        reason: reason.into(),
    }
}
