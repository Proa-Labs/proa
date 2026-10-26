// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Round-trip tests: parse a `html_sync!` body, re-emit it via `ToTokens`,
//! re-parse the result, re-emit again, and assert the second emission is
//! identical to the first. This is the property rsjs M7 relies on: rewriting
//! a parsed tree and feeding the emitted tokens back through proa's
//! `html_sync!` must produce a structurally-equivalent template.
//!
//! We don't compare against the *original* input string because span and
//! whitespace fidelity are intentionally not preserved. Equality on the
//! second-and-third emissions is the strongest property the emitter promises:
//! the AST -> tokens -> AST cycle is a fixed point.

use proa_dsl::parse::{Attribute, Element, HtmlRoot, Node, XmlRoot};
use quote::ToTokens;

fn parse_str(s: &str) -> HtmlRoot {
    syn::parse_str::<HtmlRoot>(s).expect("parse failed")
}

fn emit_str(root: &HtmlRoot) -> String {
    root.to_token_stream().to_string()
}

fn emit_xml_str(root: &XmlRoot) -> String {
    root.to_token_stream().to_string()
}

/// Parse, emit, parse again, emit again. The two emissions must match.
#[track_caller]
fn assert_round_trip(input: &str) -> HtmlRoot {
    let first = parse_str(input);
    let first_emitted = emit_str(&first);

    let second = syn::parse_str::<HtmlRoot>(&first_emitted)
        .unwrap_or_else(|e| panic!("re-parse failed for emitted tokens `{first_emitted}`: {e}"));
    let second_emitted = emit_str(&second);

    assert_eq!(
        first_emitted, second_emitted,
        "AST -> tokens -> AST -> tokens is not a fixed point"
    );
    second
}

#[track_caller]
fn assert_xml_round_trip(input: &str) -> XmlRoot {
    let first = syn::parse_str::<XmlRoot>(input).expect("parse failed");
    let first_emitted = emit_xml_str(&first);

    let second = syn::parse_str::<XmlRoot>(&first_emitted)
        .unwrap_or_else(|e| panic!("re-parse failed for emitted tokens `{first_emitted}`: {e}"));
    let second_emitted = emit_xml_str(&second);

    assert_eq!(
        first_emitted, second_emitted,
        "XML AST -> tokens -> AST -> tokens is not a fixed point"
    );
    second
}

#[test]
fn plain_element_with_text() {
    let root = assert_round_trip(r#"<div>"hi"</div>"#);
    assert_eq!(root.nodes.len(), 1);
    match &root.nodes[0] {
        Node::Element(e) => {
            assert_eq!(e.name.to_string(), "div");
            assert_eq!(e.children.len(), 1);
            matches!(&e.children[0], Node::Text(_));
        }
        _ => panic!("expected element"),
    }
}

#[test]
fn dashed_attribute() {
    let root = assert_round_trip(r#"<div data-foo="bar"></div>"#);
    let elem = first_element(&root);
    assert_eq!(elem.attrs.len(), 1);
    match &elem.attrs[0] {
        Attribute::Static { name, value } => {
            // Parser flattens dashes to underscores.
            assert_eq!(name.to_string(), "data_foo");
            assert_eq!(value.value(), "bar");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn deeply_dashed_attribute() {
    // Three-segment dashed name to exercise the split + rejoin path.
    let root = assert_round_trip(r#"<div data-rsjs-test="1"></div>"#);
    let elem = first_element(&root);
    match &elem.attrs[0] {
        Attribute::Static { name, value } => {
            assert_eq!(name.to_string(), "data_rsjs_test");
            assert_eq!(value.value(), "1");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn keyword_attribute_type() {
    let root = assert_round_trip(r#"<input type="text"/>"#);
    let elem = first_element(&root);
    assert!(elem.self_closing);
    match &elem.attrs[0] {
        Attribute::Static { name, value } => {
            assert_eq!(name.to_string(), "type");
            assert_eq!(value.value(), "text");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn keyword_attribute_for() {
    let root = assert_round_trip(r#"<label for="email">"Email"</label>"#);
    let elem = first_element(&root);
    match &elem.attrs[0] {
        Attribute::Static { name, value } => {
            assert_eq!(name.to_string(), "for");
            assert_eq!(value.value(), "email");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn keyword_attribute_in_dashed_position() {
    // `aria-in` is contrived but exercises a keyword in a non-leading slot.
    let root = assert_round_trip(r#"<div aria-in="value"></div>"#);
    let elem = first_element(&root);
    match &elem.attrs[0] {
        Attribute::Static { name, .. } => {
            assert_eq!(name.to_string(), "aria_in");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn keyword_else_attribute_in_dashed_position() {
    let root = assert_round_trip(r#"<em data-rsjs-cond-else="0:0">"Off"</em>"#);
    let elem = first_element(&root);
    match &elem.attrs[0] {
        Attribute::Static { name, value } => {
            assert_eq!(name.to_string(), "data_rsjs_cond_else");
            assert_eq!(value.value(), "0:0");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn boolean_attribute() {
    let root = assert_round_trip(r#"<button disabled>"Go"</button>"#);
    let elem = first_element(&root);
    assert_eq!(elem.attrs.len(), 1);
    match &elem.attrs[0] {
        Attribute::Bool { name } => assert_eq!(name.to_string(), "disabled"),
        _ => panic!("expected bool attr"),
    }
}

#[test]
fn dynamic_attribute() {
    let root = assert_round_trip(r#"<div class={classes}>"hi"</div>"#);
    let elem = first_element(&root);
    match &elem.attrs[0] {
        Attribute::Dynamic { name, .. } => assert_eq!(name.to_string(), "class"),
        _ => panic!("expected dynamic attr"),
    }
}

#[test]
fn numeric_attribute_part() {
    // Exercises the LitInt branch of the emitter for parts like the `2` in
    // `aria-level-2`. The parser flattens this to `aria_level_2`.
    let root = assert_round_trip(r#"<div aria-level-2="x"></div>"#);
    let elem = first_element(&root);
    match &elem.attrs[0] {
        Attribute::Static { name, .. } => assert_eq!(name.to_string(), "aria_level_2"),
        _ => panic!("expected static attr"),
    }
}

#[test]
fn nested_mixed_children() {
    let root =
        assert_round_trip(r#"<div class="outer">"hello "<span>{name}</span>" world"<br/></div>"#);
    let elem = first_element(&root);
    assert_eq!(elem.children.len(), 4);
    matches!(&elem.children[0], Node::Text(_));
    matches!(&elem.children[1], Node::Element(_));
    matches!(&elem.children[2], Node::Text(_));
    matches!(&elem.children[3], Node::Element(_));
}

#[test]
fn fragment_with_children() {
    let root = assert_round_trip(r#"<><span>"a"</span><span>"b"</span></>"#);
    assert_eq!(root.nodes.len(), 1);
    matches!(&root.nodes[0], Node::Fragment(_));
}

#[test]
fn void_element_self_closing_form() {
    assert_round_trip(r#"<img src="/x.jpg"/>"#);
}

#[test]
fn void_element_implicit() {
    // `<br>` without `/>` is still treated as self-closing because `br` is a
    // void element. The emitter writes it back in `<br/>` form, which the
    // parser still accepts.
    let root = assert_round_trip(r#"<br>"#);
    let elem = first_element(&root);
    assert!(elem.self_closing);
}

#[test]
fn hyphenated_custom_element_tag() {
    // HTML5 custom-element tag with two segments. Parser stores it with
    // `-` flattened to `_`; `tag_str()` recovers the dashed source form.
    let root = assert_round_trip(r#"<rsjs-island id="counter">"hi"</rsjs-island>"#);
    let elem = first_element(&root);
    assert_eq!(elem.name.to_string(), "rsjs_island");
    assert_eq!(elem.tag_str(), "rsjs-island");
    assert_eq!(elem.attrs.len(), 1);
    match &elem.attrs[0] {
        Attribute::Static { name, value } => {
            assert_eq!(name.to_string(), "id");
            assert_eq!(value.value(), "counter");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn deeply_hyphenated_custom_element_self_closing() {
    // Three-segment custom-element tag in self-closing form. Exercises
    // the split + rejoin path the same way `deeply_dashed_attribute`
    // does for attributes.
    let root = assert_round_trip(r#"<my-deeply-nested-component foo="bar"/>"#);
    let elem = first_element(&root);
    assert!(elem.self_closing);
    assert_eq!(elem.name.to_string(), "my_deeply_nested_component");
    assert_eq!(elem.tag_str(), "my-deeply-nested-component");
}

#[test]
fn hyphenated_tag_with_dashed_attributes() {
    // Both the tag name and the attribute names contain hyphens. The two
    // dash-aware parsers (tag-name, attribute-name) must not bleed into
    // each other.
    let root = assert_round_trip(r#"<rsjs-island data-rsjs-id="x" aria-label="y"></rsjs-island>"#);
    let elem = first_element(&root);
    assert_eq!(elem.tag_str(), "rsjs-island");
    assert_eq!(elem.attrs.len(), 2);
}

#[test]
fn nested_hyphenated_tags() {
    // Nested custom elements: closing-tag matching must work with the
    // hyphenated form, not the underscore-flattened storage form.
    let root = assert_round_trip(r#"<rsjs-island><my-button>"click"</my-button></rsjs-island>"#);
    let outer = first_element(&root);
    assert_eq!(outer.tag_str(), "rsjs-island");
    let inner = match &outer.children[0] {
        Node::Element(e) => e,
        _ => panic!("expected nested element"),
    };
    assert_eq!(inner.tag_str(), "my-button");
}

#[test]
fn keyword_tag_svg_use() {
    let root = assert_round_trip(r##"<svg><use href="#heart"/></svg>"##);
    let outer = first_element(&root);
    assert_eq!(outer.tag_str(), "svg");
    let inner = match &outer.children[0] {
        Node::Element(e) => e,
        _ => panic!("expected nested <use> element"),
    };
    assert_eq!(inner.tag_str(), "use");
    assert!(inner.self_closing);
}

#[test]
fn keyword_like_tag_names_round_trip() {
    let root = assert_round_trip(r#"<div><type>"T"</type><for>"F"</for></div>"#);
    let outer = first_element(&root);
    let first = match &outer.children[0] {
        Node::Element(e) => e,
        _ => panic!("expected <type> element"),
    };
    let second = match &outer.children[1] {
        Node::Element(e) => e,
        _ => panic!("expected <for> element"),
    };
    assert_eq!(first.tag_str(), "type");
    assert_eq!(second.tag_str(), "for");
}

#[test]
fn xml_namespaced_tag_and_attribute_round_trip() {
    let root = assert_xml_round_trip(r##"<svg:path xlink:href="#heart" xml:lang="en"/>"##);
    let elem = first_xml_element(&root);
    assert_eq!(elem.tag_str(), "svg:path");
    assert_eq!(elem.name.to_string(), "svg_path");
    assert_eq!(elem.attrs.len(), 2);
    match &elem.attrs[0] {
        Attribute::Static { name, value } => {
            assert_eq!(name.as_markup_str(), "xlink:href");
            assert_eq!(name.to_string(), "xlink_href");
            assert_eq!(value.value(), "#heart");
        }
        _ => panic!("expected static attr"),
    }
}

#[test]
fn xml_declaration_round_trips_optional_fields() {
    for declaration in [
        r#"<?xml version="1.0"?>"#,
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>"#,
        r#"<?xml version="1.0" standalone="no"?>"#,
    ] {
        let root = assert_xml_round_trip(&format!("{declaration}<feed/>"));
        match &root.nodes[0] {
            Node::XmlDeclaration(parsed) => assert_eq!(parsed.to_string(), declaration),
            _ => panic!("expected XML declaration"),
        }
        assert!(matches!(&root.nodes[1], Node::Element(_)));
    }
}

#[test]
fn html_rejects_namespaced_tag_syntax() {
    assert!(syn::parse_str::<HtmlRoot>(r#"<svg:path/>"#).is_err());
}

#[test]
fn xml_does_not_infer_html_void_elements() {
    let root = assert_xml_round_trip(r#"<path></path>"#);
    let elem = first_xml_element(&root);
    assert_eq!(elem.tag_str(), "path");
    assert!(!elem.self_closing);
}

#[test]
fn xml_rejects_multiple_namespace_separators() {
    assert!(syn::parse_str::<XmlRoot>(r#"<a:b:c/>"#).is_err());
}

#[test]
fn multiple_attributes_mixed() {
    let root = assert_round_trip(
        r#"<input type="text" name="email" required class={classes} data-test-id="x"/>"#,
    );
    let elem = first_element(&root);
    assert_eq!(elem.attrs.len(), 5);
}

fn first_element(root: &HtmlRoot) -> &Element {
    match &root.nodes[0] {
        Node::Element(e) => e,
        _ => panic!("expected first node to be an element"),
    }
}

fn first_xml_element(root: &XmlRoot) -> &Element {
    match &root.nodes[0] {
        Node::Element(e) => e,
        _ => panic!("expected first node to be an element"),
    }
}
