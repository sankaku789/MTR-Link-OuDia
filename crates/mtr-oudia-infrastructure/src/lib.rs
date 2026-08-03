//! Application の Port 実装を置く Infrastructure 層。

use futures_util::StreamExt;
use mtr_oudia_application::DomainPort;
#[cfg(not(target_os = "windows"))]
use mtr_oudia_application::ListeningPortProvider;
use mtr_oudia_application::{
    ApplicationError, MtrApiClient, MtrEndpoint, MtrSnapshotResponse, async_trait,
};
use mtr_oudia_domain::{
    DomainLayer, MtrNetworkSnapshot, MtrRouteSnapshot, MtrStopSnapshot, ServiceTimeMillis,
};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_500);
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[cfg(target_os = "windows")]
mod windows;

/// Windows IP Helper API で待受ポートを得る Provider。
pub struct WindowsListeningPortProvider;

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
    client: reqwest::Client,
}

impl ReqwestMtrApiClient {
    /// redirect を拒否し、MTR API 用の通信上限を設定した client を作る。
    pub fn new() -> Result<Self, ApplicationError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(RESPONSE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            // localhost API を環境・OSの HTTP proxy に転送しない。
            .no_proxy()
            .build()
            .map_err(|error| ApplicationError::Transport {
                message: error.to_string(),
            })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl MtrApiClient for ReqwestMtrApiClient {
    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrSnapshotResponse, ApplicationError> {
        let response = self
            .client
            .get(endpoint.stations_and_routes_url(dimension))
            .send()
            .await
            .map_err(map_reqwest_error)?;
        if !response.status().is_success() {
            return Err(ApplicationError::InvalidResponse {
                reason: format!("HTTP status {}", response.status()),
            });
        }
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
        let body = std::str::from_utf8(&bytes).map_err(|_| ApplicationError::InvalidResponse {
            reason: "UTF-8 JSON ではありません".to_owned(),
        })?;
        parse_mtr_response(body, endpoint, dimension, current_unix_millis()?)
    }
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
