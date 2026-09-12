// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Test the html_tree! macro

use proa_macros::html_tree;

#[test]
fn test_html_tree_basic_element() {
    let tree = html_tree! {
        <div class="container">
            <span>"Hello"</span>
        </div>
    };

    // Should have no component dependencies (only HTML elements)
    let deps = tree.component_dependencies();
    assert!(deps.is_empty());
}

#[test]
fn test_html_tree_with_component() {
    let tree = html_tree! {
        <div>
            <Spinner class="animate-spin" />
        </div>
    };

    // Should detect the Spinner component
    let deps = tree.component_dependencies();
    assert_eq!(deps, vec!["Spinner"]);
    assert!(tree.uses_component("Spinner"));
    assert!(!tree.uses_component("Button"));
}

#[test]
fn test_html_tree_nested_components() {
    let tree = html_tree! {
        <div>
            <Button size="lg">
                <Icon name="check" />
                "Save"
            </Button>
        </div>
    };

    let deps = tree.component_dependencies();
    assert!(deps.contains(&"Button"));
    assert!(deps.contains(&"Icon"));
}

#[test]
fn test_html_tree_self_closing_component() {
    let tree = html_tree! {
        <button>
            <Spinner />
            "Loading..."
        </button>
    };

    assert!(tree.uses_component("Spinner"));
}
