// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Final-link representation for standalone `#[rsjs::client]` helpers.
//!
//! Descriptor Client IR deliberately retains caller-local
//! [`HelperCallEdgeId`] references. This module is the package-neutral bridge
//! between a relocation-authoritative final plan and deployable JavaScript:
//! callers provide opaque canonical helper identities, exact helper bodies,
//! and an edge-to-target table for each caller. No Rust path spelling is
//! consulted here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::abi::HelperCallEdgeId;
use crate::analysis::{
    validate_client_helper_block_types, ClientHelperBodyTypeError, ClientHelperTargetSignature,
};
use crate::client::{
    emit_client_block_js_with_context_and_indent, ClientBlock, ClientEmitContext, ClientEmitError,
    ClientLocalId, ClientParam, ClientType,
};
use crate::{validate_client_helper_block, ClientHelperValidationError};

/// Namespace reserved for unresolved references in non-deployable descriptor
/// JavaScript.
///
/// Final linked emitters must never place an executable identifier from this
/// namespace in an asset. Keeping the spelling in `rsjs_ir` makes the emitter
/// and the framework's final lexical guard share one protocol constant.
pub const UNLINKED_HELPER_IDENTIFIER_PREFIX: &str = "__rsjs_unlinked_";

/// Opaque canonical identity of one linked helper.
///
/// Final-plan JSON transports the canonical semantic-key bytes, not a Rust
/// source path. Keeping those bytes opaque here prevents final emission from
/// accidentally reintroducing token-spelling or package-layout inference.
/// Lexicographic byte ordering is also the sole input to stable private JS
/// function numbering.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LinkedHelperKey(Box<[u8]>);

impl LinkedHelperKey {
    /// Preserve an already validated canonical identity byte-for-byte.
    pub fn from_canonical_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, LinkedHelperKeyError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(LinkedHelperKeyError);
        }
        Ok(Self(bytes.into_boxed_slice()))
    }

    /// Return the exact canonical identity bytes supplied by the final plan.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for LinkedHelperKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LinkedHelperKey(")?;
        for byte in self.as_bytes() {
            write!(formatter, "{byte:02x}")?;
        }
        formatter.write_str(")")
    }
}

/// Canonical helper identities may not be empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkedHelperKeyError;

impl fmt::Display for LinkedHelperKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("linked helper canonical identity must not be empty")
    }
}

impl std::error::Error for LinkedHelperKeyError {}

/// One relocation-resolved call in a component or helper body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedHelperCallBinding {
    pub edge: HelperCallEdgeId,
    pub target: LinkedHelperKey,
}

impl LinkedHelperCallBinding {
    pub fn new(edge: HelperCallEdgeId, target: LinkedHelperKey) -> Self {
        Self { edge, target }
    }
}

/// Exact codegen material for one reachable standalone helper.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedHelperDefinition {
    pub key: LinkedHelperKey,
    pub params: Vec<ClientParam>,
    pub return_ty: ClientType,
    pub body: ClientBlock,
    /// Caller-local helper edges in `body`, resolved by the final linker.
    pub calls: Vec<LinkedHelperCallBinding>,
}

impl LinkedHelperDefinition {
    pub fn new(
        key: LinkedHelperKey,
        params: Vec<ClientParam>,
        return_ty: ClientType,
        body: ClientBlock,
        calls: Vec<LinkedHelperCallBinding>,
    ) -> Self {
        Self {
            key,
            params,
            return_ty,
            body,
            calls,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LinkedHelperFunctionId(u32);

impl LinkedHelperFunctionId {
    fn from_index(index: usize) -> Result<Self, LinkedHelperLinkError> {
        let index = u32::try_from(index).map_err(|_| LinkedHelperLinkError::TooManyHelpers)?;
        Ok(Self(index))
    }

    pub(crate) fn js_name(self) -> String {
        format!("__rsjs_helper_{}", self.0)
    }
}

/// One final, caller-local helper edge after relocation resolution.
///
/// Function identity and parameter types travel as one value on purpose. The
/// final emitter must use the same authenticated target both to spell the JS
/// callee and to restore contextual Rust literal domains at its arguments;
/// parallel maps could drift and silently attach a signature to the wrong
/// private function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkedHelperCallTarget {
    function: LinkedHelperFunctionId,
    parameter_types: Box<[ClientType]>,
}

impl LinkedHelperCallTarget {
    pub(crate) fn function(&self) -> LinkedHelperFunctionId {
        self.function
    }

    pub(crate) fn parameter_types(&self) -> &[ClientType] {
        &self.parameter_types
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ValidatedHelperDefinition {
    params: Vec<ClientParam>,
    return_ty: ClientType,
    body: ClientBlock,
    calls: BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
    call_arities: BTreeMap<HelperCallEdgeId, usize>,
}

/// A caller-specific, transitively closed helper program.
///
/// Construction is fail-closed. Every helper body must have an exact local
/// edge table, every target must be defined, and the definition set must equal
/// the transitive closure from `root_calls`. Cycles are valid because emitted
/// JavaScript function declarations are hoisted.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedHelperProgram {
    root_calls: BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
    helpers: BTreeMap<LinkedHelperKey, ValidatedHelperDefinition>,
    function_ids: BTreeMap<LinkedHelperKey, LinkedHelperFunctionId>,
}

impl LinkedHelperProgram {
    pub fn new(
        root_calls: Vec<LinkedHelperCallBinding>,
        helpers: Vec<LinkedHelperDefinition>,
    ) -> Result<Self, LinkedHelperLinkError> {
        Self::new_with_root_boundary_policy(root_calls, helpers, true)
    }

    /// Validate a closed definition graph whose roots are synthetic compiler
    /// probes rather than deployable component/helper call boundaries.
    ///
    /// This retains signature, body, edge, typing, and reachability checks but
    /// deliberately skips the public value-object boundary rule. Final
    /// deployable programs must always use [`Self::new`].
    pub fn new_for_internal_validation(
        root_calls: Vec<LinkedHelperCallBinding>,
        helpers: Vec<LinkedHelperDefinition>,
    ) -> Result<Self, LinkedHelperLinkError> {
        Self::new_with_root_boundary_policy(root_calls, helpers, false)
    }

    fn new_with_root_boundary_policy(
        root_calls: Vec<LinkedHelperCallBinding>,
        helpers: Vec<LinkedHelperDefinition>,
        enforce_deployable_value_boundary: bool,
    ) -> Result<Self, LinkedHelperLinkError> {
        let root_calls = collect_bindings(LinkedHelperCaller::Root, root_calls)?;
        let mut definitions = BTreeMap::new();

        for definition in helpers {
            let key = definition.key;
            validate_helper_signature(
                &key,
                &definition.params,
                &definition.return_ty,
                &definition.body,
            )?;
            let calls =
                collect_bindings(LinkedHelperCaller::Helper(key.clone()), definition.calls)?;
            let call_arities =
                validate_helper_body(&key, &definition.params, &definition.body, &calls)?;
            let validated = ValidatedHelperDefinition {
                params: definition.params,
                return_ty: definition.return_ty,
                body: definition.body,
                calls,
                call_arities,
            };
            if definitions.insert(key.clone(), validated).is_some() {
                return Err(LinkedHelperLinkError::DuplicateDefinition { helper: key });
            }
        }

        validate_targets(&root_calls, &definitions)?;
        if enforce_deployable_value_boundary {
            validate_root_value_boundaries(&root_calls, &definitions)?;
        }
        for (key, definition) in &definitions {
            validate_definition_targets(key, definition, &definitions)?;
        }
        for (key, definition) in &definitions {
            validate_definition_types(key, definition, &definitions)?;
        }

        let reachable = reachable_helpers(&root_calls, &definitions);
        if let Some(helper) = definitions
            .keys()
            .find(|helper| !reachable.contains(*helper))
        {
            return Err(LinkedHelperLinkError::UnreachableDefinition {
                helper: helper.clone(),
            });
        }

        let mut function_ids = BTreeMap::new();
        for (index, key) in definitions.keys().enumerate() {
            function_ids.insert(key.clone(), LinkedHelperFunctionId::from_index(index)?);
        }

        Ok(Self {
            root_calls,
            helpers: definitions,
            function_ids,
        })
    }

    /// Whether the closed program contains no helper functions.
    pub fn is_empty(&self) -> bool {
        self.helpers.is_empty()
    }

    /// Number of functions in the closed program.
    pub fn len(&self) -> usize {
        self.helpers.len()
    }

    pub(crate) fn helper_bodies(&self) -> impl Iterator<Item = &ClientBlock> {
        self.helpers.values().map(|helper| &helper.body)
    }

    pub(crate) fn validate_root_calls(
        &self,
        actual: &BTreeMap<HelperCallEdgeId, usize>,
    ) -> Result<(), LinkedHelperEmitError> {
        for edge in actual.keys() {
            if !self.root_calls.contains_key(edge) {
                return Err(LinkedHelperEmitError::MissingRootBinding { edge: *edge });
            }
        }
        for edge in self.root_calls.keys() {
            if !actual.contains_key(edge) {
                return Err(LinkedHelperEmitError::ExtraRootBinding { edge: *edge });
            }
        }
        for (edge, actual) in actual {
            let target = self
                .root_calls
                .get(edge)
                .expect("root call/binding exactness was validated above");
            let expected = self
                .helpers
                .get(target)
                .expect("root helper target existence was validated at construction")
                .params
                .len();
            if *actual != expected {
                return Err(LinkedHelperEmitError::RootCallArityMismatch {
                    edge: *edge,
                    target: target.clone(),
                    expected,
                    actual: *actual,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn emit_private_functions(
        &self,
        out: &mut String,
    ) -> Result<(), LinkedHelperEmitError> {
        for (key, helper) in &self.helpers {
            let id = self.function_id(key);
            out.push_str("function ");
            out.push_str(&id.js_name());
            out.push('(');
            for (index, _) in helper.params.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&helper_parameter_js_name(index));
            }
            out.push_str(") {\n");

            let mut context = ClientEmitContext::default()
                .with_linked_helper_calls(self.call_targets_for(&helper.calls));
            for (index, _) in helper.params.iter().enumerate() {
                context = context
                    .with_local_name(ClientLocalId(index as u32), helper_parameter_js_name(index));
            }
            emit_client_block_js_with_context_and_indent(out, &helper.body, &context, 1)?;
            out.push_str("}\n\n");
        }
        Ok(())
    }

    fn function_id(&self, key: &LinkedHelperKey) -> LinkedHelperFunctionId {
        *self
            .function_ids
            .get(key)
            .expect("validated helper target must have a stable function id")
    }

    pub(crate) fn root_call_targets(&self) -> BTreeMap<HelperCallEdgeId, LinkedHelperCallTarget> {
        self.call_targets_for(&self.root_calls)
    }

    pub(crate) fn root_target_signatures(
        &self,
    ) -> BTreeMap<HelperCallEdgeId, ClientHelperTargetSignature> {
        self.root_calls
            .iter()
            .map(|(edge, target)| {
                let target = self
                    .helpers
                    .get(target)
                    .expect("root helper target existence was validated at construction");
                (
                    *edge,
                    ClientHelperTargetSignature::new(
                        target.params.iter().map(|param| param.ty.clone()).collect(),
                        target.return_ty.clone(),
                    ),
                )
            })
            .collect()
    }

    fn call_targets_for(
        &self,
        calls: &BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
    ) -> BTreeMap<HelperCallEdgeId, LinkedHelperCallTarget> {
        calls
            .iter()
            .map(|(edge, target)| {
                let helper = self
                    .helpers
                    .get(target)
                    .expect("linked helper call target existence was validated at construction");
                (
                    *edge,
                    LinkedHelperCallTarget {
                        function: self.function_id(target),
                        parameter_types: helper
                            .params
                            .iter()
                            .map(|parameter| parameter.ty.clone())
                            .collect(),
                    },
                )
            })
            .collect()
    }
}

/// Helper parameters are positional in Client IR. Final emission deliberately
/// owns their JavaScript spelling so Rust raw identifiers, Unicode source
/// names, or forged plan text can never become executable JavaScript syntax.
fn helper_parameter_js_name(index: usize) -> String {
    format!("__rsjs_arg_{index}")
}

/// Identifies the local edge namespace involved in a link error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedHelperCaller {
    Root,
    Helper(LinkedHelperKey),
}

/// Location of an invalid type in a linked helper signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkedHelperSignaturePosition {
    Parameter(usize),
    Return,
}

/// Invalid closed-helper program supplied by final-plan codegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedHelperLinkError {
    DuplicateBinding {
        caller: LinkedHelperCaller,
        edge: HelperCallEdgeId,
    },
    DuplicateDefinition {
        helper: LinkedHelperKey,
    },
    MissingBodyBinding {
        helper: LinkedHelperKey,
        edge: HelperCallEdgeId,
    },
    ExtraBodyBinding {
        helper: LinkedHelperKey,
        edge: HelperCallEdgeId,
    },
    UnknownTarget {
        caller: LinkedHelperCaller,
        edge: HelperCallEdgeId,
        target: LinkedHelperKey,
    },
    UnreachableDefinition {
        helper: LinkedHelperKey,
    },
    InvalidBody {
        helper: LinkedHelperKey,
        source: Box<ClientHelperValidationError>,
    },
    InvalidBodyType {
        helper: LinkedHelperKey,
        source: Box<ClientHelperBodyTypeError>,
    },
    CallArityMismatch {
        caller: LinkedHelperCaller,
        edge: HelperCallEdgeId,
        target: LinkedHelperKey,
        expected: usize,
        actual: usize,
    },
    UnsupportedSignatureType {
        helper: LinkedHelperKey,
        position: LinkedHelperSignaturePosition,
        ty: ClientType,
    },
    ValueObjectCrossesRootBoundary {
        edge: HelperCallEdgeId,
        target: LinkedHelperKey,
        position: LinkedHelperSignaturePosition,
        ty: ClientType,
    },
    NestedOptionType {
        helper: LinkedHelperKey,
        position: LinkedHelperSignaturePosition,
        ty: ClientType,
    },
    ReturnShapeMismatch {
        helper: LinkedHelperKey,
        expected: ClientType,
        has_value: bool,
    },
    NonUnitReturnMayFallThrough {
        helper: LinkedHelperKey,
        expected: ClientType,
    },
    TooManyHelpers,
}

impl fmt::Display for LinkedHelperLinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBinding { caller, edge } => write!(
                formatter,
                "linked helper caller {caller:?} binds local edge {} more than once",
                edge.index()
            ),
            Self::DuplicateDefinition { helper } => {
                write!(
                    formatter,
                    "linked helper {helper:?} is defined more than once"
                )
            }
            Self::MissingBodyBinding { helper, edge } => write!(
                formatter,
                "linked helper {helper:?} body references unbound local edge {}",
                edge.index()
            ),
            Self::ExtraBodyBinding { helper, edge } => write!(
                formatter,
                "linked helper {helper:?} declares unused local edge {}",
                edge.index()
            ),
            Self::UnknownTarget {
                caller,
                edge,
                target,
            } => write!(
                formatter,
                "linked helper caller {caller:?} edge {} targets missing helper {target:?}",
                edge.index()
            ),
            Self::UnreachableDefinition { helper } => write!(
                formatter,
                "linked helper program contains unreachable definition {helper:?}"
            ),
            Self::InvalidBody { helper, source } => {
                write!(formatter, "linked helper {helper:?} has invalid Client IR: {source}")
            }
            Self::InvalidBodyType { helper, source } => write!(
                formatter,
                "linked helper {helper:?} has ill-typed Client IR: {source}"
            ),
            Self::CallArityMismatch {
                caller,
                edge,
                target,
                expected,
                actual,
            } => write!(
                formatter,
                "linked helper caller {caller:?} edge {} invokes {target:?} with {actual} arguments; expected {expected}",
                edge.index()
            ),
            Self::UnsupportedSignatureType {
                helper,
                position,
                ty,
            } => write!(
                formatter,
                "linked helper {helper:?} has unsupported {position:?} signature type {ty:?}"
            ),
            Self::ValueObjectCrossesRootBoundary {
                edge,
                target,
                position,
                ty,
            } => write!(
                formatter,
                "deployable root edge {} targets {target:?} whose {position:?} type {ty:?} is a helper-internal value object",
                edge.index()
            ),
            Self::NestedOptionType {
                helper,
                position,
                ty,
            } => write!(
                formatter,
                "linked helper {helper:?} has directly nested Option in {position:?} signature type {ty:?}"
            ),
            Self::ReturnShapeMismatch {
                helper,
                expected,
                has_value,
            } => write!(
                formatter,
                "linked helper {helper:?} has a return statement with value={has_value} incompatible with declared return type {expected:?}"
            ),
            Self::NonUnitReturnMayFallThrough { helper, expected } => write!(
                formatter,
                "linked helper {helper:?} with declared return type {expected:?} has a control-flow path that falls through"
            ),
            Self::TooManyHelpers => formatter.write_str(
                "linked helper program exceeds the u32 private-function identifier space",
            ),
        }
    }
}

impl std::error::Error for LinkedHelperLinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidBody { source, .. } => Some(source.as_ref()),
            Self::InvalidBodyType { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// Final deployable emission rejected a caller/program mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedHelperEmitError {
    MissingRootBinding {
        edge: HelperCallEdgeId,
    },
    ExtraRootBinding {
        edge: HelperCallEdgeId,
    },
    ConflictingRootCallArity {
        edge: HelperCallEdgeId,
        first: usize,
        second: usize,
    },
    RootCallArityMismatch {
        edge: HelperCallEdgeId,
        target: LinkedHelperKey,
        expected: usize,
        actual: usize,
    },
    InvalidRootType(Box<ClientHelperBodyTypeError>),
    InvalidProgram(Box<LinkedHelperLinkError>),
    Client(ClientEmitError),
}

impl fmt::Display for LinkedHelperEmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRootBinding { edge } => write!(
                formatter,
                "deployable caller references unbound helper edge {}",
                edge.index()
            ),
            Self::ExtraRootBinding { edge } => write!(
                formatter,
                "deployable caller declares unused helper edge {}",
                edge.index()
            ),
            Self::ConflictingRootCallArity {
                edge,
                first,
                second,
            } => write!(
                formatter,
                "deployable caller observes helper edge {} with conflicting argument counts {first} and {second}",
                edge.index()
            ),
            Self::RootCallArityMismatch {
                edge,
                target,
                expected,
                actual,
            } => write!(
                formatter,
                "deployable caller edge {} invokes {target:?} with {actual} arguments; expected {expected}",
                edge.index()
            ),
            Self::InvalidRootType(source) => {
                write!(formatter, "deployable caller has an ill-typed helper crossing: {source}")
            }
            Self::InvalidProgram(error) => error.fmt(formatter),
            Self::Client(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LinkedHelperEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRootType(source) => Some(source.as_ref()),
            Self::InvalidProgram(error) => Some(error.as_ref()),
            Self::Client(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LinkedHelperLinkError> for LinkedHelperEmitError {
    fn from(error: LinkedHelperLinkError) -> Self {
        Self::InvalidProgram(Box::new(error))
    }
}

impl From<ClientEmitError> for LinkedHelperEmitError {
    fn from(error: ClientEmitError) -> Self {
        Self::Client(error)
    }
}

impl From<ClientHelperBodyTypeError> for LinkedHelperEmitError {
    fn from(error: ClientHelperBodyTypeError) -> Self {
        Self::InvalidRootType(Box::new(error))
    }
}

fn collect_bindings(
    caller: LinkedHelperCaller,
    bindings: Vec<LinkedHelperCallBinding>,
) -> Result<BTreeMap<HelperCallEdgeId, LinkedHelperKey>, LinkedHelperLinkError> {
    let mut result = BTreeMap::new();
    for binding in bindings {
        if result.insert(binding.edge, binding.target).is_some() {
            return Err(LinkedHelperLinkError::DuplicateBinding {
                caller,
                edge: binding.edge,
            });
        }
    }
    Ok(result)
}

fn validate_helper_signature(
    helper: &LinkedHelperKey,
    params: &[ClientParam],
    return_ty: &ClientType,
    body: &ClientBlock,
) -> Result<(), LinkedHelperLinkError> {
    for (index, param) in params.iter().enumerate() {
        validate_helper_signature_type(
            helper,
            LinkedHelperSignaturePosition::Parameter(index),
            &param.ty,
            &param.ty,
            false,
        )?;
    }
    validate_helper_signature_type(
        helper,
        LinkedHelperSignaturePosition::Return,
        return_ty,
        return_ty,
        false,
    )?;
    validate_return_shapes(helper, body, return_ty)?;
    if *return_ty != ClientType::Unit && block_can_fall_through(body) {
        return Err(LinkedHelperLinkError::NonUnitReturnMayFallThrough {
            helper: helper.clone(),
            expected: return_ty.clone(),
        });
    }
    Ok(())
}

fn validate_helper_signature_type(
    helper: &LinkedHelperKey,
    position: LinkedHelperSignaturePosition,
    ty: &ClientType,
    complete_ty: &ClientType,
    inside_option: bool,
) -> Result<(), LinkedHelperLinkError> {
    match ty {
        ClientType::Unit
        | ClientType::Bool
        | ClientType::I32
        | ClientType::U32
        | ClientType::I64
        | ClientType::U64
        | ClientType::F64
        | ClientType::String => Ok(()),
        ClientType::ValueObject { type_key, fields }
            if valid_value_object_schema(type_key, fields) =>
        {
            Ok(())
        }
        ClientType::Option(inner) => {
            if inside_option {
                return Err(LinkedHelperLinkError::NestedOptionType {
                    helper: helper.clone(),
                    position,
                    ty: complete_ty.clone(),
                });
            }
            validate_helper_signature_type(helper, position, inner, complete_ty, true)
        }
        ClientType::Vec(inner) => {
            validate_helper_signature_type(helper, position, inner, complete_ty, false)
        }
        _ => Err(LinkedHelperLinkError::UnsupportedSignatureType {
            helper: helper.clone(),
            position,
            ty: complete_ty.clone(),
        }),
    }
}

fn validate_root_value_boundaries(
    roots: &BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
    definitions: &BTreeMap<LinkedHelperKey, ValidatedHelperDefinition>,
) -> Result<(), LinkedHelperLinkError> {
    for (edge, target) in roots {
        let definition = definitions
            .get(target)
            .expect("root helper target existence was validated before its boundary");
        for (index, parameter) in definition.params.iter().enumerate() {
            if contains_value_object(&parameter.ty) {
                return Err(LinkedHelperLinkError::ValueObjectCrossesRootBoundary {
                    edge: *edge,
                    target: target.clone(),
                    position: LinkedHelperSignaturePosition::Parameter(index),
                    ty: parameter.ty.clone(),
                });
            }
        }
        if contains_value_object(&definition.return_ty) {
            return Err(LinkedHelperLinkError::ValueObjectCrossesRootBoundary {
                edge: *edge,
                target: target.clone(),
                position: LinkedHelperSignaturePosition::Return,
                ty: definition.return_ty.clone(),
            });
        }
    }
    Ok(())
}

fn contains_value_object(ty: &ClientType) -> bool {
    match ty {
        ClientType::ValueObject { .. } => true,
        ClientType::Option(inner) | ClientType::Vec(inner) => contains_value_object(inner),
        _ => false,
    }
}

fn valid_value_object_schema(type_key: &str, fields: &[crate::client::ClientObjectField]) -> bool {
    let mut names = BTreeSet::new();
    type_key.len() == 64
        && type_key.bytes().all(|byte| byte.is_ascii_hexdigit())
        && fields.iter().all(|field| {
            !field.name.is_empty()
                && names.insert(field.name.as_str())
                && matches!(
                    field.ty,
                    ClientType::Bool
                        | ClientType::I32
                        | ClientType::U32
                        | ClientType::I64
                        | ClientType::U64
                        | ClientType::F64
                        | ClientType::String
                )
        })
}

fn validate_return_shapes(
    helper: &LinkedHelperKey,
    block: &ClientBlock,
    return_ty: &ClientType,
) -> Result<(), LinkedHelperLinkError> {
    for statement in &block.stmts {
        match statement {
            crate::client::ClientStmt::Return(value) => {
                let has_value = value.is_some();
                if *return_ty != ClientType::Unit && !has_value {
                    return Err(LinkedHelperLinkError::ReturnShapeMismatch {
                        helper: helper.clone(),
                        expected: return_ty.clone(),
                        has_value,
                    });
                }
            }
            crate::client::ClientStmt::If {
                then_block,
                else_block,
                ..
            } => {
                validate_return_shapes(helper, then_block, return_ty)?;
                if let Some(else_block) = else_block {
                    validate_return_shapes(helper, else_block, return_ty)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn block_can_fall_through(block: &ClientBlock) -> bool {
    let mut can_fall_through = true;
    for statement in &block.stmts {
        if !can_fall_through {
            break;
        }
        can_fall_through = match statement {
            crate::client::ClientStmt::Return(_) => false,
            crate::client::ClientStmt::If {
                then_block,
                else_block,
                ..
            } => {
                block_can_fall_through(then_block)
                    || else_block.as_ref().is_none_or(block_can_fall_through)
            }
            _ => true,
        };
    }
    can_fall_through
}

fn validate_helper_body(
    key: &LinkedHelperKey,
    params: &[ClientParam],
    body: &ClientBlock,
    calls: &BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
) -> Result<BTreeMap<HelperCallEdgeId, usize>, LinkedHelperLinkError> {
    let report = validate_client_helper_block(body, params.len()).map_err(|source| {
        LinkedHelperLinkError::InvalidBody {
            helper: key.clone(),
            source: Box::new(source),
        }
    })?;
    let actual = report
        .calls
        .iter()
        .map(|call| call.edge)
        .collect::<BTreeSet<_>>();
    if let Some(edge) = actual.iter().find(|edge| !calls.contains_key(edge)) {
        return Err(LinkedHelperLinkError::MissingBodyBinding {
            helper: key.clone(),
            edge: *edge,
        });
    }
    if let Some(edge) = calls.keys().find(|edge| !actual.contains(edge)) {
        return Err(LinkedHelperLinkError::ExtraBodyBinding {
            helper: key.clone(),
            edge: *edge,
        });
    }
    Ok(report
        .calls
        .into_iter()
        .map(|call| (call.edge, call.argument_count))
        .collect())
}

fn validate_targets(
    calls: &BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
    definitions: &BTreeMap<LinkedHelperKey, ValidatedHelperDefinition>,
) -> Result<(), LinkedHelperLinkError> {
    validate_definition_targets_inner(LinkedHelperCaller::Root, calls, definitions)
}

fn validate_definition_targets(
    helper: &LinkedHelperKey,
    definition: &ValidatedHelperDefinition,
    definitions: &BTreeMap<LinkedHelperKey, ValidatedHelperDefinition>,
) -> Result<(), LinkedHelperLinkError> {
    validate_definition_targets_inner(
        LinkedHelperCaller::Helper(helper.clone()),
        &definition.calls,
        definitions,
    )?;
    for (edge, target) in &definition.calls {
        let target_definition = definitions
            .get(target)
            .expect("helper target existence was validated above");
        let actual = *definition
            .call_arities
            .get(edge)
            .expect("helper body/binding exactness was validated above");
        let expected = target_definition.params.len();
        if actual != expected {
            return Err(LinkedHelperLinkError::CallArityMismatch {
                caller: LinkedHelperCaller::Helper(helper.clone()),
                edge: *edge,
                target: target.clone(),
                expected,
                actual,
            });
        }
    }
    Ok(())
}

fn validate_definition_types(
    helper: &LinkedHelperKey,
    definition: &ValidatedHelperDefinition,
    definitions: &BTreeMap<LinkedHelperKey, ValidatedHelperDefinition>,
) -> Result<(), LinkedHelperLinkError> {
    let targets = definition
        .calls
        .iter()
        .map(|(edge, target)| {
            let target = definitions
                .get(target)
                .expect("helper target existence was validated before type checking");
            (
                *edge,
                ClientHelperTargetSignature::new(
                    target.params.iter().map(|param| param.ty.clone()).collect(),
                    target.return_ty.clone(),
                ),
            )
        })
        .collect();
    let parameter_types = definition
        .params
        .iter()
        .map(|param| param.ty.clone())
        .collect::<Vec<_>>();
    validate_client_helper_block_types(
        &definition.body,
        &parameter_types,
        &definition.return_ty,
        &targets,
    )
    .map_err(|source| LinkedHelperLinkError::InvalidBodyType {
        helper: helper.clone(),
        source: Box::new(source),
    })
}

fn validate_definition_targets_inner(
    caller: LinkedHelperCaller,
    calls: &BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
    definitions: &BTreeMap<LinkedHelperKey, ValidatedHelperDefinition>,
) -> Result<(), LinkedHelperLinkError> {
    for (edge, target) in calls {
        if !definitions.contains_key(target) {
            return Err(LinkedHelperLinkError::UnknownTarget {
                caller,
                edge: *edge,
                target: target.clone(),
            });
        }
    }
    Ok(())
}

fn reachable_helpers(
    roots: &BTreeMap<HelperCallEdgeId, LinkedHelperKey>,
    definitions: &BTreeMap<LinkedHelperKey, ValidatedHelperDefinition>,
) -> BTreeSet<LinkedHelperKey> {
    let mut reachable = BTreeSet::new();
    let mut pending = roots.values().cloned().collect::<Vec<_>>();
    while let Some(key) = pending.pop() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        if let Some(definition) = definitions.get(&key) {
            pending.extend(definition.calls.values().cloned());
        }
    }
    reachable
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{ClientHelperInferredType, ClientHelperTypeSite};
    use crate::client::{
        CheckedIntegerOp, ClientBinaryOp, ClientExpr, ClientFunctionRef, ClientObjectField,
        ClientPlace, ClientStmt, ClientStructFieldValue, ClientUnaryOp, IntegerDomain,
    };

    fn key(byte: u8) -> LinkedHelperKey {
        LinkedHelperKey::from_canonical_bytes(vec![byte]).unwrap()
    }

    fn helper_call(edge: u32) -> ClientExpr {
        ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(edge),
                authored_path: "ignored::spelling".to_owned(),
                name: "spelling".to_owned(),
            },
            vec![ClientExpr::local(ClientLocalId(0))],
        )
    }

    fn definition(
        key: LinkedHelperKey,
        body: ClientBlock,
        calls: Vec<LinkedHelperCallBinding>,
    ) -> LinkedHelperDefinition {
        LinkedHelperDefinition::new(
            key,
            vec![ClientParam::new("value", ClientType::I32)],
            ClientType::I32,
            body,
            calls,
        )
    }

    #[test]
    fn empty_program_is_valid() {
        let program = LinkedHelperProgram::new(vec![], vec![]).unwrap();
        assert!(program.is_empty());
    }

    #[test]
    fn rejects_duplicate_helper_definitions() {
        let target = key(1);
        let duplicate = definition(
            target.clone(),
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                ClientLocalId(0),
            )))]),
            vec![],
        );

        let error = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![duplicate.clone(), duplicate],
        )
        .unwrap_err();

        assert_eq!(
            error,
            LinkedHelperLinkError::DuplicateDefinition { helper: target }
        );
    }

    #[test]
    fn emits_transitive_cycle_once_in_canonical_key_order() {
        let first = key(1);
        let second = key(2);
        let first_body = ClientBlock::from_stmts(vec![ClientStmt::Return(Some(helper_call(0)))]);
        let second_body = ClientBlock::from_stmts(vec![ClientStmt::Return(Some(helper_call(0)))]);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(7),
                second.clone(),
            )],
            vec![
                definition(
                    second.clone(),
                    second_body,
                    vec![LinkedHelperCallBinding::new(
                        HelperCallEdgeId::new(0),
                        first.clone(),
                    )],
                ),
                definition(
                    first.clone(),
                    first_body,
                    vec![LinkedHelperCallBinding::new(
                        HelperCallEdgeId::new(0),
                        second.clone(),
                    )],
                ),
            ],
        )
        .unwrap();

        let mut js = String::new();
        program.emit_private_functions(&mut js).unwrap();

        assert_eq!(js.matches("function __rsjs_helper_0(").count(), 1);
        assert_eq!(js.matches("function __rsjs_helper_1(").count(), 1);
        assert!(
            js.find("function __rsjs_helper_0(").unwrap()
                < js.find("function __rsjs_helper_1(").unwrap()
        );
        assert!(js.contains("return __rsjs_helper_1(__rsjs_arg_0);"));
        assert!(js.contains("return __rsjs_helper_0(__rsjs_arg_0);"));
        assert!(!js.contains("__rsjs_unlinked_"));
    }

    #[test]
    fn helper_parameter_names_are_emitter_owned() {
        let target = key(1);
        let definition = LinkedHelperDefinition::new(
            target.clone(),
            vec![ClientParam::new(
                "x) { globalThis.pwned = true; } function injected(",
                ClientType::I32,
            )],
            ClientType::I32,
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                ClientLocalId(0),
            )))]),
            vec![],
        );
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target,
            )],
            vec![definition],
        )
        .unwrap();

        let mut js = String::new();
        program.emit_private_functions(&mut js).unwrap();

        assert_eq!(
            js,
            "function __rsjs_helper_0(__rsjs_arg_0) {\n    return __rsjs_arg_0;\n}\n\n"
        );
        assert!(!js.contains("pwned"));
        assert!(!js.contains("injected"));
    }

    #[test]
    fn rejects_missing_extra_and_unknown_edges() {
        let target = key(1);
        let missing = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![definition(
                target.clone(),
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(helper_call(0)))]),
                vec![],
            )],
        );
        assert!(matches!(
            missing,
            Err(LinkedHelperLinkError::MissingBodyBinding { edge, .. })
                if edge == HelperCallEdgeId::new(0)
        ));

        let extra = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![definition(
                target.clone(),
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::int(1)))]),
                vec![LinkedHelperCallBinding::new(
                    HelperCallEdgeId::new(0),
                    target.clone(),
                )],
            )],
        );
        assert!(matches!(
            extra,
            Err(LinkedHelperLinkError::ExtraBodyBinding { edge, .. })
                if edge == HelperCallEdgeId::new(0)
        ));

        let unknown = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                key(9),
            )],
            vec![],
        );
        assert!(matches!(
            unknown,
            Err(LinkedHelperLinkError::UnknownTarget { edge, .. })
                if edge == HelperCallEdgeId::new(0)
        ));
    }

    #[test]
    fn rejects_unreachable_definitions_and_arbitrary_runtime_calls() {
        let unreachable = key(3);
        let program = LinkedHelperProgram::new(
            vec![],
            vec![definition(
                unreachable.clone(),
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::int(1)))]),
                vec![],
            )],
        );
        assert_eq!(
            program.unwrap_err(),
            LinkedHelperLinkError::UnreachableDefinition {
                helper: unreachable
            }
        );

        let ambient = key(4);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                ambient.clone(),
            )],
            vec![definition(
                ambient.clone(),
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::call(
                    ClientFunctionRef::Runtime("globalThis.pwn".to_owned()),
                    vec![],
                )))]),
                vec![],
            )],
        );
        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::InvalidBody { source, .. })
                if matches!(
                    source.as_ref(),
                    ClientHelperValidationError::UnsupportedRuntimeFunction { runtime }
                        if runtime == "globalThis.pwn"
                )
        ));
    }

    #[test]
    fn accepts_only_the_owned_len_runtime_intrinsic() {
        let target = key(5);
        let len = ClientExpr::call(
            ClientFunctionRef::Runtime(crate::client::CLIENT_HELPER_LEN_INTRINSIC_JS.to_owned()),
            vec![ClientExpr::local(ClientLocalId(0))],
        );
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                target,
                vec![ClientParam::new("value", ClientType::String)],
                ClientType::Bool,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::binary(
                    ClientBinaryOp::Eq,
                    len,
                    ClientExpr::int(1),
                )))]),
                vec![],
            )],
        )
        .unwrap();
        assert_eq!(program.len(), 1);

        for actual in [0, 2] {
            let target = key(7 + actual as u8);
            let args = (0..actual)
                .map(|_| ClientExpr::local(ClientLocalId(0)))
                .collect();
            let program = LinkedHelperProgram::new(
                vec![LinkedHelperCallBinding::new(
                    HelperCallEdgeId::new(0),
                    target.clone(),
                )],
                vec![definition(
                    target,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::call(
                        ClientFunctionRef::Runtime(
                            crate::client::CLIENT_HELPER_LEN_INTRINSIC_JS.to_owned(),
                        ),
                        args,
                    )))]),
                    vec![],
                )],
            );
            assert!(matches!(
                program,
                Err(LinkedHelperLinkError::InvalidBody { source, .. })
                    if matches!(
                        source.as_ref(),
                        ClientHelperValidationError::InvalidRuntimeFunctionArity {
                            expected: 1,
                            actual: found,
                            ..
                        } if *found == actual
                    )
            ));
        }
    }

    #[test]
    fn helper_numeric_semantics_remain_structural() {
        let target = key(6);
        let body = ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::binary(
            ClientBinaryOp::Checked {
                op: CheckedIntegerOp::Add,
                domain: IntegerDomain::I32,
            },
            ClientExpr::local(ClientLocalId(0)),
            ClientExpr::int(1),
        )))]);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![definition(target, body, vec![])],
        )
        .unwrap();
        let mut js = String::new();
        program.emit_private_functions(&mut js).unwrap();
        assert!(js.contains("checked_int_add(__rsjs_arg_0, 1, \"i32\")"));
    }

    #[test]
    fn rejects_helper_to_helper_call_arity_mismatch() {
        let caller = key(10);
        let target = key(11);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                caller.clone(),
            )],
            vec![
                definition(
                    caller.clone(),
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(helper_call(0)))]),
                    vec![LinkedHelperCallBinding::new(
                        HelperCallEdgeId::new(0),
                        target.clone(),
                    )],
                ),
                LinkedHelperDefinition::new(
                    target.clone(),
                    vec![],
                    ClientType::I32,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::int(1)))]),
                    vec![],
                ),
            ],
        );

        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::CallArityMismatch {
                caller: LinkedHelperCaller::Helper(actual_caller),
                edge,
                target: actual_target,
                expected: 0,
                actual: 1,
            }) if actual_caller == caller
                && actual_target == target
                && edge == HelperCallEdgeId::new(0)
        ));
    }

    #[test]
    fn validates_signature_shapes_and_non_unit_return_control_flow() {
        let unsupported = key(12);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                unsupported.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                unsupported.clone(),
                vec![ClientParam::new("value", ClientType::Unknown)],
                ClientType::I32,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::int(1)))]),
                vec![],
            )],
        );
        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::UnsupportedSignatureType {
                helper,
                position: LinkedHelperSignaturePosition::Parameter(0),
                ty: ClientType::Unknown,
            }) if helper == unsupported
        ));

        let f32_helper = key(15);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                f32_helper.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                f32_helper.clone(),
                vec![ClientParam::new("value", ClientType::F32)],
                ClientType::Bool,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::bool(true)))]),
                vec![],
            )],
        );
        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::UnsupportedSignatureType {
                helper,
                position: LinkedHelperSignaturePosition::Parameter(0),
                ty: ClientType::F32,
            }) if helper == f32_helper
        ));

        let nested = key(13);
        let nested_type = ClientType::option(ClientType::option(ClientType::I32));
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                nested.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                nested.clone(),
                vec![],
                nested_type.clone(),
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::Literal(
                    crate::Value::Null,
                )))]),
                vec![],
            )],
        );
        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::NestedOptionType {
                helper,
                position: LinkedHelperSignaturePosition::Return,
                ty,
            }) if helper == nested && ty == nested_type
        ));

        let empty_return = key(14);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                empty_return.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                empty_return.clone(),
                vec![],
                ClientType::Bool,
                ClientBlock::from_stmts(vec![ClientStmt::Return(None)]),
                vec![],
            )],
        );
        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::ReturnShapeMismatch {
                helper,
                expected: ClientType::Bool,
                has_value: false,
            }) if helper == empty_return
        ));

        let fallthrough = key(15);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                fallthrough.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                fallthrough.clone(),
                vec![],
                ClientType::Bool,
                ClientBlock::from_stmts(vec![ClientStmt::let_value(
                    ClientLocalId(0),
                    "value",
                    ClientType::Unknown,
                    ClientExpr::bool(true),
                )]),
                vec![],
            )],
        );
        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::NonUnitReturnMayFallThrough {
                helper,
                expected: ClientType::Bool,
            }) if helper == fallthrough
        ));
    }

    #[test]
    fn unit_helper_may_return_a_unit_valued_tail_call() {
        let caller = key(16);
        let target = key(17);
        let call = ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(0),
                authored_path: "crate::target".to_owned(),
                name: "target".to_owned(),
            },
            vec![],
        );
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                caller.clone(),
            )],
            vec![
                LinkedHelperDefinition::new(
                    caller,
                    vec![],
                    ClientType::Unit,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(call))]),
                    vec![LinkedHelperCallBinding::new(
                        HelperCallEdgeId::new(0),
                        target.clone(),
                    )],
                ),
                LinkedHelperDefinition::new(
                    target,
                    vec![],
                    ClientType::Unit,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(None)]),
                    vec![],
                ),
            ],
        )
        .unwrap();

        assert_eq!(program.len(), 2);
    }

    #[test]
    fn rejects_integer_argument_for_string_helper() {
        let caller = key(20);
        let target = key(21);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                caller.clone(),
            )],
            vec![
                LinkedHelperDefinition::new(
                    caller.clone(),
                    vec![ClientParam::new("value", ClientType::I32)],
                    ClientType::String,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(helper_call(0)))]),
                    vec![LinkedHelperCallBinding::new(
                        HelperCallEdgeId::new(0),
                        target.clone(),
                    )],
                ),
                LinkedHelperDefinition::new(
                    target,
                    vec![ClientParam::new("text", ClientType::String)],
                    ClientType::String,
                    ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
                        ClientLocalId(0),
                    )))]),
                    vec![],
                ),
            ],
        );

        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::InvalidBodyType { helper, source })
                if helper == caller && matches!(
                    source.as_ref(),
                    ClientHelperBodyTypeError::TypeMismatch {
                        site: ClientHelperTypeSite::HelperArgument { edge, index: 0 },
                        expected: ClientType::String,
                        actual: ClientHelperInferredType::Exact(ClientType::I32),
                    } if *edge == HelperCallEdgeId::new(0)
                )
        ));
    }

    #[test]
    fn rejects_bad_return_type() {
        let target = key(22);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                target.clone(),
                vec![],
                ClientType::Bool,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::string(
                    "not a bool",
                )))]),
                vec![],
            )],
        );

        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::InvalidBodyType { helper, source })
                if helper == target && matches!(
                    source.as_ref(),
                    ClientHelperBodyTypeError::TypeMismatch {
                        site: ClientHelperTypeSite::Return,
                        expected: ClientType::Bool,
                        actual: ClientHelperInferredType::Exact(ClientType::String),
                    }
                )
        ));
    }

    #[test]
    fn rejects_bad_local_initializer_before_checked_arithmetic() {
        let target = key(23);
        let checked_add = ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Add,
            domain: IntegerDomain::U32,
        };
        let body = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(0),
                "forged_number",
                ClientType::Unknown,
                ClientExpr::string("wrong"),
            ),
            ClientStmt::Return(Some(ClientExpr::binary(
                checked_add,
                ClientExpr::local(ClientLocalId(0)),
                ClientExpr::int(1),
            ))),
        ]);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                target.clone(),
                vec![],
                ClientType::U32,
                body,
                vec![],
            )],
        );

        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::InvalidBodyType { helper, source })
                if helper == target && matches!(
                    source.as_ref(),
                    ClientHelperBodyTypeError::TypeMismatch {
                        site: ClientHelperTypeSite::BinaryLeft(op),
                        expected: ClientType::U32,
                        actual: ClientHelperInferredType::Exact(ClientType::String),
                    } if *op == checked_add
                )
        ));
    }

    #[test]
    fn rejects_operator_operand_mismatch() {
        let target = key(24);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                target.clone(),
                vec![
                    ClientParam::new("text", ClientType::String),
                    ClientParam::new("flag", ClientType::Bool),
                ],
                ClientType::Bool,
                ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::binary(
                    ClientBinaryOp::And,
                    ClientExpr::local(ClientLocalId(0)),
                    ClientExpr::local(ClientLocalId(1)),
                )))]),
                vec![],
            )],
        );

        assert!(matches!(
            program,
            Err(LinkedHelperLinkError::InvalidBodyType { helper, source })
                if helper == target && matches!(
                    source.as_ref(),
                    ClientHelperBodyTypeError::TypeMismatch {
                        site: ClientHelperTypeSite::BinaryLeft(ClientBinaryOp::And),
                        expected: ClientType::Bool,
                        actual: ClientHelperInferredType::Exact(ClientType::String),
                    }
                )
        ));
    }

    #[test]
    fn accepts_calendar_shaped_u32_code_with_contextual_literals() {
        fn checked(operation: CheckedIntegerOp, left: ClientExpr, right: ClientExpr) -> ClientExpr {
            ClientExpr::binary(
                ClientBinaryOp::Checked {
                    op: operation,
                    domain: IntegerDomain::U32,
                },
                left,
                right,
            )
        }

        let view_ym = ClientExpr::local(ClientLocalId(0));
        let sunday_start = ClientExpr::local(ClientLocalId(1));
        let index = ClientExpr::local(ClientLocalId(2));
        let year = ClientLocalId(3);
        let month = ClientLocalId(4);
        let adjusted_year = ClientLocalId(5);
        let sunday_dow = ClientLocalId(6);
        let first_offset = ClientLocalId(7);
        let days = ClientLocalId(8);
        let limit = ClientLocalId(9);
        let slot = ClientLocalId(10);
        let serial = ClientLocalId(11);

        let body = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                year,
                "year",
                ClientType::Unknown,
                checked(CheckedIntegerOp::Div, view_ym.clone(), ClientExpr::int(100)),
            ),
            ClientStmt::let_value(
                month,
                "month",
                ClientType::Unknown,
                checked(CheckedIntegerOp::Rem, view_ym.clone(), ClientExpr::int(100)),
            ),
            ClientStmt::let_value(
                adjusted_year,
                "adjusted_year",
                ClientType::Unknown,
                ClientExpr::Conditional {
                    condition: Box::new(ClientExpr::binary(
                        ClientBinaryOp::Lt,
                        ClientExpr::local(month),
                        ClientExpr::int(3),
                    )),
                    then_expr: Box::new(checked(
                        CheckedIntegerOp::Sub,
                        ClientExpr::local(year),
                        ClientExpr::int(1),
                    )),
                    else_expr: Box::new(ClientExpr::local(year)),
                },
            ),
            ClientStmt::let_value(
                sunday_dow,
                "sunday_dow",
                ClientType::Unknown,
                checked(
                    CheckedIntegerOp::Rem,
                    checked(
                        CheckedIntegerOp::Add,
                        ClientExpr::local(adjusted_year),
                        ClientExpr::int(1),
                    ),
                    ClientExpr::int(7),
                ),
            ),
            ClientStmt::let_value(
                first_offset,
                "first_offset",
                ClientType::Unknown,
                ClientExpr::Conditional {
                    condition: Box::new(sunday_start),
                    then_expr: Box::new(checked(
                        CheckedIntegerOp::Rem,
                        checked(
                            CheckedIntegerOp::Add,
                            ClientExpr::local(sunday_dow),
                            ClientExpr::int(1),
                        ),
                        ClientExpr::int(7),
                    )),
                    else_expr: Box::new(ClientExpr::local(sunday_dow)),
                },
            ),
            ClientStmt::let_value(
                days,
                "days",
                ClientType::Unknown,
                ClientExpr::Conditional {
                    condition: Box::new(ClientExpr::binary(
                        ClientBinaryOp::Eq,
                        ClientExpr::local(month),
                        ClientExpr::int(2),
                    )),
                    then_expr: Box::new(ClientExpr::int(29)),
                    else_expr: Box::new(ClientExpr::int(31)),
                },
            ),
            ClientStmt::let_value(
                limit,
                "limit",
                ClientType::Unknown,
                checked(
                    CheckedIntegerOp::Add,
                    ClientExpr::local(first_offset),
                    ClientExpr::local(days),
                ),
            ),
            ClientStmt::let_value(
                slot,
                "slot",
                ClientType::Unknown,
                ClientExpr::Conditional {
                    condition: Box::new(ClientExpr::binary(
                        ClientBinaryOp::Or,
                        ClientExpr::binary(
                            ClientBinaryOp::Lt,
                            index.clone(),
                            ClientExpr::local(first_offset),
                        ),
                        ClientExpr::binary(
                            ClientBinaryOp::Ge,
                            index.clone(),
                            ClientExpr::local(limit),
                        ),
                    )),
                    then_expr: Box::new(ClientExpr::int(0)),
                    else_expr: Box::new(checked(
                        CheckedIntegerOp::Add,
                        checked(
                            CheckedIntegerOp::Sub,
                            index,
                            ClientExpr::local(first_offset),
                        ),
                        ClientExpr::int(1),
                    )),
                },
            ),
            ClientStmt::let_value(
                serial,
                "serial",
                ClientType::Unknown,
                checked(
                    CheckedIntegerOp::Add,
                    checked(CheckedIntegerOp::Mul, view_ym, ClientExpr::int(100)),
                    ClientExpr::local(slot),
                ),
            ),
            ClientStmt::If {
                condition: ClientExpr::binary(
                    ClientBinaryOp::Eq,
                    ClientExpr::local(slot),
                    ClientExpr::int(0),
                ),
                then_block: ClientBlock::from_stmts(vec![ClientStmt::Return(Some(
                    ClientExpr::int(0),
                ))]),
                else_block: None,
            },
            ClientStmt::Return(Some(ClientExpr::local(serial))),
        ]);

        let target = key(25);
        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                target.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                target,
                vec![
                    ClientParam::new("view_ym", ClientType::U32),
                    ClientParam::new("sunday_start", ClientType::Bool),
                    ClientParam::new("index", ClientType::U32),
                ],
                ClientType::U32,
                body,
                vec![],
            )],
        )
        .unwrap();

        assert_eq!(program.len(), 1);
    }

    #[test]
    fn linked_value_object_methods_are_internal_and_transitively_emitted() {
        let type_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let object_ty = ClientType::value_object(
            type_key,
            vec![ClientObjectField {
                name: "open".to_owned(),
                ty: ClientType::Bool,
            }],
        );
        let constructor = key(40);
        let toggle = key(41);
        let root = key(42);
        let call = |edge: u32, name: &str, args: Vec<ClientExpr>| {
            ClientExpr::call(
                ClientFunctionRef::Helper {
                    edge: HelperCallEdgeId::new(edge),
                    authored_path: format!("crate::{name}"),
                    name: name.to_owned(),
                },
                args,
            )
        };

        let constructor_body =
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::StructInit {
                type_name: type_key.to_owned(),
                fields: vec![ClientStructFieldValue {
                    name: "open".to_owned(),
                    value: ClientExpr::bool(false),
                }],
            }))]);
        let self_local = ClientLocalId(1);
        let toggle_body = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                self_local,
                "self_value",
                ClientType::Unknown,
                ClientExpr::local(ClientLocalId(0)),
            ),
            ClientStmt::AssignPlace {
                place: ClientPlace::field(ClientPlace::local(self_local), "open"),
                value: ClientExpr::unary(
                    ClientUnaryOp::Not,
                    ClientExpr::Field {
                        base: Box::new(ClientExpr::local(self_local)),
                        name: "open".to_owned(),
                    },
                ),
            },
            ClientStmt::Return(None),
        ]);
        let object_local = ClientLocalId(0);
        let root_body = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                object_local,
                "object",
                ClientType::Unknown,
                call(0, "construct", vec![]),
            ),
            ClientStmt::Expr(call(1, "toggle", vec![ClientExpr::local(object_local)])),
            ClientStmt::Return(Some(ClientExpr::Field {
                base: Box::new(ClientExpr::local(object_local)),
                name: "open".to_owned(),
            })),
        ]);

        let program = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                root.clone(),
            )],
            vec![
                LinkedHelperDefinition::new(
                    constructor.clone(),
                    vec![],
                    object_ty.clone(),
                    constructor_body,
                    vec![],
                ),
                LinkedHelperDefinition::new(
                    toggle.clone(),
                    vec![ClientParam::new("self", object_ty.clone())],
                    ClientType::Unit,
                    toggle_body,
                    vec![],
                ),
                LinkedHelperDefinition::new(
                    root,
                    vec![],
                    ClientType::Bool,
                    root_body,
                    vec![
                        LinkedHelperCallBinding::new(HelperCallEdgeId::new(0), constructor),
                        LinkedHelperCallBinding::new(HelperCallEdgeId::new(1), toggle),
                    ],
                ),
            ],
        )
        .unwrap();

        let mut js = String::new();
        program.emit_private_functions(&mut js).unwrap();
        assert_eq!(js.matches("function __rsjs_helper_").count(), 3);
        assert!(js.contains("[\"open\"] = "));
    }

    #[test]
    fn rejects_value_object_at_deployable_root_boundary() {
        let type_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let object_ty = ClientType::value_object(
            type_key,
            vec![ClientObjectField {
                name: "open".to_owned(),
                ty: ClientType::Bool,
            }],
        );
        let constructor = key(43);
        let body =
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::StructInit {
                type_name: type_key.to_owned(),
                fields: vec![ClientStructFieldValue {
                    name: "open".to_owned(),
                    value: ClientExpr::bool(false),
                }],
            }))]);
        let error = LinkedHelperProgram::new(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                constructor.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                constructor.clone(),
                vec![],
                object_ty.clone(),
                body.clone(),
                vec![],
            )],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            LinkedHelperLinkError::ValueObjectCrossesRootBoundary {
                position: LinkedHelperSignaturePosition::Return,
                ..
            }
        ));

        LinkedHelperProgram::new_for_internal_validation(
            vec![LinkedHelperCallBinding::new(
                HelperCallEdgeId::new(0),
                constructor.clone(),
            )],
            vec![LinkedHelperDefinition::new(
                constructor,
                vec![],
                object_ty,
                body,
                vec![],
            )],
        )
        .expect("synthetic compiler validation is not a deployable object boundary");
    }
}
