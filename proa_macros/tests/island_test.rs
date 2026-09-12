// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::render_to_string;
use proa_macros::{html_sync, island};

// ── Basic: Counter with typed props ─────────────────────────────

#[island(path = "tests/fixtures/Counter.tsx")]
pub struct Counter;

#[test]
fn counter_props_struct_has_correct_fields() {
    // This test proves the macro generated CounterProps with the right fields.
    // If any field name or type is wrong, this won't compile.
    let _props = CounterProps {
        initial_count: 5.0,
        label: "Clicks".into(),
        disabled: None,
    };
}

#[test]
fn counter_island_renders_html_with_props() {
    let island = Counter.island(CounterProps {
        initial_count: 10.0,
        label: "Score".into(),
        disabled: Some(true),
    });

    let html = render_to_string(island);

    assert!(
        html.contains(r#"data-island="Counter""#),
        "missing data-island attr: {html}"
    );
    assert!(
        html.contains(r#""initialCount":10"#),
        "missing initialCount prop: {html}"
    );
    assert!(
        html.contains(r#""label":"Score""#),
        "missing label prop: {html}"
    );
    assert!(
        html.contains(r#""disabled":true"#),
        "missing disabled prop: {html}"
    );
}

#[test]
fn counter_island_omits_none_optional() {
    let island = Counter.island(CounterProps {
        initial_count: 0.0,
        label: "Zero".into(),
        disabled: None,
    });

    let html = render_to_string(island);

    // disabled: None should be skipped by serde
    assert!(
        !html.contains("disabled"),
        "None field should be omitted: {html}"
    );
}

#[test]
fn counter_island_works_inside_html_macro() {
    let output = render_to_string(html_sync! {
        <div class="wrapper">
            {Counter.island(CounterProps {
                initial_count: 42.0,
                label: "Test".into(),
                disabled: None,
            })}
        </div>
    });

    assert!(
        output.starts_with(r#"<div class="wrapper">"#),
        "missing wrapper div: {output}"
    );
    assert!(
        output.contains(r#"data-island="Counter""#),
        "missing island: {output}"
    );
    assert!(
        output.contains(r#""initialCount":42"#),
        "missing prop: {output}"
    );
}

// ── No props: component with no explicit interface ──────────────

#[island(path = "tests/fixtures/NoProps.tsx")]
pub struct NoProps;

#[test]
fn no_props_island_renders() {
    let island = NoProps.island(NoPropsProps);

    let html = render_to_string(island);

    assert!(
        html.contains(r#"data-island="NoProps""#),
        "missing data-island: {html}"
    );
}

// ── UserCard: arrays and Record types ───────────────────────────

#[island(path = "tests/fixtures/UserCard.tsx")]
pub struct UserCard;

#[test]
fn usercard_props_struct_has_correct_fields() {
    // tags is required (string[]), metadata is optional (Record<string, string>?)
    let _props = UserCardProps {
        name: "Jane".into(),
        email: "jane@test.com".into(),
        avatar: "/img.png".into(),
        tags: vec!["a".into(), "b".into()],
        metadata: None,
    };
}

#[test]
fn usercard_island_renders_with_array_props() {
    let island = UserCard.island(UserCardProps {
        name: "Alice".into(),
        email: "alice@example.com".into(),
        avatar: "/alice.png".into(),
        tags: vec!["rust".into(), "react".into()],
        metadata: None,
    });

    let html = render_to_string(island);

    assert!(
        html.contains(r#"data-island="UserCard""#),
        "missing data-island: {html}"
    );
    assert!(
        html.contains(r#""name":"Alice""#),
        "missing name prop: {html}"
    );
    assert!(
        html.contains(r#""tags":["rust","react"]"#),
        "missing tags prop: {html}"
    );
}

// ── Accordion: children field (React.ReactNode) ─────────────────

#[island(path = "tests/fixtures/Accordion.tsx")]
pub struct Accordion;

#[test]
fn accordion_props_has_children_field() {
    // Children field should exist and be skipped from serialization
    let _props = AccordionProps {
        title: "FAQ".into(),
        default_open: Some(true),
        children: Default::default(),
    };
}

#[test]
fn accordion_island_renders_without_children_in_json() {
    let island = Accordion.island(AccordionProps {
        title: "Details".into(),
        default_open: None,
        children: Default::default(),
    });

    let html = render_to_string(island);

    assert!(
        html.contains(r#""title":"Details""#),
        "missing title: {html}"
    );
    // children should NOT appear in the JSON props (it's #[serde(skip)])
    assert!(
        !html.contains(r#""children""#),
        "children should be skipped: {html}"
    );
}

// ── Module resolution via tsconfig alias ────────────────────────

#[island(module = "@fixtures/Counter", export = "Counter")]
pub struct CounterViaAlias;

#[test]
fn module_alias_resolves_correctly() {
    let island = CounterViaAlias.island(CounterViaAliasProps {
        initial_count: 1.0,
        label: "Alias".into(),
        disabled: None,
    });

    let html = render_to_string(island);

    assert!(
        html.contains(r#"data-island="CounterViaAlias""#),
        "missing island: {html}"
    );
    assert!(html.contains(r#""initialCount":1"#), "missing prop: {html}");
}
