// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Per-page state and renderer for `<Steps>` containers.
#[derive(Debug, Default)]
pub(super) struct Steps {
    depth: usize,
}

impl Steps {
    pub(super) fn open(&mut self, html: &mut String) {
        self.depth += 1;
        html.push_str("<div class=\"proa-docs-steps\">");
    }

    pub(super) fn close(&mut self, html: &mut String) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        html.push_str("</div>");
        true
    }

    pub(super) fn is_open(&self) -> bool {
        self.depth > 0
    }
}

/// Build-time renderer for a `<Step>` item.
pub(super) struct Step;

impl Step {
    pub(super) fn open(html: &mut String) {
        html.push_str("<div class=\"proa-docs-step\">");
    }

    pub(super) fn close(html: &mut String) {
        html.push_str("</div>");
    }
}
