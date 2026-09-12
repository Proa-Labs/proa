// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Build-time lowerer for the self-closing `<AgentTerminal />` directive.
pub(crate) struct AgentTerminal;

impl AgentTerminal {
    pub(crate) fn emit_markdown(out: &mut String) {
        out.push_str(
            "\n```text\nAgent fetches page -> receives server-rendered HTML -> extracts content without client JavaScript\n```\n\n",
        );
    }
}
