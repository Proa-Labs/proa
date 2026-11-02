---
name: proa-markdown
description: Author typed Proa Markdown renderers with md_sync! or md!, including nested templates, Rust control flow, shared HTML/Markdown content, and Markdown exports.
---

# Proa Markdown

Read and follow the [shared Markdown authoring skill](../../../.agents/skills/proa-markdown/SKILL.md)
and its linked document-rendering guide. They are the source of truth for both
Claude Code and Codex; keep the complete examples and verification workflow there.

Use multiline raw templates for documents and substantial sections, with
headings, lists, and blank lines visible in the source. Keep short fragments on
one line: `md_sync! { "- {item}\n" }` or `md_sync! { "**In stock**" }`.
Preserve a row's trailing `\n`; raw delimiters are unnecessary for simple inline
content. Text outside `{ ... }` is Markdown; slots are Rust at every depth.
Re-enter Markdown through a nested macro inside Rust `if`, `for`, or `match`
bodies. Use fewer hashes when a nested fragment really needs a raw literal.
Bare control words in prose stay text.
