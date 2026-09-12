// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Closed-world type verification for standalone client helpers.
//!
//! The ordinary island [`super::typing::TypeEnv`] is an inference service: it
//! deliberately returns [`ClientType::Unknown`] for calls and for shapes whose
//! surrounding island does not provide a proof.  A linked helper program has a
//! stronger contract.  At that boundary every parameter, return, local call
//! edge, and target signature is known, so malformed transported IR must be
//! rejected rather than allowed to rely on JavaScript coercion.
//!
//! Integer and floating-point literals need special treatment here.  Client IR
//! stores their browser representation (`Value::Int` is a Number and
//! `Value::BigInt` is a BigInt), but an unsuffixed Rust literal's precise
//! domain comes from context.  The verifier therefore keeps literals
//! contextual until a checked operator, parameter, local use, helper call, or
//! return signature supplies that domain.  This accepts valid `u32` calendar
//! arithmetic without treating every Number literal as an `i32`, while still
//! rejecting representation changes and out-of-domain values.

use std::collections::BTreeMap;
use std::fmt;

use crate::abi::HelperCallEdgeId;
use crate::client::{
    is_client_helper_runtime_intrinsic, ClientBinaryOp, ClientBlock, ClientBrowserEffect,
    ClientBrowserRead, ClientExpr, ClientFunctionRef, ClientLocalId, ClientNodeRefEffect,
    ClientPlace, ClientStmt, ClientType, ClientUnaryOp, IntegerDomain, StorageValueType,
};
use crate::types::SignalId;
use crate::Value;

use super::typing::TypeEnv;

/// Complete signature of one relocation-resolved helper-call target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHelperTargetSignature {
    pub params: Vec<ClientType>,
    pub return_ty: ClientType,
}

impl ClientHelperTargetSignature {
    pub fn new(params: Vec<ClientType>, return_ty: ClientType) -> Self {
        Self { params, return_ty }
    }
}

/// A value shape inferred while checking a standalone-helper body.
///
/// Literal variants are contextual source values, not additional public ABI
/// types.  `Optional` represents the erased JavaScript shape of a conditional
/// such as `if flag { Some(value) } else { None }` until an enclosing signature
/// supplies the option's inner type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientHelperInferredType {
    Exact(ClientType),
    NumberIntegerLiteral {
        min: i128,
        max: i128,
    },
    WideIntegerLiteral {
        min: i128,
        max: i128,
    },
    FloatLiteral,
    Null,
    Optional(Box<ClientHelperInferredType>),
    /// A `vec![...]`/array literal whose element type remains contextual.
    ///
    /// `ClientExpr::ArrayLit` stores representation-level [`Value`] leaves,
    /// so treating `[1]` as an exact `Vec<i32>` here would repeat the same
    /// premature-defaulting bug as treating the scalar `1` as an exact
    /// `i32`. The relocation-authenticated callee signature is the first
    /// phase that can recover Rust's contextual element type.
    ArrayLiteral {
        element: Option<Box<ClientHelperInferredType>>,
    },
    /// A helper-local nominal record whose field expressions may still be
    /// contextual until a linked value-object signature supplies their types.
    ValueObject {
        type_key: String,
        fields: Vec<(String, ClientHelperInferredType)>,
    },
    /// Rust `String::len`, slice `len`, and `Vec::len` return `usize`, which is
    /// intentionally not a helper ABI type.  The value may participate in a
    /// comparison with a contextual Number literal but not in arithmetic or a
    /// typed helper crossing.
    Length,
}

impl fmt::Display for ClientHelperInferredType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(ty) => write!(formatter, "{ty:?}"),
            Self::NumberIntegerLiteral { min, max } if min == max => {
                write!(formatter, "contextual Number integer literal {min}")
            }
            Self::NumberIntegerLiteral { min, max } => write!(
                formatter,
                "contextual Number integer literals in the range {min}..={max}"
            ),
            Self::WideIntegerLiteral { min, max } if min == max => {
                write!(formatter, "contextual BigInt integer literal {min}")
            }
            Self::WideIntegerLiteral { min, max } => write!(
                formatter,
                "contextual BigInt integer literals in the range {min}..={max}"
            ),
            Self::FloatLiteral => formatter.write_str("contextual float literal"),
            Self::Null => formatter.write_str("None/null"),
            Self::Optional(inner) => write!(formatter, "optional {inner}"),
            Self::ArrayLiteral { element: None } => {
                formatter.write_str("contextual empty array literal")
            }
            Self::ArrayLiteral {
                element: Some(element),
            } => write!(formatter, "contextual array literal of {element}"),
            Self::ValueObject { type_key, fields } => write!(
                formatter,
                "value object {type_key} with {} fields",
                fields.len()
            ),
            Self::Length => formatter.write_str("usize length"),
        }
    }
}

/// The typed position whose contract an expression failed to satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientHelperTypeSite {
    LetInitializer(ClientLocalId),
    Assignment(ClientLocalId),
    SignalSet(SignalId),
    SignalUpdate(SignalId),
    IfCondition,
    RangeStart,
    RangeEnd,
    Return,
    ConditionalCondition,
    UnaryOperand(ClientUnaryOp),
    BinaryLeft(ClientBinaryOp),
    BinaryRight(ClientBinaryOp),
    IsNonEmptyTarget,
    StringTarget,
    StringArgument,
    RuntimeArgument,
    BrowserArgument,
    NodeRefArgument,
    HelperArgument {
        edge: HelperCallEdgeId,
        index: usize,
    },
}

/// A structurally valid standalone-helper body violates its closed signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientHelperBodyTypeError {
    TooManyParameters {
        count: usize,
    },
    UnknownLocal {
        local: ClientLocalId,
    },
    MissingTargetSignature {
        edge: HelperCallEdgeId,
    },
    HelperArityMismatch {
        edge: HelperCallEdgeId,
        expected: usize,
        actual: usize,
    },
    TypeMismatch {
        site: ClientHelperTypeSite,
        expected: ClientType,
        actual: ClientHelperInferredType,
    },
    InvalidUnaryOperand {
        op: ClientUnaryOp,
        actual: ClientHelperInferredType,
    },
    InvalidCollectionOperand {
        site: ClientHelperTypeSite,
        actual: ClientHelperInferredType,
    },
    InvalidBinaryOperands {
        op: ClientBinaryOp,
        left: ClientHelperInferredType,
        right: ClientHelperInferredType,
    },
    IncompatibleConditionalBranches {
        then_ty: ClientHelperInferredType,
        else_ty: ClientHelperInferredType,
    },
    InvalidCheckedNegDomain {
        domain: IntegerDomain,
    },
    UnsupportedFunctionReference {
        function: ClientFunctionRef,
    },
    UnsupportedStatement {
        statement: String,
    },
    UnsupportedExpression {
        expression: String,
    },
}

impl fmt::Display for ClientHelperBodyTypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyParameters { count } => write!(
                formatter,
                "standalone helper has {count} typed parameters, exceeding the ClientLocalId space"
            ),
            Self::UnknownLocal { local } => write!(
                formatter,
                "standalone helper type checking encountered unbound local {}",
                local.0
            ),
            Self::MissingTargetSignature { edge } => write!(
                formatter,
                "standalone helper edge {} has no resolved target signature",
                edge.index()
            ),
            Self::HelperArityMismatch {
                edge,
                expected,
                actual,
            } => write!(
                formatter,
                "standalone helper edge {} has {actual} arguments; target signature expects {expected}",
                edge.index()
            ),
            Self::TypeMismatch {
                site,
                expected,
                actual,
            } => write!(
                formatter,
                "standalone helper {site:?} expects {expected:?}, but expression has {actual}"
            ),
            Self::InvalidUnaryOperand { op, actual } => write!(
                formatter,
                "standalone helper unary operator {op:?} cannot consume {actual}"
            ),
            Self::InvalidCollectionOperand { site, actual } => write!(
                formatter,
                "standalone helper {site:?} requires a String, Vec, or slice value, but expression has {actual}"
            ),
            Self::InvalidBinaryOperands { op, left, right } => write!(
                formatter,
                "standalone helper binary operator {op:?} cannot combine {left} and {right}"
            ),
            Self::IncompatibleConditionalBranches {
                then_ty,
                else_ty,
            } => write!(
                formatter,
                "standalone helper conditional branches have incompatible types {then_ty} and {else_ty}"
            ),
            Self::InvalidCheckedNegDomain { domain } => write!(
                formatter,
                "standalone helper uses checked negation with unsigned domain {}",
                domain.as_str()
            ),
            Self::UnsupportedFunctionReference { function } => write!(
                formatter,
                "standalone helper type checking encountered unsupported function reference {function:?}"
            ),
            Self::UnsupportedStatement { statement } => write!(
                formatter,
                "standalone helper type checking encountered unsupported statement {statement}"
            ),
            Self::UnsupportedExpression { expression } => write!(
                formatter,
                "standalone helper type checking encountered unsupported expression {expression}"
            ),
        }
    }
}

impl std::error::Error for ClientHelperBodyTypeError {}

/// Verify one standalone-helper body against its own signature and the exact
/// signatures selected for all of its relocation-resolved call edges.
///
/// This pass is deliberately closed-world: callers must provide every helper
/// target in `targets`.  It complements (and does not replace) structural and
/// origin-freedom validation.
pub fn validate_client_helper_block_types(
    block: &ClientBlock,
    parameter_types: &[ClientType],
    return_ty: &ClientType,
    targets: &BTreeMap<HelperCallEdgeId, ClientHelperTargetSignature>,
) -> Result<(), ClientHelperBodyTypeError> {
    u32::try_from(parameter_types.len()).map_err(|_| {
        ClientHelperBodyTypeError::TooManyParameters {
            count: parameter_types.len(),
        }
    })?;
    let mut verifier = HelperTypeVerifier {
        targets,
        locals: parameter_types
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                (
                    ClientLocalId(
                        u32::try_from(index)
                            .expect("parameter count was checked against ClientLocalId space"),
                    ),
                    ClientHelperInferredType::Exact(ty.clone()),
                )
            })
            .collect(),
        return_ty,
    };
    verifier.verify_block(block)
}

/// Verify every relocation-resolved standalone-helper call reachable from a
/// component/root expression.
///
/// Root Client IR has a wider vocabulary than a standalone-helper body: it
/// may read signals, browser state, event payloads, queries, and node refs.
/// [`TypeEnv`] remains authoritative for those leaf types. This verifier adds
/// the information unavailable before final linking -- exact helper target
/// signatures -- and propagates helper return types through the typed
/// expression contexts that consume them.
///
/// Validation is intentionally scoped to expressions that contain a helper
/// call. Unrelated root IR retains the ordinary best-effort `TypeEnv`
/// semantics, while a helper crossing itself is fail-closed: an `Unknown`
/// argument cannot satisfy a concrete helper parameter type.
pub fn validate_client_root_helper_expr_types(
    expression: &ClientExpr,
    env: TypeEnv<'_>,
    targets: &BTreeMap<HelperCallEdgeId, ClientHelperTargetSignature>,
    expected: Option<(&ClientType, ClientHelperTypeSite)>,
) -> Result<(), ClientHelperBodyTypeError> {
    let verifier = RootHelperTypeVerifier { targets, env };
    let inferred = verifier.infer_expr(expression)?;
    if inferred.contains_helper {
        if let Some((expected, site)) = expected {
            require_type(&inferred.ty, expected, site)?;
        }
    }
    Ok(())
}

/// Verify every relocation-resolved standalone-helper call reachable from a
/// component/root statement block.
///
/// The verifier binds locals in source order and checks helper results against
/// exact statement contracts such as `let` annotations, assignments, signal
/// writes, update bodies, and boolean conditions.
pub fn validate_client_root_helper_block_types(
    block: &ClientBlock,
    env: TypeEnv<'_>,
    targets: &BTreeMap<HelperCallEdgeId, ClientHelperTargetSignature>,
) -> Result<(), ClientHelperBodyTypeError> {
    RootHelperTypeVerifier { targets, env }.verify_block(block)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootExprInference {
    ty: ClientHelperInferredType,
    contains_helper: bool,
}

impl RootExprInference {
    fn leaf(ty: ClientHelperInferredType) -> Self {
        Self {
            ty,
            contains_helper: false,
        }
    }

    fn with_helper(ty: ClientHelperInferredType) -> Self {
        Self {
            ty,
            contains_helper: true,
        }
    }
}

struct RootHelperTypeVerifier<'a, 'env> {
    targets: &'a BTreeMap<HelperCallEdgeId, ClientHelperTargetSignature>,
    env: TypeEnv<'env>,
}

impl RootHelperTypeVerifier<'_, '_> {
    fn verify_block(mut self, block: &ClientBlock) -> Result<(), ClientHelperBodyTypeError> {
        self.verify_block_inner(block)
    }

    fn verify_block_inner(&mut self, block: &ClientBlock) -> Result<(), ClientHelperBodyTypeError> {
        for statement in &block.stmts {
            match statement {
                ClientStmt::Expr(expression) => {
                    self.infer_expr(expression)?;
                }
                ClientStmt::Let {
                    local, ty, value, ..
                } => {
                    let inferred = value
                        .as_ref()
                        .map(|value| self.infer_expr(value))
                        .transpose()?;
                    if let Some(inferred) = &inferred {
                        if inferred.contains_helper && !ty.is_unknown() {
                            require_type(
                                &inferred.ty,
                                ty,
                                ClientHelperTypeSite::LetInitializer(*local),
                            )?;
                        }
                    }
                    let bound = if ty.is_unknown() {
                        inferred
                            .as_ref()
                            .and_then(|inferred| exact_client_type(&inferred.ty))
                            .unwrap_or(ClientType::Unknown)
                    } else {
                        ty.clone()
                    };
                    self.env.bind_local(*local, bound);
                }
                ClientStmt::Assign { local, value } => {
                    let inferred = self.infer_expr(value)?;
                    let expected = self.env.local_type(*local);
                    if inferred.contains_helper && !expected.is_unknown() {
                        require_type(
                            &inferred.ty,
                            &expected,
                            ClientHelperTypeSite::Assignment(*local),
                        )?;
                    }
                }
                ClientStmt::AssignPlace { value, .. } => {
                    self.infer_expr(value)?;
                }
                ClientStmt::SignalSet { id, value } => {
                    self.verify_signal_value(*id, value, ClientHelperTypeSite::SignalSet(*id))?;
                }
                ClientStmt::SignalUpdate { id, closure } => {
                    self.verify_signal_value(
                        *id,
                        &closure.body,
                        ClientHelperTypeSite::SignalUpdate(*id),
                    )?;
                }
                ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
                    for argument in args {
                        self.infer_expr(argument)?;
                    }
                }
                ClientStmt::CallbackInvoke { payload, .. } => {
                    self.infer_expr(payload)?;
                }
                ClientStmt::BrowserEffect(effect) => self.verify_browser_effect(effect)?,
                ClientStmt::NodeRefEffect { effect, .. } => {
                    self.verify_node_ref_effect(effect)?;
                }
                ClientStmt::Return(value) => {
                    if let Some(value) = value {
                        self.infer_expr(value)?;
                    }
                }
                ClientStmt::Block(block) => self.verify_block_inner(block)?,
                ClientStmt::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    self.verify_expected_expr(
                        condition,
                        &ClientType::Bool,
                        ClientHelperTypeSite::IfCondition,
                    )?;
                    self.verify_block_inner(then_block)?;
                    if let Some(else_block) = else_block {
                        self.verify_block_inner(else_block)?;
                    }
                }
                ClientStmt::ForRange {
                    local,
                    integer_domain,
                    start,
                    end,
                    body,
                    ..
                } => {
                    let range_ty = integer_domain.client_type();
                    self.verify_required_expr_type(
                        start,
                        &range_ty,
                        ClientHelperTypeSite::RangeStart,
                    )?;
                    self.verify_required_expr_type(end, &range_ty, ClientHelperTypeSite::RangeEnd)?;
                    self.env.bind_local(*local, range_ty);
                    self.env.bind_local_integer_domain(*local, *integer_domain);
                    self.verify_block_inner(body)?;
                }
                ClientStmt::EventPreventDefault
                | ClientStmt::EventStopPropagation
                | ClientStmt::QueryRefetch { .. }
                | ClientStmt::QueryInvalidate { .. } => {}
            }
        }
        Ok(())
    }

    fn verify_signal_value(
        &self,
        id: SignalId,
        value: &ClientExpr,
        site: ClientHelperTypeSite,
    ) -> Result<(), ClientHelperBodyTypeError> {
        let expected = self.env.signal_type(id);
        if expected.is_unknown() {
            self.infer_expr(value)?;
            return Ok(());
        }
        self.verify_expected_expr(value, &expected, site)
            .map(|_| ())
    }

    fn verify_expected_expr(
        &self,
        expression: &ClientExpr,
        expected: &ClientType,
        site: ClientHelperTypeSite,
    ) -> Result<RootExprInference, ClientHelperBodyTypeError> {
        let inferred = self.infer_expr(expression)?;
        if inferred.contains_helper {
            require_type(&inferred.ty, expected, site)?;
        }
        Ok(inferred)
    }

    fn verify_required_expr_type(
        &self,
        expression: &ClientExpr,
        expected: &ClientType,
        site: ClientHelperTypeSite,
    ) -> Result<RootExprInference, ClientHelperBodyTypeError> {
        let inferred = self.infer_expr(expression)?;
        require_type(&inferred.ty, expected, site)?;
        Ok(inferred)
    }

    fn infer_expr(
        &self,
        expression: &ClientExpr,
    ) -> Result<RootExprInference, ClientHelperBodyTypeError> {
        match expression {
            ClientExpr::Unit => Ok(RootExprInference::leaf(ClientHelperInferredType::Exact(
                ClientType::Unit,
            ))),
            ClientExpr::Literal(value) => Ok(RootExprInference::leaf(infer_literal(value))),
            ClientExpr::ArrayLit(elements) => {
                Ok(RootExprInference::leaf(infer_array_literal(elements)))
            }
            ClientExpr::Local(_)
            | ClientExpr::SignalRead(_)
            | ClientExpr::QueryRead { .. }
            | ClientExpr::MutationRead { .. }
            | ClientExpr::EventRead(_)
            | ClientExpr::EventCurrentTarget
            | ClientExpr::EventCollectionLen(_)
            | ClientExpr::NodeRefRead { .. }
            | ClientExpr::EventTargetWithinNodeRef { .. }
            | ClientExpr::EventAttribute { .. }
            | ClientExpr::EventPayload { .. }
            | ClientExpr::CallbackPayload
            | ClientExpr::NodePayload { .. } => Ok(RootExprInference::leaf(
                ClientHelperInferredType::Exact(self.env.type_of(expression)),
            )),
            ClientExpr::QueryKey(parts) => {
                let contains_helper = self.infer_many(parts)?.contains_helper;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                    contains_helper,
                })
            }
            ClientExpr::EventCollectionRead { index, .. } => {
                let index = self.infer_expr(index)?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                    contains_helper: index.contains_helper,
                })
            }
            ClientExpr::DomRead { target, .. } => {
                let target = self.infer_expr(target)?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::BrowserRead(read) => self.infer_browser_read(expression, read),
            ClientExpr::Encoding { value, .. } => {
                let value = self.infer_expr(value)?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                    contains_helper: value.contains_helper,
                })
            }
            ClientExpr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let condition = self.verify_expected_expr(
                    condition,
                    &ClientType::Bool,
                    ClientHelperTypeSite::ConditionalCondition,
                )?;
                let then_ty = self.infer_expr(then_expr)?;
                let else_ty = self.infer_expr(else_expr)?;
                let contains_helper =
                    condition.contains_helper || then_ty.contains_helper || else_ty.contains_helper;
                let ty = match join_types(&then_ty.ty, &else_ty.ty) {
                    Some(ty) => ty,
                    None if contains_helper => {
                        return Err(ClientHelperBodyTypeError::IncompatibleConditionalBranches {
                            then_ty: then_ty.ty,
                            else_ty: else_ty.ty,
                        });
                    }
                    None => ClientHelperInferredType::Exact(self.env.type_of(expression)),
                };
                Ok(RootExprInference {
                    ty,
                    contains_helper,
                })
            }
            ClientExpr::IsNonEmpty { target } => {
                let target = self.infer_expr(target)?;
                if target.contains_helper && !is_string_or_vec(&target.ty) {
                    return Err(ClientHelperBodyTypeError::InvalidCollectionOperand {
                        site: ClientHelperTypeSite::IsNonEmptyTarget,
                        actual: target.ty,
                    });
                }
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(ClientType::Bool),
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::StringToLowercase { target }
            | ClientExpr::StringTransform { target, .. } => {
                let target = self.verify_expected_expr(
                    target,
                    &ClientType::String,
                    ClientHelperTypeSite::StringTarget,
                )?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(ClientType::String),
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::StringContains { haystack, needle }
            | ClientExpr::StringPredicate {
                haystack, needle, ..
            } => {
                let haystack = self.verify_expected_expr(
                    haystack,
                    &ClientType::String,
                    ClientHelperTypeSite::StringTarget,
                )?;
                let needle = self.verify_expected_expr(
                    needle,
                    &ClientType::String,
                    ClientHelperTypeSite::StringArgument,
                )?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(ClientType::Bool),
                    contains_helper: haystack.contains_helper || needle.contains_helper,
                })
            }
            ClientExpr::Project { target, path } => {
                let target = self.infer_expr(target)?;
                let ty = exact_client_type(&target.ty)
                    .map(|ty| ty.project_path(path))
                    .unwrap_or(ClientType::Unknown);
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(ty),
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::AsType { target, .. } => {
                let target = self.infer_expr(target)?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::IsPresent { target } => {
                let target = self.infer_expr(target)?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(ClientType::Bool),
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::Eq { target, value } | ClientExpr::Neq { target, value } => {
                let target = self.infer_expr(target)?;
                let literal = infer_literal(value);
                if target.contains_helper && !comparison_compatible(&target.ty, &literal) {
                    return Err(ClientHelperBodyTypeError::InvalidBinaryOperands {
                        op: if matches!(expression, ClientExpr::Eq { .. }) {
                            ClientBinaryOp::Eq
                        } else {
                            ClientBinaryOp::Ne
                        },
                        left: target.ty,
                        right: literal,
                    });
                }
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(ClientType::Bool),
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::Coalesce { target, fallback } => {
                let target = self.infer_expr(target)?;
                let fallback = infer_literal(fallback);
                let ty = match join_types(&target.ty, &fallback) {
                    Some(ty) => ty,
                    None if target.contains_helper => {
                        return Err(ClientHelperBodyTypeError::IncompatibleConditionalBranches {
                            then_ty: target.ty,
                            else_ty: fallback,
                        });
                    }
                    None => ClientHelperInferredType::Exact(self.env.type_of(expression)),
                };
                Ok(RootExprInference {
                    ty,
                    contains_helper: target.contains_helper,
                })
            }
            ClientExpr::Unary { op, expr } => {
                let operand = self.infer_expr(expr)?;
                let ty = if operand.contains_helper {
                    infer_unary_type(*op, operand.ty)?
                } else {
                    ClientHelperInferredType::Exact(self.env.type_of(expression))
                };
                Ok(RootExprInference {
                    ty,
                    contains_helper: operand.contains_helper,
                })
            }
            ClientExpr::Binary { op, left, right } => {
                let left = self.infer_expr(left)?;
                let right = self.infer_expr(right)?;
                let contains_helper = left.contains_helper || right.contains_helper;
                let ty = if contains_helper {
                    infer_binary_type(*op, left.ty, right.ty)?
                } else {
                    ClientHelperInferredType::Exact(self.env.type_of(expression))
                };
                Ok(RootExprInference {
                    ty,
                    contains_helper,
                })
            }
            ClientExpr::StructInit { fields, .. } => {
                let mut contains_helper = false;
                for field in fields {
                    contains_helper |= self.infer_expr(&field.value)?.contains_helper;
                }
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                    contains_helper,
                })
            }
            ClientExpr::Field { base, .. } => {
                let base = self.infer_expr(base)?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                    contains_helper: base.contains_helper,
                })
            }
            ClientExpr::Index { base, index } => {
                let base = self.infer_expr(base)?;
                let index = self.infer_expr(index)?;
                Ok(RootExprInference {
                    ty: ClientHelperInferredType::Exact(ClientType::Unknown),
                    contains_helper: base.contains_helper || index.contains_helper,
                })
            }
            ClientExpr::Call { function, args } => self.infer_root_call(expression, function, args),
        }
    }

    fn infer_root_call(
        &self,
        expression: &ClientExpr,
        function: &ClientFunctionRef,
        args: &[ClientExpr],
    ) -> Result<RootExprInference, ClientHelperBodyTypeError> {
        let ClientFunctionRef::Helper { edge, .. } = function else {
            let args = self.infer_many(args)?;
            return Ok(RootExprInference {
                ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
                contains_helper: args.contains_helper,
            });
        };
        let signature = self
            .targets
            .get(edge)
            .ok_or(ClientHelperBodyTypeError::MissingTargetSignature { edge: *edge })?;
        if args.len() != signature.params.len() {
            return Err(ClientHelperBodyTypeError::HelperArityMismatch {
                edge: *edge,
                expected: signature.params.len(),
                actual: args.len(),
            });
        }
        for (index, (argument, expected)) in args.iter().zip(&signature.params).enumerate() {
            let actual = self.infer_expr(argument)?;
            require_helper_argument_type(
                &actual.ty,
                expected,
                ClientHelperTypeSite::HelperArgument { edge: *edge, index },
            )?;
        }
        Ok(RootExprInference::with_helper(
            ClientHelperInferredType::Exact(signature.return_ty.clone()),
        ))
    }

    fn infer_many(
        &self,
        expressions: &[ClientExpr],
    ) -> Result<RootExprInference, ClientHelperBodyTypeError> {
        let mut contains_helper = false;
        for expression in expressions {
            contains_helper |= self.infer_expr(expression)?.contains_helper;
        }
        Ok(RootExprInference {
            ty: ClientHelperInferredType::Exact(ClientType::Unknown),
            contains_helper,
        })
    }

    fn infer_browser_read(
        &self,
        expression: &ClientExpr,
        read: &ClientBrowserRead,
    ) -> Result<RootExprInference, ClientHelperBodyTypeError> {
        let contains_helper = match read {
            ClientBrowserRead::Window(_) | ClientBrowserRead::Location(_) => false,
            ClientBrowserRead::Storage {
                value_type,
                key,
                fallback,
                ..
            } => {
                let key = self.verify_expected_expr(
                    key,
                    &ClientType::String,
                    ClientHelperTypeSite::BrowserArgument,
                )?;
                let expected = storage_value_client_type(*value_type);
                let fallback = fallback
                    .as_ref()
                    .map(|fallback| {
                        self.verify_expected_expr(
                            fallback,
                            &expected,
                            ClientHelperTypeSite::BrowserArgument,
                        )
                    })
                    .transpose()?;
                key.contains_helper || fallback.is_some_and(|fallback| fallback.contains_helper)
            }
            ClientBrowserRead::UrlSearchParam { key, fallback } => {
                let key = self.verify_expected_expr(
                    key,
                    &ClientType::String,
                    ClientHelperTypeSite::BrowserArgument,
                )?;
                let fallback = fallback
                    .as_ref()
                    .map(|fallback| {
                        self.verify_expected_expr(
                            fallback,
                            &ClientType::String,
                            ClientHelperTypeSite::BrowserArgument,
                        )
                    })
                    .transpose()?;
                key.contains_helper || fallback.is_some_and(|fallback| fallback.contains_helper)
            }
            ClientBrowserRead::Declared { args, .. } => self.infer_many(args)?.contains_helper,
        };
        Ok(RootExprInference {
            ty: ClientHelperInferredType::Exact(self.env.type_of(expression)),
            contains_helper,
        })
    }

    fn verify_browser_effect(
        &self,
        effect: &ClientBrowserEffect,
    ) -> Result<(), ClientHelperBodyTypeError> {
        let string = |expression: &ClientExpr| {
            self.verify_expected_expr(
                expression,
                &ClientType::String,
                ClientHelperTypeSite::BrowserArgument,
            )
            .map(|_| ())
        };
        match effect {
            ClientBrowserEffect::WindowScrollTo { x, y }
            | ClientBrowserEffect::WindowScrollBy { x, y } => {
                self.infer_expr(x)?;
                self.infer_expr(y)?;
            }
            ClientBrowserEffect::WindowPrint
            | ClientBrowserEffect::LocationReload
            | ClientBrowserEffect::HistoryBack
            | ClientBrowserEffect::HistoryForward => {}
            ClientBrowserEffect::LocationSetHash { value } => string(value)?,
            ClientBrowserEffect::LocationAssign { url }
            | ClientBrowserEffect::LocationReplace { url }
            | ClientBrowserEffect::HistoryPushState { url }
            | ClientBrowserEffect::HistoryReplaceState { url } => string(url)?,
            ClientBrowserEffect::HistoryGo { delta } => {
                self.infer_expr(delta)?;
            }
            ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
                string(name)?;
                if let Some(detail) = detail {
                    self.infer_expr(detail)?;
                }
            }
            ClientBrowserEffect::DocumentSetTitle { title } => string(title)?,
            ClientBrowserEffect::ClipboardWriteText { text } => string(text)?,
            ClientBrowserEffect::ClipboardReadTextInto { target, fallback } => {
                if let Some(fallback) = fallback {
                    self.verify_signal_value(
                        *target,
                        fallback,
                        ClientHelperTypeSite::BrowserArgument,
                    )?;
                }
            }
            ClientBrowserEffect::WebAuthnCreateJsonInto {
                options_json,
                target,
                fallback,
            }
            | ClientBrowserEffect::WebAuthnGetJsonInto {
                options_json,
                target,
                fallback,
            } => {
                string(options_json)?;
                if let Some(fallback) = fallback {
                    self.verify_signal_value(
                        *target,
                        fallback,
                        ClientHelperTypeSite::BrowserArgument,
                    )?;
                }
            }
            ClientBrowserEffect::EventFileTextInto {
                target, fallback, ..
            } => {
                if let Some(fallback) = fallback {
                    self.verify_signal_value(
                        *target,
                        fallback,
                        ClientHelperTypeSite::BrowserArgument,
                    )?;
                }
            }
            ClientBrowserEffect::StorageSetItem { key, value, .. } => {
                string(key)?;
                string(value)?;
            }
            ClientBrowserEffect::StorageRemoveItem { key, .. }
            | ClientBrowserEffect::UrlDeleteSearchParam { key, .. } => string(key)?,
            ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
                string(key)?;
                string(value)?;
            }
            ClientBrowserEffect::Declared { args, .. } => {
                for arg in args {
                    self.infer_expr(arg)?;
                }
            }
        }
        Ok(())
    }

    fn verify_node_ref_effect(
        &self,
        effect: &ClientNodeRefEffect,
    ) -> Result<(), ClientHelperBodyTypeError> {
        let string = |expression: &ClientExpr| {
            self.verify_expected_expr(
                expression,
                &ClientType::String,
                ClientHelperTypeSite::NodeRefArgument,
            )
            .map(|_| ())
        };
        match effect {
            ClientNodeRefEffect::ScrollTo { x, y } | ClientNodeRefEffect::ScrollBy { x, y } => {
                self.infer_expr(x)?;
                self.infer_expr(y)?;
            }
            ClientNodeRefEffect::SetPointerCapture(value)
            | ClientNodeRefEffect::ReleasePointerCapture(value) => {
                self.infer_expr(value)?;
            }
            ClientNodeRefEffect::SetValue(value)
            | ClientNodeRefEffect::SetCustomValidity(value)
            | ClientNodeRefEffect::SetTextContent(value)
            | ClientNodeRefEffect::AddClass(value)
            | ClientNodeRefEffect::RemoveClass(value)
            | ClientNodeRefEffect::ToggleClass(value)
            | ClientNodeRefEffect::SetAttribute { value, .. }
            | ClientNodeRefEffect::SetStyleProperty { value, .. } => string(value)?,
            ClientNodeRefEffect::SetChecked(value) => {
                self.verify_expected_expr(
                    value,
                    &ClientType::Bool,
                    ClientHelperTypeSite::NodeRefArgument,
                )?;
            }
            ClientNodeRefEffect::SetSelectionRange(start, end) => {
                self.infer_expr(start)?;
                self.infer_expr(end)?;
            }
            ClientNodeRefEffect::Focus
            | ClientNodeRefEffect::Blur
            | ClientNodeRefEffect::ScrollIntoView
            | ClientNodeRefEffect::Click
            | ClientNodeRefEffect::RequestSubmit
            | ClientNodeRefEffect::Reset
            | ClientNodeRefEffect::ReportValidity
            | ClientNodeRefEffect::ShowModal
            | ClientNodeRefEffect::Close
            | ClientNodeRefEffect::ShowPopover
            | ClientNodeRefEffect::HidePopover
            | ClientNodeRefEffect::TogglePopover
            | ClientNodeRefEffect::Select
            | ClientNodeRefEffect::RemoveAttribute { .. } => {}
        }
        Ok(())
    }
}

struct HelperTypeVerifier<'a> {
    targets: &'a BTreeMap<HelperCallEdgeId, ClientHelperTargetSignature>,
    locals: BTreeMap<ClientLocalId, ClientHelperInferredType>,
    return_ty: &'a ClientType,
}

impl HelperTypeVerifier<'_> {
    fn verify_block(&mut self, block: &ClientBlock) -> Result<(), ClientHelperBodyTypeError> {
        for statement in &block.stmts {
            match statement {
                ClientStmt::Let {
                    local, ty, value, ..
                } => {
                    let value = value.as_ref().ok_or_else(|| {
                        ClientHelperBodyTypeError::UnsupportedStatement {
                            statement: format!("uninitialized local {}", local.0),
                        }
                    })?;
                    let inferred = self.infer_expr(value)?;
                    let bound = if ty.is_unknown() {
                        inferred
                    } else {
                        self.require_type(
                            &inferred,
                            ty,
                            ClientHelperTypeSite::LetInitializer(*local),
                        )?;
                        ClientHelperInferredType::Exact(ty.clone())
                    };
                    self.locals.insert(*local, bound);
                }
                ClientStmt::Return(value) => {
                    let actual = match value {
                        Some(value) => self.infer_expr(value)?,
                        None => ClientHelperInferredType::Exact(ClientType::Unit),
                    };
                    self.require_type(&actual, self.return_ty, ClientHelperTypeSite::Return)?;
                }
                ClientStmt::Expr(expression) => {
                    self.infer_expr(expression)?;
                }
                ClientStmt::Assign { local, value } => {
                    let expected = self
                        .locals
                        .get(local)
                        .cloned()
                        .ok_or(ClientHelperBodyTypeError::UnknownLocal { local: *local })?;
                    let actual = self.infer_expr(value)?;
                    if join_types(&expected, &actual).is_none() {
                        return Err(ClientHelperBodyTypeError::TypeMismatch {
                            site: ClientHelperTypeSite::Assignment(*local),
                            expected: exact_client_type(&expected).unwrap_or(ClientType::Unknown),
                            actual,
                        });
                    }
                }
                ClientStmt::AssignPlace { place, value } => {
                    let (local, expected) = self.infer_place(place)?;
                    let actual = self.infer_expr(value)?;
                    if let Some(expected) = exact_client_type(&expected) {
                        self.require_type(
                            &actual,
                            &expected,
                            ClientHelperTypeSite::Assignment(local),
                        )?;
                    } else if join_types(&expected, &actual).is_none() {
                        return Err(ClientHelperBodyTypeError::TypeMismatch {
                            site: ClientHelperTypeSite::Assignment(local),
                            expected: ClientType::Unknown,
                            actual,
                        });
                    }
                }
                ClientStmt::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    let condition = self.infer_expr(condition)?;
                    self.require_type(
                        &condition,
                        &ClientType::Bool,
                        ClientHelperTypeSite::IfCondition,
                    )?;
                    let outer = self.locals.clone();
                    self.verify_block(then_block)?;
                    self.locals = outer.clone();
                    if let Some(else_block) = else_block {
                        self.verify_block(else_block)?;
                    }
                    self.locals = outer;
                }
                unsupported => {
                    return Err(ClientHelperBodyTypeError::UnsupportedStatement {
                        statement: format!("{unsupported:?}"),
                    });
                }
            }
        }
        Ok(())
    }

    fn infer_expr(
        &self,
        expression: &ClientExpr,
    ) -> Result<ClientHelperInferredType, ClientHelperBodyTypeError> {
        match expression {
            ClientExpr::Unit => Ok(ClientHelperInferredType::Exact(ClientType::Unit)),
            ClientExpr::Literal(value) => Ok(infer_literal(value)),
            ClientExpr::ArrayLit(elements) => Ok(infer_array_literal(elements)),
            ClientExpr::Local(local) => self
                .locals
                .get(local)
                .cloned()
                .ok_or(ClientHelperBodyTypeError::UnknownLocal { local: *local }),
            ClientExpr::Unary { op, expr } => self.infer_unary(*op, expr),
            ClientExpr::Binary { op, left, right } => self.infer_binary(*op, left, right),
            ClientExpr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let condition = self.infer_expr(condition)?;
                self.require_type(
                    &condition,
                    &ClientType::Bool,
                    ClientHelperTypeSite::ConditionalCondition,
                )?;
                let then_ty = self.infer_expr(then_expr)?;
                let else_ty = self.infer_expr(else_expr)?;
                join_types(&then_ty, &else_ty).ok_or(
                    ClientHelperBodyTypeError::IncompatibleConditionalBranches { then_ty, else_ty },
                )
            }
            ClientExpr::IsNonEmpty { target } => {
                let target = self.infer_expr(target)?;
                if !is_string_or_vec(&target) {
                    return Err(ClientHelperBodyTypeError::InvalidCollectionOperand {
                        site: ClientHelperTypeSite::IsNonEmptyTarget,
                        actual: target,
                    });
                }
                Ok(ClientHelperInferredType::Exact(ClientType::Bool))
            }
            ClientExpr::StringToLowercase { target }
            | ClientExpr::StringTransform { target, .. } => {
                let target = self.infer_expr(target)?;
                self.require_type(
                    &target,
                    &ClientType::String,
                    ClientHelperTypeSite::StringTarget,
                )?;
                Ok(ClientHelperInferredType::Exact(ClientType::String))
            }
            ClientExpr::StringContains { haystack, needle }
            | ClientExpr::StringPredicate {
                haystack, needle, ..
            } => {
                let haystack = self.infer_expr(haystack)?;
                self.require_type(
                    &haystack,
                    &ClientType::String,
                    ClientHelperTypeSite::StringTarget,
                )?;
                let needle = self.infer_expr(needle)?;
                self.require_type(
                    &needle,
                    &ClientType::String,
                    ClientHelperTypeSite::StringArgument,
                )?;
                Ok(ClientHelperInferredType::Exact(ClientType::Bool))
            }
            ClientExpr::StructInit { type_name, fields } => {
                let mut inferred = Vec::with_capacity(fields.len());
                for field in fields {
                    inferred.push((field.name.clone(), self.infer_expr(&field.value)?));
                }
                Ok(ClientHelperInferredType::ValueObject {
                    type_key: type_name.clone(),
                    fields: inferred,
                })
            }
            ClientExpr::Field { base, name } => {
                let base = self.infer_expr(base)?;
                infer_value_object_field(&base, name).ok_or_else(|| {
                    ClientHelperBodyTypeError::UnsupportedExpression {
                        expression: format!("field `{name}` on {base}"),
                    }
                })
            }
            ClientExpr::Call { function, args } => self.infer_call(function, args),
            unsupported => Err(ClientHelperBodyTypeError::UnsupportedExpression {
                expression: format!("{unsupported:?}"),
            }),
        }
    }

    fn infer_unary(
        &self,
        op: ClientUnaryOp,
        expression: &ClientExpr,
    ) -> Result<ClientHelperInferredType, ClientHelperBodyTypeError> {
        let actual = self.infer_expr(expression)?;
        infer_unary_type(op, actual)
    }

    fn infer_binary(
        &self,
        op: ClientBinaryOp,
        left: &ClientExpr,
        right: &ClientExpr,
    ) -> Result<ClientHelperInferredType, ClientHelperBodyTypeError> {
        let left = self.infer_expr(left)?;
        let right = self.infer_expr(right)?;
        infer_binary_type(op, left, right)
    }

    fn infer_call(
        &self,
        function: &ClientFunctionRef,
        args: &[ClientExpr],
    ) -> Result<ClientHelperInferredType, ClientHelperBodyTypeError> {
        match function {
            ClientFunctionRef::Helper { edge, .. } => {
                let signature = self
                    .targets
                    .get(edge)
                    .ok_or(ClientHelperBodyTypeError::MissingTargetSignature { edge: *edge })?;
                if args.len() != signature.params.len() {
                    return Err(ClientHelperBodyTypeError::HelperArityMismatch {
                        edge: *edge,
                        expected: signature.params.len(),
                        actual: args.len(),
                    });
                }
                for (index, (argument, expected)) in args.iter().zip(&signature.params).enumerate()
                {
                    let actual = self.infer_expr(argument)?;
                    require_helper_argument_type(
                        &actual,
                        expected,
                        ClientHelperTypeSite::HelperArgument { edge: *edge, index },
                    )?;
                }
                Ok(ClientHelperInferredType::Exact(signature.return_ty.clone()))
            }
            ClientFunctionRef::Runtime(runtime)
                if is_client_helper_runtime_intrinsic(runtime) && args.len() == 1 =>
            {
                let actual = self.infer_expr(&args[0])?;
                if !is_string_or_vec(&actual) {
                    return Err(ClientHelperBodyTypeError::InvalidCollectionOperand {
                        site: ClientHelperTypeSite::RuntimeArgument,
                        actual,
                    });
                }
                Ok(ClientHelperInferredType::Length)
            }
            unsupported => Err(ClientHelperBodyTypeError::UnsupportedFunctionReference {
                function: unsupported.clone(),
            }),
        }
    }

    fn require_type(
        &self,
        actual: &ClientHelperInferredType,
        expected: &ClientType,
        site: ClientHelperTypeSite,
    ) -> Result<(), ClientHelperBodyTypeError> {
        require_type(actual, expected, site)
    }

    fn infer_place(
        &self,
        place: &ClientPlace,
    ) -> Result<(ClientLocalId, ClientHelperInferredType), ClientHelperBodyTypeError> {
        match place {
            ClientPlace::Local(local) => {
                let inferred = self
                    .locals
                    .get(local)
                    .ok_or(ClientHelperBodyTypeError::UnknownLocal { local: *local })?;
                Ok((*local, inferred.clone()))
            }
            ClientPlace::Field { base, name } => {
                let (local, base_ty) = self.infer_place(base)?;
                let field_ty = infer_value_object_field(&base_ty, name).ok_or_else(|| {
                    ClientHelperBodyTypeError::UnsupportedExpression {
                        expression: format!("assignment to unknown field `{name}` on {base_ty:?}"),
                    }
                })?;
                Ok((local, field_ty))
            }
        }
    }
}

fn infer_value_object_field(
    object: &ClientHelperInferredType,
    name: &str,
) -> Option<ClientHelperInferredType> {
    match object {
        ClientHelperInferredType::ValueObject { fields, .. } => fields
            .iter()
            .find_map(|(field, ty)| (field == name).then(|| ty.clone())),
        ClientHelperInferredType::Exact(ty) => {
            client_type_field(ty, name).map(ClientHelperInferredType::Exact)
        }
        _ => None,
    }
}

fn client_type_field(object: &ClientType, name: &str) -> Option<ClientType> {
    match object {
        ClientType::Object(fields) | ClientType::ValueObject { fields, .. } => fields
            .iter()
            .find_map(|field| (field.name == name).then(|| field.ty.clone())),
        _ => None,
    }
}

fn infer_unary_type(
    op: ClientUnaryOp,
    actual: ClientHelperInferredType,
) -> Result<ClientHelperInferredType, ClientHelperBodyTypeError> {
    match op {
        ClientUnaryOp::Not => {
            require_type(
                &actual,
                &ClientType::Bool,
                ClientHelperTypeSite::UnaryOperand(op),
            )?;
            Ok(ClientHelperInferredType::Exact(ClientType::Bool))
        }
        ClientUnaryOp::CheckedNeg(domain) => {
            if !domain.is_signed() {
                return Err(ClientHelperBodyTypeError::InvalidCheckedNegDomain { domain });
            }
            let ty = domain.client_type();
            require_type(&actual, &ty, ClientHelperTypeSite::UnaryOperand(op))?;
            Ok(ClientHelperInferredType::Exact(ty))
        }
        ClientUnaryOp::Neg => match actual {
            ClientHelperInferredType::Exact(ClientType::F32) => {
                Ok(ClientHelperInferredType::Exact(ClientType::F32))
            }
            ClientHelperInferredType::Exact(ClientType::F64) => {
                Ok(ClientHelperInferredType::Exact(ClientType::F64))
            }
            ClientHelperInferredType::FloatLiteral => Ok(ClientHelperInferredType::FloatLiteral),
            actual => Err(ClientHelperBodyTypeError::InvalidUnaryOperand { op, actual }),
        },
    }
}

fn infer_binary_type(
    op: ClientBinaryOp,
    left: ClientHelperInferredType,
    right: ClientHelperInferredType,
) -> Result<ClientHelperInferredType, ClientHelperBodyTypeError> {
    match op {
        ClientBinaryOp::Checked { domain, .. } => {
            let ty = domain.client_type();
            require_type(&left, &ty, ClientHelperTypeSite::BinaryLeft(op))?;
            require_type(&right, &ty, ClientHelperTypeSite::BinaryRight(op))?;
            Ok(ClientHelperInferredType::Exact(ty))
        }
        ClientBinaryOp::And | ClientBinaryOp::Or => {
            require_type(
                &left,
                &ClientType::Bool,
                ClientHelperTypeSite::BinaryLeft(op),
            )?;
            require_type(
                &right,
                &ClientType::Bool,
                ClientHelperTypeSite::BinaryRight(op),
            )?;
            Ok(ClientHelperInferredType::Exact(ClientType::Bool))
        }
        ClientBinaryOp::Eq
        | ClientBinaryOp::Ne
        | ClientBinaryOp::Lt
        | ClientBinaryOp::Le
        | ClientBinaryOp::Gt
        | ClientBinaryOp::Ge => {
            if !comparison_compatible(&left, &right) {
                return Err(ClientHelperBodyTypeError::InvalidBinaryOperands { op, left, right });
            }
            Ok(ClientHelperInferredType::Exact(ClientType::Bool))
        }
        ClientBinaryOp::Add => {
            if matches!(left, ClientHelperInferredType::Exact(ClientType::String))
                && matches!(right, ClientHelperInferredType::Exact(ClientType::String))
            {
                return Ok(ClientHelperInferredType::Exact(ClientType::String));
            }
            infer_float_binary(op, left, right)
        }
        ClientBinaryOp::Sub | ClientBinaryOp::Mul | ClientBinaryOp::Div | ClientBinaryOp::Rem => {
            infer_float_binary(op, left, right)
        }
        // Standalone helper integer arithmetic is required to retain an exact
        // Checked domain. Dynamic and legacy truncating operators have no
        // valid closed helper-signature interpretation.
        ClientBinaryOp::DivInt
        | ClientBinaryOp::DynAdd
        | ClientBinaryOp::DynSub
        | ClientBinaryOp::DynMul
        | ClientBinaryOp::DynDiv
        | ClientBinaryOp::DynDivInt
        | ClientBinaryOp::DynRem
        | ClientBinaryOp::DynEq
        | ClientBinaryOp::DynNe => {
            Err(ClientHelperBodyTypeError::InvalidBinaryOperands { op, left, right })
        }
    }
}

fn require_type(
    actual: &ClientHelperInferredType,
    expected: &ClientType,
    site: ClientHelperTypeSite,
) -> Result<(), ClientHelperBodyTypeError> {
    if is_assignable(actual, expected) {
        Ok(())
    } else {
        Err(ClientHelperBodyTypeError::TypeMismatch {
            site,
            expected: expected.clone(),
            actual: actual.clone(),
        })
    }
}

fn require_helper_argument_type(
    actual: &ClientHelperInferredType,
    expected: &ClientType,
    site: ClientHelperTypeSite,
) -> Result<(), ClientHelperBodyTypeError> {
    if is_helper_argument_assignable(actual, expected) {
        Ok(())
    } else {
        Err(ClientHelperBodyTypeError::TypeMismatch {
            site,
            expected: expected.clone(),
            actual: actual.clone(),
        })
    }
}

fn exact_client_type(inferred: &ClientHelperInferredType) -> Option<ClientType> {
    match inferred {
        ClientHelperInferredType::Exact(ty) => Some(ty.clone()),
        _ => None,
    }
}

fn storage_value_client_type(value_type: StorageValueType) -> ClientType {
    match value_type {
        StorageValueType::String => ClientType::String,
        StorageValueType::Bool => ClientType::Bool,
        StorageValueType::I64 => ClientType::I64,
        StorageValueType::F64 => ClientType::F64,
    }
}

fn infer_literal(value: &Value) -> ClientHelperInferredType {
    match value {
        Value::Bool(_) => ClientHelperInferredType::Exact(ClientType::Bool),
        Value::Int(value) => ClientHelperInferredType::NumberIntegerLiteral {
            min: *value as i128,
            max: *value as i128,
        },
        Value::BigInt(value) => ClientHelperInferredType::WideIntegerLiteral {
            min: *value,
            max: *value,
        },
        Value::Float(_) => ClientHelperInferredType::FloatLiteral,
        Value::String(_) => ClientHelperInferredType::Exact(ClientType::String),
        Value::Null => ClientHelperInferredType::Null,
    }
}

fn infer_array_literal(elements: &[Value]) -> ClientHelperInferredType {
    let mut element = None;
    for value in elements {
        let next = infer_literal(value);
        element = Some(match element {
            None => next,
            Some(previous) => join_types(&previous, &next)
                .unwrap_or(ClientHelperInferredType::Exact(ClientType::Unknown)),
        });
    }
    ClientHelperInferredType::ArrayLiteral {
        element: element.map(Box::new),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssignmentContext {
    /// Preserve the concrete JavaScript representation carried by Client IR.
    Strict,
    /// Recover the source type of an unsuffixed literal from a
    /// relocation-authenticated helper parameter signature.
    HelperArgument,
}

fn is_assignable(actual: &ClientHelperInferredType, expected: &ClientType) -> bool {
    is_assignable_in_context(actual, expected, AssignmentContext::Strict)
}

fn is_helper_argument_assignable(actual: &ClientHelperInferredType, expected: &ClientType) -> bool {
    is_assignable_in_context(actual, expected, AssignmentContext::HelperArgument)
}

fn is_assignable_in_context(
    actual: &ClientHelperInferredType,
    expected: &ClientType,
    context: AssignmentContext,
) -> bool {
    if let ClientHelperInferredType::Exact(actual) = actual {
        if actual == expected {
            return true;
        }
    }

    if let ClientType::Option(inner) = expected {
        return match actual {
            ClientHelperInferredType::Null => true,
            ClientHelperInferredType::Optional(actual) => {
                is_assignable_in_context(actual, inner, context)
            }
            // `Some(value)` is representation-transparent in Client IR.  Its
            // source wrapper is authenticated before this pass, so accepting
            // the inner value here is the only faithful closed-IR rule.
            actual => is_assignable_in_context(actual, inner, context),
        };
    }

    match actual {
        ClientHelperInferredType::ValueObject { type_key, fields } => match expected {
            ClientType::ValueObject {
                type_key: expected_key,
                fields: expected_fields,
            } => {
                type_key == expected_key
                    && fields.len() == expected_fields.len()
                    && fields
                        .iter()
                        .zip(expected_fields)
                        .all(|((name, actual), expected)| {
                            name == &expected.name
                                && is_assignable_in_context(actual, &expected.ty, context)
                        })
            }
            _ => false,
        },
        ClientHelperInferredType::NumberIntegerLiteral { min, max } => match expected {
            ClientType::I32 => *min >= i32::MIN as i128 && *max <= i32::MAX as i128,
            ClientType::U32 => *min >= 0 && *max <= u32::MAX as i128,
            // Only a resolved helper signature authenticates the missing Rust
            // context of an unsuffixed literal. Other typed positions must
            // preserve the Number representation transported by Client IR.
            ClientType::I64 if context == AssignmentContext::HelperArgument => {
                *min >= i64::MIN as i128 && *max <= i64::MAX as i128
            }
            ClientType::U64 if context == AssignmentContext::HelperArgument => {
                *min >= 0 && *max <= u64::MAX as i128
            }
            _ => false,
        },
        ClientHelperInferredType::WideIntegerLiteral { min, max } => match expected {
            ClientType::I64 => *min >= i64::MIN as i128 && *max <= i64::MAX as i128,
            ClientType::U64 => *min >= 0 && *max <= u64::MAX as i128,
            _ => false,
        },
        ClientHelperInferredType::ArrayLiteral { element } => match expected {
            ClientType::Vec(expected) => element
                .as_deref()
                .is_none_or(|actual| is_assignable_in_context(actual, expected, context)),
            _ => false,
        },
        ClientHelperInferredType::FloatLiteral => {
            matches!(expected, ClientType::F32 | ClientType::F64)
        }
        ClientHelperInferredType::Exact(_)
        | ClientHelperInferredType::Null
        | ClientHelperInferredType::Optional(_)
        | ClientHelperInferredType::Length => false,
    }
}

fn is_string_or_vec(actual: &ClientHelperInferredType) -> bool {
    matches!(
        actual,
        ClientHelperInferredType::Exact(ClientType::String | ClientType::Vec(_))
            | ClientHelperInferredType::ArrayLiteral { .. }
    )
}

fn infer_float_binary(
    op: ClientBinaryOp,
    left: ClientHelperInferredType,
    right: ClientHelperInferredType,
) -> Result<ClientHelperInferredType, ClientHelperBodyTypeError> {
    match (&left, &right) {
        (
            ClientHelperInferredType::Exact(ClientType::F32),
            ClientHelperInferredType::Exact(ClientType::F32)
            | ClientHelperInferredType::FloatLiteral,
        )
        | (
            ClientHelperInferredType::FloatLiteral,
            ClientHelperInferredType::Exact(ClientType::F32),
        ) => Ok(ClientHelperInferredType::Exact(ClientType::F32)),
        (
            ClientHelperInferredType::Exact(ClientType::F64),
            ClientHelperInferredType::Exact(ClientType::F64)
            | ClientHelperInferredType::FloatLiteral,
        )
        | (
            ClientHelperInferredType::FloatLiteral,
            ClientHelperInferredType::Exact(ClientType::F64),
        ) => Ok(ClientHelperInferredType::Exact(ClientType::F64)),
        (ClientHelperInferredType::FloatLiteral, ClientHelperInferredType::FloatLiteral) => {
            Ok(ClientHelperInferredType::FloatLiteral)
        }
        _ => Err(ClientHelperBodyTypeError::InvalidBinaryOperands { op, left, right }),
    }
}

fn comparison_compatible(
    left: &ClientHelperInferredType,
    right: &ClientHelperInferredType,
) -> bool {
    if inferred_is_value_object(left) || inferred_is_value_object(right) {
        return false;
    }
    match (left, right) {
        (ClientHelperInferredType::Length, ClientHelperInferredType::Length) => return true,
        (
            ClientHelperInferredType::Length,
            ClientHelperInferredType::NumberIntegerLiteral { min, .. },
        )
        | (
            ClientHelperInferredType::NumberIntegerLiteral { min, .. },
            ClientHelperInferredType::Length,
        ) => return *min >= 0,
        _ => {}
    }

    // `Some(value)` is erased to `value`, so a null/value comparison is a
    // well-typed Option comparison even though the wrapper is not recoverable
    // from Client IR alone.
    if matches!(left, ClientHelperInferredType::Null)
        || matches!(right, ClientHelperInferredType::Null)
    {
        return true;
    }

    join_types(left, right).is_some()
}

fn inferred_is_value_object(value: &ClientHelperInferredType) -> bool {
    matches!(
        value,
        ClientHelperInferredType::ValueObject { .. }
            | ClientHelperInferredType::Exact(ClientType::ValueObject { .. })
    )
}

fn join_types(
    left: &ClientHelperInferredType,
    right: &ClientHelperInferredType,
) -> Option<ClientHelperInferredType> {
    if left == right {
        return Some(left.clone());
    }

    match (left, right) {
        (
            ClientHelperInferredType::NumberIntegerLiteral {
                min: left_min,
                max: left_max,
            },
            ClientHelperInferredType::NumberIntegerLiteral {
                min: right_min,
                max: right_max,
            },
        ) => Some(ClientHelperInferredType::NumberIntegerLiteral {
            min: (*left_min).min(*right_min),
            max: (*left_max).max(*right_max),
        }),
        (
            ClientHelperInferredType::WideIntegerLiteral {
                min: left_min,
                max: left_max,
            },
            ClientHelperInferredType::WideIntegerLiteral {
                min: right_min,
                max: right_max,
            },
        ) => Some(ClientHelperInferredType::WideIntegerLiteral {
            min: (*left_min).min(*right_min),
            max: (*left_max).max(*right_max),
        }),
        (actual, ClientHelperInferredType::Exact(expected)) if is_assignable(actual, expected) => {
            Some(ClientHelperInferredType::Exact(expected.clone()))
        }
        (ClientHelperInferredType::Exact(expected), actual) if is_assignable(actual, expected) => {
            Some(ClientHelperInferredType::Exact(expected.clone()))
        }
        (ClientHelperInferredType::Null, other) | (other, ClientHelperInferredType::Null) => {
            Some(optional_type(other.clone()))
        }
        (ClientHelperInferredType::Optional(left), ClientHelperInferredType::Optional(right)) => {
            join_types(left, right).map(|inner| ClientHelperInferredType::Optional(Box::new(inner)))
        }
        (ClientHelperInferredType::Optional(inner), other)
        | (other, ClientHelperInferredType::Optional(inner)) => join_types(inner, other)
            .map(|inner| ClientHelperInferredType::Optional(Box::new(inner))),
        (
            ClientHelperInferredType::ArrayLiteral {
                element: left_element,
            },
            ClientHelperInferredType::ArrayLiteral {
                element: right_element,
            },
        ) => {
            let element = match (left_element, right_element) {
                (None, None) => None,
                (Some(element), None) | (None, Some(element)) => Some(element.clone()),
                (Some(left), Some(right)) => join_types(left, right).map(Box::new),
            };
            Some(ClientHelperInferredType::ArrayLiteral { element })
        }
        _ => None,
    }
}

fn optional_type(inner: ClientHelperInferredType) -> ClientHelperInferredType {
    match inner {
        ClientHelperInferredType::Null => ClientHelperInferredType::Null,
        ClientHelperInferredType::Exact(ClientType::Option(_))
        | ClientHelperInferredType::Optional(_) => inner,
        inner => ClientHelperInferredType::Optional(Box::new(inner)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::typing::TypeEnv;
    use crate::client::{ClientFunctionRef, IntegerDomain};

    fn helper_ref(edge: HelperCallEdgeId, name: &str) -> ClientFunctionRef {
        ClientFunctionRef::Helper {
            edge,
            authored_path: format!("crate::{name}"),
            name: name.to_owned(),
        }
    }

    #[test]
    fn root_helper_arguments_contextually_type_wide_literals_through_option_and_vec() {
        let edge = HelperCallEdgeId::new(0);
        let nested_wide = ClientType::option(ClientType::vec(ClientType::option(ClientType::I64)));
        let targets = BTreeMap::from([(
            edge,
            ClientHelperTargetSignature::new(vec![ClientType::U64, nested_wide], ClientType::Unit),
        )]);
        let call = ClientExpr::call(
            helper_ref(edge, "consume_wide_literals"),
            vec![
                ClientExpr::int(8),
                ClientExpr::ArrayLit(vec![Value::Int(9), Value::Null, Value::Int(10)]),
            ],
        );

        validate_client_root_helper_expr_types(&call, TypeEnv::new(&[], None), &targets, None)
            .expect(
            "a resolved helper signature must contextually type scalar and nested wide literals",
        );
    }

    #[test]
    fn helper_to_helper_arguments_contextually_type_wide_number_literals() {
        let edge = HelperCallEdgeId::new(0);
        let targets = BTreeMap::from([(
            edge,
            ClientHelperTargetSignature::new(vec![ClientType::I64], ClientType::I64),
        )]);
        let block = ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::call(
            helper_ref(edge, "wide_identity"),
            vec![ClientExpr::int(41)],
        )))]);

        validate_client_helper_block_types(&block, &[], &ClientType::I64, &targets)
            .expect("a helper-to-helper call must use its resolved parameter signature");
    }

    fn wide_range_call_block(edge: HelperCallEdgeId) -> ClientBlock {
        let local = ClientLocalId(0);
        ClientBlock::from_stmts(vec![ClientStmt::ForRange {
            local,
            name: "index".to_owned(),
            integer_domain: IntegerDomain::U64,
            start: ClientExpr::Literal(Value::BigInt(0)),
            end: ClientExpr::Literal(Value::BigInt(3)),
            inclusive: false,
            body: ClientBlock::from_stmts(vec![ClientStmt::Expr(ClientExpr::call(
                ClientFunctionRef::Helper {
                    edge,
                    authored_path: "crate::wide_identity".to_owned(),
                    name: "wide_identity".to_owned(),
                },
                vec![ClientExpr::local(local)],
            ))]),
        }])
    }

    #[test]
    fn root_range_local_uses_its_exact_wide_domain_at_helper_crossings() {
        let edge = HelperCallEdgeId::new(0);
        let block = wide_range_call_block(edge);
        let targets = BTreeMap::from([(
            edge,
            ClientHelperTargetSignature::new(vec![ClientType::U64], ClientType::U64),
        )]);

        validate_client_root_helper_block_types(&block, TypeEnv::new(&[], None), &targets)
            .expect("a u64 range induction variable must satisfy a u64 helper parameter");

        let mismatched_targets = BTreeMap::from([(
            edge,
            ClientHelperTargetSignature::new(vec![ClientType::I64], ClientType::I64),
        )]);
        let error = validate_client_root_helper_block_types(
            &block,
            TypeEnv::new(&[], None),
            &mismatched_targets,
        )
        .expect_err("a u64 range induction variable must not be retyped as i64");
        assert!(matches!(
            error,
            ClientHelperBodyTypeError::TypeMismatch {
                site: ClientHelperTypeSite::HelperArgument {
                    edge: actual_edge,
                    index: 0,
                },
                expected: ClientType::I64,
                actual: ClientHelperInferredType::Exact(ClientType::U64),
            } if actual_edge == edge
        ));

        let mut invalid_bounds = wide_range_call_block(edge);
        let ClientStmt::ForRange { start, .. } = &mut invalid_bounds.stmts[0] else {
            unreachable!("the test fixture starts with a range")
        };
        *start = ClientExpr::Literal(Value::BigInt(-1));
        let error = validate_client_root_helper_block_types(
            &invalid_bounds,
            TypeEnv::new(&[], None),
            &targets,
        )
        .expect_err("u64 range metadata must not bless a negative ordinary bound");
        assert!(matches!(
            error,
            ClientHelperBodyTypeError::TypeMismatch {
                site: ClientHelperTypeSite::RangeStart,
                expected: ClientType::U64,
                actual: ClientHelperInferredType::WideIntegerLiteral { min: -1, max: -1 },
            }
        ));

        let mut number_backed_bounds = wide_range_call_block(edge);
        let ClientStmt::ForRange { start, .. } = &mut number_backed_bounds.stmts[0] else {
            unreachable!("the test fixture starts with a range")
        };
        *start = ClientExpr::int(0);
        let error = validate_client_root_helper_block_types(
            &number_backed_bounds,
            TypeEnv::new(&[], None),
            &targets,
        )
        .expect_err("u64 range metadata must not widen a Number-backed bound");
        assert!(matches!(
            error,
            ClientHelperBodyTypeError::TypeMismatch {
                site: ClientHelperTypeSite::RangeStart,
                expected: ClientType::U64,
                actual: ClientHelperInferredType::NumberIntegerLiteral { min: 0, max: 0 },
            }
        ));
    }
}
