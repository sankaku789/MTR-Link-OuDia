use mtr_oudia_domain::OutboundRuntime;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
