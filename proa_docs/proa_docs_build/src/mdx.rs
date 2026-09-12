// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::components::{attr_value, AgentTerminal, Cards, Files};
use crate::config::{MdxConfig, MdxImageConfig};
use std::fmt::Write;

pub fn lower_mdx(raw: &str, config: &MdxConfig) -> Result<String, String> {
    if !config.enabled {
        return Ok(raw.to_string());
    }

    let mut out = String::with_capacity(raw.len());
    let mut in_code = false;
    let mut in_comment = false;
    let mut cards: Option<Cards> = None;
    let mut files: Option<Files> = None;

    for raw_line in raw.lines() {
        let mut line = if config.normalize_brace_escapes {
            normalize_mdx_escapes(raw_line)
        } else {
            raw_line.to_string()
        };
        if let Some(normalized) = crate::components::normalize_tabs_items(&line) {
            line = normalized;
        }
        if config.strip_comments && !in_code {
            let had_comment = in_comment || line.contains("{/*");
            line = strip_mdx_comments(&line, &mut in_comment);
            if had_comment && line.trim().is_empty() {
                // Keep the blank line so the comment cannot weld the blocks
                // on either side of it into one paragraph.
                out.push('\n');
                continue;
            }
        }
        let trimmed = line.trim();

        if is_fence_line(trimmed) {
            if cards.is_some() || files.is_some() {
                return Err("code fences are not supported inside MDX component blocks".to_string());
            }
            if in_code {
                out.push_str(trimmed);
                out.push('\n');
                in_code = false;
            } else {
                if config.normalize_fence_metadata {
                    out.push_str(&normalize_fence_open(trimmed));
                } else {
                    out.push_str(trimmed);
                }
                out.push('\n');
                in_code = true;
            }
            continue;
        }

        if in_code {
            out.push_str(&line);
            out.push('\n');
            continue;
        }

        if config.reject_exports && trimmed.starts_with("export ") {
            return Err("unsupported MDX export statement".to_string());
        }
        if config.strip_imports && is_import_line(trimmed) {
            continue;
        }

        if let Some(card_list) = cards.as_mut() {
            if trimmed == "</Cards>" {
                let cards = cards.take().expect("cards block");
                cards.emit_markdown(&mut out);
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            card_list.push(trimmed)?;
            continue;
        }

        if let Some(file_tree) = files.as_mut() {
            if trimmed == "</Files>" {
                let file_tree = files.take().expect("file tree block");
                file_tree.emit_markdown(&mut out);
                continue;
            }
            file_tree.push(trimmed)?;
            continue;
        }

        if config.cards && trimmed == "<Cards>" {
            cards = Some(Cards::default());
            continue;
        }
        if config.files && trimmed == "<Files>" {
            files = Some(Files::default());
            continue;
        }
        if config.agent_terminal && trimmed == "<AgentTerminal />" {
            AgentTerminal::emit_markdown(&mut out);
            continue;
        }

        if config.remove_layout_divs && is_layout_tag(trimmed) {
            continue;
        }

        if config.images.enabled {
            if let Some(image) = lower_image(trimmed, &config.images) {
                out.push_str(&image);
                out.push('\n');
                continue;
            }
        }

        if config.lower_paragraph_tags {
            if let Some(text) = lower_paragraph(trimmed) {
                let _ = writeln!(out, "{text}");
                out.push('\n');
                continue;
            }
        }

        // A container tag is handed to the Markdown renderer as raw HTML, and
        // its children are meant to flow through the normal pipeline. That
        // only happens when the tag stands alone as its own HTML block:
        // CommonMark ends such a block at a blank line, so without one the
        // children stay inside the tag's block and are dropped entirely.
        // Authors write the tags flush left, so only reflow those; an indented
        // tag may sit inside a list, where a blank line would end the list.
        if !line.starts_with(char::is_whitespace) && crate::components::is_container_tag(trimmed) {
            end_block(&mut out);
            out.push_str(trimmed);
            out.push_str("\n\n");
            continue;
        }

        if config.normalize_class_name {
            line = line.replace("className=", "class=");
        }
        if config.normalize_style_objects {
            line = normalize_style_objects(&line);
        }
        out.push_str(&line);
        out.push('\n');
    }

    if cards.is_some() {
        return Err("unclosed <Cards> block".to_string());
    }
    if files.is_some() {
        return Err("unclosed <Files> block".to_string());
    }
    if in_code {
        return Err("unclosed fenced code block".to_string());
    }

    Ok(out)
}

/// Ensure `out` ends with a blank line, so whatever comes next starts a new
/// Markdown block. No-op at the start of the document or after an existing
/// blank line, so this never inserts stray vertical space.
fn end_block(out: &mut String) {
    if out.is_empty() || out.ends_with("\n\n") {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}

fn normalize_mdx_escapes(line: &str) -> String {
    line.replace("{'{'}", "{").replace("{'}'}", "}")
}

/// Remove `{/* ... */}` comments, tracking spans that continue across lines.
///
/// MDX treats these as syntax rather than content, so they are dropped before
/// the Markdown parser runs. That keeps them out of the rendered HTML *and* out
/// of `page.markdown`, which is what the `.md` routes, search, and
/// `llms-full.txt` serve. Callers must skip fenced code, where the same
/// characters are a legitimate part of a JavaScript sample.
fn strip_mdx_comments(line: &str, in_comment: &mut bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        if *in_comment {
            let Some(index) = rest.find("*/}") else {
                return out;
            };
            *in_comment = false;
            rest = &rest[index + 3..];
        } else {
            let Some(index) = rest.find("{/*") else {
                out.push_str(rest);
                return out;
            };
            out.push_str(&rest[..index]);
            *in_comment = true;
            rest = &rest[index + 3..];
        }
    }
}

fn is_fence_line(trimmed: &str) -> bool {
    trimmed.starts_with("```")
}

fn normalize_fence_open(trimmed: &str) -> String {
    let info = trimmed.trim_start_matches("```").trim();
    if info.is_empty() {
        return "```".to_string();
    }
    let normalized = crate::code_fence::CodeFenceInfo::parse(info).normalized();
    if normalized.is_empty() {
        "```".to_string()
    } else {
        format!("```{normalized}")
    }
}

fn is_import_line(trimmed: &str) -> bool {
    trimmed.starts_with("import ") && trimmed.contains(" from ")
}

// proa-perf-ok(scope): build-time MDX lowering converts authored style objects into Markdown-safe text, not request-path HTML
fn normalize_style_objects(line: &str) -> String {
    let mut remaining = line;
    let mut out = String::with_capacity(line.len());
    while let Some(start) = remaining.find("style={{") {
        out.push_str(&remaining[..start]);
        let body_start = start + "style={{".len();
        let Some(end) = remaining[body_start..].find("}}") else {
            out.push_str(&remaining[start..]);
            return out;
        };
        let body = &remaining[body_start..body_start + end];
        let _ = write!(out, "style=\"{css}", css = style_object_to_css(body));
        out.push('"');
        remaining = &remaining[body_start + end + 2..];
    }
    out.push_str(remaining);
    out
}

fn style_object_to_css(body: &str) -> String {
    let mut css = String::new();
    for item in body.split(',') {
        let Some((name, value)) = item.split_once(':') else {
            continue;
        };
        let name = camel_to_kebab(name.trim());
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if name.is_empty() || value.is_empty() {
            continue;
        }
        if !css.is_empty() {
            css.push_str("; ");
        }
        let _ = write!(css, "{name}: {value}");
    }
    css
}

fn camel_to_kebab(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_layout_tag(trimmed: &str) -> bool {
    matches!(trimmed, "</div>" | "<div>")
        || trimmed.starts_with("<div ")
        || trimmed.starts_with("<div>")
}

fn lower_paragraph(trimmed: &str) -> Option<String> {
    let content = trimmed.strip_prefix("<p")?;
    let (_, content) = content.split_once('>')?;
    let content = content.strip_suffix("</p>")?;
    let content = content
        .replace("<strong>", "**")
        .replace("</strong>", "**")
        .replace("<code>", "`")
        .replace("</code>", "`");
    Some(content)
}

fn lower_image(trimmed: &str, config: &MdxImageConfig) -> Option<String> {
    if !trimmed.starts_with("<img ") {
        return None;
    }
    let src = normalize_image_path(&attr_value(trimmed, "src")?, config);
    let alt = attr_value(trimmed, "alt").unwrap_or_else(|| config.default_alt.clone());
    Some(format!("[Image: {alt}]({src})"))
}

fn normalize_image_path(path: &str, config: &MdxImageConfig) -> String {
    for (from, to) in &config.prefix_rewrites {
        if let Some(rest) = path.strip_prefix(from) {
            return format!("{to}{rest}");
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_single_line_and_inline_mdx_comments() {
        let lowered = lower_mdx(
            "{/* hidden */}\nBefore {/* mid */}after\n",
            &MdxConfig::proa(),
        )
        .unwrap();
        assert!(!lowered.contains("hidden"), "{lowered}");
        assert!(!lowered.contains("mid"), "{lowered}");
        assert!(lowered.contains("Before after"), "{lowered}");
    }

    #[test]
    fn strips_multi_line_comments_including_fenced_looking_content() {
        // A commented-out block often contains fences; they must not toggle
        // code state on the way through.
        let lowered = lower_mdx(
            "Keep me\n\n{/* The installer:\n\n```bash\nrm -rf /\n```\n\n- bullet */}\n\nAlso keep\n",
            &MdxConfig::proa(),
        )
        .unwrap();
        assert!(lowered.contains("Keep me"));
        assert!(lowered.contains("Also keep"));
        assert!(!lowered.contains("The installer"), "{lowered}");
        assert!(!lowered.contains("rm -rf"), "{lowered}");
        assert!(!lowered.contains("bullet"), "{lowered}");
    }

    #[test]
    fn keeps_comment_syntax_inside_fenced_code() {
        let lowered = lower_mdx(
            "```jsx\n{/* a real JSX comment */}\n```\n",
            &MdxConfig::proa(),
        )
        .unwrap();
        assert!(
            lowered.contains("{/* a real JSX comment */}"),
            "code samples must survive verbatim: {lowered}"
        );
    }

    #[test]
    fn comment_stripping_can_be_disabled() {
        let config = MdxConfig::proa().strip_comments(false);
        let lowered = lower_mdx("{/* kept */}\n", &config).unwrap();
        assert!(lowered.contains("kept"), "{lowered}");
    }

    #[test]
    fn proa_preset_lowers_cards_files_images_and_agent_terminal() {
        let raw = r#"---
title: Demo
---
import X from "y"

<Cards>
  <Card title="Runtime" description="Render a registry." href="/docs/runtime" />
</Cards>

<Files>
  <Folder name="src">
    <File name="main.rs" />
  </Folder>
</Files>

<img src="/images/demo.png" alt="Demo" />
<AgentTerminal />
"#;

        let lowered = lower_mdx(raw, &MdxConfig::proa()).unwrap();

        assert!(lowered.contains("- [Runtime](/docs/runtime): Render a registry."));
        assert!(lowered.contains("```proa-filetree\nsrc/\n  main.rs\n```"));
        assert!(lowered.contains("[Image: Demo](/static/images/demo.png)"));
        assert!(lowered.contains("```text\nAgent fetches page"));
        assert!(!lowered.contains("import X"));
    }

    #[test]
    fn normalizes_fence_metadata_and_inline_attrs() {
        let raw = "```rust filename=\"src/main file.rs\" title=\"ignored.rs\" {1,3-4}\nfn main() {}\n```\n<div className=\"grid\" style={{gridTemplateColumns: \"1fr 1fr\"}}>\n</div>\n";
        let lowered = lower_mdx(raw, &MdxConfig::proa()).unwrap();

        assert!(
            lowered.contains("```rust filename=\"src/main file.rs\" {1,3-4}\nfn main() {}\n```")
        );
        assert!(!lowered.contains("title=\"ignored.rs\""));
        assert!(!lowered.contains("className="));
        assert!(!lowered.contains("<div"));
    }

    #[test]
    fn disabled_config_returns_source() {
        let raw = "<Cards>\n</Cards>\n";
        assert_eq!(lower_mdx(raw, &MdxConfig::disabled()).unwrap(), raw);
    }
}
