use mtr_oudia_domain::{
    DomainError, KijunDiaIndex, LineEnding, OudiaDirection, TextEncoding, parse_oudia,
};

const SIMPLE_116: &[u8] = include_bytes!("../../../fixtures/oudia/filetype-1.16/simple.oud2");
const SIMPLE_117: &[u8] = include_bytes!("../../../fixtures/oudia/filetype-1.17/simple.oud2");

#[test]
fn preserves_utf8_lf_input_bytes_and_extracts_the_reference_train() {
    let source = parse_oudia(SIMPLE_116.to_vec()).unwrap();

    assert_eq!(source.encoding, TextEncoding::Utf8);
    assert!(!source.bom);
    assert_eq!(source.line_ending, LineEnding::Lf);
    assert_eq!(source.unchanged_bytes(), SIMPLE_116);
    assert_eq!(source.document.file_type, "OuDiaSecond.1.16");
    assert_eq!(source.document.kijun_dia_index, KijunDiaIndex::Valid(0));
    assert_eq!(source.document.diagrams.len(), 1);
    let train = &source.document.diagrams[0].trains[0];
    assert_eq!(train.direction, OudiaDirection::Kudari);
    assert_eq!(train.eki_jikoku.cells.len(), 3);
    assert!(train.eki_jikoku.cells[1].is_empty());
    assert_eq!(train.eki_jikoku.cells[0].handling_code, Some(1));
    assert_eq!(train.eki_jikoku.cells[0].track_index, Some(1));
}

#[test]
fn accepts_compact_departure_and_arrival_departure_times_with_tracks() {
    let source = parse_oudia(
        b"FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\nDia.\nKudari.\nRessya.\nEkiJikoku=1;1000$4,2$1,1;100236/100253$2,1;100753/$1\n.\n.\n.\n"
            .to_vec(),
    )
    .unwrap();
    let cells = &source.document.diagrams[0].trains[0].eki_jikoku.cells;

    assert_eq!(cells[0].arrival, None);
    assert_eq!(cells[0].departure.unwrap().hour, 10);
    assert_eq!(cells[0].departure.unwrap().minute, 0);
    assert_eq!(cells[0].track_index, Some(4));
    assert_eq!(cells[1].handling_code, Some(2));
    assert_eq!(cells[1].arrival, None);
    assert_eq!(cells[1].departure, None);
    assert_eq!(cells[1].track_index, Some(1));
    assert_eq!(cells[2].arrival.unwrap().second, 36);
    assert_eq!(cells[2].departure.unwrap().second, 53);
    assert_eq!(cells[3].arrival.unwrap().minute, 7);
    assert_eq!(cells[3].departure, None);
}

#[test]
fn preserves_crlf_and_multibyte_property_byte_ranges() {
    let source = parse_oudia(SIMPLE_117.to_vec()).unwrap();
    let property = source
        .document
        .properties
        .iter()
        .find(|property| property.key == "FileType")
        .unwrap();

    assert_eq!(source.line_ending, LineEnding::CrLf);
    assert_eq!(
        &source.bytes[property.value_range.start()..property.value_range.end()],
        b"OuDiaSecond.1.17"
    );
    assert_eq!(
        source.document.diagrams[0].trains[0].direction,
        OudiaDirection::Nobori
    );
}

#[test]
fn handles_utf8_bom_and_keeps_unknown_records_in_order() {
    let bom_source =
        parse_oudia(include_bytes!("../../../fixtures/oudia/utf8-bom.oud2").to_vec()).unwrap();
    let unknown_source =
        parse_oudia(include_bytes!("../../../fixtures/oudia/unknown-fields.oud2").to_vec())
            .unwrap();

    assert!(bom_source.bom);
    assert_eq!(
        unknown_source.document.kijun_dia_index,
        KijunDiaIndex::Invalid
    );
    assert_eq!(unknown_source.document.sections[0].name, "Future");
    assert_eq!(
        unknown_source.document.unknown_lines[0].raw,
        "unknown raw line"
    );
    assert_eq!(unknown_source.unchanged_bytes(), unknown_source.bytes);
}

#[test]
fn falls_back_to_cp932_only_after_utf8_strict_decode_fails() {
    let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode("FileType=OuDiaSecond.1.16\n備考=駅\n");
    let source = parse_oudia(encoded.into_owned()).unwrap();

    assert_eq!(source.encoding, TextEncoding::Windows31J);
    let property = &source.document.properties[1];
    assert_eq!(property.value, "駅");
    assert_eq!(
        &source.bytes[property.value_range.start()..property.value_range.end()],
        &[0x89, 0x77]
    );
}

#[test]
fn retains_manual_selection_state_for_missing_or_out_of_range_kijun_dia_index() {
    let missing = parse_oudia(b"FileType=OuDiaSecond.1.16\n".to_vec()).unwrap();
    let out_of_range =
        parse_oudia(b"FileType=OuDiaSecond.1.16\nKijunDiaIndex=0\n".to_vec()).unwrap();

    assert_eq!(missing.document.kijun_dia_index, KijunDiaIndex::Missing);
    assert_eq!(
        out_of_range.document.kijun_dia_index,
        KijunDiaIndex::OutOfRange { index: 0 }
    );
}

#[test]
fn rejects_unsafe_or_unsupported_input() {
    for fixture in [
        include_bytes!("../../../fixtures/oudia/invalid/unknown-eki-jikoku.oud2").as_slice(),
        include_bytes!("../../../fixtures/oudia/invalid/broken-section.oud2").as_slice(),
        include_bytes!("../../../fixtures/oudia/invalid/unsupported-filetype.oud2").as_slice(),
    ] {
        assert!(parse_oudia(fixture.to_vec()).is_err());
    }
    assert!(matches!(
        parse_oudia(vec![0xff, 0xfe, 0x00, 0x00]),
        Err(DomainError::UnsupportedEncoding { .. })
    ));
}
