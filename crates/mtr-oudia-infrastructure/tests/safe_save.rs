use std::fs;

use mtr_oudia_domain::OudiaPatch;
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
fn rejects_changed_input_and_equivalent_paths() {
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

    let input = directory.path().join("again.oud2");
    fs::write(&input, b"FileType=OuDiaSecond.1.16\n").unwrap();
    let equivalent = directory.path().join(".").join("again.oud2");
    assert!(matches!(
        writer.save(
            &input,
            equivalent,
            writer.hash(b"FileType=OuDiaSecond.1.16\n"),
            &OudiaPatch::new(vec![]).unwrap()
        ),
        Err(SafeSaveError::SamePath)
    ));
}
