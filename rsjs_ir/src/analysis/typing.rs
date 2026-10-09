// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Whole-island typing: assign a [`ClientType`] to every
//! [`ClientExpr`] given island context.
//!
//! # Representation choice
//!
//! Types are **computed on demand, not stored on expression nodes**.
//! `ClientExpr` nodes carry no ids and no type slots, so a stored
//! side table would either key on node addresses (fragile under the
//! rewrites optimization passes perform) or require threading fresh
//! ids through every constructor in the macro frontend, the JSON
//! parsers, and the hand-built test fixtures. Instead, [`TypeEnv`]
//! bundles the island context a query needs (signal declarations,
//! the expression arena for derived-cell bodies, and a local-binding
//! environment built from [`ClientStmt::Let`] / `ForRange` bindings)
//! and [`TypeEnv::type_of`] recomputes the type of any expression
//! structurally. Typing is a cheap syntax-directed walk, expressions
//! are small, and recomputation can never go stale. Dependency collection
//! follows the same derived-data model.
//!
//! # Types mean client representation
//!
//! As documented on [`crate::reactive::SignalDecl::ty`], a
//! [`ClientType`] describes the runtime value shape on the client.
//! Fixed-width integer expressions also carry an [`IntegerDomain`], which is
//! the exact Rust signedness/width used for checked operations:
//!
//! - [`ClientType::I32`] / [`ClientType::U32`] are Number-backed
//!   integers. Their JS values remain `number` for browser interoperability,
//!   while checked operations enforce the exact `i32` or `u32` domain.
//! - [`ClientType::I64`] / [`ClientType::U64`] are BigInt-backed
//!   integers at every magnitude. Integer literals meeting them become BigInt
//!   literals, and checked operations enforce the exact `i64` or `u64`
//!   domain before mutation.
//! - [`ClientType::F32`] / [`ClientType::F64`] are JS `number`s with
//!   no integer proof.
//! - [`ClientType::Unknown`] is the honest fallback. Numeric
//!   rewrites must treat it as "no proof" and change nothing.
//!
//! The accepted client-arithmetic domains are exactly `i32`, `u32`, `i64`,
//! and `u64`. Boundary codecs may transport fields of other integer widths,
//! but doing so never assigns those values an arithmetic domain here.
//!
//! Closure parameters need no environment entry: the macro frontend
//! lowers `signal.update(|c| ...)` parameter references to
//! [`ClientExpr::SignalRead`] of the receiving signal, so an update
//! closure parameter is typed from the receiving signal's
//! declaration automatically.

use std::collections::HashMap;

use crate::client::{
    AsType, ClientBinaryOp, ClientBlock, ClientBrowserRead, ClientExpr, ClientLocalId,
    ClientObjectField, ClientStmt, ClientType, ClientUnaryOp, EventCollectionField, EventValue,
    IntegerDomain, MutationAccessor, NodeRefAccessor, QueryAccessor, StorageValueType, WindowField,
};
use crate::island::{ExprArena, Island};
use crate::reactive::{QueryDecl, SignalDecl};
use crate::types::{QueryId, SignalId};

/// Client-side numeric representation class of a [`ClientType`].
///
/// This is the representation projection consumed by numeric lowering.
/// Exact overflow and signedness behavior comes from [`IntegerDomain`];
/// this enum distinguishes Number, BigInt, and runtime-selected values for
/// literal representation and mixing checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumericWidth {
    /// Number-backed integer ([`ClientType::I32`] / [`ClientType::U32`]).
    NumberInt,
    /// Number-backed float ([`ClientType::F32`] / [`ClientType::F64`]).
    NumberFloat,
    /// BigInt-backed integer ([`ClientType::I64`] / [`ClientType::U64`]).
    BigInt,
    /// A scalar whose representation the page seed pins at runtime
    /// ([`ClientType::Dyn`]). Neither Number-backed nor BigInt-backed as far
    /// as the compiler is concerned.
    Dynamic,
}

impl NumericWidth {
    /// `true` for the two Number-backed classes.
    pub fn is_number(self) -> bool {
        matches!(self, Self::NumberInt | Self::NumberFloat)
    }
}

/// Numeric representation class of a [`ClientType`], or `None` for
/// non-numeric and [`ClientType::Unknown`] types.
pub fn numeric_width(ty: &ClientType) -> Option<NumericWidth> {
    match ty {
        ClientType::I32 | ClientType::U32 => Some(NumericWidth::NumberInt),
        ClientType::F32 | ClientType::F64 => Some(NumericWidth::NumberFloat),
        ClientType::I64 | ClientType::U64 => Some(NumericWidth::BigInt),
        ClientType::Dyn => Some(NumericWidth::Dynamic),
        _ => None,
    }
}

/// Runtime numeric anchors found inside a numeric expression tree.
///
/// An *anchor* is a non-literal numeric producer (a signal read, a
/// local read, an event read, ...) whose representation is pinned at
/// runtime. Literals are deliberately not anchors: an unsuffixed literal can
/// inherit a proven surrounding domain, while an anchored read cannot change
/// domains. Numeric lowering uses anchors to promote contextual literals and
/// reject genuinely mixed fixed-width expressions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NumericAnchors {
    /// A Number-backed producer (int or float) appears in a numeric
    /// position.
    pub number: bool,
    /// A BigInt-backed producer appears in a numeric position.
    pub wide: bool,
    /// A representation-pinned-at-runtime producer (a shared-state field, or
    /// anything derived from one) appears in a numeric position. The compiler
    /// cannot prove its width, so literal coercion and mixed-width rejection
    /// must both stand down and the operator lowers to a `Dyn*` runtime helper.
    pub dynamic: bool,
}

impl NumericAnchors {
    fn merge(self, other: Self) -> Self {
        Self {
            number: self.number || other.number,
            wide: self.wide || other.wide,
            dynamic: self.dynamic || other.dynamic,
        }
    }
}

/// Island typing context: signal declarations, the expression arena
/// (for typing memo-cell bodies), and the local environment of
/// the block being typed.
///
/// Construct with [`TypeEnv::new`] (from builder parts) or
/// [`TypeEnv::for_island`], bind block locals with
/// [`TypeEnv::bind_local`] (or let [`block_types`] do it), then query
/// [`TypeEnv::type_of`].
pub struct TypeEnv<'a> {
    signals: &'a [SignalDecl],
    queries: &'a [QueryDecl],
    exprs: Option<&'a ExprArena>,
    locals: HashMap<ClientLocalId, ClientType>,
    local_integer_domains: HashMap<ClientLocalId, IntegerDomain>,
}

impl<'a> TypeEnv<'a> {
    /// Construct a typing context from island parts. `exprs` is
    /// needed only to resolve memo (derived) cell types whose
    /// declarations were not backfilled; pass `None` when no arena
    /// exists yet.
    pub fn new(signals: &'a [SignalDecl], exprs: Option<&'a ExprArena>) -> Self {
        Self {
            signals,
            queries: &[],
            exprs,
            locals: HashMap::new(),
            local_integer_domains: HashMap::new(),
        }
    }

    /// Construct a typing context from island parts including query
    /// declarations.
    pub fn new_with_queries(
        signals: &'a [SignalDecl],
        queries: &'a [QueryDecl],
        exprs: Option<&'a ExprArena>,
    ) -> Self {
        Self {
            signals,
            queries,
            exprs,
            locals: HashMap::new(),
            local_integer_domains: HashMap::new(),
        }
    }

    /// Construct a typing context for a finished [`Island`].
    pub fn for_island(island: &'a Island) -> Self {
        Self::new_with_queries(&island.signals, &island.queries, Some(&island.exprs))
    }

    /// Bind a block-local to a type. Local ids are dense per handler
    /// body, so a single environment per body is sufficient; nested
    /// blocks never reuse an id for a different binding.
    pub fn bind_local(&mut self, local: ClientLocalId, ty: ClientType) {
        self.locals.insert(local, ty);
    }

    /// Bind a block-local's proven Rust fixed-width integer domain.
    pub fn bind_local_integer_domain(&mut self, local: ClientLocalId, domain: IntegerDomain) {
        self.local_integer_domains.insert(local, domain);
    }

    /// Type of a block-local, or `Unknown` if unbound.
    pub fn local_type(&self, local: ClientLocalId) -> ClientType {
        self.locals
            .get(&local)
            .cloned()
            .unwrap_or(ClientType::Unknown)
    }

    /// Proven Rust fixed-width integer domain of a block-local.
    pub fn local_integer_domain(&self, local: ClientLocalId) -> Option<IntegerDomain> {
        self.local_integer_domains.get(&local).copied()
    }

    /// Declared client value shape of a signal, resolving memo
    /// (derived) cells through their body when the declaration was
    /// not backfilled. Unallocated ids type as `Unknown`.
    pub fn signal_type(&self, id: SignalId) -> ClientType {
        let mut visiting = Vec::new();
        self.signal_type_inner(id, &mut visiting)
    }

    /// Proven Rust fixed-width integer domain of a signal declaration.
    pub fn signal_integer_domain(&self, id: SignalId) -> Option<IntegerDomain> {
        let mut visiting = Vec::new();
        self.signal_integer_domain_inner(id, &mut visiting)
    }

    /// Declared client value shape of a query's `data()` accessor.
    pub fn query_data_type(&self, id: QueryId) -> ClientType {
        self.queries
            .get(id.0 as usize)
            .map(|decl| decl.data_ty.clone())
            .unwrap_or(ClientType::Unknown)
    }

    fn signal_type_inner(&self, id: SignalId, visiting: &mut Vec<SignalId>) -> ClientType {
        let Some(decl) = self.signals.get(id.0 as usize) else {
            return ClientType::Unknown;
        };
        if !decl.ty.is_unknown() {
            return decl.ty.clone();
        }
        let (Some(body), Some(exprs)) = (decl.derived, self.exprs) else {
            return ClientType::Unknown;
        };
        // Memo cells share the signal id space and may (in
        // hand-built IR) form reference cycles; bail out instead of
        // recursing forever. Macro-built islands declare memo
        // bodies in source order, so the guard never trips there.
        if visiting.contains(&id) {
            return ClientType::Unknown;
        }
        if body.0 as usize >= exprs.len() {
            return ClientType::Unknown;
        }
        visiting.push(id);
        let ty = self.type_of_inner(exprs.client_expr(body), visiting);
        visiting.pop();
        ty
    }

    fn signal_integer_domain_inner(
        &self,
        id: SignalId,
        visiting: &mut Vec<SignalId>,
    ) -> Option<IntegerDomain> {
        let decl = self.signals.get(id.0 as usize)?;
        if let Some(domain) = decl.integer_domain {
            return Some(domain);
        }
        let (Some(body), Some(exprs)) = (decl.derived, self.exprs) else {
            return None;
        };
        if visiting.contains(&id) || body.0 as usize >= exprs.len() {
            return None;
        }
        visiting.push(id);
        let domain = self.integer_domain_of_inner(exprs.client_expr(body), visiting);
        visiting.pop();
        domain
    }

    /// Proven Rust fixed-width integer domain of an expression.
    ///
    /// Plain literals intentionally return `None`: their Rust type may be
    /// inferred from the surrounding operand. The macro frontend resolves
    /// those source literals before it creates a checked operator, whose
    /// domain is then durable in Client IR.
    pub fn integer_domain_of(&self, expr: &ClientExpr) -> Option<IntegerDomain> {
        let mut visiting = Vec::new();
        self.integer_domain_of_inner(expr, &mut visiting)
    }

    fn integer_domain_of_inner(
        &self,
        expr: &ClientExpr,
        visiting: &mut Vec<SignalId>,
    ) -> Option<IntegerDomain> {
        match expr {
            ClientExpr::Local(id) => self.local_integer_domain(*id),
            ClientExpr::SignalRead(id) => self.signal_integer_domain_inner(*id, visiting),
            ClientExpr::Binary {
                op: ClientBinaryOp::Checked { domain, .. },
                ..
            }
            | ClientExpr::Unary {
                op: ClientUnaryOp::CheckedNeg(domain),
                ..
            } => Some(*domain),
            ClientExpr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let then_domain = self.integer_domain_of_inner(then_expr, visiting);
                let else_domain = self.integer_domain_of_inner(else_expr, visiting);
                (then_domain == else_domain)
                    .then_some(then_domain)
                    .flatten()
            }
            ClientExpr::QueryRead {
                accessor: QueryAccessor::FetchCount,
                ..
            }
            | ClientExpr::EventCollectionLen(_) => Some(IntegerDomain::U32),
            ClientExpr::EventRead(value) => client_type_integer_domain(&event_value_type(*value)),
            ClientExpr::EventCollectionRead { field, .. } => {
                client_type_integer_domain(&event_collection_field_type(*field))
            }
            ClientExpr::NodeRefRead { accessor, .. } => {
                client_type_integer_domain(&node_ref_accessor_type(accessor))
            }
            ClientExpr::DomRead { accessor, .. } => {
                client_type_integer_domain(&node_ref_accessor_type(accessor))
            }
            ClientExpr::BrowserRead(read) => client_type_integer_domain(&browser_read_type(read)),
            ClientExpr::AsType {
                ty: AsType::I64, ..
            } => Some(IntegerDomain::I64),
            _ => None,
        }
    }

    /// Client value shape of an expression under this environment.
    /// `Unknown` is the honest answer for anything the island context
    /// cannot prove.
    pub fn type_of(&self, expr: &ClientExpr) -> ClientType {
        let mut visiting = Vec::new();
        self.type_of_inner(expr, &mut visiting)
    }

    fn type_of_inner(&self, expr: &ClientExpr, visiting: &mut Vec<SignalId>) -> ClientType {
        match expr {
            ClientExpr::Unit => ClientType::Unit,
            ClientExpr::Literal(value) => ClientType::of_value(value),
            ClientExpr::ArrayLit(elems) => {
                let mut elem_ty: Option<ClientType> = None;
                for elem in elems {
                    let ty = ClientType::of_value(elem);
                    match &elem_ty {
                        None => elem_ty = Some(ty),
                        Some(seen) if *seen == ty => {}
                        Some(_) => return ClientType::vec(ClientType::Unknown),
                    }
                }
                ClientType::vec(elem_ty.unwrap_or(ClientType::Unknown))
            }
            ClientExpr::QueryKey(_) => ClientType::vec(ClientType::Unknown),
            ClientExpr::Local(id) => self.local_type(*id),
            ClientExpr::SignalRead(id) => self.signal_type_inner(*id, visiting),
            ClientExpr::QueryRead { id, accessor } => match accessor {
                QueryAccessor::Data => self.query_data_type(*id),
                _ => query_accessor_type(*accessor),
            },
            ClientExpr::MutationRead { accessor, .. } => mutation_accessor_type(*accessor),
            ClientExpr::EventRead(value) => event_value_type(*value),
            ClientExpr::EventCurrentTarget => ClientType::DomElement,
            ClientExpr::DomRead { accessor, .. } => node_ref_accessor_type(accessor),
            // DOM collection `length` attributes are Web IDL `unsigned long`.
            ClientExpr::EventCollectionLen(_) => ClientType::U32,
            ClientExpr::EventCollectionRead { field, .. } => event_collection_field_type(*field),
            ClientExpr::NodeRefRead { accessor, .. } => node_ref_accessor_type(accessor),
            ClientExpr::EventTargetWithinNodeRef { .. } => ClientType::Bool,
            ClientExpr::EventAttribute { .. } => ClientType::String,
            // Payload attributes are serialized as JSON strings in the DOM,
            // but `JSON.parse` restores the original Rust value shape at the
            // client expression boundary. Without source-schema metadata that
            // shape is unknown; treating the transport as the value type would
            // incorrectly reject boolean/numeric loop fields.
            ClientExpr::EventPayload { .. }
            | ClientExpr::NodePayload { .. }
            | ClientExpr::CallbackPayload => ClientType::Unknown,
            ClientExpr::BrowserRead(read) => browser_read_type(read),
            ClientExpr::Encoding { op, .. } => {
                use crate::client::EncodingOp;
                match op {
                    EncodingOp::Utf8String
                    | EncodingOp::Base64Encode
                    | EncodingOp::Base64UrlEncode => ClientType::String,
                    // Byte-producing transforms yield a Uint8Array,
                    // which has no ClientType shape yet.
                    _ => ClientType::Unknown,
                }
            }
            ClientExpr::Conditional {
                then_expr,
                else_expr,
                ..
            } => unify(
                self.type_of_inner(then_expr, visiting),
                self.type_of_inner(else_expr, visiting),
            ),
            ClientExpr::IsNonEmpty { .. }
            | ClientExpr::StringContains { .. }
            | ClientExpr::StringPredicate { .. }
            | ClientExpr::IsPresent { .. }
            | ClientExpr::Eq { .. }
            | ClientExpr::Neq { .. } => ClientType::Bool,
            ClientExpr::StringToLowercase { .. } | ClientExpr::StringTransform { .. } => {
                ClientType::String
            }
            ClientExpr::Project { target, path } => {
                self.type_of_inner(target, visiting).project_path(path)
            }
            ClientExpr::AsType { ty, .. } => as_type_type(*ty),
            ClientExpr::Coalesce { target, fallback } => unify(
                self.type_of_inner(target, visiting),
                ClientType::of_value(fallback),
            ),
            ClientExpr::Unary { op, expr } => match op {
                ClientUnaryOp::Not => ClientType::Bool,
                ClientUnaryOp::Neg => {
                    let inner = self.type_of_inner(expr, visiting);
                    if numeric_width(&inner).is_some() {
                        inner
                    } else {
                        ClientType::Unknown
                    }
                }
                ClientUnaryOp::CheckedNeg(domain) => domain.client_type(),
            },
            ClientExpr::Binary { op, left, right } => match op {
                ClientBinaryOp::Eq
                | ClientBinaryOp::Ne
                | ClientBinaryOp::Lt
                | ClientBinaryOp::Le
                | ClientBinaryOp::Gt
                | ClientBinaryOp::Ge
                | ClientBinaryOp::And
                | ClientBinaryOp::Or => ClientType::Bool,
                ClientBinaryOp::DynEq | ClientBinaryOp::DynNe => ClientType::Bool,
                // A representation-dispatching helper returns whatever
                // representation its operands carried, so the result is itself
                // runtime-pinned. Typing it `Dyn` (rather than `Unknown`) is
                // what lets `(count.get() + 1) * 2` keep dispatching on the
                // outer `*` instead of emitting an inline operator over a
                // BigInt.
                ClientBinaryOp::DynAdd
                | ClientBinaryOp::DynSub
                | ClientBinaryOp::DynMul
                | ClientBinaryOp::DynDiv
                | ClientBinaryOp::DynDivInt
                | ClientBinaryOp::DynRem => ClientType::Dyn,
                ClientBinaryOp::Checked { domain, .. } => domain.client_type(),
                ClientBinaryOp::Add
                | ClientBinaryOp::Sub
                | ClientBinaryOp::Mul
                | ClientBinaryOp::Div
                | ClientBinaryOp::DivInt
                | ClientBinaryOp::Rem => unify_arithmetic(
                    self.type_of_inner(left, visiting),
                    self.type_of_inner(right, visiting),
                ),
            },
            ClientExpr::StructInit { fields, .. } => ClientType::object(
                fields
                    .iter()
                    .map(|field| ClientObjectField {
                        name: field.name.clone(),
                        ty: self.type_of_inner(&field.value, visiting),
                    })
                    .collect::<Vec<_>>(),
            ),
            ClientExpr::Field { base, name } => match self.type_of_inner(base, visiting) {
                ClientType::Object(fields) => fields
                    .into_iter()
                    .find(|field| field.name == *name)
                    .map(|field| field.ty)
                    .unwrap_or(ClientType::Unknown),
                _ => ClientType::Unknown,
            },
            // Helper calls and dynamic indexing have no proof; stay honest.
            ClientExpr::Call {
                function: crate::ClientFunctionRef::Runtime(name),
                ..
            } if name == "format_text" || name == "format_text_attr" => ClientType::String,
            ClientExpr::Index { .. } | ClientExpr::Call { .. } => ClientType::Unknown,
        }
    }

    /// Collect the runtime numeric anchors of a numeric subtree: the
    /// representation classes of every non-literal numeric producer,
    /// recursing through arithmetic operands and conditional arms
    /// exactly like literal coercion does.
    pub fn numeric_anchors(&self, expr: &ClientExpr) -> NumericAnchors {
        match expr {
            ClientExpr::Literal(_) => NumericAnchors::default(),
            ClientExpr::Binary {
                op:
                    ClientBinaryOp::Add
                    | ClientBinaryOp::Sub
                    | ClientBinaryOp::Mul
                    | ClientBinaryOp::Div
                    | ClientBinaryOp::DivInt
                    | ClientBinaryOp::Rem
                    | ClientBinaryOp::DynAdd
                    | ClientBinaryOp::DynSub
                    | ClientBinaryOp::DynMul
                    | ClientBinaryOp::DynDiv
                    | ClientBinaryOp::DynDivInt
                    | ClientBinaryOp::DynRem
                    | ClientBinaryOp::Checked { .. },
                left,
                right,
            } => self
                .numeric_anchors(left)
                .merge(self.numeric_anchors(right)),
            ClientExpr::Conditional {
                then_expr,
                else_expr,
                ..
            } => self
                .numeric_anchors(then_expr)
                .merge(self.numeric_anchors(else_expr)),
            ClientExpr::Unary {
                op: ClientUnaryOp::Neg | ClientUnaryOp::CheckedNeg(_),
                expr,
            } => self.numeric_anchors(expr),
            other => match numeric_width(&self.type_of(other)) {
                Some(NumericWidth::Dynamic) => NumericAnchors {
                    number: false,
                    wide: false,
                    dynamic: true,
                },
                Some(width) if width.is_number() => NumericAnchors {
                    number: true,
                    wide: false,
                    dynamic: false,
                },
                Some(_) => NumericAnchors {
                    number: false,
                    wide: true,
                    dynamic: false,
                },
                None => NumericAnchors::default(),
            },
        }
    }
}

/// Walk a [`ClientBlock`], binding every `Let` / `ForRange` local
/// into `env` so later [`TypeEnv::type_of`] queries see them.
///
/// A `Let` local takes the type of its initializer when the
/// environment can prove one, falling back to the declared
/// [`ClientStmt::Let`] `ty`. `ForRange` locals retain the exact fixed-width
/// integer domain proved by the source frontend.
pub fn block_types(env: &mut TypeEnv<'_>, block: &ClientBlock) {
    for stmt in &block.stmts {
        match stmt {
            ClientStmt::Let {
                local, ty, value, ..
            } => {
                let bound = match value {
                    Some(value) => {
                        let inferred = env.type_of(value);
                        if inferred.is_unknown() {
                            ty.clone()
                        } else {
                            inferred
                        }
                    }
                    None => ty.clone(),
                };
                env.bind_local(*local, bound);
                if let Some(value) = value {
                    if let Some(domain) = env.integer_domain_of(value) {
                        env.bind_local_integer_domain(*local, domain);
                    }
                }
            }
            ClientStmt::ForRange {
                local,
                integer_domain,
                body,
                ..
            } => {
                env.bind_local(*local, integer_domain.client_type());
                env.bind_local_integer_domain(*local, *integer_domain);
                block_types(env, body);
            }
            ClientStmt::Block(inner) => block_types(env, inner),
            ClientStmt::If {
                then_block,
                else_block,
                ..
            } => {
                block_types(env, then_block);
                if let Some(else_block) = else_block {
                    block_types(env, else_block);
                }
            }
            _ => {}
        }
    }
}

/// Unify two types: equal types stand, everything else is `Unknown`.
fn unify(a: ClientType, b: ClientType) -> ClientType {
    if a == b {
        a
    } else {
        ClientType::Unknown
    }
}

/// Unify arithmetic operand types into a result type. Same-width
/// operands keep their (left) type; mixing the two Number-backed
/// classes yields `F64` (JS arithmetic on Numbers produces a Number
/// with no integer proof); anything involving `Unknown` or a
/// Number/BigInt mix stays `Unknown`.
fn unify_arithmetic(left: ClientType, right: ClientType) -> ClientType {
    // A runtime-pinned operand absorbs: the result's representation is whatever
    // the helper produced, which is only known at run time.
    if left.is_dyn() || right.is_dyn() {
        return ClientType::Dyn;
    }
    let (Some(lw), Some(rw)) = (numeric_width(&left), numeric_width(&right)) else {
        return ClientType::Unknown;
    };
    if lw == rw {
        return left;
    }
    match (lw, rw) {
        (NumericWidth::NumberInt, NumericWidth::NumberFloat)
        | (NumericWidth::NumberFloat, NumericWidth::NumberInt) => ClientType::F64,
        _ => ClientType::Unknown,
    }
}

/// Client value shape of a [`QueryAccessor`] read.
pub fn query_accessor_type(accessor: QueryAccessor) -> ClientType {
    match accessor {
        QueryAccessor::Status => ClientType::String,
        QueryAccessor::IsLoading
        | QueryAccessor::IsSuccess
        | QueryAccessor::IsError
        | QueryAccessor::IsRefetching
        | QueryAccessor::IsStale => ClientType::Bool,
        // Timestamps, byte counters, and progress are plain JS Numbers.
        // The public Rust authoring stubs intentionally expose them as f64
        // rather than claiming exact u64 values that JavaScript cannot
        // represent across the full domain.
        QueryAccessor::DataUpdatedAt
        | QueryAccessor::ErrorUpdatedAt
        | QueryAccessor::DownloadLoaded
        | QueryAccessor::DownloadPercent => ClientType::F64,
        QueryAccessor::FetchCount => ClientType::U32,
        QueryAccessor::DownloadTotal => ClientType::option(ClientType::F64),
        QueryAccessor::Data | QueryAccessor::Error => ClientType::Unknown,
    }
}

/// Client value shape of a [`MutationAccessor`] read.
pub fn mutation_accessor_type(accessor: MutationAccessor) -> ClientType {
    match accessor {
        MutationAccessor::Status => ClientType::String,
        MutationAccessor::IsIdle
        | MutationAccessor::IsPending
        | MutationAccessor::IsSuccess
        | MutationAccessor::IsError => ClientType::Bool,
        MutationAccessor::Data | MutationAccessor::Error => ClientType::Unknown,
    }
}

/// Client value shape of an [`EventValue`] read.
pub fn event_value_type(value: EventValue) -> ClientType {
    match value {
        EventValue::TargetChecked
        | EventValue::CurrentTargetChecked
        | EventValue::Repeat
        | EventValue::ShiftKey
        | EventValue::MetaKey
        | EventValue::CtrlKey
        | EventValue::AltKey
        | EventValue::IsPrimary => ClientType::Bool,
        EventValue::Button
        | EventValue::Detail
        | EventValue::PointerId
        | EventValue::TiltX
        | EventValue::TiltY
        | EventValue::ChangedTouchIdentifier => ClientType::I32,
        EventValue::Buttons
        | EventValue::DetailU32
        | EventValue::Twist
        | EventValue::DeltaMode
        | EventValue::DataTransferFileCount
        | EventValue::TargetFileCount => ClientType::U32,
        EventValue::ClientX
        | EventValue::ClientY
        | EventValue::PageX
        | EventValue::PageY
        | EventValue::ScreenX
        | EventValue::ScreenY
        | EventValue::OffsetX
        | EventValue::OffsetY
        | EventValue::MovementX
        | EventValue::MovementY
        | EventValue::Pressure
        | EventValue::TangentialPressure
        | EventValue::PointerWidth
        | EventValue::PointerHeight
        | EventValue::DeltaX
        | EventValue::DeltaY
        | EventValue::DeltaZ
        | EventValue::CurrentTargetScrollTop
        | EventValue::CurrentTargetScrollLeft
        | EventValue::ElapsedTime
        | EventValue::DataTransferFirstFileSize
        | EventValue::TargetFirstFileSize
        | EventValue::ChangedTouchClientX
        | EventValue::ChangedTouchClientY
        | EventValue::ChangedTouchPageX
        | EventValue::ChangedTouchPageY
        | EventValue::ChangedTouchScreenX
        | EventValue::ChangedTouchScreenY => ClientType::F64,
        _ => ClientType::String,
    }
}

/// Client value shape of an [`EventCollectionField`] read.
pub fn event_collection_field_type(field: EventCollectionField) -> ClientType {
    match field {
        EventCollectionField::FileName
        | EventCollectionField::FileType
        | EventCollectionField::DataTransferItemKind
        | EventCollectionField::DataTransferItemType => ClientType::String,
        EventCollectionField::TouchIdentifier => ClientType::I32,
        EventCollectionField::TouchClientX
        | EventCollectionField::TouchClientY
        | EventCollectionField::TouchPageX
        | EventCollectionField::TouchPageY
        | EventCollectionField::TouchScreenX
        | EventCollectionField::TouchScreenY
        | EventCollectionField::FileSize => ClientType::F64,
    }
}

/// Client value shape of a [`ClientBrowserRead`].
pub fn browser_read_type(read: &ClientBrowserRead) -> ClientType {
    match read {
        ClientBrowserRead::Window(WindowField::InnerWidth | WindowField::InnerHeight) => {
            ClientType::I32
        }
        ClientBrowserRead::Window(WindowField::ScrollX | WindowField::ScrollY) => ClientType::F64,
        ClientBrowserRead::Location(_) => ClientType::String,
        ClientBrowserRead::Storage { value_type, .. } => match value_type {
            StorageValueType::String => ClientType::String,
            StorageValueType::Bool => ClientType::Bool,
            // `get_i64_or` preserves the full Rust `i64` domain as a JS BigInt.
            StorageValueType::I64 => ClientType::I64,
            StorageValueType::F64 => ClientType::F64,
        },
        ClientBrowserRead::UrlSearchParam { .. } => ClientType::String,
        // Registry-declared reads carry their type on the registry
        // entry. An unknown id types as Unknown; JS emission rejects
        // it with a proper error.
        ClientBrowserRead::Declared { helper, .. } => crate::browser_registry::find_by_id(helper)
            .map(|spec| spec.value_type.clone())
            .unwrap_or(ClientType::Unknown),
    }
}

/// Client value shape of a [`NodeRefAccessor`] read.
pub fn node_ref_accessor_type(accessor: &NodeRefAccessor) -> ClientType {
    match accessor {
        NodeRefAccessor::Checked
        | NodeRefAccessor::CheckValidity
        | NodeRefAccessor::ValidityValid
        | NodeRefAccessor::WillValidate
        | NodeRefAccessor::HasAttribute { .. } => ClientType::Bool,
        NodeRefAccessor::SelectionStart
        | NodeRefAccessor::SelectionEnd
        | NodeRefAccessor::FileCount => ClientType::U32,
        NodeRefAccessor::ClientWidth
        | NodeRefAccessor::ClientHeight
        | NodeRefAccessor::ScrollWidth
        | NodeRefAccessor::ScrollHeight
        | NodeRefAccessor::OffsetWidth
        | NodeRefAccessor::OffsetHeight => ClientType::I32,
        NodeRefAccessor::ScrollTop
        | NodeRefAccessor::ScrollLeft
        | NodeRefAccessor::RectX
        | NodeRefAccessor::RectY
        | NodeRefAccessor::RectTop
        | NodeRefAccessor::RectRight
        | NodeRefAccessor::RectBottom
        | NodeRefAccessor::RectLeft
        | NodeRefAccessor::RectWidth
        | NodeRefAccessor::RectHeight
        | NodeRefAccessor::FirstFileSize => ClientType::F64,
        _ => ClientType::String,
    }
}

/// Client value shape of an [`AsType`] projection terminator.
///
/// `as_i64()` always returns an in-range BigInt on the client. Safe integral
/// Numbers and canonical decimal strings are converted to BigInt; fractional,
/// unsafe, malformed, and out-of-range values use the `0n` fallback, mirroring
/// the Rust projection fallback. A JSON numeric token beyond JavaScript's safe
/// integer range has already lost precision and is deliberately rejected.
pub fn as_type_type(ty: AsType) -> ClientType {
    match ty {
        AsType::Str => ClientType::String,
        AsType::F64 | AsType::F64FromInt => ClientType::F64,
        AsType::Bool => ClientType::Bool,
        AsType::Array => ClientType::vec(ClientType::Unknown),
        AsType::I64 => ClientType::I64,
        AsType::Object => ClientType::Unknown,
    }
}

fn client_type_integer_domain(ty: &ClientType) -> Option<IntegerDomain> {
    match ty {
        ClientType::I32 => Some(IntegerDomain::I32),
        ClientType::U32 => Some(IntegerDomain::U32),
        ClientType::I64 => Some(IntegerDomain::I64),
        ClientType::U64 => Some(IntegerDomain::U64),
        _ => None,
    }
}

/// Backfill [`SignalDecl::ty`] for memo (derived) cells of a
/// finished island from their body types. Idempotent; leaves
/// non-derived declarations untouched.
pub fn backfill_derived_signal_types(island: &mut Island) {
    let mut updates: Vec<(usize, ClientType, Option<IntegerDomain>)> = Vec::new();
    {
        let env = TypeEnv::for_island(island);
        for (idx, decl) in island.signals.iter().enumerate() {
            if decl.derived.is_some() && decl.ty.is_unknown() {
                let ty = env.signal_type(decl.id);
                if !ty.is_unknown() {
                    updates.push((idx, ty, env.signal_integer_domain(decl.id)));
                }
            }
        }
    }
    for (idx, ty, integer_domain) in updates {
        island.signals[idx].ty = ty;
        island.signals[idx].integer_domain = integer_domain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::IslandBuilder;
    use crate::client::ClientObjectField;
    use crate::types::ExprId;
    use crate::value::Value;

    fn int_signal(b: &mut IslandBuilder) -> SignalId {
        b.signal(Value::Int(0))
    }

    #[test]
    fn literals_type_by_representation() {
        let env = TypeEnv::new(&[], None);
        assert_eq!(env.type_of(&ClientExpr::int(1)), ClientType::I32);
        assert_eq!(
            env.type_of(&ClientExpr::Literal(Value::BigInt(9_007_199_254_740_993))),
            ClientType::I64
        );
        assert_eq!(env.type_of(&ClientExpr::float(1.5)), ClientType::F64);
        assert_eq!(env.type_of(&ClientExpr::string("x")), ClientType::String);
        assert_eq!(env.type_of(&ClientExpr::bool(true)), ClientType::Bool);
    }

    #[test]
    fn serialized_payload_transport_does_not_claim_a_source_value_type() {
        let env = TypeEnv::new(&[], None);
        assert_eq!(
            env.type_of(&ClientExpr::EventPayload {
                attr: "data-rsjs-payload-p0".to_string(),
            }),
            ClientType::Unknown
        );
        assert_eq!(
            env.type_of(&ClientExpr::NodePayload {
                attr: "data-rsjs-payload-p1".to_string(),
                tokens: "item.disabled".to_string(),
            }),
            ClientType::Unknown
        );
    }

    #[test]
    fn browser_numeric_types_match_public_rust_stubs() {
        for field in [WindowField::InnerWidth, WindowField::InnerHeight] {
            assert_eq!(
                browser_read_type(&ClientBrowserRead::Window(field)),
                ClientType::I32
            );
        }
        for field in [WindowField::ScrollX, WindowField::ScrollY] {
            assert_eq!(
                browser_read_type(&ClientBrowserRead::Window(field)),
                ClientType::F64
            );
        }

        for accessor in [
            NodeRefAccessor::SelectionStart,
            NodeRefAccessor::SelectionEnd,
            NodeRefAccessor::FileCount,
        ] {
            assert_eq!(node_ref_accessor_type(&accessor), ClientType::U32);
        }
        for accessor in [
            NodeRefAccessor::ClientWidth,
            NodeRefAccessor::ClientHeight,
            NodeRefAccessor::ScrollWidth,
            NodeRefAccessor::ScrollHeight,
            NodeRefAccessor::OffsetWidth,
            NodeRefAccessor::OffsetHeight,
        ] {
            assert_eq!(node_ref_accessor_type(&accessor), ClientType::I32);
        }
        for accessor in [
            NodeRefAccessor::ScrollTop,
            NodeRefAccessor::ScrollLeft,
            NodeRefAccessor::RectX,
            NodeRefAccessor::RectY,
            NodeRefAccessor::RectTop,
            NodeRefAccessor::RectRight,
            NodeRefAccessor::RectBottom,
            NodeRefAccessor::RectLeft,
            NodeRefAccessor::RectWidth,
            NodeRefAccessor::RectHeight,
            NodeRefAccessor::FirstFileSize,
        ] {
            assert_eq!(node_ref_accessor_type(&accessor), ClientType::F64);
        }

        for value in [
            EventValue::Button,
            EventValue::Detail,
            EventValue::PointerId,
            EventValue::TiltX,
            EventValue::TiltY,
            EventValue::ChangedTouchIdentifier,
        ] {
            assert_eq!(event_value_type(value), ClientType::I32);
        }
        for value in [
            EventValue::Buttons,
            EventValue::DetailU32,
            EventValue::Twist,
            EventValue::DeltaMode,
            EventValue::DataTransferFileCount,
            EventValue::TargetFileCount,
        ] {
            assert_eq!(event_value_type(value), ClientType::U32);
        }
        for value in [
            EventValue::ClientX,
            EventValue::ClientY,
            EventValue::PageX,
            EventValue::PageY,
            EventValue::ScreenX,
            EventValue::ScreenY,
            EventValue::OffsetX,
            EventValue::OffsetY,
            EventValue::MovementX,
            EventValue::MovementY,
            EventValue::Pressure,
            EventValue::TangentialPressure,
            EventValue::PointerWidth,
            EventValue::PointerHeight,
            EventValue::DeltaX,
            EventValue::DeltaY,
            EventValue::DeltaZ,
            EventValue::CurrentTargetScrollTop,
            EventValue::CurrentTargetScrollLeft,
            EventValue::ElapsedTime,
            EventValue::DataTransferFirstFileSize,
            EventValue::TargetFirstFileSize,
            EventValue::ChangedTouchClientX,
            EventValue::ChangedTouchClientY,
            EventValue::ChangedTouchPageX,
            EventValue::ChangedTouchPageY,
            EventValue::ChangedTouchScreenX,
            EventValue::ChangedTouchScreenY,
        ] {
            assert_eq!(event_value_type(value), ClientType::F64);
        }

        assert_eq!(
            event_collection_field_type(EventCollectionField::TouchIdentifier),
            ClientType::I32
        );
        for field in [
            EventCollectionField::TouchClientX,
            EventCollectionField::TouchClientY,
            EventCollectionField::TouchPageX,
            EventCollectionField::TouchPageY,
            EventCollectionField::TouchScreenX,
            EventCollectionField::TouchScreenY,
            EventCollectionField::FileSize,
        ] {
            assert_eq!(event_collection_field_type(field), ClientType::F64);
        }

        for accessor in [
            QueryAccessor::DataUpdatedAt,
            QueryAccessor::ErrorUpdatedAt,
            QueryAccessor::DownloadLoaded,
            QueryAccessor::DownloadPercent,
        ] {
            assert_eq!(query_accessor_type(accessor), ClientType::F64);
        }
        assert_eq!(
            query_accessor_type(QueryAccessor::FetchCount),
            ClientType::U32
        );
        assert_eq!(
            query_accessor_type(QueryAccessor::DownloadTotal),
            ClientType::option(ClientType::F64)
        );
        assert_eq!(
            browser_read_type(&ClientBrowserRead::Storage {
                area: crate::client::BrowserStorageArea::Local,
                value_type: StorageValueType::I64,
                key: Box::new(ClientExpr::string("count")),
                fallback: Some(Box::new(ClientExpr::Literal(Value::BigInt(0)))),
            }),
            ClientType::I64
        );
        assert_eq!(as_type_type(AsType::I64), ClientType::I64);
        assert_eq!(as_type_type(AsType::F64FromInt), ClientType::F64);
    }

    #[test]
    fn browser_integer_reads_carry_exact_rust_domains() {
        use crate::client::EventCollection;
        use crate::types::{NodeRefId, QueryId};

        let env = TypeEnv::new(&[], None);
        assert_eq!(
            env.integer_domain_of(&ClientExpr::BrowserRead(ClientBrowserRead::Window(
                WindowField::InnerWidth,
            ))),
            Some(IntegerDomain::I32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::NodeRefRead {
                id: NodeRefId(0),
                accessor: NodeRefAccessor::SelectionStart,
            }),
            Some(IntegerDomain::U32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::NodeRefRead {
                id: NodeRefId(0),
                accessor: NodeRefAccessor::ClientWidth,
            }),
            Some(IntegerDomain::I32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::EventRead(EventValue::Buttons)),
            Some(IntegerDomain::U32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::EventRead(EventValue::DetailU32)),
            Some(IntegerDomain::U32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::EventRead(EventValue::PointerId)),
            Some(IntegerDomain::I32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::EventCollectionLen(
                EventCollection::ChangedTouches,
            )),
            Some(IntegerDomain::U32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::QueryRead {
                id: QueryId(0),
                accessor: QueryAccessor::FetchCount,
            }),
            Some(IntegerDomain::U32)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::BrowserRead(ClientBrowserRead::Storage {
                area: crate::client::BrowserStorageArea::Local,
                value_type: StorageValueType::I64,
                key: Box::new(ClientExpr::string("count")),
                fallback: Some(Box::new(ClientExpr::Literal(Value::BigInt(0)))),
            })),
            Some(IntegerDomain::I64)
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::AsType {
                target: Box::new(ClientExpr::int(1)),
                ty: AsType::I64,
            }),
            Some(IntegerDomain::I64)
        );
    }

    #[test]
    fn signal_reads_type_from_declarations() {
        let mut b = IslandBuilder::new();
        let n = int_signal(&mut b);
        let wide = b.signal(Value::BigInt(9_007_199_254_740_993));
        let runtime = b.signal_typed(Value::Null, ClientType::Unknown);
        let island = b.build();
        let env = TypeEnv::for_island(&island);
        assert_eq!(env.type_of(&ClientExpr::signal_read(n)), ClientType::I32);
        assert_eq!(env.type_of(&ClientExpr::signal_read(wide)), ClientType::I64);
        assert_eq!(
            env.type_of(&ClientExpr::signal_read(runtime)),
            ClientType::Unknown
        );
    }

    #[test]
    fn structural_object_projection_types_nested_paths() {
        let ty = ClientType::object(vec![ClientObjectField {
            name: "results".to_string(),
            ty: ClientType::vec(ClientType::object(vec![ClientObjectField {
                name: "url".to_string(),
                ty: ClientType::String,
            }])),
        }]);
        assert_eq!(
            ty.project_path(&crate::client::JsonPointer::new("/results/0/url")),
            ClientType::String
        );
        assert_eq!(
            ty.project_path(&crate::client::JsonPointer::new("/results/title")),
            ClientType::Unknown
        );
    }

    #[test]
    fn query_data_type_drives_projection_type() {
        let mut b = IslandBuilder::new();
        let key = b.expr(ClientExpr::string("search"));
        let server_initial = b.expr(ClientExpr::Literal(Value::Null));
        let client_fetcher = b.expr(ClientExpr::string("/api/search"));
        let q = b.query_typed(
            key,
            server_initial,
            client_fetcher,
            ClientType::object(vec![ClientObjectField {
                name: "has_results".to_string(),
                ty: ClientType::Bool,
            }]),
            crate::reactive::QueryOptions::default(),
        );
        let island = b.build();
        let env = TypeEnv::for_island(&island);
        let expr = ClientExpr::Project {
            target: Box::new(ClientExpr::QueryRead {
                id: q,
                accessor: QueryAccessor::Data,
            }),
            path: crate::client::JsonPointer::new("/has_results"),
        };
        assert_eq!(env.type_of(&expr), ClientType::Bool);
    }

    #[test]
    fn memo_cells_type_from_their_bodies() {
        let mut b = IslandBuilder::new();
        let n = int_signal(&mut b);
        let body = b.expr(ClientExpr::binary(
            ClientBinaryOp::Mul,
            ClientExpr::signal_read(n),
            ClientExpr::int(2),
        ));
        let total = b.memo(body);
        let island = b.build();
        let env = TypeEnv::for_island(&island);
        assert_eq!(
            env.type_of(&ClientExpr::signal_read(total)),
            ClientType::I32
        );
    }

    #[test]
    fn derived_cycles_resolve_to_unknown() {
        let mut b = IslandBuilder::new();
        // A self-referential derived body; impossible from the macro
        // frontend, but hand-built IR must not hang the pass.
        let body = b.expr(ClientExpr::SignalRead(SignalId(0)));
        assert_eq!(body, ExprId(0));
        let cell = b.memo(body);
        let island = b.build();
        let env = TypeEnv::for_island(&island);
        assert_eq!(
            env.type_of(&ClientExpr::signal_read(cell)),
            ClientType::Unknown
        );
    }

    #[test]
    fn locals_type_from_environment_and_blocks() {
        let mut b = IslandBuilder::new();
        let n = int_signal(&mut b);
        let island = b.build();
        let mut env = TypeEnv::for_island(&island);
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(0),
                "half",
                ClientType::Unknown,
                ClientExpr::signal_read(n),
            ),
            ClientStmt::ForRange {
                local: ClientLocalId(1),
                name: "wide_index".to_owned(),
                integer_domain: IntegerDomain::U64,
                start: ClientExpr::Literal(Value::BigInt(0)),
                end: ClientExpr::Literal(Value::BigInt(3)),
                inclusive: false,
                body: ClientBlock::new(),
            },
        ]);
        block_types(&mut env, &block);
        assert_eq!(
            env.type_of(&ClientExpr::local(ClientLocalId(0))),
            ClientType::I32
        );
        assert_eq!(
            env.type_of(&ClientExpr::local(ClientLocalId(7))),
            ClientType::Unknown
        );
        assert_eq!(
            env.type_of(&ClientExpr::local(ClientLocalId(1))),
            ClientType::U64
        );
        assert_eq!(
            env.integer_domain_of(&ClientExpr::local(ClientLocalId(1))),
            Some(IntegerDomain::U64)
        );
    }

    #[test]
    fn arithmetic_unifies_widths() {
        let mut b = IslandBuilder::new();
        let n = int_signal(&mut b);
        let island = b.build();
        let env = TypeEnv::for_island(&island);
        let int_plus_int = ClientExpr::binary(
            ClientBinaryOp::Add,
            ClientExpr::signal_read(n),
            ClientExpr::int(1),
        );
        assert_eq!(env.type_of(&int_plus_int), ClientType::I32);
        let int_plus_float = ClientExpr::binary(
            ClientBinaryOp::Add,
            ClientExpr::signal_read(n),
            ClientExpr::float(0.5),
        );
        assert_eq!(env.type_of(&int_plus_float), ClientType::F64);
        let mixed_width = ClientExpr::binary(
            ClientBinaryOp::Add,
            ClientExpr::signal_read(n),
            ClientExpr::Literal(Value::BigInt(9_007_199_254_740_993)),
        );
        assert_eq!(env.type_of(&mixed_width), ClientType::Unknown);
    }

    #[test]
    fn anchors_ignore_literals_and_find_reads() {
        let mut b = IslandBuilder::new();
        let n = int_signal(&mut b);
        let wide = b.signal(Value::BigInt(9_007_199_254_740_993));
        let island = b.build();
        let env = TypeEnv::for_island(&island);
        let literal_only = ClientExpr::binary(
            ClientBinaryOp::Add,
            ClientExpr::Literal(Value::BigInt(9_007_199_254_740_993)),
            ClientExpr::int(1),
        );
        assert_eq!(
            env.numeric_anchors(&literal_only),
            NumericAnchors::default()
        );
        let number_anchor = ClientExpr::binary(
            ClientBinaryOp::Add,
            ClientExpr::signal_read(n),
            ClientExpr::int(1),
        );
        assert_eq!(
            env.numeric_anchors(&number_anchor),
            NumericAnchors {
                number: true,
                wide: false,
                dynamic: false
            }
        );
        let both = ClientExpr::binary(
            ClientBinaryOp::Add,
            ClientExpr::signal_read(n),
            ClientExpr::signal_read(wide),
        );
        assert_eq!(
            env.numeric_anchors(&both),
            NumericAnchors {
                number: true,
                wide: true,
                dynamic: false
            }
        );
    }
}
