// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Framework-owned component tree IR for code generation and analysis.
//!
//! [`proa_macros::html_tree!`](https://docs.rs/proa_macros/latest/proa_macros/macro.html_tree.html)
//! produces this representation without coupling the macro expansion to a
//! component library.

/// The complete render output represented as a component tree.
#[derive(Debug, Clone)]
pub struct ComponentTree {
    /// The root node of the tree.
    pub root: TreeNode,
}

/// A node in a component tree.
#[derive(Debug, Clone)]
pub enum TreeNode {
    /// An HTML element such as `<div>` or `<button>`.
    Element(ElementNode),

    /// A component reference such as `<Spinner />`.
    Component(ComponentNode),

    /// A text literal.
    Text(TextNode),

    /// A dynamic expression.
    Expression(ExpressionNode),

    /// Conditional rendering.
    Conditional(ConditionalNode),

    /// A fragment containing zero or more nodes.
    Fragment(Vec<TreeNode>),

    /// A component's children slot.
    ChildrenSlot,
}

/// An HTML element node.
#[derive(Debug, Clone)]
pub struct ElementNode {
    /// The tag name.
    pub tag: &'static str,

    /// Attributes on the element.
    pub attrs: Vec<TreeAttr>,

    /// Child nodes.
    pub children: Vec<TreeNode>,

    /// Whether the source element was self-closing.
    pub self_closing: bool,
}

/// A component reference node.
#[derive(Debug, Clone)]
pub struct ComponentNode {
    /// The component name.
    pub name: &'static str,

    /// Props passed to the component.
    pub props: Vec<TreeAttr>,

    /// Child nodes supplied to the component.
    pub children: Vec<TreeNode>,
}

/// A text literal node.
#[derive(Debug, Clone)]
pub struct TextNode {
    /// The literal text.
    pub content: &'static str,
}

/// A dynamic expression node.
#[derive(Debug, Clone)]
pub struct ExpressionNode {
    /// The expression's source representation.
    pub expr: &'static str,
}

/// A conditional-rendering node.
#[derive(Debug, Clone)]
pub struct ConditionalNode {
    /// The condition's source representation.
    pub condition: &'static str,

    /// The node rendered when the condition is true.
    pub then_node: Box<TreeNode>,

    /// The optional node rendered when the condition is false.
    pub else_node: Option<Box<TreeNode>>,
}

/// An attribute on an element or component.
#[derive(Debug, Clone)]
pub enum TreeAttr {
    /// A static attribute such as `class="container"`.
    Static {
        /// The attribute name.
        name: &'static str,
        /// The literal value.
        value: &'static str,
    },

    /// A dynamic attribute such as `class={expr}`.
    Dynamic {
        /// The attribute name.
        name: &'static str,
        /// The expression's source representation.
        expr: &'static str,
    },

    /// A boolean attribute such as `disabled`.
    Bool {
        /// The attribute name.
        name: &'static str,
    },

    /// A computed attribute.
    Computed {
        /// The attribute name.
        name: &'static str,
        /// The expression's source representation.
        expr: &'static str,
    },

    /// A spread-props expression.
    Spread {
        /// The expression being spread.
        expr: &'static str,
    },
}

impl ComponentTree {
    /// Creates a component tree with `root` as its root node.
    pub const fn new(root: TreeNode) -> Self {
        Self { root }
    }

    /// Returns whether this tree references `name`.
    pub fn uses_component(&self, name: &str) -> bool {
        self.root.uses_component(name)
    }

    /// Returns each referenced component name once, in traversal order.
    pub fn component_dependencies(&self) -> Vec<&'static str> {
        let mut dependencies = Vec::new();
        self.root.collect_components(&mut dependencies);
        dependencies
    }
}

impl TreeNode {
    fn uses_component(&self, name: &str) -> bool {
        match self {
            Self::Component(component) => {
                component.name == name
                    || component
                        .children
                        .iter()
                        .any(|child| child.uses_component(name))
            }
            Self::Element(element) => element
                .children
                .iter()
                .any(|child| child.uses_component(name)),
            Self::Conditional(conditional) => {
                conditional.then_node.uses_component(name)
                    || conditional
                        .else_node
                        .as_ref()
                        .is_some_and(|node| node.uses_component(name))
            }
            Self::Fragment(nodes) => nodes.iter().any(|node| node.uses_component(name)),
            Self::Text(_) | Self::Expression(_) | Self::ChildrenSlot => false,
        }
    }

    fn collect_components(&self, dependencies: &mut Vec<&'static str>) {
        match self {
            Self::Component(component) => {
                if !dependencies.contains(&component.name) {
                    dependencies.push(component.name);
                }
                for child in &component.children {
                    child.collect_components(dependencies);
                }
            }
            Self::Element(element) => {
                for child in &element.children {
                    child.collect_components(dependencies);
                }
            }
            Self::Conditional(conditional) => {
                conditional.then_node.collect_components(dependencies);
                if let Some(else_node) = &conditional.else_node {
                    else_node.collect_components(dependencies);
                }
            }
            Self::Fragment(nodes) => {
                for node in nodes {
                    node.collect_components(dependencies);
                }
            }
            Self::Text(_) | Self::Expression(_) | Self::ChildrenSlot => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentNode, ComponentTree, ConditionalNode, ElementNode, TreeNode};

    fn component(name: &'static str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode::Component(ComponentNode {
            name,
            props: Vec::new(),
            children,
        })
    }

    #[test]
    fn finds_components_through_each_container_shape() {
        let tree = ComponentTree::new(TreeNode::Element(ElementNode {
            tag: "main",
            attrs: Vec::new(),
            children: vec![TreeNode::Conditional(ConditionalNode {
                condition: "ready",
                then_node: Box::new(component("Primary", Vec::new())),
                else_node: Some(Box::new(TreeNode::Fragment(vec![component(
                    "Fallback",
                    Vec::new(),
                )]))),
            })],
            self_closing: false,
        }));

        assert!(tree.uses_component("Primary"));
        assert!(tree.uses_component("Fallback"));
        assert!(!tree.uses_component("Missing"));
    }

    #[test]
    fn dependencies_are_unique_and_keep_traversal_order() {
        let tree = ComponentTree::new(TreeNode::Fragment(vec![
            component("Button", vec![component("Icon", Vec::new())]),
            component("Button", Vec::new()),
        ]));

        assert_eq!(tree.component_dependencies(), vec!["Button", "Icon"]);
    }
}
