use mtr_oudia_domain::MtrId;

#[test]
fn hex_and_java_long_have_identical_bit_identity() {
    for (hex, number) in [
        ("0000000000000001", 1),
        ("7FFFFFFFFFFFFFFF", i64::MAX),
        ("8000000000000000", i64::MIN),
        ("FFFFFFFFFFFFFFFF", -1),
    ] {
        let id = MtrId::from_hex(hex).unwrap();
        assert_eq!(id, MtrId::from_java_long(number));
        assert_eq!(id.to_hex(), hex);
    }
    assert_eq!(
        MtrId::from_hex("aBcD").unwrap().to_hex(),
        "000000000000ABCD"
    );
}

#[test]
fn rejects_invalid_or_out_of_range_hex() {
    for value in ["", "-1", "+1", "0x01", " 01", "g", "10000000000000000"] {
        assert!(MtrId::from_hex(value).is_err(), "{value}");
    }
}
