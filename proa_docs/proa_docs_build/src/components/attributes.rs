// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Read a quoted attribute from an authored MDX tag.
pub(crate) fn value(raw: &str, name: &str) -> Option<String> {
    let mut search = raw;
    loop {
        let start = search.find(name)?;
        let before_ok = start == 0
            || search[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after = &search[start + name.len()..];
        if before_ok {
            if let Some(after) = after.strip_prefix('=') {
                let quote = after.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let rest = &after[1..];
                    let end = rest.find(quote)?;
                    return Some(rest[..end].to_string());
                }
            }
        }
        search = after;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_exact_quoted_attributes() {
        let raw = r#"<Thing database-name="wrong" name='right' />"#;
        assert_eq!(value(raw, "name").as_deref(), Some("right"));
        assert_eq!(value(raw, "missing"), None);
    }
}
