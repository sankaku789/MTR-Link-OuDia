use mtr_oudia_domain::*;

#[test]
fn specified_start_time_is_used_without_changing_normal_mode() {
    let zero = ServiceTimeMillis::new(0).unwrap();
    let run = ServiceTimeMillis::new(30_000).unwrap();
    let start = ServiceTimeMillis::new(29_730_000).unwrap();
    let precise = generate_timetable_from_durations_at(&[zero, zero], &[run], start).unwrap();
    assert_eq!(precise.stops[0].rounded_departure_display.as_deref(), Some("08:15:30"));
    assert_eq!(precise.stops[1].rounded_arrival_display.as_deref(), Some("08:16:00"));
    let normal = generate_timetable_from_durations(&[zero, zero], &[run]).unwrap();
    assert_eq!(normal.stops[0].rounded_departure_display.as_deref(), Some("10:00:00"));
}

#[test]
fn precision_preserves_start_and_resolves_platform_not_numeric_index() {
    let source = parse_oudia(b"FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nEki.\nEkimei=A\nEkiTrack2Cont.\nEkiTrack2.\nTrackName=2\nTrackRyakusyou=2\n.\nEkiTrack2.\nTrackName=1\nTrackRyakusyou=1\n.\n.\n.\nEki.\nEkimei=B\nEkiTrack2Cont.\nEkiTrack2.\nTrackRyakusyou=1\n.\n.\n.\nDia.\nKudari.\nRessya.\nRessyabangou=001\nEkiJikoku=1;081530$0,1;081600/\n.\n.\n.\n".to_vec()).unwrap();
    let ReferenceDiagramSelection::Selected(templates) = build_oudia_route_templates(&source.document) else { panic!() };
    let tracks = build_platform_patch(&source, &templates.templates[0], &[vec![0], vec![1]], &["1".into(), "1".into()]).unwrap();
    let saved = parse_oudia(tracks.apply(&source.bytes).unwrap()).unwrap();
    let cells = &saved.document.diagrams[0].trains[0].eki_jikoku.cells;
    assert_eq!(cells[0].track_index, Some(1));
    assert_eq!(cells[1].track_index, Some(0));
    assert_eq!(cells[0].departure.unwrap().hour, 8);
    assert!(saved.bytes.windows(b"TrackName=2".len()).any(|bytes| bytes == b"TrackName=2"));
    assert!(build_platform_patch(&source, &templates.templates[0], &[vec![0], vec![1]], &["9".into(), "1".into()]).is_err());
    let duplicate = parse_oudia(String::from_utf8(source.bytes.clone()).unwrap().replace("TrackRyakusyou=2", "TrackRyakusyou=1").into_bytes()).unwrap();
    assert!(build_platform_patch(&duplicate, &templates.templates[0], &[vec![0], vec![1]], &["1".into(), "1".into()]).is_err());
    let zero = ServiceTimeMillis::new(0).unwrap();
    let run = ServiceTimeMillis::new(30_000).unwrap();
    let normal = generate_timetable_from_durations(&[zero, zero], &[run]).unwrap();
    let normal_patch = build_eki_jikoku_patch(&source, &templates.templates[0], &normal, OperationPolicy::Preserve).unwrap();
    let normal_saved = parse_oudia(normal_patch.apply(&source.bytes).unwrap()).unwrap();
    assert_eq!(normal_saved.document.diagrams[0].trains[0].eki_jikoku.cells[0].track_index, Some(0));
    assert_eq!(normal_saved.document.diagrams[0].trains[0].eki_jikoku.cells[1].track_index, None);
    let precise = generate_timetable_from_durations_at(&[zero, zero], &[run], ServiceTimeMillis::new(29_730_000).unwrap()).unwrap();
    let outbound = build_conversion_patch(&source, &templates.templates[0], &precise, &[vec![0], vec![1]], OperationPolicy::Preserve, Some(OutboundRuntime::from_seconds(107).unwrap()), zero).unwrap();
    let combined = OudiaPatch::new(outbound.replacements().iter().chain(tracks.replacements()).cloned().collect()).unwrap();
    let bytes = combined.apply(&source.bytes).unwrap();
    assert!(String::from_utf8(bytes).unwrap().contains("Operation0B=3/081343$/"));
}

#[test]
fn reverse_direction_and_untimed_cells_compose_time_before_platform() {
    let stations = ["A", "B", "C"].iter().map(|name| format!("Eki.\nEkimei={name}\nEkiTrack2Cont.\nEkiTrack2.\nTrackRyakusyou={name}\n.\n.\n.\n")).collect::<String>();
    let original = format!("FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\n{stations}Dia.\nNobori.\nRessya.\nEkiJikoku=1;081530,1,1;081600/\n.\n.\n.\n");
    let source = parse_oudia(original.into_bytes()).unwrap();
    let ReferenceDiagramSelection::Selected(templates) = build_oudia_route_templates(&source.document) else { panic!() };
    let template = &templates.templates[0];
    let groups = [vec![2], vec![1], vec![0]];
    let tracks = build_platform_patch(&source, template, &groups, &["C".into(), "B".into(), "A".into()]).unwrap();
    let zero = ServiceTimeMillis::new(0).unwrap();
    let run = ServiceTimeMillis::new(1_000).unwrap();
    let timetable = generate_timetable_from_durations_at(&[zero, zero, zero], &[run, run], ServiceTimeMillis::new(29_730_000).unwrap()).unwrap();
    let times = build_eki_jikoku_patch_with_groups(&source, template, &timetable, &groups, OperationPolicy::Preserve).unwrap();
    let combined = OudiaPatch::new(times.replacements().iter().chain(tracks.replacements()).cloned().collect()).unwrap();
    let saved = parse_oudia(combined.apply(&source.bytes).unwrap()).unwrap();
    let cells = &saved.document.diagrams[0].trains[0].eki_jikoku.cells;
    assert!(cells.iter().all(|cell| cell.track_index == Some(0)));
    assert_eq!(cells[1].arrival.unwrap().second, 31);
    assert_eq!(cells[1].departure.unwrap().second, 31);
}
