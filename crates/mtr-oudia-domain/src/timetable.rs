//! MTR の相対時分から基準運転時分を生成する Domain 処理。

use crate::{DomainError, MtrRouteSnapshot, ServiceTimeMillis};

/// 基準始発駅の固定発時刻。
pub const FIXED_START_TIME: ServiceTimeMillis = ServiceTimeMillis::TEN_OCLOCK;

/// 駅ごとの生成結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedStop {
    pub station_index: usize,
    pub arrival: Option<ServiceTimeMillis>,
    pub departure: Option<ServiceTimeMillis>,
    pub rounded_arrival_seconds: Option<i64>,
    pub rounded_departure_seconds: Option<i64>,
    pub rounded_arrival_display: Option<String>,
    pub rounded_departure_display: Option<String>,
}

/// 基準運転時分の生成結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTimetable {
    pub stops: Vec<GeneratedStop>,
    pub crosses_midnight: bool,
}

/// MTR 正規化路線から時刻表を生成する。
pub fn generate_timetable(route: &MtrRouteSnapshot) -> Result<GeneratedTimetable, DomainError> {
    let dwells = route
        .stops
        .iter()
        .map(|stop| stop.dwell_millis)
        .collect::<Vec<_>>();
    let runs = route.stops[..route.stops.len().saturating_sub(1)]
        .iter()
        .map(|stop| stop.run_millis_to_next)
        .collect::<Option<Vec<_>>>()
        .ok_or(DomainError::InvalidTimetable {
            reason: "駅間運転時分がありません",
        })?;
    generate_timetable_from_durations(&dwells, &runs)
}

/// 停車時分と駅間運転時分から時刻表を生成する。
pub fn generate_timetable_from_durations(
    dwells: &[ServiceTimeMillis],
    runs: &[ServiceTimeMillis],
) -> Result<GeneratedTimetable, DomainError> {
    if dwells.len() < 2 {
        return Err(DomainError::InvalidTimetable {
            reason: "駅数は2以上必要です",
        });
    }
    if runs.len() != dwells.len() - 1 {
        return Err(DomainError::InvalidTimetable {
            reason: "駅間運転時分数が駅数と一致しません",
        });
    }
    let mut current = FIXED_START_TIME;
    let mut stops = Vec::with_capacity(dwells.len());
    stops.push(build_stop(0, None, Some(current))?);
    for index in 1..dwells.len() {
        current = current.checked_add(runs[index - 1])?;
        let arrival = current;
        let departure = if index + 1 == dwells.len() {
            None
        } else {
            current = current.checked_add(dwells[index])?;
            Some(current)
        };
        stops.push(build_stop(index, Some(arrival), departure)?);
    }
    validate_rounded_order(&stops)?;
    let crosses_midnight = stops.iter().any(|stop| {
        stop.arrival.is_some_and(|time| time.millis() >= 86_400_000)
            || stop
                .departure
                .is_some_and(|time| time.millis() >= 86_400_000)
    });
    Ok(GeneratedTimetable {
        stops,
        crosses_midnight,
    })
}

/// 秒を `HH:MM:SS` 形式へ変換する。24時以上もそのまま時間部へ表示する。
pub fn format_preview_time(seconds: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

fn build_stop(
    index: usize,
    arrival: Option<ServiceTimeMillis>,
    departure: Option<ServiceTimeMillis>,
) -> Result<GeneratedStop, DomainError> {
    let rounded_arrival_seconds = arrival
        .map(ServiceTimeMillis::rounded_seconds)
        .transpose()?;
    let rounded_departure_seconds = departure
        .map(ServiceTimeMillis::rounded_seconds)
        .transpose()?;
    Ok(GeneratedStop {
        station_index: index,
        arrival,
        departure,
        rounded_arrival_display: rounded_arrival_seconds.map(format_preview_time),
        rounded_departure_display: rounded_departure_seconds.map(format_preview_time),
        rounded_arrival_seconds,
        rounded_departure_seconds,
    })
}

fn validate_rounded_order(stops: &[GeneratedStop]) -> Result<(), DomainError> {
    let mut previous_departure = None;
    for stop in stops {
        if let (Some(previous), Some(arrival)) = (previous_departure, stop.rounded_arrival_seconds)
            && arrival < previous
        {
            return Err(DomainError::InvalidTimetable {
                reason: "丸め後時刻が逆行しています",
            });
        }
        if let (Some(arrival), Some(departure)) =
            (stop.rounded_arrival_seconds, stop.rounded_departure_seconds)
            && departure < arrival
        {
            return Err(DomainError::InvalidTimetable {
                reason: "丸め後時刻が逆行しています",
            });
        }
        previous_departure = stop.rounded_departure_seconds.or(previous_departure);
    }
    Ok(())
}
