// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for `#[rsjs::client]`.
//!
//! Positive tests in this file expand the macro through the real
//! proc-macro pipeline (not via the in-crate unit tests, which only
//! check token-level shapes). They verify:
//!
//! 1. The generated metadata struct exists and exposes the documented
//!    consts (`NAME`, `PARAMS`, `RETURN_TY`, `BODY_HIR_VERSION`,
//!    `BODY_HIR_JSON`, `BODY_IR_VERSION`, `BODY_IR_JSON`).
//! 2. The generated server-side function delegates to
//!    `rsjs::__runtime::run_client_helper`, which evaluates the
//!    lowered primitive helper body against captured arguments.
//!
//! Negative cases (closure body, async body, `cx` reference,
//! unrecognised method call) are covered by `compile_fail` doctests
//! on the `rsjs::client` re-export so they exercise the real macro at
//! `cargo test` time without needing a trybuild dev-dep.

#[rsjs::client]
pub fn subtotal(price: i64, qty: i64) -> i64 {
    price * qty
}

#[rsjs::client]
pub fn remainder(value: i64, divisor: i64) -> i64 {
    value % divisor
}

#[rsjs::client]
pub fn adjusted_subtotal(price: i64, qty: i64) -> i64 {
    let subtotal = price * qty;
    subtotal + 5
}

#[rsjs::client]
pub fn is_filled(s: String) -> bool {
    !s.is_empty()
}

#[rsjs::client]
pub fn is_blank(s: String) -> bool {
    s.is_empty()
}

#[rsjs::client]
pub fn label_has_seven_bytes(s: String) -> bool {
    s.len() == 7
}

#[rsjs::client]
pub fn lower_label(s: String) -> String {
    s.to_lowercase()
}

#[rsjs::client]
pub fn label_contains(s: String, needle: &'static str) -> bool {
    s.contains(needle)
}

#[rsjs::client]
pub fn discounted(price: i64) -> i64 {
    if price > 100 {
        price - 10
    } else {
        price
    }
}

#[rsjs::client]
pub fn invert(flag: bool) -> bool {
    !flag
}

#[rsjs::client]
pub fn negative(value: i64) -> i64 {
    -value
}

#[rsjs::client]
pub fn price_label(price: i64) -> String {
    if price > 100 {
        return "premium".to_string();
    }
    "standard".to_string()
}

#[rsjs::client]
pub fn status_label(status: i64) -> String {
    match status {
        0 => "new".to_string(),
        1 => "ready".to_string(),
        2..=4 => "busy".to_string(),
        _ => "unknown".to_string(),
    }
}

#[rsjs::client]
pub fn optional_label(value: Option<&'static str>) -> String {
    if value.is_some() {
        "set".to_string()
    } else {
        "missing".to_string()
    }
}

#[rsjs::client]
pub fn optional_missing(value: Option<&'static str>) -> bool {
    value.is_none()
}

#[rsjs::client]
pub fn maybe_discount(price: i64, enabled: bool) -> Option<i64> {
    if enabled {
        Some(price - 10)
    } else {
        None
    }
}

#[rsjs::client]
pub fn has_three_labels(labels: Vec<&'static str>) -> bool {
    labels.len() == 3
}

#[rsjs::client]
pub fn has_any_score(scores: &[i64]) -> bool {
    !scores.is_empty()
}

#[rsjs::client]
pub fn no_optional_scores(scores: Vec<Option<i64>>) -> bool {
    scores.is_empty()
}

#[rsjs::client]
pub fn echo_scores(scores: Vec<i64>) -> Vec<i64> {
    scores
}

#[rsjs::client]
pub fn trim_label(s: String) -> String {
    s.trim().to_string()
}

#[rsjs::client]
pub fn adjust_local(value: i32, increase: bool) -> i32 {
    let mut result: i32 = value;
    if increase {
        result += 2_i32;
    } else {
        result -= 2_i32;
    }
    result
}

#[rsjs::client]
#[allow(unused_assignments)]
pub fn reassign_unannotated_bool() -> bool {
    let mut value = false;
    value = true;
    value
}

#[rsjs::client]
#[allow(unused_assignments)]
pub fn reassign_unannotated_i32() -> i32 {
    let mut value = 0;
    value = 2;
    value = 5;
    value
}

#[rsjs::client]
#[allow(unused_assignments)]
pub fn compound_assign_unannotated_i32() -> i32 {
    let mut value = 0_i32;
    value += 5_i32;
    value
}

#[rsjs::client]
#[allow(unused_assignments)]
pub fn reassign_unannotated_string() -> String {
    let mut value = "a".to_string();
    value = "b".to_string();
    value
}

#[rsjs::client]
#[allow(unused_assignments)]
pub fn reassign_unannotated_f64() -> f64 {
    let mut value = 1.0;
    value = 2.0;
    value
}

struct MenuController {
    open: bool,
    label: &'static str,
}

#[rsjs::client]
pub fn toggle_menu_value_object() -> bool {
    let mut menu = MenuController {
        open: false,
        label: "Menu",
    };
    menu.open = !menu.open;
    menu.open && menu.label == "Menu"
}

#[rsjs::client]
pub fn open_menu_when(enabled: bool) -> bool {
    let mut menu = MenuController {
        open: false,
        label: "Menu",
    };
    if enabled {
        menu.open = true;
        return menu.open;
    }
    menu.open
}

struct CounterValue {
    count: i32,
}

#[rsjs::client]
pub fn increment_counter_value_object() -> i32 {
    let mut counter = CounterValue { count: 0_i32 };
    counter.count += 1;
    counter.count
}

#[rsjs::client]
pub mod menu_object_methods {
    pub struct MenuController {
        open: bool,
    }

    #[derive(Default)]
    pub struct CounterController {
        count: i32,
    }

    #[derive(Clone, Copy)]
    pub struct Point {
        x: i32,
    }

    impl MenuController {
        pub fn new(open: bool) -> Self {
            Self { open }
        }

        pub fn toggle(&mut self) {
            self.open = !self.open;
        }

        pub fn is_open(&self) -> bool {
            self.open
        }
    }

    impl CounterController {
        pub fn new() -> Self {
            Self { count: 0_i32 }
        }

        pub fn adjust(&mut self, enabled: bool, amount: i32) {
            let mut step: i32 = amount;
            step += 1_i32;
            if enabled {
                self.count += step;
            } else {
                self.count -= step;
            }
        }

        pub fn value(&self) -> i32 {
            self.count
        }

        pub fn projected(&self, offset: i32, double: bool) -> i32 {
            let mut value: i32 = self.count + offset;
            if double {
                value *= 2_i32;
            }
            value
        }
    }

    impl Point {
        pub fn new(x: i32) -> Self {
            Self { x }
        }

        pub fn set_x(&mut self, x: i32) {
            self.x = x;
        }

        pub fn x(&self) -> i32 {
            self.x
        }
    }

    pub fn toggled() -> bool {
        let mut menu = MenuController::new(false);
        menu.toggle();
        menu.is_open()
    }

    pub fn adjusted(enabled: bool) -> i32 {
        let amount: i32 = 1_i32;
        let mut counter = CounterController::new();
        counter.adjust(enabled, amount);
        counter.value()
    }

    pub fn projected(double: bool) -> i32 {
        let counter = CounterController { count: 3_i32 };
        counter.projected(1_i32, double)
    }

    pub fn projected_local(double: bool) -> i32 {
        let counter = CounterController { count: 3_i32 };
        let value: i32 = counter.projected(1_i32, double);
        value + 1_i32
    }

    pub fn copied_value_stays_independent() -> i32 {
        let a = Point::new(1_i32);
        let mut b = a;
        b.set_x(5_i32);
        a.x()
    }
}

#[test]
fn subtotal_metadata_struct_exposes_documented_consts() {
    assert_eq!(__rsjs_client_subtotal_meta::NAME, "subtotal");
    assert_eq!(
        __rsjs_client_subtotal_meta::PARAMS,
        &[("price", "i64"), ("qty", "i64")]
    );
    assert_eq!(__rsjs_client_subtotal_meta::RETURN_TY, "i64");
    assert_eq!(__rsjs_client_subtotal_meta::BODY_HIR_VERSION, 1);
    let body = __rsjs_client_subtotal_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"BinaryOp""#), "body: {body}");
    assert!(body.contains(r#""op":"CheckedI64Mul""#), "body: {body}");
    assert!(body.contains(r#""name":"price""#), "body: {body}");
    assert!(body.contains(r#""name":"qty""#), "body: {body}");

    assert_eq!(
        __rsjs_client_subtotal_meta::BODY_IR_VERSION,
        rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION
    );
    let body_ir = __rsjs_client_subtotal_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"binary""#), "body_ir: {body_ir}");
    assert!(
        body_ir.contains(r#""op":"checked_i64_mul""#),
        "body_ir: {body_ir}"
    );
    assert!(body_ir.contains(r#""local":0"#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""local":1"#), "body_ir: {body_ir}");
}

#[test]
fn remainder_metadata_lowers_to_v3_remainder() {
    assert_eq!(__rsjs_client_remainder_meta::NAME, "remainder");
    assert_eq!(
        __rsjs_client_remainder_meta::PARAMS,
        &[("value", "i64"), ("divisor", "i64")]
    );
    assert_eq!(__rsjs_client_remainder_meta::RETURN_TY, "i64");
    let body = __rsjs_client_remainder_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"BinaryOp""#), "body: {body}");
    assert!(body.contains(r#""op":"CheckedI64Rem""#), "body: {body}");

    assert_eq!(
        __rsjs_client_remainder_meta::BODY_IR_VERSION,
        rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION
    );
    let body_ir = __rsjs_client_remainder_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"binary""#), "body_ir: {body_ir}");
    assert!(
        body_ir.contains(r#""op":"checked_i64_rem""#),
        "body_ir: {body_ir}"
    );
}

#[test]
fn let_binding_metadata_lowers_to_v3_local() {
    assert_eq!(
        __rsjs_client_adjusted_subtotal_meta::NAME,
        "adjusted_subtotal"
    );
    let body = __rsjs_client_adjusted_subtotal_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"Block""#), "body: {body}");
    assert!(body.contains(r#""kind":"Let""#), "body: {body}");
    assert!(body.contains(r#""name":"subtotal""#), "body: {body}");

    let body_ir = __rsjs_client_adjusted_subtotal_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"block""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""kind":"let""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""local":2"#), "body_ir: {body_ir}");
}

#[test]
fn value_object_metadata_lowers_struct_fields_and_assignment() {
    let body = __rsjs_client_toggle_menu_value_object_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"StructInit""#), "body: {body}");
    assert!(body.contains(r#""kind":"AssignField""#), "body: {body}");
    assert!(body.contains(r#""kind":"Field""#), "body: {body}");

    let body_ir = __rsjs_client_toggle_menu_value_object_meta::BODY_IR_JSON;
    assert!(
        body_ir.contains(r#""kind":"struct_init""#),
        "body_ir: {body_ir}"
    );
    assert!(
        body_ir.contains(r#""kind":"assign_place""#),
        "body_ir: {body_ir}"
    );
    assert!(body_ir.contains(r#""kind":"field""#), "body_ir: {body_ir}");
    rsjs_ir::parse_client_body_json_str(body_ir)
        .expect("value-object helper Client IR should round-trip through the public parser");
}

#[test]
fn value_object_module_links_inherent_methods_without_copying_their_bodies() {
    let body_ir = menu_object_methods::__rsjs_client_toggled_meta::BODY_IR_JSON;
    assert!(
        body_ir.contains("__rsjs_object_menu_controller_method_new")
            && body_ir.contains("__rsjs_object_menu_controller_method_toggle")
            && body_ir.contains("__rsjs_object_menu_controller_method_is_open")
            && body_ir.contains(r#""kind":"expr""#)
            && !body_ir.contains(r#""kind":"assign_place""#),
        "body_ir: {body_ir}"
    );
    assert!(menu_object_methods::toggled());

    let adjusted_ir = menu_object_methods::__rsjs_client_adjusted_meta::BODY_IR_JSON;
    assert!(
        adjusted_ir.contains("__rsjs_object_counter_controller_method_new")
            && adjusted_ir.contains("__rsjs_object_counter_controller_method_adjust")
            && adjusted_ir.contains("__rsjs_object_counter_controller_method_value")
            && !adjusted_ir.contains(r#""kind":"if""#),
        "adjusted_ir: {adjusted_ir}"
    );
    assert_eq!(menu_object_methods::adjusted(true), 2);
    assert_eq!(menu_object_methods::adjusted(false), -2);

    let projected_ir = menu_object_methods::__rsjs_client_projected_meta::BODY_IR_JSON;
    assert!(
        projected_ir.contains("__rsjs_object_counter_controller_from_fields")
            && projected_ir.contains("__rsjs_object_counter_controller_method_projected")
            && !projected_ir.contains(r#""kind":"if""#),
        "projected_ir: {projected_ir}"
    );
    assert_eq!(menu_object_methods::projected(true), 8);
    assert_eq!(menu_object_methods::projected(false), 4);
    assert_eq!(menu_object_methods::projected_local(true), 9);
    assert_eq!(menu_object_methods::projected_local(false), 5);
}

#[test]
fn mutable_scalar_locals_lower_to_typed_assignments() {
    let body = __rsjs_client_adjust_local_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"AssignLocal""#), "body: {body}");

    let body_ir = __rsjs_client_adjust_local_meta::BODY_IR_JSON;
    assert!(
        body_ir.contains(r#""kind":"assign""#)
            && body_ir.contains(r#""op":"checked_i32_add""#)
            && body_ir.contains(r#""op":"checked_i32_sub""#),
        "body_ir: {body_ir}"
    );
    assert_eq!(adjust_local(5, true), 7);
    assert_eq!(adjust_local(5, false), 3);
}

#[test]
fn unannotated_mutable_scalar_locals_observe_assignments() {
    assert!(reassign_unannotated_bool());
    assert_eq!(reassign_unannotated_i32(), 5_i32);
    assert_eq!(compound_assign_unannotated_i32(), 5_i32);
    assert_eq!(reassign_unannotated_string(), "b");
    assert_eq!(reassign_unannotated_f64(), 2.0_f64);

    for body in [
        __rsjs_client_reassign_unannotated_bool_meta::BODY_IR_JSON,
        __rsjs_client_reassign_unannotated_i32_meta::BODY_IR_JSON,
        __rsjs_client_compound_assign_unannotated_i32_meta::BODY_IR_JSON,
        __rsjs_client_reassign_unannotated_string_meta::BODY_IR_JSON,
        __rsjs_client_reassign_unannotated_f64_meta::BODY_IR_JSON,
    ] {
        assert!(body.contains(r#""kind":"assign""#), "body_ir: {body}");
    }
}

#[test]
fn copied_value_objects_do_not_alias_mutation() {
    assert_eq!(menu_object_methods::copied_value_stays_independent(), 1_i32);
}

#[test]
fn is_filled_metadata_lowers_to_is_non_empty() {
    assert_eq!(__rsjs_client_is_filled_meta::NAME, "is_filled");
    assert_eq!(__rsjs_client_is_filled_meta::PARAMS, &[("s", "String")]);
    assert_eq!(__rsjs_client_is_filled_meta::RETURN_TY, "bool");
    let body = __rsjs_client_is_filled_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"IsNonEmpty""#), "body: {body}");
    assert!(body.contains(r#""kind":"Param""#), "body: {body}");
    assert!(body.contains(r#""name":"s""#), "body: {body}");

    assert_eq!(
        __rsjs_client_is_filled_meta::BODY_IR_VERSION,
        rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION
    );
    let body_ir = __rsjs_client_is_filled_meta::BODY_IR_JSON;
    assert!(
        body_ir.contains(r#""kind":"is_non_empty""#),
        "body_ir: {body_ir}"
    );
    assert!(body_ir.contains(r#""kind":"local""#), "body_ir: {body_ir}");
}

#[test]
fn is_empty_metadata_lowers_to_not_is_non_empty() {
    assert_eq!(__rsjs_client_is_blank_meta::NAME, "is_blank");
    let body = __rsjs_client_is_blank_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"UnaryOp""#), "body: {body}");
    assert!(body.contains(r#""op":"Not""#), "body: {body}");
    assert!(body.contains(r#""kind":"IsNonEmpty""#), "body: {body}");

    let body_ir = __rsjs_client_is_blank_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"unary""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""op":"not""#), "body_ir: {body_ir}");
    assert!(
        body_ir.contains(r#""kind":"is_non_empty""#),
        "body_ir: {body_ir}"
    );
}

#[test]
fn len_metadata_lowers_to_runtime_call() {
    assert_eq!(
        __rsjs_client_label_has_seven_bytes_meta::NAME,
        "label_has_seven_bytes"
    );
    let body = __rsjs_client_label_has_seven_bytes_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"Len""#), "body: {body}");

    let body_ir = __rsjs_client_label_has_seven_bytes_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"call""#), "body_ir: {body_ir}");
    assert!(
        body_ir.contains(r#""kind":"runtime""#),
        "body_ir: {body_ir}"
    );
    assert!(
        body_ir.contains("new TextEncoder().encode(__v).length"),
        "body_ir: {body_ir}"
    );
}

#[test]
fn string_helper_metadata_lowers_to_v3_string_nodes() {
    let lower_body = __rsjs_client_lower_label_meta::BODY_HIR_JSON;
    assert!(
        lower_body.contains(r#""kind":"StringToLowercase""#),
        "body: {lower_body}"
    );
    let lower_ir = __rsjs_client_lower_label_meta::BODY_IR_JSON;
    assert!(
        lower_ir.contains(r#""kind":"string_to_lowercase""#),
        "body_ir: {lower_ir}"
    );

    let contains_body = __rsjs_client_label_contains_meta::BODY_HIR_JSON;
    assert!(
        contains_body.contains(r#""kind":"StringContains""#),
        "body: {contains_body}"
    );
    let contains_ir = __rsjs_client_label_contains_meta::BODY_IR_JSON;
    assert!(
        contains_ir.contains(r#""kind":"string_contains""#),
        "body_ir: {contains_ir}"
    );
}

#[test]
fn option_metadata_lowers_to_null_literals_and_checks() {
    assert_eq!(
        __rsjs_client_optional_label_meta::PARAMS,
        &[("value", "Option<&str>")]
    );
    assert_eq!(__rsjs_client_optional_label_meta::RETURN_TY, "String");
    let label_body = __rsjs_client_optional_label_meta::BODY_HIR_JSON;
    assert!(label_body.contains(r#""op":"Neq""#), "body: {label_body}");
    assert!(label_body.contains(r#""v":"Null""#), "body: {label_body}");
    let label_ir = __rsjs_client_optional_label_meta::BODY_IR_JSON;
    assert!(label_ir.contains(r#""op":"ne""#), "body_ir: {label_ir}");
    assert!(label_ir.contains(r#""kind":"null""#), "body_ir: {label_ir}");

    let missing_body = __rsjs_client_optional_missing_meta::BODY_HIR_JSON;
    assert!(
        missing_body.contains(r#""op":"Eq""#),
        "body: {missing_body}"
    );

    assert_eq!(__rsjs_client_maybe_discount_meta::RETURN_TY, "Option<i64>");
    let discount_body = __rsjs_client_maybe_discount_meta::BODY_HIR_JSON;
    assert!(
        discount_body.contains(r#""v":"Null""#),
        "body: {discount_body}"
    );
    assert!(
        discount_body.contains(r#""op":"CheckedI64Sub""#),
        "body: {discount_body}"
    );
}

#[test]
fn vec_and_slice_metadata_lowers_to_array_helpers() {
    assert_eq!(
        __rsjs_client_has_three_labels_meta::PARAMS,
        &[("labels", "Vec<&str>")]
    );
    assert_eq!(__rsjs_client_has_three_labels_meta::RETURN_TY, "bool");
    let len_ir = __rsjs_client_has_three_labels_meta::BODY_IR_JSON;
    assert!(len_ir.contains(r#""kind":"call""#), "body_ir: {len_ir}");
    assert!(len_ir.contains("__v.length"), "body_ir: {len_ir}");

    assert_eq!(
        __rsjs_client_has_any_score_meta::PARAMS,
        &[("scores", "&[i64]")]
    );
    let slice_body = __rsjs_client_has_any_score_meta::BODY_HIR_JSON;
    assert!(
        slice_body.contains(r#""kind":"IsNonEmpty""#),
        "body: {slice_body}"
    );

    assert_eq!(
        __rsjs_client_no_optional_scores_meta::PARAMS,
        &[("scores", "Vec<Option<i64>>")]
    );
    let optional_vec_body = __rsjs_client_no_optional_scores_meta::BODY_HIR_JSON;
    assert!(
        optional_vec_body.contains(r#""kind":"UnaryOp""#),
        "body: {optional_vec_body}"
    );

    assert_eq!(__rsjs_client_echo_scores_meta::RETURN_TY, "Vec<i64>");
}

#[test]
fn conditional_metadata_lowers_to_v3_conditional() {
    assert_eq!(__rsjs_client_discounted_meta::NAME, "discounted");
    let body = __rsjs_client_discounted_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"Conditional""#), "body: {body}");
    assert!(body.contains(r#""op":"Gt""#), "body: {body}");

    let body_ir = __rsjs_client_discounted_meta::BODY_IR_JSON;
    assert!(
        body_ir.contains(r#""kind":"conditional""#),
        "body_ir: {body_ir}"
    );
    assert!(body_ir.contains(r#""op":"gt""#), "body_ir: {body_ir}");
}

#[test]
fn unary_metadata_lowers_to_v3_unary() {
    let body = __rsjs_client_invert_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"UnaryOp""#), "body: {body}");
    assert!(body.contains(r#""op":"Not""#), "body: {body}");

    let body_ir = __rsjs_client_negative_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"unary""#), "body_ir: {body_ir}");
    assert!(
        body_ir.contains(r#""op":"checked_i64_neg""#),
        "body_ir: {body_ir}"
    );
}

#[test]
fn early_return_metadata_lowers_to_v3_block() {
    let body_ir = __rsjs_client_price_label_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"block""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""kind":"if""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""kind":"return""#), "body_ir: {body_ir}");
}

#[test]
fn early_return_body_hir_matches_conformance_snapshot() {
    let actual: serde_json::Value =
        serde_json::from_str(__rsjs_client_price_label_meta::BODY_HIR_JSON)
            .expect("BODY_HIR_JSON should be valid JSON");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../rsjs_conformance/snapshots/client_helper/control_flow_early_return.body-hir.json"
    ))
    .expect("snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

#[test]
fn early_return_body_ir_matches_conformance_snapshot() {
    let actual: serde_json::Value =
        serde_json::from_str(__rsjs_client_price_label_meta::BODY_IR_JSON)
            .expect("BODY_IR_JSON should be valid JSON");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../rsjs_conformance/snapshots/client_helper/control_flow_early_return.body-ir.json"
    ))
    .expect("snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

#[test]
fn tail_match_metadata_lowers_to_v3_block() {
    let body = __rsjs_client_status_label_meta::BODY_HIR_JSON;
    assert!(body.contains(r#""kind":"Block""#), "body: {body}");
    assert!(body.contains(r#""kind":"Let""#), "body: {body}");
    assert!(body.contains(r#""kind":"If""#), "body: {body}");
    assert!(body.contains(r#""op":"Eq""#), "body: {body}");
    assert!(body.contains(r#""op":"Le""#), "body: {body}");
    assert!(body.contains("__rsjs_match_0"), "body: {body}");

    let body_ir = __rsjs_client_status_label_meta::BODY_IR_JSON;
    assert!(body_ir.contains(r#""kind":"block""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""kind":"let""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""kind":"if""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""op":"eq""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""op":"ge""#), "body_ir: {body_ir}");
    assert!(body_ir.contains(r#""op":"le""#), "body_ir: {body_ir}");
    assert!(body_ir.contains("__rsjs_match_0"), "body_ir: {body_ir}");
}

#[test]
fn generated_helpers_evaluate_on_the_server() {
    assert_eq!(subtotal(2, 3), 6);
    assert_eq!(remainder(10, 4), 2);
    assert_eq!(adjusted_subtotal(2, 3), 11);
    assert_eq!(adjust_local(5, true), 7);
    assert_eq!(adjust_local(5, false), 3);
    assert!(is_filled("ready".to_string()));
    assert!(!is_filled(String::new()));
    assert!(is_blank(String::new()));
    assert!(!is_blank("ready".to_string()));
    assert!(!label_has_seven_bytes("ready".to_string()));
    assert!(label_has_seven_bytes("한😊".to_string()));
    assert_eq!(trim_label("\u{0085}ready\u{0085}".to_string()), "ready");
    assert_eq!(
        trim_label("\u{feff}ready\u{feff}".to_string()),
        "\u{feff}ready\u{feff}"
    );
    assert_eq!(lower_label("Ready".to_string()), "ready");
    assert!(label_contains("ready".to_string(), "ea"));
    assert!(!label_contains("ready".to_string(), "zz"));
    assert_eq!(discounted(75), 75);
    assert_eq!(discounted(150), 140);
    assert!(invert(false));
    assert!(!invert(true));
    assert_eq!(negative(42), -42);
    assert_eq!(price_label(75), "standard");
    assert_eq!(price_label(150), "premium");
    assert_eq!(status_label(0), "new");
    assert_eq!(status_label(1), "ready");
    assert_eq!(status_label(3), "busy");
    assert_eq!(status_label(9), "unknown");
    assert_eq!(optional_label(Some("ready")), "set");
    assert_eq!(optional_label(None), "missing");
    assert!(!optional_missing(Some("ready")));
    assert!(optional_missing(None));
    assert_eq!(maybe_discount(100, true), Some(90));
    assert_eq!(maybe_discount(100, false), None);
    assert!(has_three_labels(vec!["new", "ready", "done"]));
    assert!(!has_three_labels(vec!["new", "ready"]));
    assert!(has_any_score(&[1, 2, 3]));
    assert!(!has_any_score(&[]));
    assert!(no_optional_scores(Vec::new()));
    assert!(!no_optional_scores(vec![Some(1), None]));
    assert_eq!(echo_scores(vec![1, 2, 3]), vec![1, 2, 3]);
    assert!(toggle_menu_value_object());
    assert!(open_menu_when(true));
    assert!(!open_menu_when(false));
    assert_eq!(increment_counter_value_object(), 1);
}
