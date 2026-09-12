// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Shared language icon used by code-block headers and explorer tabs.
pub(super) struct LanguageIcon {
    icon: crate::code_icon::CodeIcon,
}

impl LanguageIcon {
    pub(super) fn new(language: &str) -> Self {
        Self {
            icon: crate::code_icon::for_language(language),
        }
    }

    pub(super) fn emit_html(self, html: &mut String) {
        html.push_str(
            "<span class=\"proa-docs-code-file-icon\" data-code-file-icon data-code-icon=\"",
        );
        html.push_str(self.icon.name);
        html.push_str("\" aria-hidden=\"true\"><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"14\" height=\"14\"><path d=\"");
        html.push_str(self.icon.path);
        html.push_str("\" fill=\"currentColor\"/></svg></span>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_the_shared_language_mapping() {
        let mut html = String::new();
        LanguageIcon::new("rs").emit_html(&mut html);
        assert!(html.contains("data-code-icon=\"rust\""));
        assert!(html.contains("aria-hidden=\"true\""));
    }
}
