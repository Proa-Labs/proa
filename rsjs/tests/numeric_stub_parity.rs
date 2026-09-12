// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Compile-time return-type parity for numeric rsjs authoring stubs.
//!
//! Client IR has matching assertions in `rsjs_ir::analysis::typing`; keeping
//! both sides explicit prevents a Rust-valid expression from being assigned a
//! different numeric domain by the browser compiler.

#[test]
fn window_and_node_ref_numeric_stubs_match_web_platform_domains() {
    let _: fn() -> i32 = rsjs::window::inner_width;
    let _: fn() -> i32 = rsjs::window::inner_height;
    let _: fn() -> f64 = rsjs::window::scroll_x;
    let _: fn() -> f64 = rsjs::window::scroll_y;

    type InputRef = rsjs::NodeRef<rsjs::HtmlInputElement>;
    let _: fn(&InputRef) -> u32 = InputRef::selection_start;
    let _: fn(&InputRef) -> u32 = InputRef::selection_end;
    let _: fn(&InputRef) -> f64 = InputRef::scroll_top;
    let _: fn(&InputRef) -> f64 = InputRef::scroll_left;
    let _: fn(&InputRef) -> i32 = InputRef::client_width;
    let _: fn(&InputRef) -> i32 = InputRef::client_height;
    let _: fn(&InputRef) -> i32 = InputRef::scroll_width;
    let _: fn(&InputRef) -> i32 = InputRef::scroll_height;
    let _: fn(&InputRef) -> i32 = InputRef::offset_width;
    let _: fn(&InputRef) -> i32 = InputRef::offset_height;
    let _: fn(&InputRef) -> f64 = InputRef::rect_x;
    let _: fn(&InputRef) -> f64 = InputRef::rect_y;
    let _: fn(&InputRef) -> f64 = InputRef::rect_top;
    let _: fn(&InputRef) -> f64 = InputRef::rect_right;
    let _: fn(&InputRef) -> f64 = InputRef::rect_bottom;
    let _: fn(&InputRef) -> f64 = InputRef::rect_left;
    let _: fn(&InputRef) -> f64 = InputRef::rect_width;
    let _: fn(&InputRef) -> f64 = InputRef::rect_height;
    let _: fn(&InputRef) -> u32 = InputRef::file_count;
    let _: fn(&InputRef) -> f64 = InputRef::first_file_size;
}

#[test]
fn event_numeric_stubs_match_web_platform_domains() {
    type Event = rsjs::Event<rsjs::elem::Any>;
    let _: fn(&Event) -> f64 = Event::current_target_scroll_top;
    let _: fn(&Event) -> f64 = Event::current_target_scroll_left;

    type Mouse = rsjs::MouseEvent<rsjs::elem::Any>;
    let _: fn(&Mouse) -> f64 = Mouse::client_x;
    let _: fn(&Mouse) -> f64 = Mouse::client_y;
    let _: fn(&Mouse) -> f64 = Mouse::page_x;
    let _: fn(&Mouse) -> f64 = Mouse::page_y;
    let _: fn(&Mouse) -> f64 = Mouse::screen_x;
    let _: fn(&Mouse) -> f64 = Mouse::screen_y;
    let _: fn(&Mouse) -> f64 = Mouse::offset_x;
    let _: fn(&Mouse) -> f64 = Mouse::offset_y;
    let _: fn(&Mouse) -> f64 = Mouse::movement_x;
    let _: fn(&Mouse) -> f64 = Mouse::movement_y;
    let _: fn(&Mouse) -> i32 = Mouse::button;
    let _: fn(&Mouse) -> u32 = Mouse::buttons;
    let _: fn(&Mouse) -> i32 = Mouse::detail;

    type Pointer = rsjs::PointerEvent<rsjs::elem::Any>;
    let _: fn(&Pointer) -> i32 = Pointer::pointer_id;
    let _: fn(&Pointer) -> f64 = Pointer::pressure;
    let _: fn(&Pointer) -> f64 = Pointer::tangential_pressure;
    let _: fn(&Pointer) -> i32 = Pointer::tilt_x;
    let _: fn(&Pointer) -> i32 = Pointer::tilt_y;
    let _: fn(&Pointer) -> u32 = Pointer::twist;
    let _: fn(&Pointer) -> f64 = Pointer::pointer_width;
    let _: fn(&Pointer) -> f64 = Pointer::pointer_height;

    type Wheel = rsjs::WheelEvent<rsjs::elem::Any>;
    let _: fn(&Wheel) -> f64 = Wheel::delta_x;
    let _: fn(&Wheel) -> f64 = Wheel::delta_y;
    let _: fn(&Wheel) -> f64 = Wheel::delta_z;
    let _: fn(&Wheel) -> u32 = Wheel::delta_mode;

    type Touch = rsjs::TouchEvent<rsjs::elem::Any>;
    let _: fn(&Touch) -> u32 = Touch::changed_touch_count;
    let _: fn(&Touch) -> u32 = Touch::target_touch_count;
    let _: fn(&Touch) -> u32 = Touch::touch_count;
    let _: fn(&Touch) -> i32 = Touch::changed_touch_identifier;
    let _: fn(&Touch) -> f64 = Touch::changed_touch_client_x;
    let _: fn(&Touch) -> f64 = Touch::changed_touch_client_y;
    let _: fn(&Touch) -> f64 = Touch::changed_touch_page_x;
    let _: fn(&Touch) -> f64 = Touch::changed_touch_page_y;
    let _: fn(&Touch) -> f64 = Touch::changed_touch_screen_x;
    let _: fn(&Touch) -> f64 = Touch::changed_touch_screen_y;
    let _: fn(&Touch, usize) -> i32 = Touch::changed_touch_identifier_at::<usize>;
    let _: fn(&Touch, usize) -> f64 = Touch::changed_touch_client_x_at::<usize>;
    let _: fn(&Touch, usize) -> i32 = Touch::target_touch_identifier_at::<usize>;
    let _: fn(&Touch, usize) -> f64 = Touch::target_touch_client_y_at::<usize>;
    let _: fn(&Touch, usize) -> i32 = Touch::touch_identifier_at::<usize>;
    let _: fn(&Touch, usize) -> f64 = Touch::touch_screen_y_at::<usize>;

    type Drag = rsjs::DragEvent<rsjs::elem::Any>;
    let _: fn(&Drag) -> u32 = Drag::data_transfer_file_count;
    let _: fn(&Drag) -> u32 = Drag::data_transfer_item_count;
    let _: fn(&Drag) -> f64 = Drag::data_transfer_first_file_size;
    let _: fn(&Drag, usize) -> f64 = Drag::data_transfer_file_size_at::<usize>;

    type Change = rsjs::ChangeEvent<rsjs::elem::Any>;
    let _: fn(&Change) -> u32 = Change::target_file_count;
    let _: fn(&Change) -> f64 = Change::target_first_file_size;
    let _: fn(&Change, usize) -> f64 = Change::target_file_size_at::<usize>;

    type Animation = rsjs::AnimationEvent<rsjs::elem::Any>;
    type Transition = rsjs::TransitionEvent<rsjs::elem::Any>;
    let _: fn(&Animation) -> f64 = Animation::elapsed_time;
    let _: fn(&Transition) -> f64 = Transition::elapsed_time;
}

#[test]
fn query_numeric_stubs_match_runtime_representations() {
    type Query = rsjs::Query<()>;
    let _: fn(&Query) -> f64 = Query::data_updated_at;
    let _: fn(&Query) -> f64 = Query::error_updated_at;
    let _: fn(&Query) -> u32 = Query::fetch_count;
    let _: fn(&Query) -> f64 = Query::download_loaded;
    let _: fn(&Query) -> Option<f64> = Query::download_total;
    let _: fn(&Query) -> f64 = Query::download_percent;

    let _: fn(rsjs::QueryValue<()>) -> i64 = rsjs::QueryValue::<()>::as_i64;
}

#[test]
fn query_i64_projection_preserves_only_exact_inputs() {
    use rsjs::__private::serde_json::{Number, Value};

    let project = rsjs::__private::json_as_i64;
    assert_eq!(project(&Value::Number(Number::from(7))), 7);
    assert_eq!(project(&Value::Number(Number::from_f64(7.0).unwrap())), 7);
    assert_eq!(project(&Value::Number(Number::from_f64(7.5).unwrap())), 0);
    assert_eq!(
        project(&Value::Number(Number::from(9_007_199_254_740_992_i64))),
        0,
        "an unsafe JSON numeric token cannot be recovered exactly in JavaScript"
    );
    assert_eq!(
        project(&Value::String("9007199254740993".to_string())),
        9_007_199_254_740_993
    );
    assert_eq!(project(&Value::String(i64::MIN.to_string())), i64::MIN);
    assert_eq!(project(&Value::String(i64::MAX.to_string())), i64::MAX);

    for malformed in [
        "",
        "-0",
        "+1",
        "01",
        " 1",
        "1.0",
        "-9223372036854775809",
        "9223372036854775808",
    ] {
        assert_eq!(
            project(&Value::String(malformed.to_string())),
            0,
            "malformed or out-of-range decimal should use the projection fallback: {malformed:?}"
        );
    }
}
