// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::shared::attr_value::AttrValue;
use proa_core::{render_to_string, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{html_sync, text};

fn render_text_value<T>(value: T) -> String
where
    T: WebRenderSync<proa_core::NoLoader, Vec<u8>>,
{
    let mut cx = WebContext::with_buffer(Vec::new());
    value.render(&mut cx).unwrap();
    String::from_utf8(cx.into_output()).unwrap()
}

macro_rules! assert_text_format {
    ($format:literal, $value:expr) => {{
        let value = $value;
        assert_eq!(
            render_text_value(text!($format, value)),
            format!($format, value),
            "format={}, value={value:?}",
            $format,
        );
    }};
}

#[test]
fn text_macro_escapes_static_text_chunks() {
    let html = render_to_string(html_sync! {
        <p>{text!("A < B & \"quotes\"")}</p>
    });

    assert_eq!(html, "<p>A &lt; B &amp; \"quotes\"</p>");
}

#[test]
fn text_macro_bare_placeholder_uses_proa_text_rendering() {
    let label = "<label&>";
    let html = render_to_string(html_sync! {
        <p>{text!("Hello {}", label)}</p>
    });

    assert_eq!(html, "<p>Hello &lt;label&amp;&gt;</p>");
}

#[test]
fn text_macro_bare_placeholder_can_render_components() {
    struct Strong(&'static str);

    impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Strong {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            html_sync! {
                <strong>{self.0}</strong>
            }
            .render(cx)
        }
    }

    let html = render_to_string(html_sync! {
        <p>{text!("A {} B", Strong("label"))}</p>
    });

    assert_eq!(html, "<p>A <strong>label</strong> B</p>");
}

#[test]
fn text_macro_formats_without_losing_text_escaping() {
    struct Dangerous;

    impl core::fmt::Display for Dangerous {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("<tag \"x\" & 'y'>")
        }
    }

    let api_label = "<api>";
    let ms = 12.345_f64;
    let html = render_to_string(html_sync! {
        <p>{text!("{:?} {:.2} {:2} {:}", api_label, ms, 7, Dangerous)}</p>
    });

    assert_eq!(
        html,
        "<p>\"&lt;api&gt;\" 12.35  7 &lt;tag \"x\" &amp; 'y'&gt;</p>"
    );
}

#[test]
fn text_macro_fixed_precision_fast_path_handles_owned_and_referenced_floats() {
    let value = 12.345_f64;
    let html = render_to_string(html_sync! {
        <p>{text!("{:.2} {:.3} {:.1}", value, &value, 1.25_f32)}</p>
    });

    assert_eq!(html, "<p>12.35 12.345 1.2</p>");
}

#[test]
fn stored_text_macro_value_uses_fixed_precision_path() {
    let value = text!("latency={:.2}", 12.345_f64);
    let mut cx = WebContext::with_buffer(Vec::new());
    value.render(&mut cx).unwrap();

    assert_eq!(
        String::from_utf8(cx.into_output()).unwrap(),
        "latency=12.35"
    );
}

#[test]
fn fixed_precision_keeps_generic_display_and_escaping_behavior() {
    struct DangerousPrecision;

    impl core::fmt::Display for DangerousPrecision {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            assert_eq!(f.precision(), Some(2));
            f.write_str("<fixed&>")
        }
    }

    fn render_generic<T: core::fmt::Display>(value: T) -> String {
        let mut cx = WebContext::with_buffer(Vec::new());
        text!("{:.2}", value).render(&mut cx).unwrap();
        String::from_utf8(cx.into_output()).unwrap()
    }

    assert_eq!(render_generic(DangerousPrecision), "&lt;fixed&amp;&gt;");
}

#[test]
fn text_macro_escapes_static_attr_chunks() {
    let html = render_to_string(html_sync! {
        <div data_label={text!("A < B & \"quotes\"")}></div>
    });

    assert_eq!(
        html,
        r#"<div data-label="A &lt; B &amp; &quot;quotes&quot;"></div>"#
    );
}

#[test]
fn text_macro_fixed_precision_fast_path_renders_attributes() {
    let html = render_to_string(html_sync! {
        <meter data_value={text!("{:.2}", 12.345_f64)}></meter>
    });

    assert_eq!(html, r#"<meter data-value="12.35"></meter>"#);
}

#[test]
fn text_macro_bare_placeholder_uses_attr_value_rendering() {
    struct AttrOnly;

    impl AttrValue for AttrOnly {
        fn should_render(&self) -> bool {
            true
        }

        fn render_attr_value<B: WriteBuf, L: proa_core::DataLoader>(
            &self,
            cx: &mut WebContext<L, B>,
        ) -> Result<(), WriteError> {
            cx.out.extend_static(b"attr-only")
        }
    }

    let html = render_to_string(html_sync! {
        <div data_value={text!("id:{}", AttrOnly)}></div>
    });

    assert_eq!(html, r#"<div data-value="id:attr-only"></div>"#);
}

#[test]
fn text_macro_formats_without_losing_attr_escaping() {
    struct Dangerous;

    impl core::fmt::Display for Dangerous {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("<tag \"x\" & 'y'>")
        }
    }

    let html = render_to_string(html_sync! {
        <div data_value={text!("value={:}", Dangerous)}></div>
    });

    assert_eq!(
        html,
        r#"<div data-value="value=&lt;tag &quot;x&quot; &amp; &#39;y&#39;&gt;"></div>"#
    );
}

#[test]
fn text_macro_literal_braces_render_as_static_text() {
    let html = render_to_string(html_sync! {
        <p>{text!("{{}}")}</p>
    });

    assert_eq!(html, "<p>{}</p>");
}

#[test]
fn text_macro_literal_braces_mix_with_placeholders() {
    let html = render_to_string(html_sync! {
        <p>{text!("a {{b}} {} {{{}}}", 1, 2)}</p>
    });

    assert_eq!(html, "<p>a {b} 1 {2}</p>");
}

#[test]
fn text_macro_literal_braces_render_in_attr_position() {
    let html = render_to_string(html_sync! {
        <div data_tpl={text!("v={{{}}}", 3)}></div>
    });

    assert_eq!(html, r#"<div data-tpl="v={3}"></div>"#);
}

#[test]
fn text_macro_value_implements_async_render_trait() {
    fn assert_async<T: proa_core::WebRender<::proa_core::NoLoader>>(_: T) {}

    assert_async(text!("hello"));
}

macro_rules! assert_primitive_integer_matrix {
    ($value:expr) => {{
        assert_text_format!("{:}", $value);
        assert_text_format!("{:2}", $value);
        assert_text_format!("{:02}", $value);
        assert_text_format!("{:04}", $value);
        assert_text_format!("{:016}", $value);
        assert_text_format!("{:>8}", $value);
        assert_text_format!("{:<8}", $value);
        assert_text_format!("{:^8}", $value);
        assert_text_format!("{:*^8}", $value);
        assert_text_format!("{:$^8}", $value);
        assert_text_format!("{:+}", $value);
        assert_text_format!("{:+08}", $value);
        assert_text_format!("{:b}", $value);
        assert_text_format!("{:o}", $value);
        assert_text_format!("{:x}", $value);
        assert_text_format!("{:X}", $value);
        assert_text_format!("{:#b}", $value);
        assert_text_format!("{:#o}", $value);
        assert_text_format!("{:#x}", $value);
        assert_text_format!("{:#X}", $value);
        assert_text_format!("{:#010x}", $value);
        assert_text_format!("{:e}", $value);
        assert_text_format!("{:E}", $value);
        assert_text_format!("{:.2e}", $value);
    }};
}

#[test]
fn formatted_integer_fast_path_covers_every_primitive_type() {
    assert_primitive_integer_matrix!(203u8);
    assert_primitive_integer_matrix!(40_003u16);
    assert_primitive_integer_matrix!(3_000_000_007u32);
    assert_primitive_integer_matrix!(12_345_678_901_234_567_890u64);
    assert_primitive_integer_matrix!(u128::MAX);
    assert_primitive_integer_matrix!(usize::MAX);
    assert_primitive_integer_matrix!(-103i8);
    assert_primitive_integer_matrix!(-30_003i16);
    assert_primitive_integer_matrix!(-2_000_000_007i32);
    assert_primitive_integer_matrix!(-8_345_678_901_234_567_890i64);
    assert_primitive_integer_matrix!(i128::MIN);
    assert_primitive_integer_matrix!(isize::MIN);
}

macro_rules! assert_negative_radix_matrix {
    ($value:expr) => {{
        assert_text_format!("{:b}", $value);
        assert_text_format!("{:o}", $value);
        assert_text_format!("{:x}", $value);
        assert_text_format!("{:X}", $value);
        assert_text_format!("{:+#010x}", $value);
    }};
}

#[test]
fn signed_radices_match_rust_twos_complement_width() {
    assert_negative_radix_matrix!(-1i8);
    assert_negative_radix_matrix!(-1i16);
    assert_negative_radix_matrix!(-1i32);
    assert_negative_radix_matrix!(-1i64);
    assert_negative_radix_matrix!(-1i128);
    assert_negative_radix_matrix!(-1isize);
}

#[test]
fn formatted_integer_fast_path_handles_owned_shared_and_mutable_references() {
    let value = -42i64;
    assert_text_format!("{:+08}", value);
    assert_text_format!("{:+08}", &value);

    let mut mutable = -42i64;
    let expected = format!("{:#018x}", &mut mutable);
    let actual = render_text_value(text!("{:#018x}", &mut mutable));
    assert_eq!(actual, expected);
}

#[test]
fn integer_scientific_rounding_and_large_values_match_rust() {
    let values = [
        0u128,
        1,
        5,
        15,
        95,
        105,
        995,
        1_005,
        9_995,
        10_000,
        u64::MAX as u128,
        u128::MAX,
    ];
    for value in values {
        assert_text_format!("{:e}", value);
        assert_text_format!("{:.0e}", value);
        assert_text_format!("{:.1e}", value);
        assert_text_format!("{:.2e}", value);
        assert_text_format!("{:.6E}", value);
        assert_text_format!("{:+020.3e}", value);
    }

    assert_text_format!("{:.200e}", u128::MAX);
}

#[test]
fn formatted_integer_fast_path_matches_rust_across_deterministic_bits() {
    let mut state = 0x6a09_e667_f3bc_c908_bb67_ae85_84ca_a73bu128;
    for _ in 0..2_000 {
        state = state
            .wrapping_mul(0x5851_f42d_4c95_7f2d_1405_7b7e_f767_814fu128)
            .wrapping_add(0x9e37_79b9_7f4a_7c15_d1b5_4a32_d192_ed03u128);
        let unsigned = state;
        let signed = state as i128;

        assert_text_format!("{:02}", unsigned);
        assert_text_format!("{:+016}", signed);
        assert_text_format!("{:0>43}", signed);
        assert_text_format!("{:>043}", signed);
        assert_text_format!("{:*^43}", signed);
        assert_text_format!("{:#010x}", signed);
        assert_text_format!("{:#042o}", signed);
        assert_text_format!("{:b}", signed);
        assert_text_format!("{:08.2}", signed);
        assert_text_format!("{:e}", signed);
        assert_text_format!("{:.0e}", signed);
        assert_text_format!("{:.2E}", signed);
    }
}

#[test]
fn integer_formatting_escapes_fill_without_changing_rust_width_semantics() {
    assert_text_format!("{:🦀^8}", 42u8);
    assert_text_format!("{:*^02}", 7u8);
    assert_eq!(render_text_value(text!("{:<^5}", 7u8)), "&lt;&lt;7&lt;&lt;");

    let html = render_to_string(html_sync! {
        <div data_value={text!("{:&>4}", 7u8)}></div>
    });
    assert_eq!(html, r#"<div data-value="&amp;&amp;&amp;7"></div>"#);
}

#[test]
fn integer_candidate_specs_keep_generic_rust_formatting_and_escaping() {
    struct Custom;

    impl core::fmt::Display for Custom {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            assert_eq!(f.width(), Some(8));
            assert!(f.sign_plus());
            assert!(f.sign_aware_zero_pad());
            f.write_str("<custom&>")
        }
    }

    fn render_generic<T: core::fmt::Display>(value: T) -> String {
        render_text_value(text!("{:+08}", value))
    }

    assert_eq!(render_generic(Custom), "&lt;custom&amp;&gt;");
}

#[test]
fn integer_candidate_specs_keep_other_format_traits_on_the_rust_path() {
    struct CustomHex;

    impl core::fmt::LowerHex for CustomHex {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            assert_eq!(f.width(), Some(8));
            assert!(f.alternate());
            assert!(f.sign_aware_zero_pad());
            f.write_str("<hex&>")
        }
    }

    fn render_generic<T: core::fmt::LowerHex>(value: T) -> String {
        render_text_value(text!("{:#08x}", value))
    }

    assert_eq!(render_generic(CustomHex), "&lt;hex&amp;&gt;");
}

#[test]
fn integer_candidate_specs_do_not_alias_explicit_float_display_to_bare_float_rendering() {
    let html = render_text_value(text!(
        "{} {:} {:08.2} {:>9.2} {:.3e} {:.3E}",
        1.0f64,
        1.0f64,
        12.345f64,
        12.345f64,
        12.345f64,
        12.345f64,
    ));
    assert_eq!(
        html,
        format!(
            "1.0 {:} {:08.2} {:>9.2} {:.3e} {:.3E}",
            1.0f64, 12.345f64, 12.345f64, 12.345f64, 12.345f64,
        )
    );
}
