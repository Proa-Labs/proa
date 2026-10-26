// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Generate ComponentTree from parsed HTML AST
//!
//! This module converts the parsed `HtmlRoot` AST into Rust code that constructs
//! a `ComponentTree` at compile time, which can then be used for TypeScript codegen.

use crate::parse::{Attribute, Element, HtmlRoot, Node};
use proc_macro2::TokenStream;
use quote::quote;

/// Known HTML elements (lowercase)
const HTML_ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "bdi",
    "bdo",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "map",
    "mark",
    "menu",
    "meta",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "selectedcontent",
    "slot",
    "small",
    "source",
    "span",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

fn is_html_element(name: &str) -> bool {
    HTML_ELEMENTS.contains(&name)
}

/// Check if an element name represents the children slot
fn is_children_element(name: &str) -> bool {
    name == "children" || name == "slot"
}

/// Generate TokenStream that constructs a ComponentTree
pub fn generate_tree(root: &HtmlRoot) -> TokenStream {
    let nodes: Vec<TokenStream> = root.nodes.iter().map(generate_node).collect();

    // If there's only one node, use it directly; otherwise wrap in Fragment
    let root_node = if nodes.len() == 1 {
        nodes.into_iter().next().unwrap()
    } else {
        quote! {
            ::proa_ir::component_tree::TreeNode::Fragment(vec![#(#nodes),*])
        }
    };

    quote! {
        ::proa_ir::component_tree::ComponentTree::new(#root_node)
    }
}

fn generate_node(node: &Node) -> TokenStream {
    match node {
        Node::Element(elem) => generate_element(elem),
        Node::Fragment(nodes) => {
            let child_nodes: Vec<TokenStream> = nodes.iter().map(generate_node).collect();
            quote! {
                ::proa_ir::component_tree::TreeNode::Fragment(vec![#(#child_nodes),*])
            }
        }
        Node::Text(lit) => {
            let text = lit.value();
            quote! {
                ::proa_ir::component_tree::TreeNode::Text(
                    ::proa_ir::component_tree::TextNode {
                        content: #text,
                    }
                )
            }
        }
        Node::Block(expr) => generate_block_node(expr),
        Node::Doctype(_) | Node::XmlDeclaration(_) => {
            // Document declarations are not part of the component tree for TS generation.
            quote! {
                ::proa_ir::component_tree::TreeNode::Text(
                    ::proa_ir::component_tree::TextNode {
                        content: "",
                    }
                )
            }
        }
    }
}

fn generate_element(elem: &Element) -> TokenStream {
    // `tag_str()` recovers the hyphenated source form (e.g. `rsjs-island`)
    // for tags the parser stored with `-` flattened to `_`. Plain tags and
    // component references (`Button`, `MyComponent`) round-trip unchanged.
    let tag_name = elem.tag_str();
    let attrs: Vec<TokenStream> = elem.attrs.iter().map(generate_attr).collect();
    let children: Vec<TokenStream> = elem.children.iter().map(generate_node).collect();
    let self_closing = elem.self_closing;

    // Check for children slot: <children /> or <slot />
    if is_children_element(&tag_name) {
        return quote! {
            ::proa_ir::component_tree::TreeNode::ChildrenSlot
        };
    }

    if is_html_element(&tag_name) {
        // HTML element
        quote! {
            ::proa_ir::component_tree::TreeNode::Element(
                ::proa_ir::component_tree::ElementNode {
                    tag: #tag_name,
                    attrs: vec![#(#attrs),*],
                    children: vec![#(#children),*],
                    self_closing: #self_closing,
                }
            )
        }
    } else {
        // Component reference
        quote! {
            ::proa_ir::component_tree::TreeNode::Component(
                ::proa_ir::component_tree::ComponentNode {
                    name: #tag_name,
                    props: vec![#(#attrs),*],
                    children: vec![#(#children),*],
                }
            )
        }
    }
}

fn generate_attr(attr: &Attribute) -> TokenStream {
    match attr {
        Attribute::Static { name, value } => {
            let name_str = name.to_string().replace('_', "-");
            let val = value.value();
            quote! {
                ::proa_ir::component_tree::TreeAttr::Static {
                    name: #name_str,
                    value: #val,
                }
            }
        }
        Attribute::Dynamic { name, value } => {
            let name_str = name.to_string().replace('_', "-");
            let expr_str = quote!(#value).to_string();
            quote! {
                ::proa_ir::component_tree::TreeAttr::Dynamic {
                    name: #name_str,
                    expr: #expr_str,
                }
            }
        }
        Attribute::Bool { name } => {
            let name_str = name.to_string().replace('_', "-");
            quote! {
                ::proa_ir::component_tree::TreeAttr::Bool {
                    name: #name_str,
                }
            }
        }
    }
}

fn generate_block_node(expr: &syn::Expr) -> TokenStream {
    let expr_str = quote!(#expr).to_string();

    // Check for common patterns:
    // 1. Children slot: `children`, `self.children`
    // 2. Conditional: `cond.then(|| ...)`, `if cond { ... }`
    // 3. Option: `opt.map(|x| ...)`

    // Simple heuristic: check if expression string contains common patterns
    if is_children_slot(&expr_str) {
        return quote! {
            ::proa_ir::component_tree::TreeNode::ChildrenSlot
        };
    }

    // Check for .then(|| pattern (conditional rendering)
    if let Some((condition, then_content)) = parse_then_pattern(expr) {
        let cond_str = quote!(#condition).to_string();
        let then_node = generate_then_content(then_content);

        return quote! {
            ::proa_ir::component_tree::TreeNode::Conditional(
                ::proa_ir::component_tree::ConditionalNode {
                    condition: #cond_str,
                    then_node: Box::new(#then_node),
                    else_node: None,
                }
            )
        };
    }

    // Default: treat as a generic expression
    quote! {
        ::proa_ir::component_tree::TreeNode::Expression(
            ::proa_ir::component_tree::ExpressionNode {
                expr: #expr_str,
            }
        )
    }
}

fn is_children_slot(expr_str: &str) -> bool {
    let normalized = expr_str.replace(' ', "");
    normalized == "children"
        || normalized == "self.children"
        || normalized.contains("children")
            && (normalized.ends_with("children") || normalized.contains("children}"))
}

/// Try to parse `cond.then(|| expr)` pattern
fn parse_then_pattern(expr: &syn::Expr) -> Option<(&syn::Expr, &syn::Expr)> {
    if let syn::Expr::MethodCall(method_call) = expr {
        if method_call.method == "then" && method_call.args.len() == 1 {
            if let syn::Expr::Closure(closure) = &method_call.args[0] {
                // The receiver is the condition, the closure body is the then content
                return Some((&method_call.receiver, &closure.body));
            }
        }
    }
    None
}

/// Generate the node for the content inside .then(|| ...)
fn generate_then_content(expr: &syn::Expr) -> TokenStream {
    // Check if it's a html_tree!{} or html!{} call - we can parse the content
    if let syn::Expr::Macro(macro_expr) = expr {
        let macro_name = macro_expr
            .mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string());
        if matches!(macro_name.as_deref(), Some("html_tree") | Some("html")) {
            // Parse the macro content as HTML nodes
            if let Ok(root) = syn::parse2::<HtmlRoot>(macro_expr.mac.tokens.clone()) {
                let nodes: Vec<TokenStream> = root.nodes.iter().map(generate_node).collect();
                if nodes.len() == 1 {
                    return nodes.into_iter().next().unwrap();
                } else {
                    return quote! {
                        ::proa_ir::component_tree::TreeNode::Fragment(vec![#(#nodes),*])
                    };
                }
            }
        }
    }

    // Fallback: treat as an expression
    let expr_str = quote!(#expr).to_string();
    quote! {
        ::proa_ir::component_tree::TreeNode::Expression(
            ::proa_ir::component_tree::ExpressionNode {
                expr: #expr_str,
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_html_element() {
        assert!(is_html_element("div"));
        assert!(is_html_element("button"));
        assert!(!is_html_element("Spinner"));
        assert!(!is_html_element("Button"));
    }
}
