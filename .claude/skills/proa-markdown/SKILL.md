---
name: proa-markdown
description: Author typed Proa Markdown renderers with md_sync! or md!, including nested templates, Rust control flow, shared HTML/Markdown content, and Markdown exports.
---

# Proa Markdown

Read and follow the [shared Markdown authoring skill](../../../.agents/skills/proa-markdown/SKILL.md)
and its linked document-rendering guide. They are the source of truth for both
Claude Code and Codex; keep the complete examples and verification workflow there.

Write normal Markdown in raw templates, with headings, lists, and blank lines
visible in the source. Text outside `{ ... }` is Markdown; slots are Rust at
every depth. Use nested raw templates with fewer hashes to re-enter Markdown
inside Rust `if`, `for`, or `match` bodies. Bare control words in prose stay text.
