// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

mod code_fence;
mod code_icon;
mod compiler;
mod components;
mod config;
mod frontmatter;
mod link_validation;
mod markdown;
mod mdx;
mod rust_emit;
mod search;
mod syntax_highlight;
mod typescript;

pub use compiler::{compile, compile_docs, try_compile, try_compile_to, CompileError};
pub use config::{
    plugins, CodeHighlight, CollectionKind, CollectionSource, ComponentIndexing, Config,
    DocsSource, ExternalLinkCheck, LinkValidation, LinkValidationMode, MarkdownPipeline, MathMode,
    MdxConfig, MdxImageConfig, MermaidConfig, MermaidFallback, MermaidRenderer, MetaSchema,
    PageSchema, Postprocess, RelativePathMode, SearchConfig,
};
pub use link_validation::{
    validate_links, LinkDiagnostic, LinkGraph, LinkGraphPage, LinkValidationReport,
};
pub use typescript::{Annotation, TypeScriptDocgen};
