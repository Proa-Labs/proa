// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Small builders for Client IR v3 tests and conversion spikes.

use super::expr::{ClientFunctionId, ClientLocalId};
use super::module::{ClientFunction, ClientModule, ClientModuleKind};
use super::stmt::{ClientBlock, ClientStmt};

/// Allocates dense ids for a single [`ClientFunction`].
#[derive(Debug, Default)]
pub struct ClientFunctionBuilder {
    next_local: u32,
    body: ClientBlock,
}

impl ClientFunctionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn local(&mut self) -> ClientLocalId {
        let id = ClientLocalId(self.next_local);
        self.next_local += 1;
        id
    }

    pub fn push(&mut self, stmt: ClientStmt) {
        self.body.push(stmt);
    }

    pub fn finish(self) -> ClientBlock {
        self.body
    }
}

/// Allocates dense function ids for a single [`ClientModule`].
#[derive(Debug)]
pub struct ClientModuleBuilder {
    module: ClientModule,
    next_function: u32,
}

impl ClientModuleBuilder {
    pub fn new(name: impl Into<String>, kind: ClientModuleKind) -> Self {
        Self {
            module: ClientModule::new(name, kind),
            next_function: 0,
        }
    }

    pub fn function_id(&mut self) -> ClientFunctionId {
        let id = ClientFunctionId(self.next_function);
        self.next_function += 1;
        id
    }

    pub fn add_function(&mut self, name: impl Into<String>) -> ClientFunctionId {
        let id = self.function_id();
        self.module.add_function(ClientFunction::new(id, name));
        id
    }

    pub fn push_function(&mut self, function: ClientFunction) {
        self.next_function = self.next_function.max(function.id.0 + 1);
        self.module.add_function(function);
    }

    pub fn finish(self) -> ClientModule {
        self.module
    }
}
