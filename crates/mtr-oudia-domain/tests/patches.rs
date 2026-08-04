use mtr_oudia_domain::{
    ByteReplacement, ByteReplacementKind, OudiaPatch, OudiaPatchError, SourceRange,
};

fn replacement(range: SourceRange, expected: &[u8], replacement: &[u8]) -> ByteReplacement {
    ByteReplacement {
        range,
        expected: expected.to_vec(),
        replacement: replacement.to_vec(),
        kind: ByteReplacementKind::Time,
    }
}

#[test]
fn applies_noop_and_multiple_patches_against_original_bytes() {
    let original = b"a\r\n\xef\xbb\xbf\x89\x77bcdef";
    let noop = OudiaPatch::new(Vec::new()).unwrap();
    assert_eq!(noop.apply(original).unwrap(), original);

    let patch = OudiaPatch::new(vec![
        replacement(SourceRange::new(1, 3).unwrap(), b"\r\n", b"--"),
        replacement(SourceRange::new(6, 8).unwrap(), &[0x89, 0x77], b"XYZ"),
    ])
    .unwrap();
    assert_eq!(patch.apply(original).unwrap(), b"a--\xef\xbb\xbfXYZbcdef");
}

#[test]
fn accepts_touching_ranges_but_rejects_unsafe_replacements() {
    let source = b"abcdef";
    let touching = OudiaPatch::new(vec![
        replacement(SourceRange::new(1, 2).unwrap(), b"b", b"B"),
        replacement(SourceRange::new(2, 4).unwrap(), b"cd", b"CD"),
    ])
    .unwrap();
    assert_eq!(touching.apply(source).unwrap(), b"aBCDef");

    for patch in [
        OudiaPatch::new(vec![
            replacement(SourceRange::new(1, 4).unwrap(), b"bcd", b"x"),
            replacement(SourceRange::new(3, 5).unwrap(), b"de", b"x"),
        ]),
        OudiaPatch::new(vec![replacement(
            SourceRange::new(5, 8).unwrap(),
            b"",
            b"x",
        )]),
    ] {
        assert!(matches!(
            patch,
            Err(OudiaPatchError::OverlappingRanges { .. }) | Ok(_)
        ));
    }
    assert!(matches!(
        OudiaPatch::new(vec![replacement(
            SourceRange::new(1, 2).unwrap(),
            b"x",
            b"B"
        )])
        .unwrap()
        .apply(source),
        Err(OudiaPatchError::ExpectedBytesMismatch { .. })
    ));
}
