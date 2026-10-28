// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{content_part, html_sync, md_sync};

pub(crate) struct Faq {
    pub question: &'static str,
    pub answer: &'static str,
}

pub(crate) const FAQS: &[Faq] = &[
    Faq {
        question: "What is Proa?",
        answer: "Proa is a Rust web framework for composable, multi-target rendering. It compiles HTML templates during the application build and supports streaming server rendering, typed Markdown, and browser islands.",
    },
    Faq {
        question: "How does Proa add browser interactivity?",
        answer: "RSJS lets you author signals, typed event handlers, and interactive islands in Rust. Proa compiles the client behavior into JavaScript while keeping the rest of the page server rendered.",
    },
    Faq {
        question: "Can Proa run alongside an existing application?",
        answer: "Yes. Route part of an existing application to Proa and adopt it incrementally. The framework also supports React interop when an existing client component needs to remain in use.",
    },
    Faq {
        question: "Can coding agents read Proa documentation?",
        answer: "Yes. Proa provides Markdown pages, a public JSON documentation API, authoring skills, and a Streamable HTTP MCP server. Documentation reads require no account or API key.",
    },
];

const TITLE: &str = "Frequently asked questions";

pub(crate) struct FaqSection<'a> {
    pub entries: &'a [Faq],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FaqSection<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section id="faq" data-faq class={super::SECTION_SHELL} aria-labelledby="faq-title">
                <div class={super::SECTION_CONTAINER}>
                    <h2 id="faq-title" class="text-3xl font-semibold tracking-tight">
                        {content_part!("title", TITLE)}
                    </h2>
                    <dl class="mt-8 grid gap-8 md:grid-cols-2">
                        {for entry in self.entries {
                            html_sync! {
                                <div>
                                    <dt class="text-lg font-medium">{entry.question}</dt>
                                    <dd class="mt-2 text-sm leading-7 text-zinc-600 dark:text-zinc-400">{entry.answer}</dd>
                                </div>
                            }
                        }}
                    </dl>
                </div>
            </section>
        }.render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for FaqSection<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            ## {content_part!("title", TITLE)}

            {for entry in self.entries {
            ### {entry.question}

            {entry.answer}

            }}"###
        }
        .render_md(cx)
    }
}
