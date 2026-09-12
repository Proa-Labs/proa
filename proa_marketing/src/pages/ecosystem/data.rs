// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Copy)]
pub(crate) struct EcosystemFilter<'a> {
    pub(crate) key: &'a str,
    pub(crate) label: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) enum AppStatus {
    Live,
}

#[derive(Clone, Copy)]
pub(crate) enum AppVisual<'a> {
    Screenshot { src: &'a str, alt: &'a str },
}

#[derive(Clone, Copy)]
pub(crate) struct EcosystemApp<'a> {
    pub(crate) industry: &'a str,
    pub(crate) href: &'a str,
    pub(crate) external: bool,
    pub(crate) name: &'a str,
    pub(crate) status: AppStatus,
    pub(crate) product_kind: &'a str,
    pub(crate) visual: AppVisual<'a>,
}

#[derive(Clone, Copy)]
pub(crate) enum ToolVersionTone {
    Default,
    Beta,
}

#[derive(Clone, Copy)]
pub(crate) enum ToolVisual<'a> {
    Island { label: &'a str },
    Components,
    Cli { command: &'a str, status: &'a str },
    Crate { initial: &'a str, command: &'a str },
    DataBinding { label: &'a str },
    Markdown,
    Og,
}

#[derive(Clone, Copy)]
pub(crate) struct EcosystemTool<'a> {
    pub(crate) category: &'a str,
    pub(crate) href: &'a str,
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
    pub(crate) version_tone: ToolVersionTone,
    pub(crate) description: &'a str,
    pub(crate) kind: &'a str,
    pub(crate) detail: &'a str,
    pub(crate) cta_label: &'a str,
    pub(crate) visual: ToolVisual<'a>,
}

#[derive(Clone, Copy)]
pub(crate) struct RegistryRequest<'a> {
    pub(crate) category: &'a str,
    pub(crate) votes: u16,
    pub(crate) name: &'a str,
    pub(crate) tag: &'a str,
    pub(crate) difficulty: Option<&'a str>,
    pub(crate) description: &'a str,
    pub(crate) builder: Option<&'a str>,
    pub(crate) description_suffix: &'a str,
    pub(crate) claimed: bool,
}

pub(crate) struct EcosystemData<'a> {
    pub(crate) app_filters: &'a [EcosystemFilter<'a>],
    pub(crate) apps: &'a [EcosystemApp<'a>],
    pub(crate) tool_filters: &'a [EcosystemFilter<'a>],
    pub(crate) tools: &'a [EcosystemTool<'a>],
    pub(crate) registry_filters: &'a [EcosystemFilter<'a>],
    pub(crate) registry_requests: &'a [RegistryRequest<'a>],
}

pub(crate) fn ecosystem_data() -> &'static EcosystemData<'static> {
    &STATIC_ECOSYSTEM_DATA
}

// Static seed data for now. The page modules depend on `ecosystem_data()`,
// so this can become a database/API-backed source without changing layout code.
static STATIC_ECOSYSTEM_DATA: EcosystemData<'static> = EcosystemData {
    app_filters: APP_FILTERS,
    apps: ECOSYSTEM_APPS,
    tool_filters: TOOL_FILTERS,
    tools: ECOSYSTEM_TOOLS,
    registry_filters: REGISTRY_FILTERS,
    registry_requests: REGISTRY_REQUESTS,
};

const APP_FILTERS: &[EcosystemFilter<'static>] = &[
    EcosystemFilter {
        key: "all",
        label: "All",
    },
    EcosystemFilter {
        key: "production",
        label: "Production",
    },
    EcosystemFilter {
        key: "playground",
        label: "Playground",
    },
    EcosystemFilter {
        key: "reference",
        label: "Reference",
    },
];

const ECOSYSTEM_APPS: &[EcosystemApp<'static>] = &[
    EcosystemApp {
        industry: "production",
        href: "https://latency.so",
        external: true,
        name: "latency.so",
        status: AppStatus::Live,
        product_kind: "Production site built with Proa",
        visual: AppVisual::Screenshot {
            src: "/static/images/synthetic/ecosystem-latency.svg",
            alt: "Synthetic latency dashboard illustration",
        },
    },
    EcosystemApp {
        industry: "playground",
        href: "/playground",
        external: false,
        name: "Playground Console",
        status: AppStatus::Live,
        product_kind: "Live Proa playground and benchmark console",
        visual: AppVisual::Screenshot {
            src: "/static/images/synthetic/ecosystem-console.svg",
            alt: "Synthetic Proa playground console illustration",
        },
    },
    EcosystemApp {
        industry: "reference",
        href: "/showcase/github-pr-diff",
        external: false,
        name: "GitHub PR diff recreation",
        status: AppStatus::Live,
        product_kind: "Reference recreation - not affiliated with GitHub",
        visual: AppVisual::Screenshot {
            src: "/static/images/synthetic/ecosystem-diff.svg",
            alt: "Synthetic code review diff illustration",
        },
    },
    EcosystemApp {
        industry: "reference",
        href: "/showcase/airbnb/proa",
        external: false,
        name: "Airbnb showcase recreation",
        status: AppStatus::Live,
        product_kind: "Reference recreation - not affiliated with Airbnb",
        visual: AppVisual::Screenshot {
            src: "/static/images/synthetic/ecosystem-travel.svg",
            alt: "Synthetic travel marketplace illustration",
        },
    },
    EcosystemApp {
        industry: "reference",
        href: "/showcase/nike/proa",
        external: false,
        name: "Nike showcase recreation",
        status: AppStatus::Live,
        product_kind: "Reference recreation - not affiliated with Nike",
        visual: AppVisual::Screenshot {
            src: "/static/images/synthetic/ecosystem-catalog.svg",
            alt: "Synthetic product catalog illustration",
        },
    },
    EcosystemApp {
        industry: "reference",
        href: "/showcase/espn/proa",
        external: false,
        name: "ESPN box score recreation",
        status: AppStatus::Live,
        product_kind: "Reference recreation - not affiliated with ESPN",
        visual: AppVisual::Screenshot {
            src: "/static/images/synthetic/ecosystem-sports.svg",
            alt: "Synthetic sports score page illustration",
        },
    },
    EcosystemApp {
        industry: "reference",
        href: "/showcase/sentry-otel/proa",
        external: false,
        name: "Sentry OpenTelemetry recreation",
        status: AppStatus::Live,
        product_kind: "Reference recreation - not affiliated with Sentry",
        visual: AppVisual::Screenshot {
            src: "/static/images/synthetic/ecosystem-observability.svg",
            alt: "Synthetic observability trace illustration",
        },
    },
];

const TOOL_FILTERS: &[EcosystemFilter<'static>] = &[
    EcosystemFilter {
        key: "all",
        label: "All",
    },
    EcosystemFilter {
        key: "core",
        label: "Core",
    },
    EcosystemFilter {
        key: "framework",
        label: "Framework",
    },
    EcosystemFilter {
        key: "ui",
        label: "UI",
    },
    EcosystemFilter {
        key: "islands",
        label: "Islands",
    },
    EcosystemFilter {
        key: "tooling",
        label: "Tooling",
    },
    EcosystemFilter {
        key: "interop",
        label: "Interop",
    },
];

const ECOSYSTEM_TOOLS: &[EcosystemTool<'static>] = &[
    EcosystemTool {
        category: "core",
        href: "/docs/core",
        name: "proa_core",
        version: "v0.3.0",
        version_tone: ToolVersionTone::Default,
        description: "Typed HTML and Markdown rendering with composable components, template macros, escaping, and class lists.",
        kind: "crate",
        detail: "SSR runtime primitives",
        cta_label: "Docs",
        visual: ToolVisual::Crate {
            initial: "P",
            command: "WebRenderSync -> WriteBuf",
        },
    },
    EcosystemTool {
        category: "ui",
        href: "https://ui.proa.so",
        name: "Proa UI",
        version: "v0.2.2",
        version_tone: ToolVersionTone::Default,
        description: "Source-owned Rust UI components for Proa apps, split between free primitives and paid Pro product surfaces.",
        kind: "crate",
        detail: "ui.proa.so",
        cta_label: "Proa UI",
        visual: ToolVisual::Components,
    },
    EcosystemTool {
        category: "islands",
        href: "/docs/framework",
        name: "rsjs",
        version: "v0.2.0",
        version_tone: ToolVersionTone::Default,
        description: "Rust-authored islands compiled into tiny vanilla JavaScript with typed signals, handlers, queries, and props.",
        kind: "crate",
        detail: "client islands",
        cta_label: "Docs",
        visual: ToolVisual::Island { label: "RSJS" },
    },
    EcosystemTool {
        category: "framework",
        href: "/docs/framework",
        name: "proa_framework_axum",
        version: "v0.3.0",
        version_tone: ToolVersionTone::Default,
        description: "Axum integration with routing, streaming SSR, request data, static assets, typed actions, security headers, and response finalization.",
        kind: "crate",
        detail: "Axum adapter",
        cta_label: "Docs",
        visual: ToolVisual::DataBinding {
            label: "route -> response",
        },
    },
    EcosystemTool {
        category: "tooling",
        href: "/docs/cli",
        name: "proa CLI",
        version: "v0.1.0",
        version_tone: ToolVersionTone::Default,
        description: "Project scaffolding, component/layout/data generators, html macro formatting, template linting, and site checks.",
        kind: "binary",
        detail: "proa and cargo-proa",
        cta_label: "Docs",
        visual: ToolVisual::Cli {
            command: "proa new site my-site --docs proa_docs",
            status: "site scaffold ready",
        },
    },
    EcosystemTool {
        category: "tooling",
        href: "/docs/proa-docs",
        name: "proa_docs + proa_docs_build",
        version: "v0.2.0",
        version_tone: ToolVersionTone::Default,
        description: "MDX content collections, meta.json navigation, generated docs registries, search indexes, TOC, and pager data.",
        kind: "crates",
        detail: "docs compiler",
        cta_label: "Docs",
        visual: ToolVisual::Markdown,
    },
    EcosystemTool {
        category: "tooling",
        href: "/docs/framework",
        name: "proa_codegen",
        version: "v0.2.0",
        version_tone: ToolVersionTone::Default,
        description: "Product-neutral TypeScript generation from an explicit component metadata manifest.",
        kind: "crate + binary",
        detail: "metadata to TypeScript",
        cta_label: "Docs",
        visual: ToolVisual::Crate {
            initial: "TS",
            command: "metadata -> TypeScript",
        },
    },
    EcosystemTool {
        category: "interop",
        href: "/docs/framework",
        name: "proa_react",
        version: "v0.3.0",
        version_tone: ToolVersionTone::Default,
        description: "React/JSX interop helpers for island bootstrapping, serialized props, and compatibility surfaces.",
        kind: "crate",
        detail: "React bridge",
        cta_label: "Docs",
        visual: ToolVisual::Crate {
            initial: "R",
            command: "React island boundary",
        },
    },
    EcosystemTool {
        category: "tooling",
        href: "https://github.com/hariria/wellformed",
        name: "wellformed",
        version: "v0.1.x",
        version_tone: ToolVersionTone::Beta,
        description: "Portable validation schemas authored once and evaluated with the same rules in TypeScript and Rust.",
        kind: "crate + npm",
        detail: "portable validation IR",
        cta_label: "GitHub",
        visual: ToolVisual::Crate {
            initial: "W",
            command: "JSON IR -> TS + Rust",
        },
    },
    EcosystemTool {
        category: "interop",
        href: "/docs",
        name: "proa_og",
        version: "v0.1.2",
        version_tone: ToolVersionTone::Default,
        description: "Open Graph image generation from Proa-rendered HTML using a real browser engine and PNG output.",
        kind: "crate",
        detail: "HTML -> image",
        cta_label: "Docs",
        visual: ToolVisual::Og,
    },
];

const REGISTRY_FILTERS: &[EcosystemFilter<'static>] = &[
    EcosystemFilter {
        key: "all",
        label: "All",
    },
    EcosystemFilter {
        key: "components",
        label: "Components",
    },
    EcosystemFilter {
        key: "adapters",
        label: "Adapters",
    },
    EcosystemFilter {
        key: "data",
        label: "Data",
    },
    EcosystemFilter {
        key: "tooling",
        label: "Tooling",
    },
    EcosystemFilter {
        key: "claimed",
        label: "Claimed",
    },
];

const REGISTRY_REQUESTS: &[RegistryRequest<'static>] = &[];
