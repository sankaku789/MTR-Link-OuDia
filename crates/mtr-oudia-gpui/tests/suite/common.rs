use mtr_oudia_application::*;

pub(super) fn dummy_snapshot(routes: Vec<(&str, usize)>) -> SnapshotDto {
    SnapshotDto {
        routes: routes
            .into_iter()
            .enumerate()
            .map(|(idx, (name, station_count))| RouteDto {
                id: format!("route-{idx}"),
                name: name.to_string(),
                stations: Vec::new(),
                station_count,
                total_run_millis: 0,
                total_dwell_millis: 0,
            })
            .collect(),
        dimensions: Vec::new(),
        api_current_time_millis: 1_000,
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn dummy_detailed_snapshot(
    routes: Vec<(&str, &str, Vec<(&str, &str, i64, Option<i64>)>)>,
    api_current_time_millis: i64,
) -> SnapshotDto {
    use mtr_oudia_application::RouteStationDto;
    SnapshotDto {
        routes: routes
            .into_iter()
            .map(|(id, name, stations)| {
                let station_count = stations.len();
                let stations = stations
                    .into_iter()
                    .map(
                        |(station_name, platform_name, dwell_millis, run_millis_to_next)| {
                            RouteStationDto {
                                station_name: station_name.to_string(),
                                platform_name: platform_name.to_string(),
                                dwell_millis,
                                run_millis_to_next,
                            }
                        },
                    )
                    .collect();
                RouteDto {
                    id: id.to_string(),
                    name: name.to_string(),
                    stations,
                    station_count,
                    total_run_millis: 120_000,
                    total_dwell_millis: 15_000,
                }
            })
            .collect(),
        dimensions: Vec::new(),
        api_current_time_millis,
    }
}

pub(super) fn dummy_inspection(
    file_type: &str,
    line_name: Option<&str>,
    station_count: usize,
) -> InspectionDto {
    InspectionDto {
        file_type: file_type.to_string(),
        line_name: line_name.map(str::to_string),
        station_count,
        kijun_status: "valid".to_string(),
        diagrams: vec![
            DiagramDto {
                index: 0,
                train_count: 12,
            },
            DiagramDto {
                index: 1,
                train_count: 8,
            },
        ],
        train_types: vec![0, 1],
        train_type_names: vec!["普通".to_string(), "急行".to_string()],
        templates: Vec::new(),
    }
}

pub(super) fn dummy_candidate(
    id: &str,
    diagram_index: usize,
    train_index: usize,
    rank: &str,
    auto_selected: bool,
    matched_stations: usize,
) -> RouteCandidateDto {
    use mtr_oudia_application::StationMappingDto;
    RouteCandidateDto {
        id: CandidateId(id.to_string()),
        diagram_index,
        train_index,
        direction: "kudari".to_string(),
        station_mappings: (0..matched_stations)
            .map(|idx| StationMappingDto {
                mtr_station_index: idx,
                oudia_station_slot: idx,
            })
            .collect(),
        rank: rank.to_string(),
        reasons: vec!["station sequence matched".to_string()],
        auto_selected,
        manual_only: rank == "manual",
    }
}

pub(super) fn dummy_preview(
    id: &str,
    stop_count: usize,
    crosses_midnight: bool,
    operation_present: bool,
    warnings: Vec<&str>,
) -> PreviewDto {
    use mtr_oudia_application::{PreviewId, PreviewStopDto};
    PreviewDto {
        precision_mode: false,
        outbound: None,
        id: PreviewId(id.to_string()),
        fixed_base_time: "10:00:00".to_string(),
        stops: (0..stop_count)
            .map(|idx| PreviewStopDto {
                updated_platform: None,
                station: format!("駅{idx}"),
                existing_arrival: (idx > 0).then(|| format!("09:{:02}:00", idx * 2)),
                existing_departure: (idx + 1 < stop_count).then(|| format!("09:{:02}:30", idx * 2)),
                raw_arrival_millis: (idx > 0).then_some(36_000_000 + (idx as i64) * 120_000),
                raw_departure_millis: (idx + 1 < stop_count)
                    .then_some(36_030_000 + (idx as i64) * 120_000),
                rounded_arrival: (idx > 0).then(|| format!("10:{:02}:00", idx * 2)),
                rounded_departure: (idx + 1 < stop_count).then(|| format!("10:{:02}:30", idx * 2)),
                run_millis: (idx + 1 < stop_count).then_some(90_000),
                dwell_millis: 30_000,
            })
            .collect(),
        warnings: warnings.into_iter().map(str::to_string).collect(),
        crosses_midnight,
        operation_present,
        policy_choices: if operation_present {
            vec!["preserve".to_string(), "remove_target_train".to_string()]
        } else {
            Vec::new()
        },
    }
}

pub(super) fn dummy_save_receipt(output_path: &str, bytes: u64, sha256: &str) -> SaveReceipt {
    SaveReceipt {
        output_path: output_path.to_string(),
        bytes,
        sha256: sha256.to_string(),
    }
}

pub(super) fn dummy_outbound_status(
    valid: bool,
    source: OutboundRuntimeSource,
) -> OutboundStatusDto {
    use mtr_oudia_application::OutboundRuntimeSetting;
    use mtr_oudia_domain::OutboundRuntime;
    OutboundStatusDto {
        setting: Some(OutboundRuntimeSetting {
            runtime_basis: Default::default(),
            dimension: 0,
            route_id: "route-0".to_string(),
            first_station_id: "st-0".to_string(),
            first_platform_name: "1番線".to_string(),
            runtime: OutboundRuntime::from_seconds(15).expect("valid outbound runtime"),
            measured_at: 1_700_000_000_000,
            source,
        }),
        valid,
        message: (!valid).then(|| "再測定が必要です".to_string()),
        duration_label: "15秒（0分15秒）".to_string(),
        first_station_name: "東京".to_string(),
        first_platform_name: "1番線".to_string(),
    }
}

pub(super) fn mapping_pairs(mappings: Option<&[StationMappingDto]>) -> Vec<(usize, usize)> {
    mappings
        .unwrap_or(&[])
        .iter()
        .map(|m| (m.mtr_station_index, m.oudia_station_slot))
        .collect()
}
