// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy)]
pub enum BenchmarkTone {
    Proa,
    React,
    Other,
}

impl BenchmarkTone {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proa => "proa",
            Self::React => "react",
            Self::Other => "other",
        }
    }

    pub const fn fill_style(self) -> &'static str {
        match self {
            Self::Proa => "height:100%;border-radius:9999px;background:var(--color-brand);",
            Self::React => "height:100%;border-radius:9999px;background:rgba(255,255,255,0.76);",
            Self::Other => "height:100%;border-radius:9999px;background:rgba(255,255,255,0.30);",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BenchmarkChartEntry {
    pub name: &'static str,
    pub value_label: &'static str,
    pub detail: &'static str,
    pub width_style: &'static str,
    pub tone: BenchmarkTone,
}

// Retained only as a source-level record of the retired size-only fixture.
// Current public comparison pages intentionally do not render these rows.
#[allow(dead_code)]
pub const FRAMEWORK_LATENCY_ENTRIES: &[BenchmarkChartEntry] = &[
    BenchmarkChartEntry {
        name: "Proa",
        value_label: "0.86 us",
        detail: "Rust",
        width_style: "width:16.6%;height:100%;",
        tone: BenchmarkTone::Proa,
    },
    BenchmarkChartEntry {
        name: "Askama",
        value_label: "1.54 us",
        detail: "Rust",
        width_style: "width:26.2%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Sailfish",
        value_label: "2.25 us",
        detail: "Rust",
        width_style: "width:32.5%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "jte",
        value_label: "8.62 us",
        detail: "Java/JVM",
        width_style: "width:54.7%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Rocker",
        value_label: "8.66 us",
        detail: "Java/JVM",
        width_style: "width:54.8%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Topcoat",
        value_label: "12.52 us",
        detail: "Rust",
        width_style: "width:60.9%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Leptos",
        value_label: "20.39 us",
        detail: "Rust",
        width_style: "width:69.0%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Handlebars",
        value_label: "22.71 us",
        detail: "JS",
        width_style: "width:70.8%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "ERB (Erubi)",
        value_label: "40.22 us",
        detail: "Ruby",
        width_style: "width:80.2%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "React (Bun)",
        value_label: "60.45 us",
        detail: "JS",
        width_style: "width:87.0%;height:100%;",
        tone: BenchmarkTone::React,
    },
    BenchmarkChartEntry {
        name: "React (Node)",
        value_label: "84.51 us",
        detail: "JS",
        width_style: "width:92.5%;height:100%;",
        tone: BenchmarkTone::React,
    },
    BenchmarkChartEntry {
        name: "Go html/tpl",
        value_label: "95.27 us",
        detail: "Go",
        width_style: "width:94.5%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Dioxus",
        value_label: "95.37 us",
        detail: "Rust",
        width_style: "width:94.5%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Jinja2",
        value_label: "114.61 us",
        detail: "Python",
        width_style: "width:97.6%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Yew",
        value_label: "116.21 us",
        detail: "Rust",
        width_style: "width:97.8%;height:100%;",
        tone: BenchmarkTone::Other,
    },
];

#[allow(dead_code)]
pub const FRAMEWORK_THROUGHPUT_ENTRIES: &[BenchmarkChartEntry] = &[
    BenchmarkChartEntry {
        name: "Proa",
        value_label: "1,168,989",
        detail: "req/s per core",
        width_style: "width:100.0%;height:100%;",
        tone: BenchmarkTone::Proa,
    },
    BenchmarkChartEntry {
        name: "Askama",
        value_label: "651,348",
        detail: "req/s per core",
        width_style: "width:88.4%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Sailfish",
        value_label: "444,953",
        detail: "req/s per core",
        width_style: "width:80.8%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "jte",
        value_label: "116,009",
        detail: "req/s per core",
        width_style: "width:54.2%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Rocker",
        value_label: "115,473",
        detail: "req/s per core",
        width_style: "width:54.1%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Topcoat",
        value_label: "79,872",
        detail: "req/s per core",
        width_style: "width:46.8%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Leptos",
        value_label: "49,039",
        detail: "req/s per core",
        width_style: "width:37.1%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Handlebars",
        value_label: "44,033",
        detail: "req/s per core",
        width_style: "width:35.0%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "ERB (Erubi)",
        value_label: "24,863",
        detail: "req/s per core",
        width_style: "width:23.6%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "React (Bun)",
        value_label: "16,543",
        detail: "req/s per core",
        width_style: "width:15.6%;height:100%;",
        tone: BenchmarkTone::React,
    },
    BenchmarkChartEntry {
        name: "React (Node)",
        value_label: "11,833",
        detail: "req/s per core",
        width_style: "width:8.9%;height:100%;",
        tone: BenchmarkTone::React,
    },
    BenchmarkChartEntry {
        name: "Go html/tpl",
        value_label: "10,496",
        detail: "req/s per core",
        width_style: "width:6.5%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Dioxus",
        value_label: "10,486",
        detail: "req/s per core",
        width_style: "width:6.5%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Jinja2",
        value_label: "8,725",
        detail: "req/s per core",
        width_style: "width:2.9%;height:100%;",
        tone: BenchmarkTone::Other,
    },
    BenchmarkChartEntry {
        name: "Yew",
        value_label: "8,605",
        detail: "req/s per core",
        width_style: "width:2.6%;height:100%;",
        tone: BenchmarkTone::Other,
    },
];

#[allow(dead_code)]
pub const AIRBNB_PAGE_LATENCY_ENTRIES: &[BenchmarkChartEntry] = &[
    BenchmarkChartEntry {
        name: "Proa (Home)",
        value_label: "4.62 us",
        detail: "158 KB",
        width_style: "width:27.8%;height:100%;",
        tone: BenchmarkTone::Proa,
    },
    BenchmarkChartEntry {
        name: "Proa (Listing)",
        value_label: "3.08 us",
        detail: "94 KB",
        width_style: "width:21.3%;height:100%;",
        tone: BenchmarkTone::Proa,
    },
    BenchmarkChartEntry {
        name: "Proa (Search)",
        value_label: "1.12 us",
        detail: "50 KB",
        width_style: "width:5.0%;height:100%;",
        tone: BenchmarkTone::Proa,
    },
    BenchmarkChartEntry {
        name: "React (Home)",
        value_label: "186.92 us",
        detail: "158 KB",
        width_style: "width:87.4%;height:100%;",
        tone: BenchmarkTone::React,
    },
    BenchmarkChartEntry {
        name: "React (Listing)",
        value_label: "248.67 us",
        detail: "94 KB",
        width_style: "width:92.0%;height:100%;",
        tone: BenchmarkTone::React,
    },
    BenchmarkChartEntry {
        name: "React (Search)",
        value_label: "45.62 us",
        detail: "50 KB",
        width_style: "width:64.7%;height:100%;",
        tone: BenchmarkTone::React,
    },
];

#[allow(dead_code)]
pub const AIRBNB_HOME_TAIL_ENTRIES: &[BenchmarkChartEntry] = &[
    BenchmarkChartEntry {
        name: "Proa P50",
        value_label: "4.62 us",
        detail: "Home page",
        width_style: "width:17.3%;height:100%;",
        tone: BenchmarkTone::Proa,
    },
    BenchmarkChartEntry {
        name: "Proa P99",
        value_label: "6.50 us",
        detail: "Home page",
        width_style: "width:23.2%;height:100%;",
        tone: BenchmarkTone::Proa,
    },
    BenchmarkChartEntry {
        name: "React P50",
        value_label: "186.92 us",
        detail: "Home page",
        width_style: "width:81.8%;height:100%;",
        tone: BenchmarkTone::React,
    },
    BenchmarkChartEntry {
        name: "React P99",
        value_label: "399.08 us",
        detail: "Home page",
        width_style: "width:95.0%;height:100%;",
        tone: BenchmarkTone::React,
    },
];
