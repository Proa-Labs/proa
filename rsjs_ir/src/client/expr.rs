// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Expressions for Client IR v3: the single rsjs expression IR.
//!
//! [`ClientExpr`] is the only expression representation in rsjs.
//! Template bindings, handler bodies, dependency collection,
//! optimization passes, the JS emitter, the SSR Rust emitter, and the
//! component descriptor sidecar all produce or consume this enum
//! directly (the legacy v1 `Expr` enum is gone). The macro frontend
//! lowers `syn` trees into `ClientExpr` at construction time; see
//! `rsjs_macros::analyze` for the lowering.
//!
//! This module also owns the shared leaf vocabularies (event reads,
//! browser reads, node-ref accessors, query/mutation accessors,
//! string/encoding operations, [`JsonPointer`]) that the expression
//! enum references.

use crate::abi::HelperCallEdgeId;
use crate::client::IntegerDomain;
use crate::types::{MutationId, NodeRefId, QueryId, SignalId};
use crate::value::Value;

/// Dense local identifier inside a [`ClientFunction`](super::ClientFunction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientLocalId(pub u32);

/// Dense function identifier inside a [`ClientModule`](super::ClientModule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientFunctionId(pub u32);

/// One named value in a nominal client-side struct construction.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientStructFieldValue {
    pub name: String,
    pub value: ClientExpr,
}

/// Reference to a function callable from Client IR v3.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClientFunctionRef {
    Local(ClientFunctionId),
    Import {
        module: String,
        name: String,
    },
    /// One compiler-observed call to a `#[rsjs::client]` helper.
    ///
    /// `edge` identifies the source call site inside its component
    /// descriptor. `authored_path` is retained for Rust SSR emission only;
    /// rustc resolves that exact spelling (including imports, aliases, and
    /// re-exports) in the relocation record emitted beside the component.
    ///
    /// The descriptor pass emits a deterministic non-deployable placeholder
    /// until final linked emission resolves the edge to a helper definition.
    Helper {
        edge: HelperCallEdgeId,
        authored_path: String,
        name: String,
    },
    Runtime(String),
    Unresolved(String),
}

/// Unary operators supported by Client IR v3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientUnaryOp {
    Not,
    Neg,
    /// Deterministic Rust integer negation for a proven signed domain.
    CheckedNeg(IntegerDomain),
}

/// Arithmetic operation whose Rust fixed-width domain is enforced on both
/// server and browser evaluators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckedIntegerOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl CheckedIntegerOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Rem => "rem",
        }
    }
}

/// Binary operators supported by Client IR v3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    /// Integer `/`: emits `Math.trunc(lhs / rhs)` to match Rust's
    /// truncation toward zero. See `BinOp::DivInt` in the island IR.
    DivInt,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    /// Arithmetic with deterministic checked Rust fixed-width semantics.
    Checked {
        op: CheckedIntegerOp,
        domain: IntegerDomain,
    },
    /// `+` with at least one [`crate::ClientType::Dyn`] operand: emits
    /// `num_add(lhs, rhs)`, which dispatches on `typeof` at run time.
    DynAdd,
    /// `-` with a `Dyn` operand: emits `num_sub(lhs, rhs)`.
    DynSub,
    /// `*` with a `Dyn` operand: emits `num_mul(lhs, rhs)`.
    DynMul,
    /// Float `/` with a `Dyn` operand: emits `num_div(lhs, rhs)`, which never
    /// truncates.
    DynDiv,
    /// Legacy Rust integer `/` with a `Dyn` operand. Retained for serialized
    /// Client IR compatibility only; current macro output proves a fixed-width
    /// domain and emits [`Self::Checked`] with [`CheckedIntegerOp::Div`].
    DynDivInt,
    /// `%` with a `Dyn` operand: emits `num_rem(lhs, rhs)`.
    DynRem,
    /// `==` with a `Dyn` operand: emits `num_eq(lhs, rhs)`. A bare `===` is
    /// wrong here, because `5n === 5` is `false` while Rust `5 == 5` is `true`.
    DynEq,
    /// `!=` with a `Dyn` operand: emits `num_ne(lhs, rhs)`.
    DynNe,
}

impl ClientBinaryOp {
    /// The runtime helper this operator lowers to, or `None` when it lowers to
    /// a bare JS infix operator.
    ///
    /// Every `Dyn*` operator has one operand whose representation the page seed
    /// pins rather than the compiler proving it, so the operator itself must
    /// dispatch at run time. Emission and the import-line gate share this table
    /// so an emitted helper can never be missing from the import list.
    pub fn dyn_helper_js(self) -> Option<&'static str> {
        Some(match self {
            Self::DynAdd => "num_add",
            Self::DynSub => "num_sub",
            Self::DynMul => "num_mul",
            Self::DynDiv => "num_div",
            Self::DynDivInt => "num_div_int",
            Self::DynRem => "num_rem",
            Self::DynEq => "num_eq",
            Self::DynNe => "num_ne",
            _ => return None,
        })
    }

    /// Runtime helper used by checked fixed-width arithmetic.
    pub const fn checked_helper_js(self) -> Option<&'static str> {
        match self {
            Self::Checked {
                op: CheckedIntegerOp::Add,
                ..
            } => Some("checked_int_add"),
            Self::Checked {
                op: CheckedIntegerOp::Sub,
                ..
            } => Some("checked_int_sub"),
            Self::Checked {
                op: CheckedIntegerOp::Mul,
                ..
            } => Some("checked_int_mul"),
            Self::Checked {
                op: CheckedIntegerOp::Div,
                ..
            } => Some("checked_int_div"),
            Self::Checked {
                op: CheckedIntegerOp::Rem,
                ..
            } => Some("checked_int_rem"),
            _ => None,
        }
    }
}

/// Whitelisted browser-global reads in Client IR v3.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ClientBrowserRead {
    Window(WindowField),
    Location(LocationField),
    Storage {
        area: BrowserStorageArea,
        value_type: StorageValueType,
        key: Box<ClientExpr>,
        fallback: Option<Box<ClientExpr>>,
    },
    UrlSearchParam {
        key: Box<ClientExpr>,
        fallback: Option<Box<ClientExpr>>,
    },
    /// A registry-declared browser read.
    ///
    /// `helper` is the stable [`crate::browser_registry`] id
    /// (`<module>.<method>`); `args` are the lowered reactive
    /// arguments in source order. The JS emitter resolves the id back
    /// through the registry entry's JS template, so new simple reads
    /// need no dedicated IR variant, JSON arm, or emitter arm.
    Declared {
        helper: String,
        args: Vec<ClientExpr>,
    },
}

/// Expression node in Client IR v3.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ClientExpr {
    Unit,
    Literal(Value),
    /// A literal array of scalar values: `["a", "b"]`. v0.1 surface:
    /// produced only for `signal.set(vec![...])` handler statements
    /// (writing a `Vec`-valued signal, e.g. one driving a keyed list).
    ArrayLit(Vec<Value>),
    /// A structural query key from the `rsjs::QueryKey::new((...))`
    /// authoring surface. Each part is an arbitrary reactive
    /// expression; the JS emitter lowers the key to an array literal
    /// of its parts.
    QueryKey(Vec<ClientExpr>),
    Local(ClientLocalId),
    SignalRead(SignalId),
    QueryRead {
        id: QueryId,
        accessor: QueryAccessor,
    },
    MutationRead {
        id: MutationId,
        accessor: MutationAccessor,
    },
    EventRead(EventValue),
    /// The concrete element selected by the active delegated event binding.
    ///
    /// This value may be stored in a handler local and inspected through
    /// [`ClientExpr::DomRead`]. It is intentionally not serializable state.
    EventCurrentTarget,
    /// Read one schema-declared scalar DOM projection from a first-class
    /// element handle.
    DomRead {
        target: Box<ClientExpr>,
        accessor: NodeRefAccessor,
    },
    EventCollectionLen(EventCollection),
    EventCollectionRead {
        collection: EventCollection,
        index: Box<ClientExpr>,
        field: EventCollectionField,
    },
    NodeRefRead {
        id: NodeRefId,
        accessor: NodeRefAccessor,
    },
    EventTargetWithinNodeRef {
        id: NodeRefId,
        inside: bool,
    },
    EventAttribute {
        source: EventAttributeSource,
        attr: String,
    },
    EventPayload {
        attr: String,
    },
    CallbackPayload,
    NodePayload {
        attr: String,
        tokens: String,
    },
    BrowserRead(ClientBrowserRead),
    Encoding {
        op: EncodingOp,
        value: Box<ClientExpr>,
    },
    Conditional {
        condition: Box<ClientExpr>,
        then_expr: Box<ClientExpr>,
        else_expr: Box<ClientExpr>,
    },
    IsNonEmpty {
        target: Box<ClientExpr>,
    },
    StringToLowercase {
        target: Box<ClientExpr>,
    },
    StringContains {
        haystack: Box<ClientExpr>,
        needle: Box<ClientExpr>,
    },
    StringTransform {
        op: StringTransformOp,
        target: Box<ClientExpr>,
    },
    StringPredicate {
        op: StringPredicateOp,
        haystack: Box<ClientExpr>,
        needle: Box<ClientExpr>,
    },
    Project {
        target: Box<ClientExpr>,
        path: JsonPointer,
    },
    AsType {
        target: Box<ClientExpr>,
        ty: AsType,
    },
    IsPresent {
        target: Box<ClientExpr>,
    },
    Eq {
        target: Box<ClientExpr>,
        value: Value,
    },
    Neq {
        target: Box<ClientExpr>,
        value: Value,
    },
    Coalesce {
        target: Box<ClientExpr>,
        fallback: Value,
    },
    Unary {
        op: ClientUnaryOp,
        expr: Box<ClientExpr>,
    },
    Binary {
        op: ClientBinaryOp,
        left: Box<ClientExpr>,
        right: Box<ClientExpr>,
    },
    /// Construct a nominal Rust-authored client value as a plain JS object.
    ///
    /// `type_name` preserves source identity for validation and diagnostics;
    /// the initial JavaScript ABI deliberately remains an own-property record.
    StructInit {
        type_name: String,
        fields: Vec<ClientStructFieldValue>,
    },
    Field {
        base: Box<ClientExpr>,
        name: String,
    },
    Index {
        base: Box<ClientExpr>,
        index: Box<ClientExpr>,
    },
    Call {
        function: ClientFunctionRef,
        args: Vec<ClientExpr>,
    },
}

impl ClientExpr {
    pub fn bool(value: bool) -> Self {
        Self::Literal(Value::Bool(value))
    }

    pub fn int(value: i64) -> Self {
        Self::Literal(Value::Int(value))
    }

    pub fn float(value: f64) -> Self {
        Self::Literal(Value::Float(value))
    }

    pub fn string(value: impl Into<String>) -> Self {
        Self::Literal(Value::String(value.into()))
    }

    pub fn local(id: ClientLocalId) -> Self {
        Self::Local(id)
    }

    pub fn signal_read(id: SignalId) -> Self {
        Self::SignalRead(id)
    }

    pub fn query_read(id: QueryId, accessor: QueryAccessor) -> Self {
        Self::QueryRead { id, accessor }
    }

    pub fn mutation_read(id: MutationId, accessor: MutationAccessor) -> Self {
        Self::MutationRead { id, accessor }
    }

    pub fn browser_read(read: ClientBrowserRead) -> Self {
        Self::BrowserRead(read)
    }

    pub fn encoding(op: EncodingOp, value: ClientExpr) -> Self {
        Self::Encoding {
            op,
            value: Box::new(value),
        }
    }

    pub fn unary(op: ClientUnaryOp, expr: ClientExpr) -> Self {
        Self::Unary {
            op,
            expr: Box::new(expr),
        }
    }

    pub fn binary(op: ClientBinaryOp, left: ClientExpr, right: ClientExpr) -> Self {
        Self::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    pub fn call(function: ClientFunctionRef, args: Vec<ClientExpr>) -> Self {
        Self::Call { function, args }
    }
}

/// Whitelisted values exposed from a bound DOM node ref.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeRefAccessor {
    /// `ref.id()` -> `ref.current?.id ?? ""`.
    Id,
    /// `ref.text_content()` -> `ref.current?.textContent ?? ""`.
    TextContent,
    /// `ref.value()` -> `ref.current?.value ?? ""`.
    Value,
    /// `ref.checked()` -> `!!ref.current?.checked`.
    Checked,
    /// `ref.check_validity()` -> `ref.current?.checkValidity() ?? true`.
    CheckValidity,
    /// `ref.validation_message()` -> `ref.current?.validationMessage ?? ""`.
    ValidationMessage,
    /// `ref.validity_valid()` -> `ref.current?.validity?.valid ?? true`.
    ValidityValid,
    /// `ref.will_validate()` -> `ref.current?.willValidate ?? false`.
    WillValidate,
    /// `ref.selection_start()` -> `ref.current?.selectionStart ?? 0`.
    SelectionStart,
    /// `ref.selection_end()` -> `ref.current?.selectionEnd ?? 0`.
    SelectionEnd,
    /// `ref.scroll_top()` -> `ref.current?.scrollTop ?? 0`.
    ScrollTop,
    /// `ref.scroll_left()` -> `ref.current?.scrollLeft ?? 0`.
    ScrollLeft,
    /// `ref.client_width()` -> `ref.current?.clientWidth ?? 0`.
    ClientWidth,
    /// `ref.client_height()` -> `ref.current?.clientHeight ?? 0`.
    ClientHeight,
    /// `ref.scroll_width()` -> `ref.current?.scrollWidth ?? 0`.
    ScrollWidth,
    /// `ref.scroll_height()` -> `ref.current?.scrollHeight ?? 0`.
    ScrollHeight,
    /// `ref.offset_width()` -> `ref.current?.offsetWidth ?? 0`.
    OffsetWidth,
    /// `ref.offset_height()` -> `ref.current?.offsetHeight ?? 0`.
    OffsetHeight,
    /// `ref.rect_x()` -> `ref.current?.getBoundingClientRect()?.x ?? 0`.
    RectX,
    /// `ref.rect_y()` -> `ref.current?.getBoundingClientRect()?.y ?? 0`.
    RectY,
    /// `ref.rect_top()` -> `ref.current?.getBoundingClientRect()?.top ?? 0`.
    RectTop,
    /// `ref.rect_right()` -> `ref.current?.getBoundingClientRect()?.right ?? 0`.
    RectRight,
    /// `ref.rect_bottom()` -> `ref.current?.getBoundingClientRect()?.bottom ?? 0`.
    RectBottom,
    /// `ref.rect_left()` -> `ref.current?.getBoundingClientRect()?.left ?? 0`.
    RectLeft,
    /// `ref.rect_width()` -> `ref.current?.getBoundingClientRect()?.width ?? 0`.
    RectWidth,
    /// `ref.rect_height()` -> `ref.current?.getBoundingClientRect()?.height ?? 0`.
    RectHeight,
    /// `ref.file_count()` -> `ref.current?.files?.length ?? 0`.
    FileCount,
    /// `ref.first_file_name()` -> `ref.current?.files?.[0]?.name ?? ""`.
    FirstFileName,
    /// `ref.first_file_type()` -> `ref.current?.files?.[0]?.type ?? ""`.
    FirstFileType,
    /// `ref.first_file_size()` -> `ref.current?.files?.[0]?.size ?? 0`.
    FirstFileSize,
    /// `ref.get_attribute(name)` -> `ref.current?.getAttribute(name) ?? ""`.
    Attribute { name: String },
    /// `ref.has_attribute(name)` -> `ref.current?.hasAttribute(name) ?? false`.
    HasAttribute { name: String },
}

/// Whitelisted DOM-event values exposed to handler expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventValue {
    /// `event.type_()` -> `event.type ?? ""`.
    Type,
    /// `event.target_value()` -> `event.target?.value ?? ""`.
    TargetValue,
    /// `event.target_checked()` -> `!!event.target?.checked`.
    TargetChecked,
    /// `event.target_date()` -> closest `[data-date]` attribute or `""`.
    TargetDate,
    /// `event.target_id()` -> `event.target?.id ?? ""`.
    TargetId,
    /// `event.current_target_id()` -> matched handler element id, or `""`.
    CurrentTargetId,
    /// `event.current_target_value()` -> matched handler element value, or `""`.
    CurrentTargetValue,
    /// `event.current_target_checked()` -> matched handler element checked state.
    CurrentTargetChecked,
    /// `event.key()` -> `event.key ?? ""`.
    Key,
    /// `event.code()` -> `event.code ?? ""`.
    Code,
    /// `event.repeat()` -> `!!event.repeat`.
    Repeat,
    /// `event.shift_key()` -> `!!event.shiftKey`.
    ShiftKey,
    /// `event.meta_key()` -> `!!event.metaKey`.
    MetaKey,
    /// `event.ctrl_key()` -> `!!event.ctrlKey`.
    CtrlKey,
    /// `event.alt_key()` -> `!!event.altKey`.
    AltKey,
    /// `event.client_x()` -> `event.clientX ?? 0`.
    ClientX,
    /// `event.client_y()` -> `event.clientY ?? 0`.
    ClientY,
    /// `event.page_x()` -> `event.pageX ?? 0`.
    PageX,
    /// `event.page_y()` -> `event.pageY ?? 0`.
    PageY,
    /// `event.screen_x()` -> `event.screenX ?? 0`.
    ScreenX,
    /// `event.screen_y()` -> `event.screenY ?? 0`.
    ScreenY,
    /// `event.offset_x()` -> `event.offsetX ?? 0`.
    OffsetX,
    /// `event.offset_y()` -> `event.offsetY ?? 0`.
    OffsetY,
    /// `event.movement_x()` -> `event.movementX ?? 0`.
    MovementX,
    /// `event.movement_y()` -> `event.movementY ?? 0`.
    MovementY,
    /// `event.button()` -> `event.button ?? 0`.
    Button,
    /// `event.buttons()` -> `event.buttons ?? 0`.
    Buttons,
    /// `event.detail()` -> `event.detail ?? 0`.
    Detail,
    /// `event.detail_u32()` -> `(event.detail ?? 0) >>> 0`.
    ///
    /// This is the exact JavaScript equivalent of Rust's `i32 as u32`
    /// conversion and is useful for application-owned synthetic event
    /// payloads that are known to be non-negative.
    DetailU32,
    /// `event.pointer_id()` -> `event.pointerId ?? 0`.
    PointerId,
    /// `event.pointer_type()` -> `event.pointerType ?? ""`.
    PointerType,
    /// `event.is_primary()` -> `!!event.isPrimary`.
    IsPrimary,
    /// `event.pressure()` -> `event.pressure ?? 0`.
    Pressure,
    /// `event.tangential_pressure()` -> `event.tangentialPressure ?? 0`.
    TangentialPressure,
    /// `event.tilt_x()` -> `event.tiltX ?? 0`.
    TiltX,
    /// `event.tilt_y()` -> `event.tiltY ?? 0`.
    TiltY,
    /// `event.twist()` -> `event.twist ?? 0`.
    Twist,
    /// `event.pointer_width()` -> `event.width ?? 0`.
    PointerWidth,
    /// `event.pointer_height()` -> `event.height ?? 0`.
    PointerHeight,
    /// `event.delta_x()` -> `event.deltaX ?? 0`.
    DeltaX,
    /// `event.delta_y()` -> `event.deltaY ?? 0`.
    DeltaY,
    /// `event.delta_z()` -> `event.deltaZ ?? 0`.
    DeltaZ,
    /// `event.delta_mode()` -> `event.deltaMode ?? 0`.
    DeltaMode,
    /// `event.current_target_scroll_top()` -> current target `scrollTop`.
    CurrentTargetScrollTop,
    /// `event.current_target_scroll_left()` -> current target `scrollLeft`.
    CurrentTargetScrollLeft,
    /// `event.input_type()` -> `event.inputType ?? ""`.
    InputType,
    /// `event.data()` -> `event.data ?? ""`.
    Data,
    /// `event.old_state()` -> `event.oldState ?? ""`.
    OldState,
    /// `event.new_state()` -> `event.newState ?? ""`.
    NewState,
    /// `event.animation_name()` -> `event.animationName ?? ""`.
    AnimationName,
    /// `event.property_name()` -> `event.propertyName ?? ""`.
    PropertyName,
    /// `event.elapsed_time()` -> `event.elapsedTime ?? 0`.
    ElapsedTime,
    /// `event.pseudo_element()` -> `event.pseudoElement ?? ""`.
    PseudoElement,
    /// `event.storage_key()` -> `event.key ?? ""`.
    StorageKey,
    /// `event.storage_old_value()` -> `event.oldValue ?? ""`.
    StorageOldValue,
    /// `event.storage_new_value()` -> `event.newValue ?? ""`.
    StorageNewValue,
    /// `event.storage_url()` -> `event.url ?? ""`.
    StorageUrl,
    /// `event.storage_area()` -> `"local"`, `"session"`, or `""`.
    StorageArea,
    /// `event.old_url()` -> `event.oldURL ?? ""`.
    OldUrl,
    /// `event.new_url()` -> `event.newURL ?? ""`.
    NewUrl,
    /// `event.clipboard_text()` -> plain-text clipboard payload or `""`.
    ClipboardText,
    /// `event.data_transfer_text()` -> plain-text drag/drop payload or `""`.
    DataTransferText,
    /// `event.data_transfer_file_count()` -> number of dropped/dragged files.
    DataTransferFileCount,
    /// `event.data_transfer_first_file_name()` -> first dragged file name.
    DataTransferFirstFileName,
    /// `event.data_transfer_first_file_type()` -> first dragged file MIME type.
    DataTransferFirstFileType,
    /// `event.data_transfer_first_file_size()` -> first dragged file size.
    DataTransferFirstFileSize,
    /// `event.target_file_count()` -> number of files selected on event target.
    TargetFileCount,
    /// `event.target_first_file_name()` -> first selected file name.
    TargetFirstFileName,
    /// `event.target_first_file_type()` -> first selected file MIME type.
    TargetFirstFileType,
    /// `event.target_first_file_size()` -> first selected file size.
    TargetFirstFileSize,
    /// `event.changed_touch_identifier()` -> first changed touch identifier.
    ChangedTouchIdentifier,
    /// `event.changed_touch_client_x()` -> first changed touch `clientX`.
    ChangedTouchClientX,
    /// `event.changed_touch_client_y()` -> first changed touch `clientY`.
    ChangedTouchClientY,
    /// `event.changed_touch_page_x()` -> first changed touch `pageX`.
    ChangedTouchPageX,
    /// `event.changed_touch_page_y()` -> first changed touch `pageY`.
    ChangedTouchPageY,
    /// `event.changed_touch_screen_x()` -> first changed touch `screenX`.
    ChangedTouchScreenX,
    /// `event.changed_touch_screen_y()` -> first changed touch `screenY`.
    ChangedTouchScreenY,
}

/// Whitelisted event-scoped browser collections exposed to handler
/// expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventCollection {
    /// `event.changedTouches`.
    ChangedTouches,
    /// `event.targetTouches`.
    TargetTouches,
    /// `event.touches`.
    Touches,
    /// `event.target.files`.
    TargetFiles,
    /// `event.dataTransfer.files`.
    DataTransferFiles,
    /// `event.dataTransfer.items`.
    DataTransferItems,
}

/// Scalar field read from an [`EventCollection`] item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventCollectionField {
    /// `Touch.identifier`.
    TouchIdentifier,
    /// `Touch.clientX`.
    TouchClientX,
    /// `Touch.clientY`.
    TouchClientY,
    /// `Touch.pageX`.
    TouchPageX,
    /// `Touch.pageY`.
    TouchPageY,
    /// `Touch.screenX`.
    TouchScreenX,
    /// `Touch.screenY`.
    TouchScreenY,
    /// `File.name`.
    FileName,
    /// `File.type`.
    FileType,
    /// `File.size`.
    FileSize,
    /// `DataTransferItem.kind`.
    DataTransferItemKind,
    /// `DataTransferItem.type`.
    DataTransferItemType,
}

/// Which event-related DOM node an [`ClientExpr::EventAttribute`] reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventAttributeSource {
    /// `event.target_attr("data-x")` -> direct event target attribute.
    Target,
    /// `event.closest_attr("data-x")` -> closest ancestor with attribute.
    Closest,
    /// `event.current_target_attr("data-x")` -> matched handler element.
    CurrentTarget,
}

/// Numeric fields exposed from `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowField {
    /// `window.innerWidth`.
    InnerWidth,
    /// `window.innerHeight`.
    InnerHeight,
    /// `window.scrollX`.
    ScrollX,
    /// `window.scrollY`.
    ScrollY,
}

/// Browser storage area targeted by [`ClientBrowserRead::Storage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BrowserStorageArea {
    /// `window.localStorage`.
    Local,
    /// `window.sessionStorage`.
    Session,
}

/// Type-level parser used by [`ClientBrowserRead::Storage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StorageValueType {
    /// Raw string storage value.
    String,
    /// JSON boolean storage value.
    Bool,
    /// Canonical decimal i64 storage value, returned as BigInt.
    I64,
    /// JSON finite-number storage value.
    F64,
}

/// Encoding / binary transforms exposed through `rsjs::encoding::*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodingOp {
    /// `rsjs::encoding::utf8_bytes(text)` -> `TextEncoder.encode(...)`.
    Utf8Bytes,
    /// `rsjs::encoding::utf8_string(bytes)` -> `TextDecoder.decode(...)`.
    Utf8String,
    /// `rsjs::encoding::base64_encode(bytes)` -> base64 string.
    Base64Encode,
    /// `rsjs::encoding::base64_decode(text)` -> `Uint8Array`.
    Base64Decode,
    /// `rsjs::encoding::base64url_encode(bytes)` -> unpadded base64url string.
    Base64UrlEncode,
    /// `rsjs::encoding::base64url_decode(text)` -> `Uint8Array`.
    Base64UrlDecode,
}

/// Unary string transforms used by [`ClientExpr::StringTransform`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StringTransformOp {
    /// `target.to_uppercase()` -> `String(target ?? "").toUpperCase()`.
    ToUppercase,
    /// `target.trim()` -> an explicit Rust `char::is_whitespace`
    /// code-point trim, avoiding JavaScript's different native whitespace set.
    Trim,
}

/// Binary string predicates used by [`ClientExpr::StringPredicate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StringPredicateOp {
    /// `haystack.starts_with(needle)` ->
    /// `String(haystack ?? "").startsWith(String(needle ?? ""))`.
    StartsWith,
    /// `haystack.ends_with(needle)` ->
    /// `String(haystack ?? "").endsWith(String(needle ?? ""))`.
    EndsWith,
}

/// String-valued fields exposed from `window.location`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LocationField {
    /// `window.location.href`.
    Href,
    /// `window.location.origin`.
    Origin,
    /// `window.location.protocol`.
    Protocol,
    /// `window.location.host`.
    Host,
    /// `window.location.hostname`.
    Hostname,
    /// `window.location.port`.
    Port,
    /// `window.location.pathname`.
    Pathname,
    /// `window.location.search`.
    Search,
    /// `window.location.hash`.
    Hash,
}

/// Which projection of a query's state to read.
///
/// Defines the supported query-state read surface. Each variant has
/// a 1:1 JS runtime accessor (`__rsjs_query_<id>.data()` for `Data`,
/// etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum QueryAccessor {
    /// `query.data()`: the typed cached value, or undefined.
    Data,
    /// `query.status()`: one of `idle`, `loading`, `success`,
    /// `error`, `refetching`.
    Status,
    /// `query.is_loading()`: true while the very first fetch is
    /// in flight.
    IsLoading,
    /// `query.is_success()`: true once a successful fetch has
    /// completed.
    IsSuccess,
    /// `query.is_error()`: true while the cache holds an error.
    IsError,
    /// `query.is_refetching()`: true while a *subsequent* fetch
    /// is in flight (not the first one).
    IsRefetching,
    /// `query.is_stale()`: true after the configured
    /// `stale_time_ms` elapses.
    IsStale,
    /// `query.error()`: the error value if `is_error()`, otherwise
    /// undefined.
    Error,
    /// `query.data_updated_at()`: millisecond timestamp of the
    /// last successful fetch, or 0 if never.
    DataUpdatedAt,
    /// `query.error_updated_at()`: millisecond timestamp of the
    /// last failed fetch, or 0 if never.
    ErrorUpdatedAt,
    /// `query.fetch_count()`: number of fetches issued for this
    /// key.
    FetchCount,
    /// `query.download_loaded()`: bytes received for the current
    /// in-flight fetch, or the final byte count after completion.
    DownloadLoaded,
    /// `query.download_total()`: total response bytes from
    /// `Content-Length`, or `null` when the server did not provide a
    /// usable length.
    DownloadTotal,
    /// `query.download_percent()`: download progress in the range
    /// `0.0..=1.0` when the total length is known, otherwise `0.0`.
    DownloadPercent,
}

/// Which projection of a mutation's state to read.
///
/// Defines the supported mutation-state read surface. Each variant has a 1:1
/// JS runtime accessor on the mutation handle the runtime
/// exposes (`__rsjs_mutation_<id>.data()` for `Data`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MutationAccessor {
    /// `mutation.status()`: one of `idle`, `pending`, `success`,
    /// `error`.
    Status,
    /// `mutation.is_idle()`: true before any `run()` has fired.
    IsIdle,
    /// `mutation.is_pending()`: true while a `run()` is in flight.
    IsPending,
    /// `mutation.is_success()`: true once a successful `run()` has
    /// completed (until the next `run()` is fired).
    IsSuccess,
    /// `mutation.is_error()`: true if the most recent `run()`
    /// rejected.
    IsError,
    /// `mutation.data()`: the most recent successful `run()` result,
    /// or undefined.
    Data,
    /// `mutation.error()`: the most recent rejected `run()` error,
    /// or undefined.
    Error,
}

/// JSON value shapes that a projection terminator can assert.
///
/// Each variant defines the type-appropriate empty value the
/// lowered JS returns when the projection's shape does not match:
/// `""` for `Str`, `0.0` for `F64`, `0n` for `I64`, `false` for
/// `Bool`, `[]` for `Array`, `{}` for `Object`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AsType {
    /// JS string. Empty fallback: `""`.
    Str,
    /// JS number (treated as f64). Empty fallback: `0.0`.
    F64,
    /// Exact Rust `f64::from(i32_or_u32)` conversion. Unlike [`Self::F64`],
    /// this is not a JSON projection assertion: the client emitter preserves
    /// the ordinary Rust conversion as `Number(value)`.
    F64FromInt,
    /// Rust `i64`, represented as BigInt on the client. Safe integral Numbers
    /// and canonical in-range decimal strings are converted to BigInt;
    /// fractional, imprecise, malformed, and out-of-range values use the `0n`
    /// fallback.
    I64,
    /// JS boolean. Empty fallback: `false`.
    Bool,
    /// JS array. Empty fallback: `[]`.
    Array,
    /// JS object. Empty fallback: `{}`.
    Object,
}

/// A JSON Pointer (RFC 6901) string used as the path of an
/// [`ClientExpr::Project`] node.
///
/// Stored as the original string; the JS emitter parses it on
/// demand into the per-segment walk it lowers to. Storing the raw
/// string keeps round-trips deterministic and makes goldens
/// trivially human-readable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsonPointer(pub String);

impl JsonPointer {
    /// Construct a `JsonPointer` from a string. Does not validate;
    /// callers (typically the parser) are responsible for
    /// surfacing malformed pointers as errors before reaching the
    /// IR.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the raw JSON Pointer string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse the pointer into its constituent segments per
    /// RFC 6901.
    ///
    /// The empty pointer `""` is the root and yields no segments.
    /// Segments are unescaped: `~1` becomes `/`, `~0` becomes
    /// `~`.
    pub fn segments(&self) -> Vec<String> {
        if self.0.is_empty() {
            return Vec::new();
        }
        self.0
            .strip_prefix('/')
            .unwrap_or(&self.0)
            .split('/')
            .map(|seg| {
                let mut out = String::with_capacity(seg.len());
                let mut chars = seg.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '~' {
                        match chars.next() {
                            Some('0') => out.push('~'),
                            Some('1') => out.push('/'),
                            Some(other) => {
                                out.push('~');
                                out.push(other);
                            }
                            None => out.push('~'),
                        }
                    } else {
                        out.push(c);
                    }
                }
                out
            })
            .collect()
    }
}
