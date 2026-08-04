use std::fs;

use mtr_oudia_domain::{ByteReplacement, ByteReplacementKind, OudiaPatch, SourceRange};
use mtr_oudia_infrastructure::{SafeOudiaWriter, SafeSaveError};

#[test]
fn saves_validated_bytes_without_overwriting_input_or_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    let output = directory.path().join("output.oud2");
    let original = b"FileType=OuDiaSecond.1.16\n";
    fs::write(&input, original).unwrap();
    let writer = SafeOudiaWriter::new();

    writer
        .save(
            &input,
            &output,
            writer.hash(original),
            &OudiaPatch::new(vec![]).unwrap(),
        )
        .unwrap();
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(fs::read(&output).unwrap(), original);
    assert_eq!(
        writer.hash(&fs::read(&output).unwrap()),
        writer.hash(original)
    );
    assert!(matches!(
        writer.save(
            &input,
            &output,
            writer.hash(original),
            &OudiaPatch::new(vec![]).unwrap()
        ),
        Err(SafeSaveError::OutputAlreadyExists)
    ));
}

#[test]
fn rejects_changed_input() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    fs::write(&input, b"FileType=OuDiaSecond.1.16\n").unwrap();
    let writer = SafeOudiaWriter::new();
    let expected = writer.hash(b"FileType=OuDiaSecond.1.16\n");
    fs::write(&input, b"FileType=OuDiaSecond.1.17\n").unwrap();
    assert!(matches!(
        writer.save(
            &input,
            directory.path().join("new.oud2"),
            expected,
            &OudiaPatch::new(vec![]).unwrap()
        ),
        Err(SafeSaveError::InputChanged)
    ));
}

#[test]
fn safely_replaces_the_original_file_after_validation() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.oud2");
    let original = b"FileType=OuDiaSecond.1.16\nNote=old\n";
    fs::write(&input, original).unwrap();
    let writer = SafeOudiaWriter::new();
    let start = original
        .windows(3)
        .position(|bytes| bytes == b"old")
        .unwrap();
    let patch = OudiaPatch::new(vec![ByteReplacement {
        range: SourceRange::new(start, start + 3).unwrap(),
        expected: b"old".to_vec(),
        replacement: b"new".to_vec(),
        kind: ByteReplacementKind::Time,
    }])
    .unwrap();

    writer
        .save(&input, &input, writer.hash(original), &patch)
        .unwrap();

    assert_eq!(
        fs::read(&input).unwrap(),
        b"FileType=OuDiaSecond.1.16\nNote=new\n"
    );
}
