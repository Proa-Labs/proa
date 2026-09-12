// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::{PageSchema, SchemaType};

#[derive(Debug, Clone, Default)]
pub struct ParsedFrontmatter {
    pub values: BTreeMap<String, Value>,
}

impl ParsedFrontmatter {
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(Value::as_str)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(Value::as_bool)
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.values.get(key).and_then(Value::as_i64)
    }

    /// A list of strings, written either as a YAML flow list
    /// (`keywords: [html_sync, html!]`) or as one comma-separated string
    /// (`keywords: html_sync, html!`). Blank entries are dropped.
    pub fn get_string_list(&self, key: &str) -> Vec<String> {
        let items: Vec<String> = match self.values.get(key) {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.clone()),
                    Value::Number(number) => Some(number.to_string()),
                    Value::Bool(flag) => Some(flag.to_string()),
                    _ => None,
                })
                .collect(),
            Some(Value::String(text)) => {
                // The simple parser keeps `[a, b]` as one string.
                let inner = text.trim();
                let inner = inner
                    .strip_prefix('[')
                    .and_then(|rest| rest.strip_suffix(']'))
                    .unwrap_or(inner);
                inner.split(',').map(str::to_owned).collect()
            }
            _ => Vec::new(),
        };
        items
            .into_iter()
            .map(|item| {
                item.trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_owned()
            })
            .filter(|item| !item.is_empty())
            .collect()
    }
}

pub fn split_frontmatter(markdown: &str) -> Result<(ParsedFrontmatter, &str), String> {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return Ok((ParsedFrontmatter::default(), markdown));
    };

    let Some(end) = rest.find("\n---") else {
        return Err("frontmatter start marker was not closed".to_string());
    };

    let raw = &rest[..end];
    let mut content = &rest[end + "\n---".len()..];
    if let Some(stripped) = content.strip_prefix("\r\n") {
        content = stripped;
    } else if let Some(stripped) = content.strip_prefix('\n') {
        content = stripped;
    }

    Ok((parse_simple_yaml(raw)?, content))
}

pub fn validate_schema(schema: &PageSchema, fm: &ParsedFrontmatter) -> Result<(), String> {
    for field in &schema.fields {
        let Some(value) = fm.values.get(&field.name) else {
            if field.required {
                return Err(format!(
                    "missing required frontmatter field `{}`",
                    field.name
                ));
            }
            continue;
        };

        match field.ty {
            SchemaType::String | SchemaType::Date => {
                if !value.is_string() {
                    return Err(format!(
                        "frontmatter field `{}` must be a string",
                        field.name
                    ));
                }
            }
            SchemaType::Bool { default } => {
                let _ = default;
                if !value.is_boolean() {
                    return Err(format!(
                        "frontmatter field `{}` must be a boolean",
                        field.name
                    ));
                }
            }
        }
    }

    Ok(())
}

fn parse_simple_yaml(raw: &str) -> Result<ParsedFrontmatter, String> {
    let mut values = BTreeMap::new();

    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            return Err(format!("invalid frontmatter line {}", index + 1));
        };

        let key = key.trim();
        if key.is_empty() {
            return Err(format!(
                "invalid empty frontmatter key on line {}",
                index + 1
            ));
        }

        values.insert(key.to_string(), parse_scalar(value.trim()));
    }

    Ok(ParsedFrontmatter { values })
}

fn parse_scalar(value: &str) -> Value {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(value) = trimmed.parse::<i64>() {
        return Value::Number(value.into());
    }
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return Value::String(trimmed[1..trimmed.len().saturating_sub(1)].to_string());
    }
    Value::String(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter() {
        let (fm, content) =
            split_frontmatter("---\ntitle: Hello\nhidden: true\nnav_order: 2\n---\n# Hello\n")
                .unwrap();
        assert_eq!(fm.get_str("title"), Some("Hello"));
        assert_eq!(fm.get_bool("hidden"), Some(true));
        assert_eq!(fm.get_i64("nav_order"), Some(2));
        assert_eq!(content, "# Hello\n");
    }

    #[test]
    fn string_lists_accept_flow_lists_and_comma_strings() {
        let (fm, _) = split_frontmatter(
            "---\nkeywords: [html_sync, \"html!\", text!]\ntags: a, b ,, c\nempty: []\n---\nbody",
        )
        .unwrap();
        assert_eq!(
            fm.get_string_list("keywords"),
            ["html_sync", "html!", "text!"]
        );
        assert_eq!(fm.get_string_list("tags"), ["a", "b", "c"]);
        assert!(fm.get_string_list("empty").is_empty());
        assert!(fm.get_string_list("missing").is_empty());
    }
}
