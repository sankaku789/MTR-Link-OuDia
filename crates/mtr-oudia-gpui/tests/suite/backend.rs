use super::common::dummy_detailed_snapshot;
use mtr_oudia_application::*;
use mtr_oudia_gpui::backend::*;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn settings_path_from_env_resolves_priority_order() {
    let from_appdata = settings_path_from_env(
        Some(OsString::from("C:\\Users\\test\\AppData\\Roaming")),
        Some(OsString::from("/xdg/config")),
        Some(OsString::from("/home/test")),
    );
    assert_eq!(
        from_appdata,
        PathBuf::from("C:\\Users\\test\\AppData\\Roaming")
            .join("MtrOudiaConverter")
            .join("settings.json")
    );

    let from_xdg = settings_path_from_env(
        None,
        Some(OsString::from("/xdg/config")),
        Some(OsString::from("/home/test")),
    );
    assert_eq!(
        from_xdg,
        PathBuf::from("/xdg/config")
            .join("MtrOudiaConverter")
            .join("settings.json")
    );

    let from_home = settings_path_from_env(
        Some(OsString::from("")),
        Some(OsString::from("")),
        Some(OsString::from("/home/test")),
    );
    assert_eq!(
        from_home,
        PathBuf::from("/home/test")
            .join(".config")
            .join("MtrOudiaConverter")
            .join("settings.json")
    );

    let fallback = settings_path_from_env(None, None, None);
    assert_eq!(
        fallback,
        PathBuf::from(".")
            .join("MtrOudiaConverter")
            .join("settings.json")
    );
}

#[test]
fn minecraft_log_paths_from_env_handles_both_single_and_empty() {
    let both = minecraft_log_paths_from_env(
        Some(OsString::from("/custom/latest.log")),
        Some(OsString::from("/appdata")),
    );
    assert_eq!(
        both,
        vec![
            PathBuf::from("/custom/latest.log"),
            PathBuf::from("/appdata")
                .join(".minecraft")
                .join("logs")
                .join("latest.log"),
        ]
    );

    let custom_only =
        minecraft_log_paths_from_env(Some(OsString::from("/custom/latest.log")), None);
    assert_eq!(custom_only, vec![PathBuf::from("/custom/latest.log")]);

    let appdata_only = minecraft_log_paths_from_env(None, Some(OsString::from("/appdata")));
    assert_eq!(
        appdata_only,
        vec![
            PathBuf::from("/appdata")
                .join(".minecraft")
                .join("logs")
                .join("latest.log"),
        ]
    );

    let empty_filtered =
        minecraft_log_paths_from_env(Some(OsString::from("")), Some(OsString::from("")));
    assert!(empty_filtered.is_empty());
}

#[test]
fn format_business_error_handles_with_and_without_detail() {
    let with_detail = BusinessError {
        kind: BusinessErrorKind::NoEndpoints,
        message: "待受ポートが見つかりません".into(),
        detail: Some("Minecraftログが見つかりません".into()),
    };
    assert_eq!(
        format_business_error(&with_detail),
        (
            "待受ポートが見つかりません".to_string(),
            "Minecraftログが見つかりません".to_string(),
        )
    );

    let without_detail = BusinessError {
        kind: BusinessErrorKind::Connection,
        message: "MTR APIへ接続できません".into(),
        detail: None,
    };
    assert_eq!(
        format_business_error(&without_detail),
        ("MTR APIへ接続できません".to_string(), String::new())
    );
}

#[allow(clippy::type_complexity)]
fn test_snapshot(
    routes: Vec<(&str, &str, Vec<(&str, &str, i64, Option<i64>)>)>,
    api_current_time_millis: i64,
) -> SnapshotDto {
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
                    total_run_millis: 0,
                    total_dwell_millis: 0,
                }
            })
            .collect(),
        dimensions: Vec::new(),
        api_current_time_millis,
    }
}

#[test]
fn poll_snapshot_until_changed_commits_only_when_routes_change() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("tokio runtime");

    let previous = test_snapshot(
        vec![("r1", "山手線", vec![("東京", "1", 10_000, Some(120_000))])],
        1_000,
    );

    // 2 回目は同一路線データ（時刻のみ更新）、3 回目で路線名変化 → 更新される。
    let mut probe_calls = 0usize;
    let mut commit_calls = 0usize;
    let updated_outcome = rt
        .block_on(poll_snapshot_until_changed(
            &previous,
            Duration::from_millis(5),
            Duration::from_millis(200),
            || {
                probe_calls += 1;
                let snapshot = if probe_calls < 3 {
                    test_snapshot(
                        vec![("r1", "山手線", vec![("東京", "1", 10_000, Some(120_000))])],
                        1_000 + (probe_calls as i64) * 2_000,
                    )
                } else {
                    test_snapshot(
                        vec![(
                            "r1",
                            "山手線（外回り）",
                            vec![("東京", "1", 10_000, Some(120_000))],
                        )],
                        9_000,
                    )
                };
                async move { Ok(snapshot) }
            },
            || {
                commit_calls += 1;
                let committed = test_snapshot(
                    vec![(
                        "r1",
                        "山手線（外回り）",
                        vec![("東京", "1", 10_000, Some(120_000))],
                    )],
                    9_100,
                );
                async move { Ok(committed) }
            },
            || false,
        ))
        .expect("polling should succeed");
    assert_eq!(probe_calls, 3);
    assert_eq!(commit_calls, 1);
    assert!(matches!(
        updated_outcome,
        RefreshSnapshotOutcome::Updated(dto) if dto.routes[0].name == "山手線（外回り）"
    ));

    // 最大待機時間まで変化なしの場合は Unchanged を返し、本セッションはコミットしない。
    let mut unchanged_probes = 0usize;
    let mut unchanged_commits = 0usize;
    let unchanged_outcome = rt
        .block_on(poll_snapshot_until_changed(
            &previous,
            Duration::from_millis(10),
            Duration::from_millis(35),
            || {
                unchanged_probes += 1;
                let snapshot = test_snapshot(
                    vec![("r1", "山手線", vec![("東京", "1", 10_000, Some(120_000))])],
                    2_000 + (unchanged_probes as i64) * 1_000,
                );
                async move { Ok(snapshot) }
            },
            || {
                unchanged_commits += 1;
                async move { unreachable!("commit must not run when unchanged") }
            },
            || false,
        ))
        .expect("unchanged polling should succeed");
    assert!(unchanged_probes >= 2);
    assert_eq!(unchanged_commits, 0);
    assert!(matches!(
        unchanged_outcome,
        RefreshSnapshotOutcome::Unchanged
    ));
}

#[test]
fn snapshot_routes_changed_detects_route_and_station_differences() {
    let base = dummy_detailed_snapshot(
        vec![(
            "r1",
            "山手線",
            vec![
                ("東京", "1番線", 15_000, Some(120_000)),
                ("神田", "2番線", 10_000, None),
            ],
        )],
        1_000,
    );

    // api_current_time_millis のみが異なる同一データは false
    let same_routes_later = dummy_detailed_snapshot(
        vec![(
            "r1",
            "山手線",
            vec![
                ("東京", "1番線", 15_000, Some(120_000)),
                ("神田", "2番線", 10_000, None),
            ],
        )],
        99_999,
    );
    assert!(!snapshot_routes_changed(&base, &same_routes_later));

    // 路線数の変化で true
    let added_route = dummy_detailed_snapshot(
        vec![
            (
                "r1",
                "山手線",
                vec![
                    ("東京", "1番線", 15_000, Some(120_000)),
                    ("神田", "2番線", 10_000, None),
                ],
            ),
            ("r2", "京浜東北線", vec![("東京", "3番線", 10_000, None)]),
        ],
        2_000,
    );
    assert!(snapshot_routes_changed(&base, &added_route));

    // 路線名の変化で true
    let renamed_route = dummy_detailed_snapshot(
        vec![(
            "r1",
            "山手線（内回り）",
            vec![
                ("東京", "1番線", 15_000, Some(120_000)),
                ("神田", "2番線", 10_000, None),
            ],
        )],
        2_000,
    );
    assert!(snapshot_routes_changed(&base, &renamed_route));

    // 駅構成（駅名・ホーム・駅数・時分）の変化で true
    let changed_station = dummy_detailed_snapshot(
        vec![(
            "r1",
            "山手線",
            vec![
                ("東京", "1番線", 15_000, Some(120_000)),
                ("秋葉原", "2番線", 10_000, None),
            ],
        )],
        2_000,
    );
    assert!(snapshot_routes_changed(&base, &changed_station));

    let changed_station_count = dummy_detailed_snapshot(
        vec![(
            "r1",
            "山手線",
            vec![
                ("東京", "1番線", 15_000, Some(120_000)),
                ("神田", "2番線", 10_000, Some(90_000)),
                ("秋葉原", "1番線", 10_000, None),
            ],
        )],
        2_000,
    );
    assert!(snapshot_routes_changed(&base, &changed_station_count));
}
