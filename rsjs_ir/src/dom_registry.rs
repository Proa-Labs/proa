// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared compiler view of the generated `rsjs_web` DOM operation registry.
//!
//! `rsjs_web` owns authoring method names, arity, capability gates, handle
//! origins, and scalar fallbacks. This adapter deliberately keeps the existing
//! stable [`NodeRefAccessor`](crate::client::NodeRefAccessor) variants while
//! the Client IR migrates incrementally.

use crate::client::{ClientType, NodeRefAccessor};

pub use rsjs_web::{
    find_dom_operation, find_dom_tag, DomArgumentKind, DomArgumentSpec, DomCapability,
    DomElementKind, DomFallback, DomHandleOrigin, DomOperation, DomOperationKind, DomOperationSpec,
    DomTagSpec, DOM_OPERATIONS, DOM_TAGS,
};

/// Resolve a value-producing operation exposed by a bound node ref.
pub fn find_node_ref_read(method: &str) -> Option<&'static DomOperationSpec> {
    find_for(method, DomOperationKind::Read, DomHandleOrigin::NodeRef)
}

/// Resolve an imperative operation exposed by a bound node ref.
pub fn find_node_ref_effect(method: &str) -> Option<&'static DomOperationSpec> {
    find_for(method, DomOperationKind::Effect, DomHandleOrigin::NodeRef)
}

/// Resolve a value-producing operation exposed by an event current target.
pub fn find_current_target_read(method: &str) -> Option<&'static DomOperationSpec> {
    find_for(
        method,
        DomOperationKind::Read,
        DomHandleOrigin::CurrentTarget,
    )
}

fn find_for(
    method: &str,
    kind: DomOperationKind,
    origin: DomHandleOrigin,
) -> Option<&'static DomOperationSpec> {
    find_dom_operation(method).filter(|spec| spec.kind == kind && spec.supports(origin))
}

/// Adapt a generated read identity to the existing stable Client IR accessor.
///
/// Attribute operations require their validated static name. Other reads
/// ignore `static_arg` and require the registry to declare zero arguments.
pub fn node_ref_accessor(
    operation: DomOperation,
    static_arg: Option<String>,
) -> Option<NodeRefAccessor> {
    Some(match operation {
        DomOperation::Id => NodeRefAccessor::Id,
        DomOperation::TextContent => NodeRefAccessor::TextContent,
        DomOperation::Attribute => NodeRefAccessor::Attribute { name: static_arg? },
        DomOperation::HasAttribute => NodeRefAccessor::HasAttribute { name: static_arg? },
        DomOperation::ScrollTop => NodeRefAccessor::ScrollTop,
        DomOperation::ScrollLeft => NodeRefAccessor::ScrollLeft,
        DomOperation::ClientWidth => NodeRefAccessor::ClientWidth,
        DomOperation::ClientHeight => NodeRefAccessor::ClientHeight,
        DomOperation::ScrollWidth => NodeRefAccessor::ScrollWidth,
        DomOperation::ScrollHeight => NodeRefAccessor::ScrollHeight,
        DomOperation::OffsetWidth => NodeRefAccessor::OffsetWidth,
        DomOperation::OffsetHeight => NodeRefAccessor::OffsetHeight,
        DomOperation::RectX => NodeRefAccessor::RectX,
        DomOperation::RectY => NodeRefAccessor::RectY,
        DomOperation::RectTop => NodeRefAccessor::RectTop,
        DomOperation::RectRight => NodeRefAccessor::RectRight,
        DomOperation::RectBottom => NodeRefAccessor::RectBottom,
        DomOperation::RectLeft => NodeRefAccessor::RectLeft,
        DomOperation::RectWidth => NodeRefAccessor::RectWidth,
        DomOperation::RectHeight => NodeRefAccessor::RectHeight,
        DomOperation::Value => NodeRefAccessor::Value,
        DomOperation::Checked => NodeRefAccessor::Checked,
        DomOperation::CheckValidity => NodeRefAccessor::CheckValidity,
        DomOperation::ValidationMessage => NodeRefAccessor::ValidationMessage,
        DomOperation::ValidityValid => NodeRefAccessor::ValidityValid,
        DomOperation::WillValidate => NodeRefAccessor::WillValidate,
        DomOperation::SelectionStart => NodeRefAccessor::SelectionStart,
        DomOperation::SelectionEnd => NodeRefAccessor::SelectionEnd,
        DomOperation::FileCount => NodeRefAccessor::FileCount,
        DomOperation::FirstFileName => NodeRefAccessor::FirstFileName,
        DomOperation::FirstFileType => NodeRefAccessor::FirstFileType,
        DomOperation::FirstFileSize => NodeRefAccessor::FirstFileSize,
        _ => return None,
    })
}

/// Scalar Client IR type promised by a generated operation fallback.
pub const fn fallback_type(fallback: DomFallback) -> ClientType {
    match fallback {
        DomFallback::Unit => ClientType::Unit,
        DomFallback::StringEmpty => ClientType::String,
        DomFallback::BoolFalse | DomFallback::BoolTrue => ClientType::Bool,
        DomFallback::I32Zero => ClientType::I32,
        DomFallback::U32Zero => ClientType::U32,
        DomFallback::F64Zero => ClientType::F64,
    }
}

/// Scalar Client IR type promised by a generated operation.
pub fn operation_type(operation: DomOperation) -> ClientType {
    fallback_type(operation.spec().fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_registry_adapts_existing_stable_accessors() {
        let value = find_current_target_read("value").expect("value read");
        assert_eq!(value.operation, DomOperation::Value);
        assert_eq!(value.args, &[]);
        assert_eq!(
            node_ref_accessor(value.operation, None),
            Some(NodeRefAccessor::Value)
        );
        assert_eq!(operation_type(value.operation), ClientType::String);

        let attribute = find_node_ref_read("get_attribute").expect("attribute read");
        assert_eq!(
            attribute.args,
            &[DomArgumentSpec {
                name: "name",
                kind: DomArgumentKind::StaticAttribute,
            }]
        );
        assert_eq!(
            node_ref_accessor(attribute.operation, Some("data-id".to_owned())),
            Some(NodeRefAccessor::Attribute {
                name: "data-id".to_owned(),
            })
        );
    }

    #[test]
    fn effect_aliases_come_from_the_generated_registry() {
        let select = find_node_ref_effect("select_all").expect("select alias");
        assert_eq!(select.operation, DomOperation::Select);
        assert!(find_current_target_read("select_all").is_none());
    }
}
