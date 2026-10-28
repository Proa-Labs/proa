// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use const_json::const_json;

#[test]
fn literal_is_const_and_preserves_json_types_and_escaping() {
    const DOCUMENT: &str = const_json!({
        "quoted": "Line\n\"quoted\"\\path",
        "unicode": "é 🚀",
        "raw": r#"{"literal":true}"#,
        "values": [true, false, null, -42, 1.25, 1e3, {"nested": []}],
    });
    let value: serde_json::Value = serde_json::from_str(DOCUMENT).unwrap();
    assert_eq!(value["quoted"], "Line\n\"quoted\"\\path");
    assert_eq!(value["unicode"], "é 🚀");
    assert_eq!(value["raw"], r#"{"literal":true}"#);
    assert_eq!(
        value["values"],
        serde_json::json!([true, false, null, -42, 1.25, 1000.0, {"nested": []}])
    );
}

#[test]
fn literal_supports_scalar_roots_and_integer_boundaries() {
    const NUMBER: &str = const_json!(18446744073709551615);
    const NEGATIVE: &str = const_json!(-9223372036854775808);
    const NEGATIVE_ZERO: &str = const_json!(-0);
    const TEXT: &str = const_json!("hello");
    const NULL: &str = const_json!(null);
    assert_eq!(serde_json::from_str::<u64>(NUMBER).unwrap(), u64::MAX);
    assert_eq!(serde_json::from_str::<i64>(NEGATIVE).unwrap(), i64::MIN);
    let zero: f64 = serde_json::from_str(NEGATIVE_ZERO).unwrap();
    assert_eq!(zero, 0.0);
    assert!(zero.is_sign_negative());
    assert_eq!(TEXT, "\"hello\"");
    assert_eq!(NULL, "null");
}
