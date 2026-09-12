// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Trust-boundary validation for relocation-resolved standalone helpers.
//!
//! Macro lowering is intentionally restrictive, but final compilation must
//! not trust that an earlier transport phase enforced the restriction. This
//! analysis revalidates the pure, origin-free Client IR subset after helper
//! call relocation and also proves the lexical/dense local-id discipline that
//! JavaScript emission relies on.

use std::collections::BTreeSet;
use std::fmt;

use crate::abi::HelperCallEdgeId;
use crate::analysis::dep_collector::{
    collect_client_block_external_dependencies, collect_client_expr_external_dependencies,
    ClientBlockDependencyError, ClientBlockExternalDependencies,
};
use crate::client::{
    is_client_helper_runtime_intrinsic, ClientBlock, ClientExpr, ClientFunctionRef, ClientLocalId,
    ClientPlace, ClientStmt, ClientType,
};

/// One relocation-resolved helper-call occurrence in structural preorder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHelperCallOccurrence {
    pub edge: HelperCallEdgeId,
    pub authored_path: String,
    pub name: String,
    pub argument_count: usize,
}

/// Proof material computed from a valid standalone-helper body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHelperValidationReport {
    /// Every helper call in structural source order. Runtime intrinsics are
    /// deliberately absent.
    pub calls: Vec<ClientHelperCallOccurrence>,
    /// Independently recomputed external dependency summary. For a valid
    /// report, only `helper_edges` can be non-empty.
    pub dependencies: ClientBlockExternalDependencies,
    /// Number of immutable locals declared after the positional parameters.
    pub declared_local_count: u32,
}

/// A relocation-resolved helper body is outside the pure standalone subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientHelperValidationError {
    TooManyParameters {
        count: usize,
    },
    TooManyLocals,
    TooManyHelperCalls,
    InvalidLetType {
        local: ClientLocalId,
        actual: ClientType,
    },
    MissingLetInitializer {
        local: ClientLocalId,
    },
    DuplicateLocalDeclaration {
        local: ClientLocalId,
    },
    NonDenseLocalDeclaration {
        expected: ClientLocalId,
        actual: ClientLocalId,
    },
    UnknownLocalReference {
        local: ClientLocalId,
    },
    OutOfScopeLocalReference {
        local: ClientLocalId,
    },
    AssignmentToParameter {
        local: ClientLocalId,
    },
    InvalidObjectFieldName {
        name: String,
    },
    DuplicateObjectField {
        name: String,
    },
    EmptyHelperPath {
        edge: HelperCallEdgeId,
    },
    InvalidHelperName {
        edge: HelperCallEdgeId,
        authored_path: String,
        actual: String,
    },
    NonDenseHelperEdge {
        expected: HelperCallEdgeId,
        actual: HelperCallEdgeId,
    },
    UnsupportedFunctionReference {
        function: ClientFunctionRef,
    },
    UnsupportedRuntimeFunction {
        runtime: String,
    },
    InvalidRuntimeFunctionArity {
        runtime: String,
        expected: usize,
        actual: usize,
    },
    UnsupportedStatement {
        statement: String,
    },
    UnsupportedExpression {
        expression: String,
    },
    DependencyAnalysis(ClientBlockDependencyError),
    NonOriginFreeDependencies {
        dependencies: Box<ClientBlockExternalDependencies>,
        expected_helper_edges: Vec<HelperCallEdgeId>,
    },
}

impl fmt::Display for ClientHelperValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyParameters { count } => write!(
                formatter,
                "standalone helper has {count} parameters, exceeding the ClientLocalId space"
            ),
            Self::TooManyLocals => {
                formatter.write_str("standalone helper exceeds the ClientLocalId space")
            }
            Self::TooManyHelperCalls => {
                formatter.write_str("standalone helper exceeds the HelperCallEdgeId space")
            }
            Self::InvalidLetType { local, actual } => write!(
                formatter,
                "standalone helper local {} has stored type {actual:?}; helper lets must retain ClientType::Unknown and infer from their initializer",
                local.0
            ),
            Self::MissingLetInitializer { local } => write!(
                formatter,
                "standalone helper local {} has no initializer",
                local.0
            ),
            Self::DuplicateLocalDeclaration { local } => write!(
                formatter,
                "standalone helper local {} is declared more than once",
                local.0
            ),
            Self::NonDenseLocalDeclaration { expected, actual } => write!(
                formatter,
                "standalone helper declares local {} out of order; expected dense local {}",
                actual.0, expected.0
            ),
            Self::UnknownLocalReference { local } => write!(
                formatter,
                "standalone helper references undeclared or forward local {}",
                local.0
            ),
            Self::OutOfScopeLocalReference { local } => write!(
                formatter,
                "standalone helper references out-of-scope local {}",
                local.0
            ),
            Self::AssignmentToParameter { local } => write!(
                formatter,
                "standalone helper assigns through immutable parameter {}",
                local.0
            ),
            Self::InvalidObjectFieldName { name } => write!(
                formatter,
                "standalone helper uses an invalid empty object field name `{name}`"
            ),
            Self::DuplicateObjectField { name } => write!(
                formatter,
                "standalone helper initializes object field `{name}` more than once"
            ),
            Self::EmptyHelperPath { edge } => write!(
                formatter,
                "standalone helper edge {} has an empty authored path",
                edge.index()
            ),
            Self::InvalidHelperName {
                edge,
                authored_path,
                actual,
            } => write!(
                formatter,
                "standalone helper edge {} has terminal name `{actual}` inconsistent with authored path `{authored_path}`",
                edge.index()
            ),
            Self::NonDenseHelperEdge { expected, actual } => write!(
                formatter,
                "standalone helper call edge {} is out of structural source order; expected dense edge {}",
                actual.index(),
                expected.index()
            ),
            Self::UnsupportedFunctionReference { function } => write!(
                formatter,
                "standalone helper contains a non-relocation-resolved function reference: {function:?}"
            ),
            Self::UnsupportedRuntimeFunction { runtime } => write!(
                formatter,
                "standalone helper contains unsupported ambient runtime function `{runtime}`"
            ),
            Self::InvalidRuntimeFunctionArity {
                runtime,
                expected,
                actual,
            } => write!(
                formatter,
                "standalone helper calls runtime function `{runtime}` with {actual} arguments; expected {expected}"
            ),
            Self::UnsupportedStatement { statement } => write!(
                formatter,
                "standalone helper contains a statement outside the pure subset: {statement}"
            ),
            Self::UnsupportedExpression { expression } => write!(
                formatter,
                "standalone helper contains an expression outside the pure subset: {expression}"
            ),
            Self::DependencyAnalysis(error) => write!(
                formatter,
                "standalone helper dependency analysis failed: {error:?}"
            ),
            Self::NonOriginFreeDependencies {
                dependencies,
                expected_helper_edges,
            } => write!(
                formatter,
                "standalone helper has non-origin-free dependencies or a mismatched helper closure: computed {dependencies:?}, expected helper edges {expected_helper_edges:?}"
            ),
        }
    }
}

impl std::error::Error for ClientHelperValidationError {}

/// Validate a relocation-resolved expression-bodied standalone helper.
pub fn validate_client_helper_expr(
    expression: &ClientExpr,
    parameter_count: usize,
) -> Result<ClientHelperValidationReport, ClientHelperValidationError> {
    let mut validator = Validator::new(parameter_count)?;
    let scope = LocalScope::default();
    validator.validate_expr(expression, &scope)?;
    let dependencies = collect_client_expr_external_dependencies(expression)
        .map_err(ClientHelperValidationError::DependencyAnalysis)?;
    validator.finish(dependencies)
}

/// Validate a relocation-resolved block-bodied standalone helper.
pub fn validate_client_helper_block(
    block: &ClientBlock,
    parameter_count: usize,
) -> Result<ClientHelperValidationReport, ClientHelperValidationError> {
    let mut validator = Validator::new(parameter_count)?;
    let mut scope = LocalScope::default();
    validator.validate_block(block, &mut scope)?;
    let dependencies = collect_client_block_external_dependencies(block)
        .map_err(ClientHelperValidationError::DependencyAnalysis)?;
    validator.finish(dependencies)
}

#[derive(Clone, Default)]
struct LocalScope {
    lets: BTreeSet<ClientLocalId>,
}

struct Validator {
    parameter_count: u32,
    next_local: u32,
    calls: Vec<ClientHelperCallOccurrence>,
}

impl Validator {
    fn new(parameter_count: usize) -> Result<Self, ClientHelperValidationError> {
        let parameter_count = u32::try_from(parameter_count).map_err(|_| {
            ClientHelperValidationError::TooManyParameters {
                count: parameter_count,
            }
        })?;
        Ok(Self {
            parameter_count,
            next_local: parameter_count,
            calls: Vec::new(),
        })
    }

    fn validate_block(
        &mut self,
        block: &ClientBlock,
        scope: &mut LocalScope,
    ) -> Result<(), ClientHelperValidationError> {
        for statement in &block.stmts {
            match statement {
                ClientStmt::Let {
                    local, ty, value, ..
                } => {
                    if *ty != ClientType::Unknown {
                        return Err(ClientHelperValidationError::InvalidLetType {
                            local: *local,
                            actual: ty.clone(),
                        });
                    }
                    let value = value.as_ref().ok_or(
                        ClientHelperValidationError::MissingLetInitializer { local: *local },
                    )?;
                    self.validate_expr(value, scope)?;
                    let expected = ClientLocalId(self.next_local);
                    if local.0 < self.next_local {
                        return Err(ClientHelperValidationError::DuplicateLocalDeclaration {
                            local: *local,
                        });
                    }
                    if *local != expected {
                        return Err(ClientHelperValidationError::NonDenseLocalDeclaration {
                            expected,
                            actual: *local,
                        });
                    }
                    self.next_local = self
                        .next_local
                        .checked_add(1)
                        .ok_or(ClientHelperValidationError::TooManyLocals)?;
                    scope.lets.insert(*local);
                }
                ClientStmt::Return(value) => {
                    if let Some(value) = value {
                        self.validate_expr(value, scope)?;
                    }
                }
                ClientStmt::Expr(expression) => self.validate_expr(expression, scope)?,
                ClientStmt::Assign { local, value } => {
                    self.validate_assignable_local(*local, scope)?;
                    self.validate_expr(value, scope)?;
                }
                ClientStmt::AssignPlace { place, value } => {
                    self.validate_place(place, scope)?;
                    self.validate_expr(value, scope)?;
                }
                ClientStmt::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    self.validate_expr(condition, scope)?;
                    let mut then_scope = scope.clone();
                    self.validate_block(then_block, &mut then_scope)?;
                    if let Some(else_block) = else_block {
                        let mut else_scope = scope.clone();
                        self.validate_block(else_block, &mut else_scope)?;
                    }
                }
                unsupported => {
                    return Err(ClientHelperValidationError::UnsupportedStatement {
                        statement: format!("{unsupported:?}"),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_expr(
        &mut self,
        expression: &ClientExpr,
        scope: &LocalScope,
    ) -> Result<(), ClientHelperValidationError> {
        match expression {
            ClientExpr::Unit | ClientExpr::Literal(_) | ClientExpr::ArrayLit(_) => Ok(()),
            ClientExpr::Local(local) => self.validate_local(*local, scope),
            ClientExpr::Unary { expr, .. }
            | ClientExpr::IsNonEmpty { target: expr }
            | ClientExpr::StringToLowercase { target: expr }
            | ClientExpr::StringTransform { target: expr, .. } => self.validate_expr(expr, scope),
            ClientExpr::Binary { left, right, .. }
            | ClientExpr::StringContains {
                haystack: left,
                needle: right,
            }
            | ClientExpr::StringPredicate {
                haystack: left,
                needle: right,
                ..
            } => {
                self.validate_expr(left, scope)?;
                self.validate_expr(right, scope)
            }
            ClientExpr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.validate_expr(condition, scope)?;
                self.validate_expr(then_expr, scope)?;
                self.validate_expr(else_expr, scope)
            }
            ClientExpr::StructInit { type_name, fields } => {
                if type_name.is_empty() {
                    return Err(ClientHelperValidationError::UnsupportedExpression {
                        expression: "StructInit with an empty type name".to_owned(),
                    });
                }
                let mut names = BTreeSet::new();
                for field in fields {
                    if field.name.is_empty() {
                        return Err(ClientHelperValidationError::InvalidObjectFieldName {
                            name: field.name.clone(),
                        });
                    }
                    if !names.insert(field.name.clone()) {
                        return Err(ClientHelperValidationError::DuplicateObjectField {
                            name: field.name.clone(),
                        });
                    }
                    self.validate_expr(&field.value, scope)?;
                }
                Ok(())
            }
            ClientExpr::Field { base, name } => {
                if name.is_empty() {
                    return Err(ClientHelperValidationError::InvalidObjectFieldName {
                        name: name.clone(),
                    });
                }
                self.validate_expr(base, scope)
            }
            ClientExpr::Call { function, args } => {
                self.validate_call(function, args.len())?;
                for argument in args {
                    self.validate_expr(argument, scope)?;
                }
                Ok(())
            }
            unsupported => Err(ClientHelperValidationError::UnsupportedExpression {
                expression: format!("{unsupported:?}"),
            }),
        }
    }

    fn validate_local(
        &self,
        local: ClientLocalId,
        scope: &LocalScope,
    ) -> Result<(), ClientHelperValidationError> {
        if local.0 < self.parameter_count || scope.lets.contains(&local) {
            return Ok(());
        }
        if local.0 < self.next_local {
            return Err(ClientHelperValidationError::OutOfScopeLocalReference { local });
        }
        Err(ClientHelperValidationError::UnknownLocalReference { local })
    }

    fn validate_assignable_local(
        &self,
        local: ClientLocalId,
        scope: &LocalScope,
    ) -> Result<(), ClientHelperValidationError> {
        if local.0 < self.parameter_count {
            return Err(ClientHelperValidationError::AssignmentToParameter { local });
        }
        self.validate_local(local, scope)
    }

    fn validate_place(
        &self,
        place: &ClientPlace,
        scope: &LocalScope,
    ) -> Result<(), ClientHelperValidationError> {
        match place {
            ClientPlace::Local(local) => self.validate_assignable_local(*local, scope),
            ClientPlace::Field { base, name } => {
                if name.is_empty() {
                    return Err(ClientHelperValidationError::InvalidObjectFieldName {
                        name: name.clone(),
                    });
                }
                self.validate_place(base, scope)
            }
        }
    }

    fn validate_call(
        &mut self,
        function: &ClientFunctionRef,
        argument_count: usize,
    ) -> Result<(), ClientHelperValidationError> {
        match function {
            ClientFunctionRef::Helper {
                edge,
                authored_path,
                name,
            } => {
                let expected_index = u32::try_from(self.calls.len())
                    .map_err(|_| ClientHelperValidationError::TooManyHelperCalls)?;
                let expected = HelperCallEdgeId::new(expected_index);
                if *edge != expected {
                    return Err(ClientHelperValidationError::NonDenseHelperEdge {
                        expected,
                        actual: *edge,
                    });
                }
                if authored_path.is_empty() {
                    return Err(ClientHelperValidationError::EmptyHelperPath { edge: *edge });
                }
                let expected_name = authored_path.rsplit("::").next().unwrap_or(authored_path);
                if name != expected_name {
                    return Err(ClientHelperValidationError::InvalidHelperName {
                        edge: *edge,
                        authored_path: authored_path.clone(),
                        actual: name.clone(),
                    });
                }
                self.calls.push(ClientHelperCallOccurrence {
                    edge: *edge,
                    authored_path: authored_path.clone(),
                    name: name.clone(),
                    argument_count,
                });
                Ok(())
            }
            ClientFunctionRef::Runtime(runtime) => {
                if !is_client_helper_runtime_intrinsic(runtime) {
                    return Err(ClientHelperValidationError::UnsupportedRuntimeFunction {
                        runtime: runtime.clone(),
                    });
                }
                if argument_count != 1 {
                    return Err(ClientHelperValidationError::InvalidRuntimeFunctionArity {
                        runtime: runtime.clone(),
                        expected: 1,
                        actual: argument_count,
                    });
                }
                Ok(())
            }
            unsupported => Err(ClientHelperValidationError::UnsupportedFunctionReference {
                function: unsupported.clone(),
            }),
        }
    }

    fn finish(
        self,
        dependencies: ClientBlockExternalDependencies,
    ) -> Result<ClientHelperValidationReport, ClientHelperValidationError> {
        let expected_helper_edges = self.calls.iter().map(|call| call.edge).collect::<Vec<_>>();
        let has_non_helper_dependencies = !dependencies.signal_reads.is_empty()
            || !dependencies.signal_writes.is_empty()
            || !dependencies.queries.is_empty()
            || !dependencies.mutations.is_empty()
            || !dependencies.node_refs.is_empty()
            || !dependencies.callback_slots.is_empty()
            || !dependencies.client_module_calls.is_empty()
            || dependencies.uses_callback_payload
            || dependencies.uses_browser_read
            || dependencies.uses_browser_write
            || dependencies.uses_event_payload
            || dependencies.uses_node_payload;
        if has_non_helper_dependencies || dependencies.helper_edges != expected_helper_edges {
            return Err(ClientHelperValidationError::NonOriginFreeDependencies {
                dependencies: Box::new(dependencies),
                expected_helper_edges,
            });
        }
        Ok(ClientHelperValidationReport {
            calls: self.calls,
            dependencies,
            declared_local_count: self.next_local - self.parameter_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientFunctionRef;
    use crate::types::SignalId;

    fn helper_call(edge: u32, args: Vec<ClientExpr>) -> ClientExpr {
        ClientExpr::call(
            ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(edge),
                authored_path: "crate::target".to_owned(),
                name: "target".to_owned(),
            },
            args,
        )
    }

    #[test]
    fn validates_pure_body_and_reports_calls_dependencies_and_locals() {
        let unit = validate_client_helper_expr(&ClientExpr::Unit, 0).unwrap();
        assert!(unit.calls.is_empty());
        assert_eq!(
            unit.dependencies,
            ClientBlockExternalDependencies::default()
        );

        let block = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(1),
                "value",
                ClientType::Unknown,
                ClientExpr::int(7),
            ),
            ClientStmt::If {
                condition: ClientExpr::bool(true),
                then_block: ClientBlock::from_stmts(vec![ClientStmt::Return(Some(helper_call(
                    0,
                    vec![ClientExpr::local(ClientLocalId(1))],
                )))]),
                else_block: Some(ClientBlock::from_stmts(vec![ClientStmt::Return(Some(
                    ClientExpr::local(ClientLocalId(0)),
                ))])),
            },
        ]);

        let report = validate_client_helper_block(&block, 1).unwrap();

        assert_eq!(report.declared_local_count, 1);
        assert_eq!(
            report.dependencies.helper_edges,
            vec![HelperCallEdgeId::new(0)]
        );
        assert_eq!(report.calls.len(), 1);
        assert_eq!(report.calls[0].argument_count, 1);
        assert_eq!(report.calls[0].authored_path, "crate::target");
    }

    #[test]
    fn rejects_free_forward_duplicate_gapped_and_out_of_scope_locals() {
        let free = ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::local(
            ClientLocalId(1),
        )))]);
        assert!(matches!(
            validate_client_helper_block(&free, 1),
            Err(ClientHelperValidationError::UnknownLocalReference {
                local: ClientLocalId(1)
            })
        ));

        let self_reference = ClientBlock::from_stmts(vec![ClientStmt::let_value(
            ClientLocalId(0),
            "self_ref",
            ClientType::Unknown,
            ClientExpr::local(ClientLocalId(0)),
        )]);
        assert!(matches!(
            validate_client_helper_block(&self_reference, 0),
            Err(ClientHelperValidationError::UnknownLocalReference {
                local: ClientLocalId(0)
            })
        ));

        let duplicate = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(0),
                "first",
                ClientType::Unknown,
                ClientExpr::int(1),
            ),
            ClientStmt::let_value(
                ClientLocalId(0),
                "again",
                ClientType::Unknown,
                ClientExpr::int(2),
            ),
        ]);
        assert!(matches!(
            validate_client_helper_block(&duplicate, 0),
            Err(ClientHelperValidationError::DuplicateLocalDeclaration {
                local: ClientLocalId(0)
            })
        ));

        let gapped = ClientBlock::from_stmts(vec![ClientStmt::let_value(
            ClientLocalId(2),
            "gap",
            ClientType::Unknown,
            ClientExpr::int(1),
        )]);
        assert!(matches!(
            validate_client_helper_block(&gapped, 0),
            Err(ClientHelperValidationError::NonDenseLocalDeclaration {
                expected: ClientLocalId(0),
                actual: ClientLocalId(2)
            })
        ));

        let branch_leak = ClientBlock::from_stmts(vec![
            ClientStmt::If {
                condition: ClientExpr::bool(true),
                then_block: ClientBlock::from_stmts(vec![ClientStmt::let_value(
                    ClientLocalId(0),
                    "branch",
                    ClientType::Unknown,
                    ClientExpr::int(1),
                )]),
                else_block: None,
            },
            ClientStmt::Return(Some(ClientExpr::local(ClientLocalId(0)))),
        ]);
        assert!(matches!(
            validate_client_helper_block(&branch_leak, 0),
            Err(ClientHelperValidationError::OutOfScopeLocalReference {
                local: ClientLocalId(0)
            })
        ));
    }

    #[test]
    fn rejects_non_origin_free_shapes_and_unclosed_functions() {
        let signal = ClientExpr::signal_read(SignalId(0));
        assert!(matches!(
            validate_client_helper_expr(&signal, 0),
            Err(ClientHelperValidationError::UnsupportedExpression { .. })
        ));

        let effect = ClientBlock::from_stmts(vec![ClientStmt::signal_set(
            SignalId(0),
            ClientExpr::int(1),
        )]);
        assert!(matches!(
            validate_client_helper_block(&effect, 0),
            Err(ClientHelperValidationError::UnsupportedStatement { .. })
        ));

        for function in [
            ClientFunctionRef::Unresolved("crate::target".to_owned()),
            ClientFunctionRef::Import {
                module: "./ambient.js".to_owned(),
                name: "run".to_owned(),
            },
            ClientFunctionRef::Local(crate::ClientFunctionId(0)),
        ] {
            let expression = ClientExpr::call(function.clone(), vec![]);
            assert!(matches!(
                validate_client_helper_expr(&expression, 0),
                Err(ClientHelperValidationError::UnsupportedFunctionReference {
                    function: actual
                }) if actual == function
            ));
        }
    }

    #[test]
    fn rejects_malformed_helper_metadata_and_runtime_intrinsics() {
        let out_of_order = helper_call(1, vec![]);
        assert!(matches!(
            validate_client_helper_expr(&out_of_order, 0),
            Err(ClientHelperValidationError::NonDenseHelperEdge {
                expected,
                actual
            }) if expected == HelperCallEdgeId::new(0)
                && actual == HelperCallEdgeId::new(1)
        ));

        let arbitrary = ClientExpr::call(
            ClientFunctionRef::Runtime("globalThis.eval".to_owned()),
            vec![],
        );
        assert!(matches!(
            validate_client_helper_expr(&arbitrary, 0),
            Err(ClientHelperValidationError::UnsupportedRuntimeFunction { .. })
        ));

        for actual in [0, 2] {
            let intrinsic = ClientExpr::call(
                ClientFunctionRef::Runtime(
                    crate::client::CLIENT_HELPER_LEN_INTRINSIC_JS.to_owned(),
                ),
                (0..actual).map(|_| ClientExpr::string("x")).collect(),
            );
            assert!(matches!(
                validate_client_helper_expr(&intrinsic, 0),
                Err(ClientHelperValidationError::InvalidRuntimeFunctionArity {
                    expected: 1,
                    actual: found,
                    ..
                }) if found == actual
            ));
        }
    }
}
