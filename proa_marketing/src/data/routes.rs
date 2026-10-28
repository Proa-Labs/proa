// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Public paths shared by routing, navigation, discovery, and OpenAPI.

use const_format::concatcp;

pub const HOME: &str = "/";
pub const DOCS: &str = "/docs";
pub const DOCS_PREFIX: &str = concatcp!(DOCS, "/");
pub const DOCS_PATTERN: &str = concatcp!(DOCS, "/*path");
pub const API_GUIDE: &str = concatcp!(DOCS, "/api");
pub const API_GUIDE_MARKDOWN: &str = concatcp!(API_GUIDE, ".md");
pub const DOCS_INDEX: &str = concatcp!(DOCS, "/llms.txt");
pub const FRAMEWORK_DOCS: &str = concatcp!(DOCS, "/framework");
pub const ABOUT: &str = "/about";
pub const ABOUT_MARKDOWN: &str = concatcp!(ABOUT, ".md");
pub const CONTACT: &str = "/contact";
pub const CONTACT_MARKDOWN: &str = concatcp!(CONTACT, ".md");
pub const PRIVACY: &str = "/privacy";
pub const TERMS: &str = "/terms";
pub const ECOSYSTEM: &str = "/ecosystem";
pub const ECOSYSTEM_PREFIX: &str = concatcp!(ECOSYSTEM, "/");
pub const APPS: &str = concatcp!(ECOSYSTEM, "/apps");
pub const SHOWCASES: &str = concatcp!(APPS, "#reference-showcases");
pub const DEVELOPER_TOOLS: &str = concatcp!(ECOSYSTEM, "/developer-tools");
pub const REGISTRY: &str = concatcp!(ECOSYSTEM, "/registry");
pub const SITEMAP: &str = "/sitemap.xml";

pub const API: &str = "/api";
pub const API_PREFIX: &str = concatcp!(API, "/");
pub const API_V1: &str = concatcp!(API, "/v1");
pub const API_SEARCH: &str = concatcp!(API_V1, "/search");
pub const API_DOCS: &str = concatcp!(API_V1, "/docs");
pub const API_PAGE: &str = concatcp!(API_DOCS, "/page");
pub const API_PATTERN: &str = concatcp!(API_V1, "/*path");
pub const LEGACY_SEARCH: &str = concatcp!(API, "/search");
pub const MCP: &str = "/mcp";
pub const MCP_CARD: &str = concatcp!(MCP, "/server-card");
pub const OPENAPI: &str = "/openapi.json";
pub const HOME_MARKDOWN: &str = "/index.md";
pub const AGENT_INDEX: &str = "/llms.txt";
pub const WELL_KNOWN_AGENT_INDEX: &str = "/.well-known/llms.txt";
pub const DOCS_CORPUS: &str = "/llms-full.txt";
pub const AUTH: &str = "/auth.md";
pub const SITE_METADATA: &str = "/.well-known/proa-site.json";
pub const ARD_CATALOG: &str = "/.well-known/ard.json";
pub const AI_CATALOG: &str = "/.well-known/ai-catalog.json";
pub const API_CATALOG: &str = "/.well-known/api-catalog";
pub const SKILLS_ROOT: &str = "/.well-known/agent-skills";
pub const SKILLS_INDEX: &str = concatcp!(SKILLS_ROOT, "/index.json");
pub const SKILL_PATTERN: &str = concatcp!(SKILLS_ROOT, "/:name/SKILL.md");

pub const STATIC_ROUTES: &[&str] = &[
    HOME,
    "/playground",
    "/playground/airbnb/listing",
    "/playground/github-pr/github-pr-diff",
    ECOSYSTEM,
    APPS,
    DEVELOPER_TOOLS,
    REGISTRY,
    PRIVACY,
    TERMS,
    ABOUT,
    CONTACT,
];
