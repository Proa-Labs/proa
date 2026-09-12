// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Declarative macros for component rendering
//!
//! These macros reduce boilerplate when implementing HTML components.

/// Render an HTML element with attributes, classes, actions, and children
///
/// # Example
///
/// ```ignore
/// render_html! {
///     cx,
///     tag: "button",
///     classes: &classes[..count],
///     attrs: {
///         "type" => Some(self.button_type.as_str()),
///         "id" => self.id,
///     },
///     bool_attrs: {
///         "disabled" => self.disabled || self.loading,
///     },
///     action: self.onclick.as_ref(),
///     children: self.children,
/// }
/// ```
#[macro_export]
macro_rules! render_html {
    (
        $cx:expr,
        tag: $tag:expr
        $(, classes: $classes:expr )?
        $(, custom_class: $custom_class:expr )?
        $(, attrs: { $( $attr_name:expr => $attr_value:expr ),* $(,)? } )?
        $(, bool_attrs: { $( $bool_name:expr => $bool_value:expr ),* $(,)? } )?
        $(, action: $action:expr )?
        $(, children: $children:expr )?
        $(,)?
    ) => {{
        #[allow(unused_imports)]
        use proa_core::text::UnsafeRaw;
        #[allow(unused_imports)]
        use proa_core::Render;

        // Opening tag
        UnsafeRaw("<").render($cx);
        UnsafeRaw($tag).render($cx);

        // Classes - we need to render the class attribute if either classes or custom_class is present
        {
            #[allow(unused_assignments)]
            let mut has_classes = false;
            #[allow(unused_assignments)]
            let mut class_count = 0;

            $(
                let class_slice = $classes;
                class_count = class_slice.len();
                has_classes = class_count > 0;
            )?

            $(
                if $custom_class.is_some() {
                    has_classes = true;
                }
            )?

            if has_classes {
                UnsafeRaw(" class=\"").render($cx);

                $(
                    let class_slice = $classes;
                    for (i, class) in class_slice.iter().enumerate() {
                        if i > 0 {
                            UnsafeRaw(" ").render($cx);
                        }
                        UnsafeRaw(class).render($cx);
                    }
                )?

                $(
                    if let Some(custom) = $custom_class {
                        if class_count > 0 {
                            UnsafeRaw(" ").render($cx);
                        }
                        UnsafeRaw(custom).render($cx);
                    }
                )?

                UnsafeRaw("\"").render($cx);
            }
        }

        // Attributes
        $(
            $(
                if let Some(val) = $attr_value {
                    UnsafeRaw(" ").render($cx);
                    UnsafeRaw($attr_name).render($cx);
                    UnsafeRaw("=\"").render($cx);
                    UnsafeRaw(val).render($cx);
                    UnsafeRaw("\"").render($cx);
                }
            )*
        )?

        // Boolean attributes
        $(
            $(
                if $bool_value {
                    UnsafeRaw(" ").render($cx);
                    UnsafeRaw($bool_name).render($cx);
                }
            )*
        )?

        // Action
        $(
            if let Some(action_data) = $action {
                UnsafeRaw(" data-action=\"").render($cx);
                UnsafeRaw(&action_data.action).render($cx);
                UnsafeRaw("\"").render($cx);

                if let Some(params_obj) = action_data.params.as_object() {
                    for (key, value) in params_obj {
                        UnsafeRaw(" data-param-").render($cx);
                        UnsafeRaw(key).render($cx);
                        UnsafeRaw("=\"").render($cx);
                        match value {
                            serde_json::Value::String(s) => {
                                UnsafeRaw(s.as_str()).render($cx);
                            }
                            serde_json::Value::Number(n) => {
                                let s = n.to_string();
                                UnsafeRaw(&s).render($cx);
                            }
                            serde_json::Value::Bool(b) => {
                                UnsafeRaw(if *b { "true" } else { "false" }).render($cx);
                            }
                            v => {
                                let s = v.to_string();
                                UnsafeRaw(&s).render($cx);
                            }
                        }
                        UnsafeRaw("\"").render($cx);
                    }
                }
            }
        )?

        // Close opening tag
        UnsafeRaw(">").render($cx);

        // Children
        $(
            if let Some(children) = $children {
                children.render($cx);
            }
        )?

        // Closing tag
        UnsafeRaw("</").render($cx);
        UnsafeRaw($tag).render($cx);
        UnsafeRaw(">").render($cx);
    }};
}
