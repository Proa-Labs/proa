// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Event attribute tables, payload accessor methods, node_ref accessor tables, and attribute safety checks.
use super::*;

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

pub(super) fn is_dynamic_attr_rejected_for_safety(name: &str) -> bool {
    // Event-handler attributes (`onclick`, `onerror`, `onmouseover`,
    // `onload`, ...) are an inline-script injection sink: lowering a dynamic
    // `onclick={expr}` to a client-side `node.setAttribute('onclick',
    // String(value))` installs attacker-controlled JavaScript with no
    // quoting or escaping defense. Reject the whole `on*` family. The check
    // is intentionally tight (starts with `on`, the remainder is ascii
    // alphabetic, total length > 2) so it catches every standard handler
    // without over-matching a hypothetical non-handler attribute. HTML has
    // no standard non-handler attribute that begins with `on`. The
    // comparison is case-insensitive to match the rsjs runtime's `/^on/i`
    // defense-in-depth guard.
    let lower = name.to_ascii_lowercase();
    if lower.len() > 2
        && lower.starts_with("on")
        && lower.as_bytes()[2..].iter().all(u8::is_ascii_alphabetic)
    {
        return true;
    }
    matches!(
        lower.as_str(),
        // Multi-URL / second-order contexts still need dedicated
        // parsers or non-attribute APIs before client-side updates
        // are safe. Single URL-bearing attrs are classified by
        // `is_dynamic_url_attr` and lowered to a sanitized binding.
        "srcset"
            | "ping"
            // `srcdoc` injects a full HTML document into an <iframe>. A
            // dynamic value is a direct HTML-injection sink, so block it
            // until a vetted server-rendered binding exists.
            | "srcdoc"
            // Style is a second-order CSS context. Keep requiring
            // static CSS/classes until a typed style binding exists.
            | "style"
    )
}

pub(super) fn is_dynamic_url_attr(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "href"
            | "src"
            | "action"
            | "formaction"
            | "poster"
            | "cite"
            | "data"
            | "longdesc"
            | "manifest"
            | "background"
            // `xlink:href` is the SVG/legacy alias for `href` and is an
            // equivalent navigation/script sink (e.g. `xlink:href` on an
            // SVG `<a>` or `javascript:` in older engines).
            | "xlink:href"
            | "xlink-href"
    )
}

pub(super) fn is_html_boolean_attr(name: &str) -> bool {
    matches!(
        name,
        "allowfullscreen"
            | "async"
            | "autofocus"
            | "autoplay"
            | "checked"
            | "controls"
            | "default"
            | "defer"
            | "disabled"
            | "formnovalidate"
            | "hidden"
            | "inert"
            | "ismap"
            | "itemscope"
            | "loop"
            | "multiple"
            | "muted"
            | "nomodule"
            | "novalidate"
            | "open"
            | "playsinline"
            | "readonly"
            | "required"
            | "reversed"
            | "selected"
    )
}

pub(super) fn expr_is_self_path(expr: &Expr) -> bool {
    matches!(expr, Expr::Path(ExprPath { path, .. }) if path.is_ident("self"))
}

pub(super) fn self_field_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Field(field) if expr_is_self_path(&field.base) => match &field.member {
            syn::Member::Named(ident) => Some(ident.to_string()),
            syn::Member::Unnamed(_) => None,
        },
        Expr::Paren(paren) => self_field_name(&paren.expr),
        _ => None,
    }
}

pub(super) fn callback_call_field(expr: &Expr) -> Option<&syn::ExprField> {
    match expr {
        Expr::Field(field) => Some(field),
        Expr::Paren(paren) => callback_call_field(&paren.expr),
        _ => None,
    }
}

pub(super) fn server_owned_self_error(expr: &Expr) -> syn::Error {
    guided_boundary_error(
        expr.span(),
        "`self` data is server-owned and cannot be read directly by client-reactive IR",
        "A `#[rsjs]` island compiles reactive slots to JavaScript. `self.field` is available \
         while the server renders HTML, but it is not client state unless you explicitly seed it \
         into the island.",
        [
            "If the value should update on the client, seed it before the template with `let value = signal(self.field);` and render `value.get()`.",
            "If the value is static server markup, render it as an independent server expression that does not also depend on live browser values.",
            "If this is a child component prop, make the child descriptor-backed and seed the prop inside that child.",
        ],
        Some("reactive.self_field"),
    )
}

pub(super) fn unsupported_reactive_expr_error(expr: &Expr) -> syn::Error {
    let (what, tracked_feature) = match expr {
        Expr::Closure(_) => (
            "closure values cannot be rendered as client-reactive expressions",
            Some("reactive.closure"),
        ),
        _ => (
            "this expression is outside the current client-reactive IR surface",
            None,
        ),
    };
    guided_boundary_error(
        expr.span(),
        what,
        "Text and attribute reactive slots compile to a small explicit Client IR so rsjs can \
         generate deterministic JavaScript. Arbitrary Rust expressions do not have a JS lowering \
         unless they fit that surface or are moved behind an explicit client helper.",
        [
            "Use a supported reactive form: literals, `signal.get()`, `signal.is_non_empty()` / `.is_empty()`, string `.to_lowercase()` / `.to_uppercase()` / `.trim()` / `.contains(...)` / `.starts_with(...)` / `.ends_with(...)`, `rsjs::encoding::*`, value `if`/`else`, arithmetic/comparison, or unary `!`.",
            "If the expression is server-only Proa content, wrap it in `rsjs::server_only(...)`.",
            "If it needs portable origin-free client computation, move that logic into a standalone helper annotated once with `#[rsjs::client]`; reachable calls are linked automatically.",
        ],
        tracked_feature,
    )
}

pub(super) fn receiver_is_event_param(receiver: &Expr, event_param: Option<&str>) -> bool {
    let Some(event_param) = event_param else {
        return false;
    };
    match receiver {
        Expr::Path(ExprPath { path, .. }) => path
            .get_ident()
            .map(|ident| ident == event_param)
            .unwrap_or(false),
        Expr::Paren(paren) => receiver_is_event_param(&paren.expr, Some(event_param)),
        _ => false,
    }
}

/// Whether `receiver` is `event.current_target()` for the active handler.
///
/// The typed Rust facade owns which methods exist on the returned element
/// handle. This syntactic recognizer only selects the equivalent existing
/// Client IR read.
pub(super) fn receiver_is_event_current_target(receiver: &Expr, event_param: Option<&str>) -> bool {
    match receiver {
        Expr::MethodCall(call) => {
            call.method == "current_target"
                && call.args.is_empty()
                && receiver_is_event_param(&call.receiver, event_param)
        }
        Expr::Paren(paren) => receiver_is_event_current_target(&paren.expr, event_param),
        _ => false,
    }
}

pub(super) fn current_target_value_method(method: &str) -> Option<EventValue> {
    let spec = rsjs_ir::dom_registry::find_current_target_read(method)?;
    match spec.operation {
        rsjs_ir::dom_registry::DomOperation::Id => Some(EventValue::CurrentTargetId),
        rsjs_ir::dom_registry::DomOperation::Value => Some(EventValue::CurrentTargetValue),
        rsjs_ir::dom_registry::DomOperation::Checked => Some(EventValue::CurrentTargetChecked),
        rsjs_ir::dom_registry::DomOperation::ScrollTop => Some(EventValue::CurrentTargetScrollTop),
        rsjs_ir::dom_registry::DomOperation::ScrollLeft => {
            Some(EventValue::CurrentTargetScrollLeft)
        }
        _ => None,
    }
}

pub(super) fn dom_read_accessor_for_call(
    call: &ExprMethodCall,
    spec: &rsjs_ir::dom_registry::DomOperationSpec,
    receiver_label: &str,
) -> syn::Result<NodeRefAccessor> {
    let static_arg = match spec.args {
        [] => {
            expect_arg_count(call, 0, &format!("{receiver_label}.{}()", spec.method))?;
            None
        }
        [argument] if argument.kind == rsjs_ir::dom_registry::DomArgumentKind::StaticAttribute => {
            expect_arg_count(call, 1, &format!("{receiver_label}.{}(name)", spec.method))?;
            Some(expect_static_node_ref_attribute_arg(
                &call.args[0],
                spec.method,
                &format!("{receiver_label}.{}(\"data-id\")", spec.method),
            )?)
        }
        _ => {
            return Err(syn::Error::new(
                call.span(),
                format!(
                    "generated DOM read `{}` has an unsupported argument declaration",
                    spec.method
                ),
            ));
        }
    };

    rsjs_ir::dom_registry::node_ref_accessor(spec.operation, static_arg).ok_or_else(|| {
        syn::Error::new(
            call.span(),
            format!(
                "generated DOM read `{}` has no Client IR accessor",
                spec.method
            ),
        )
    })
}

pub(super) fn unknown_current_target_method_error(
    span: proc_macro2::Span,
    method: &str,
) -> syn::Error {
    let supported = rsjs_ir::dom_registry::DOM_OPERATIONS
        .iter()
        .filter(|spec| {
            spec.kind == rsjs_ir::dom_registry::DomOperationKind::Read
                && spec.supports(rsjs_ir::dom_registry::DomHandleOrigin::CurrentTarget)
        })
        .map(|spec| format!("`{}()`", spec.method))
        .collect::<Vec<_>>()
        .join(", ");
    syn::Error::new(
        span,
        format!(
            "`#[rsjs]` does not recognise current-target method `.{method}()`; \
             generated scalar reads are {supported}. Current-target handles cannot perform \
             DOM effects or escape their event handler"
        ),
    )
}

pub(super) fn event_value_method(method: &str) -> Option<EventValue> {
    match method {
        "type_" => Some(EventValue::Type),
        "event_type" => Some(EventValue::Type),
        "value" => Some(EventValue::TargetValue),
        "checked" => Some(EventValue::TargetChecked),
        "target_value" => Some(EventValue::TargetValue),
        "target_checked" => Some(EventValue::TargetChecked),
        "target_date" => Some(EventValue::TargetDate),
        "date" => Some(EventValue::TargetDate),
        "target_id" => Some(EventValue::TargetId),
        "current_target_id" => Some(EventValue::CurrentTargetId),
        "current_value" => Some(EventValue::CurrentTargetValue),
        "current_target_value" => Some(EventValue::CurrentTargetValue),
        "current_checked" => Some(EventValue::CurrentTargetChecked),
        "current_target_checked" => Some(EventValue::CurrentTargetChecked),
        "key" => Some(EventValue::Key),
        "code" => Some(EventValue::Code),
        "repeat" => Some(EventValue::Repeat),
        "shift_key" => Some(EventValue::ShiftKey),
        "meta_key" => Some(EventValue::MetaKey),
        "ctrl_key" => Some(EventValue::CtrlKey),
        "alt_key" => Some(EventValue::AltKey),
        "client_x" => Some(EventValue::ClientX),
        "client_y" => Some(EventValue::ClientY),
        "page_x" => Some(EventValue::PageX),
        "page_y" => Some(EventValue::PageY),
        "screen_x" => Some(EventValue::ScreenX),
        "screen_y" => Some(EventValue::ScreenY),
        "offset_x" => Some(EventValue::OffsetX),
        "offset_y" => Some(EventValue::OffsetY),
        "movement_x" => Some(EventValue::MovementX),
        "movement_y" => Some(EventValue::MovementY),
        "button" => Some(EventValue::Button),
        "buttons" => Some(EventValue::Buttons),
        "detail" => Some(EventValue::Detail),
        "detail_u32" => Some(EventValue::DetailU32),
        "pointer_id" => Some(EventValue::PointerId),
        "pointer_type" => Some(EventValue::PointerType),
        "is_primary" => Some(EventValue::IsPrimary),
        "pressure" => Some(EventValue::Pressure),
        "tangential_pressure" => Some(EventValue::TangentialPressure),
        "tilt_x" => Some(EventValue::TiltX),
        "tilt_y" => Some(EventValue::TiltY),
        "twist" => Some(EventValue::Twist),
        "pointer_width" => Some(EventValue::PointerWidth),
        "pointer_height" => Some(EventValue::PointerHeight),
        "delta_x" => Some(EventValue::DeltaX),
        "delta_y" => Some(EventValue::DeltaY),
        "delta_z" => Some(EventValue::DeltaZ),
        "delta_mode" => Some(EventValue::DeltaMode),
        "current_target_scroll_top" => Some(EventValue::CurrentTargetScrollTop),
        "current_target_scroll_left" => Some(EventValue::CurrentTargetScrollLeft),
        "input_type" => Some(EventValue::InputType),
        "data" => Some(EventValue::Data),
        "old_state" => Some(EventValue::OldState),
        "new_state" => Some(EventValue::NewState),
        "animation_name" => Some(EventValue::AnimationName),
        "property_name" => Some(EventValue::PropertyName),
        "elapsed_time" => Some(EventValue::ElapsedTime),
        "pseudo_element" => Some(EventValue::PseudoElement),
        "storage_key" => Some(EventValue::StorageKey),
        "storage_old_value" => Some(EventValue::StorageOldValue),
        "storage_new_value" => Some(EventValue::StorageNewValue),
        "storage_url" => Some(EventValue::StorageUrl),
        "storage_area" => Some(EventValue::StorageArea),
        "old_url" => Some(EventValue::OldUrl),
        "new_url" => Some(EventValue::NewUrl),
        "clipboard_text" => Some(EventValue::ClipboardText),
        "data_transfer_text" => Some(EventValue::DataTransferText),
        "data_transfer_file_count" => Some(EventValue::DataTransferFileCount),
        "data_transfer_first_file_name" => Some(EventValue::DataTransferFirstFileName),
        "data_transfer_first_file_type" => Some(EventValue::DataTransferFirstFileType),
        "data_transfer_first_file_size" => Some(EventValue::DataTransferFirstFileSize),
        "target_file_count" => Some(EventValue::TargetFileCount),
        "target_first_file_name" => Some(EventValue::TargetFirstFileName),
        "target_first_file_type" => Some(EventValue::TargetFirstFileType),
        "target_first_file_size" => Some(EventValue::TargetFirstFileSize),
        "changed_touch_identifier" => Some(EventValue::ChangedTouchIdentifier),
        "changed_touch_client_x" => Some(EventValue::ChangedTouchClientX),
        "changed_touch_client_y" => Some(EventValue::ChangedTouchClientY),
        "changed_touch_page_x" => Some(EventValue::ChangedTouchPageX),
        "changed_touch_page_y" => Some(EventValue::ChangedTouchPageY),
        "changed_touch_screen_x" => Some(EventValue::ChangedTouchScreenX),
        "changed_touch_screen_y" => Some(EventValue::ChangedTouchScreenY),
        _ => None,
    }
}

pub(super) fn event_cmd_or_ctrl_shortcut_expr(
    mc: &ExprMethodCall,
    method: &str,
) -> syn::Result<Option<FrontendExpr>> {
    if method != "is_cmd_or_ctrl" {
        return Ok(None);
    }
    expect_arg_count(mc, 1, "event.is_cmd_or_ctrl(key)")?;
    let Expr::Lit(ExprLit {
        lit: Lit::Str(key), ..
    }) = &mc.args[0]
    else {
        return Err(syn::Error::new(
            mc.args[0].span(),
            "`event.is_cmd_or_ctrl(...)` requires a static string literal key, \
             for example `event.is_cmd_or_ctrl(\"k\")`.",
        ));
    };
    let key_value = key.value();
    if key_value.is_empty() {
        return Err(syn::Error::new(
            key.span(),
            "`event.is_cmd_or_ctrl(...)` requires a non-empty key string.",
        ));
    }
    Ok(Some(FrontendExpr::BinaryOp {
        op: BinOp::And,
        lhs: Box::new(FrontendExpr::BinaryOp {
            op: BinOp::Or,
            lhs: Box::new(FrontendExpr::EventValue(EventValue::MetaKey)),
            rhs: Box::new(FrontendExpr::EventValue(EventValue::CtrlKey)),
        }),
        rhs: Box::new(FrontendExpr::BinaryOp {
            op: BinOp::Eq,
            lhs: Box::new(FrontendExpr::StringToLowercase {
                target: Box::new(FrontendExpr::EventValue(EventValue::Key)),
            }),
            rhs: Box::new(FrontendExpr::Literal(Value::String(
                key_value.to_lowercase(),
            ))),
        }),
    }))
}

pub(super) fn event_collection_len_method(method: &str) -> Option<EventCollection> {
    match method {
        "changed_touch_count" => Some(EventCollection::ChangedTouches),
        "target_touch_count" => Some(EventCollection::TargetTouches),
        "touch_count" => Some(EventCollection::Touches),
        "target_file_count" => Some(EventCollection::TargetFiles),
        "data_transfer_file_count" => Some(EventCollection::DataTransferFiles),
        "data_transfer_item_count" => Some(EventCollection::DataTransferItems),
        _ => None,
    }
}

pub(super) fn event_collection_read_method(
    method: &str,
) -> Option<(EventCollection, EventCollectionField)> {
    let (prefix, field) = event_collection_read_method_parts(method)?;
    let collection = match prefix {
        "changed_touch" => EventCollection::ChangedTouches,
        "target_touch" => EventCollection::TargetTouches,
        "touch" => EventCollection::Touches,
        "target_file" => EventCollection::TargetFiles,
        "data_transfer_file" => EventCollection::DataTransferFiles,
        "data_transfer_item" => EventCollection::DataTransferItems,
        _ => return None,
    };
    Some((collection, field))
}

pub(super) fn event_collection_read_method_parts(
    method: &str,
) -> Option<(&'static str, EventCollectionField)> {
    match method {
        "changed_touch_identifier_at" => {
            Some(("changed_touch", EventCollectionField::TouchIdentifier))
        }
        "changed_touch_client_x_at" => Some(("changed_touch", EventCollectionField::TouchClientX)),
        "changed_touch_client_y_at" => Some(("changed_touch", EventCollectionField::TouchClientY)),
        "changed_touch_page_x_at" => Some(("changed_touch", EventCollectionField::TouchPageX)),
        "changed_touch_page_y_at" => Some(("changed_touch", EventCollectionField::TouchPageY)),
        "changed_touch_screen_x_at" => Some(("changed_touch", EventCollectionField::TouchScreenX)),
        "changed_touch_screen_y_at" => Some(("changed_touch", EventCollectionField::TouchScreenY)),
        "target_touch_identifier_at" => {
            Some(("target_touch", EventCollectionField::TouchIdentifier))
        }
        "target_touch_client_x_at" => Some(("target_touch", EventCollectionField::TouchClientX)),
        "target_touch_client_y_at" => Some(("target_touch", EventCollectionField::TouchClientY)),
        "target_touch_page_x_at" => Some(("target_touch", EventCollectionField::TouchPageX)),
        "target_touch_page_y_at" => Some(("target_touch", EventCollectionField::TouchPageY)),
        "target_touch_screen_x_at" => Some(("target_touch", EventCollectionField::TouchScreenX)),
        "target_touch_screen_y_at" => Some(("target_touch", EventCollectionField::TouchScreenY)),
        "touch_identifier_at" => Some(("touch", EventCollectionField::TouchIdentifier)),
        "touch_client_x_at" => Some(("touch", EventCollectionField::TouchClientX)),
        "touch_client_y_at" => Some(("touch", EventCollectionField::TouchClientY)),
        "touch_page_x_at" => Some(("touch", EventCollectionField::TouchPageX)),
        "touch_page_y_at" => Some(("touch", EventCollectionField::TouchPageY)),
        "touch_screen_x_at" => Some(("touch", EventCollectionField::TouchScreenX)),
        "touch_screen_y_at" => Some(("touch", EventCollectionField::TouchScreenY)),
        "target_file_name_at" => Some(("target_file", EventCollectionField::FileName)),
        "target_file_type_at" => Some(("target_file", EventCollectionField::FileType)),
        "target_file_size_at" => Some(("target_file", EventCollectionField::FileSize)),
        "data_transfer_file_name_at" => {
            Some(("data_transfer_file", EventCollectionField::FileName))
        }
        "data_transfer_file_type_at" => {
            Some(("data_transfer_file", EventCollectionField::FileType))
        }
        "data_transfer_file_size_at" => {
            Some(("data_transfer_file", EventCollectionField::FileSize))
        }
        "data_transfer_item_kind_at" => Some((
            "data_transfer_item",
            EventCollectionField::DataTransferItemKind,
        )),
        "data_transfer_item_type_at" => Some((
            "data_transfer_item",
            EventCollectionField::DataTransferItemType,
        )),
        _ => None,
    }
}

pub(super) fn event_effect_method(method: &str) -> bool {
    matches!(
        method,
        "target_first_file_text_into"
            | "target_first_file_text_into_or"
            | "data_transfer_first_file_text_into"
            | "data_transfer_first_file_text_into_or"
    )
}

pub(super) fn event_attribute_method(method: &str) -> Option<EventAttributeSource> {
    match method {
        "target_attr" | "target_attribute" => Some(EventAttributeSource::Target),
        "closest_attr" | "closest_attribute" => Some(EventAttributeSource::Closest),
        "current_target_attr" | "current_target_attribute" => {
            Some(EventAttributeSource::CurrentTarget)
        }
        _ => None,
    }
}

pub(super) fn expect_static_event_attribute_arg(
    mc: &ExprMethodCall,
    method: &str,
) -> syn::Result<String> {
    expect_arg_count(mc, 1, &format!("event.{method}(attr_name)"))?;
    let Expr::Lit(ExprLit {
        lit: Lit::Str(attr),
        ..
    }) = &mc.args[0]
    else {
        return Err(syn::Error::new(
            mc.args[0].span(),
            format!(
                "`event.{method}(...)` requires a static string literal attribute name, \
                 for example `event.{method}(\"data-date\")`. Dynamic attribute names \
                 are not supported because rsjs needs to emit a safe selector."
            ),
        ));
    };
    let value = attr.value();
    if !is_safe_event_attribute_name(&value) {
        return Err(syn::Error::new(
            attr.span(),
            format!(
                "`{value}` is not a supported event attribute name. Use a simple HTML \
                 attribute name such as `id`, `role`, `aria-expanded`, or `data-date`."
            ),
        ));
    }
    Ok(value)
}

pub(super) fn expect_static_node_ref_attribute_arg(
    expr: &Expr,
    method: &str,
    example: &str,
) -> syn::Result<String> {
    let Expr::Lit(ExprLit {
        lit: Lit::Str(attr),
        ..
    }) = expr
    else {
        return Err(syn::Error::new(
            expr.span(),
            format!(
                "`node_ref.{method}(...)` requires a static string literal attribute name, \
                 for example `{example}`. Dynamic attribute names are not supported because \
                 rsjs needs to emit safe DOM code."
            ),
        ));
    };
    let value = attr.value();
    if !is_safe_event_attribute_name(&value) {
        return Err(syn::Error::new(
            attr.span(),
            format!(
                "`{value}` is not a supported attribute name. Use a simple HTML attribute name \
                 such as `id`, `role`, `aria-expanded`, or `data-date`."
            ),
        ));
    }
    Ok(value)
}

pub(super) fn expect_static_style_property_arg(expr: &Expr) -> syn::Result<String> {
    let Expr::Lit(ExprLit {
        lit: Lit::Str(name),
        ..
    }) = expr
    else {
        return Err(syn::Error::new(
            expr.span(),
            "`node_ref.set_style_property(...)` requires a static string literal property name, \
             for example `node_ref.set_style_property(\"background\", value)`.",
        ));
    };
    let value = name.value();
    if !is_safe_style_property_name(&value) {
        return Err(syn::Error::new(
            name.span(),
            format!(
                "`{value}` is not a supported style property name. Use a simple CSS property \
                 name such as `background`, `display`, or `--component-accent`."
            ),
        ));
    }
    Ok(value)
}

pub(super) fn is_safe_style_property_name(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let bytes = value.as_bytes();
    if bytes[0] == b'-' {
        if !value.starts_with("--") {
            return false;
        }
    } else if !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

impl Analyzer {
    pub(super) fn expect_node_ref_event_target_arg(
        &mut self,
        mc: &ExprMethodCall,
        method: &str,
    ) -> syn::Result<NodeRefId> {
        expect_arg_count(mc, 1, &format!("event.{method}(node_ref)"))?;
        self.resolve_node_ref_path(&mc.args[0]).map_err(|_| {
            syn::Error::new(
                mc.args[0].span(),
                format!(
                    "`event.{method}(...)` requires a node ref declared with \
                     `let root = rsjs::node_ref::<...>();`; pass the ref name \
                     directly, for example `event.{method}(root)`."
                ),
            )
        })
    }
}

pub(super) fn is_safe_event_attribute_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

pub(super) fn node_ref_accessor_method(method: &str) -> Option<NodeRefAccessor> {
    let spec = rsjs_ir::dom_registry::find_node_ref_read(method)?;
    if !spec.args.is_empty() {
        return None;
    }
    rsjs_ir::dom_registry::node_ref_accessor(spec.operation, None)
}

pub(super) fn node_ref_effect_method(method: &str) -> Option<()> {
    rsjs_ir::dom_registry::find_node_ref_effect(method).map(|_| ())
}

pub(super) fn unknown_node_ref_method_error(span: proc_macro2::Span, method: &str) -> syn::Error {
    fn signature(spec: &rsjs_ir::dom_registry::DomOperationSpec) -> String {
        let args = spec
            .args
            .iter()
            .map(|argument| argument.name)
            .collect::<Vec<_>>()
            .join(", ");
        format!("`{}({args})`", spec.method)
    }

    let supported = |kind| {
        rsjs_ir::dom_registry::DOM_OPERATIONS
            .iter()
            .filter(|spec| {
                spec.kind == kind && spec.supports(rsjs_ir::dom_registry::DomHandleOrigin::NodeRef)
            })
            .map(signature)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let reads = supported(rsjs_ir::dom_registry::DomOperationKind::Read);
    let effects = supported(rsjs_ir::dom_registry::DomOperationKind::Effect);
    syn::Error::new(
        span,
        format!(
            "`#[rsjs]` does not recognise node-ref method `.{method}()`; \
             generated value reads are {reads}. Generated statement effects are {effects}."
        ),
    )
}

pub(super) fn expect_arg_count(
    call: &ExprMethodCall,
    expected: usize,
    label: &str,
) -> syn::Result<()> {
    if call.args.len() == expected {
        return Ok(());
    }
    Err(syn::Error::new(
        call.span(),
        format!(
            "`{label}` takes exactly {expected} argument{}",
            if expected == 1 { "" } else { "s" }
        ),
    ))
}

pub(super) fn expect_call_arg_count(
    call: &ExprCall,
    expected: usize,
    label: &str,
) -> syn::Result<()> {
    if call.args.len() == expected {
        return Ok(());
    }
    Err(syn::Error::new(
        call.span(),
        format!(
            "`{label}` takes exactly {expected} argument{}",
            if expected == 1 { "" } else { "s" }
        ),
    ))
}

pub(super) fn match_event_name(name: &str) -> Option<EventName> {
    EventName::from_attr_name(name)
}

pub(super) fn unsupported_event_attribute_error(span: proc_macro2::Span, name: &str) -> syn::Error {
    let supported = EventName::ALL
        .iter()
        .map(|event| format!("`{}`", event.as_attr_name()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut message = format!(
        "`#[rsjs]` does not recognise event handler attribute `{name}`. \
         Supported event handler attributes are {supported}."
    );
    if matches!(name, "on_mouseenter" | "on_mouseleave") {
        message.push_str(
            " Delegated `mouseenter` / `mouseleave` are intentionally not exposed yet; \
             use the bubbling `on_mouseover` / `on_mouseout` events for now.",
        );
    }
    syn::Error::new(span, message)
}

pub(super) fn match_input_kind(name: &str) -> Option<InputKind> {
    match name {
        "bind_value" => Some(InputKind::Value),
        "bind_checked" => Some(InputKind::Checked),
        "bind_selected" => Some(InputKind::Selected),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_u32_maps_to_the_typed_event_value() {
        assert_eq!(
            event_value_method("detail_u32"),
            Some(EventValue::DetailU32)
        );
        assert_eq!(event_value_method("detail"), Some(EventValue::Detail));
    }

    #[test]
    fn typed_current_target_methods_reuse_existing_event_values() {
        assert_eq!(
            current_target_value_method("value"),
            Some(EventValue::CurrentTargetValue)
        );
        assert_eq!(
            current_target_value_method("checked"),
            Some(EventValue::CurrentTargetChecked)
        );
        assert_eq!(current_target_value_method("key"), None);
    }
}
