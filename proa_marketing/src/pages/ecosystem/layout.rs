// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    DataLoader, RenderOutcome, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html, html_sync};

use crate::{
    components::{
        landing_shell::{LandingHeader, MarketingNavActive},
        search::SearchModal,
    },
    layout::{DocumentMetadata, PageMeta, RsjsRuntimeTags, STYLESHEET_HREF, THEME_BOOT},
};

pub(crate) struct EcosystemStandaloneDocument<C> {
    pub(crate) meta: PageMeta,
    pub(crate) content: C,
}

pub(crate) struct EcosystemMockFooter;

struct EcosystemMockLogo {
    class: &'static str,
}

const ECOSYSTEM_SCRIPT: &str = r##"
(function () {
  var bar = document.getElementById("filter-bar");
  var grid = document.getElementById("showcase-grid");
  var search = document.getElementById("ecosystem-app-search") || document.getElementById("ecosystem-tool-search") || document.getElementById("search");
  var count = document.getElementById("count");
  var empty = document.getElementById("empty-state");
  var clear = document.getElementById("clear-search");
  if (!bar || !grid || !search || !count || !empty) return;
  var cards = Array.prototype.slice.call(grid.querySelectorAll(".ecosystem-app-card, .ecosystem-tool-card, .card"));
  var total = cards.length;
  var activeFilter = "all";
  var refresh = function () {
    var q = (search.value || "").trim().toLowerCase();
    var shown = 0;
    cards.forEach(function (card) {
      var matchIndustry = activeFilter === "all" || card.dataset.industry === activeFilter;
      var matchSearch = !q || card.textContent.toLowerCase().indexOf(q) !== -1;
      var show = matchIndustry && matchSearch;
      card.style.display = show ? "" : "none";
      if (show) shown += 1;
    });
    count.textContent = "Showing " + shown + " of " + total;
    empty.classList.toggle("hidden", shown !== 0);
  };
  bar.querySelectorAll("button").forEach(function (button) {
    button.addEventListener("click", function () {
      activeFilter = button.dataset.filter || "all";
      bar.querySelectorAll("button").forEach(function (candidate) {
        candidate.dataset.active = String(candidate === button);
      });
      refresh();
    });
  });
  search.addEventListener("input", refresh);
  if (clear) {
    clear.addEventListener("click", function () {
      activeFilter = "all";
      bar.querySelectorAll("button").forEach(function (button) {
        button.dataset.active = String(button.dataset.filter === "all");
      });
      search.value = "";
      refresh();
    });
  }
  refresh();
})();

(function () {
  var bar = document.getElementById("registry-filter-bar");
  var list = document.getElementById("registry-list");
  var search = document.getElementById("registry-search");
  var count = document.getElementById("registry-count");
  var empty = document.getElementById("registry-empty-state");
  var clear = document.getElementById("registry-clear-search");
  if (!bar || !list || !search || !count || !empty) return;
  var items = Array.prototype.slice.call(list.querySelectorAll(".registry-request"));
  var total = items.length;
  var activeFilter = "all";
  var refresh = function () {
    var q = (search.value || "").trim().toLowerCase();
    var shown = 0;
    items.forEach(function (item) {
      var matchCat = activeFilter === "all" || item.dataset.cat === activeFilter;
      var matchSearch = !q || item.textContent.toLowerCase().indexOf(q) !== -1;
      var show = matchCat && matchSearch;
      item.style.display = show ? "" : "none";
      if (show) shown += 1;
    });
    count.textContent = "Showing " + shown + " of " + total;
    empty.classList.toggle("hidden", shown !== 0);
  };
  bar.querySelectorAll("button").forEach(function (button) {
    button.addEventListener("click", function () {
      activeFilter = button.dataset.filter || "all";
      bar.querySelectorAll("button").forEach(function (candidate) {
        candidate.dataset.active = String(candidate === button);
      });
      refresh();
    });
  });
  search.addEventListener("input", refresh);
  if (clear) {
    clear.addEventListener("click", function () {
      activeFilter = "all";
      bar.querySelectorAll("button").forEach(function (button) {
        button.dataset.active = String(button.dataset.filter === "all");
      });
      search.value = "";
      refresh();
    });
  }
  list.querySelectorAll(".registry-vote").forEach(function (button) {
    var number = button.querySelector(".registry-vote-count");
    if (!number) return;
    var base = parseInt(number.textContent || "0", 10);
    var voted = false;
    button.addEventListener("click", function (event) {
      event.preventDefault();
      voted = !voted;
      number.textContent = String(base + (voted ? 1 : 0));
      button.style.borderColor = voted ? "#dd7627" : "";
      button.style.backgroundColor = voted ? "#dd76270a" : "";
      number.style.color = voted ? "#dd7627" : "";
    });
  });
  refresh();
})();
"##;

impl<B, L, C> proa_core::WebRender<L, B> for EcosystemStandaloneDocument<C>
where
    B: WriteBuf,
    L: DataLoader,
    C: WebRenderSync<L, B> + Send + 'static,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            html! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        <meta charset="utf-8" />
                        <meta name="viewport" content="width=device-width, initial-scale=1" />
                        {proa_core::to_async(DocumentMetadata::new(self.meta))}
                        <link rel="icon" href="/static/favicon.ico" />
                        <link rel="icon" type="image/svg+xml" href="/static/favicon.svg" />
                        <link rel="stylesheet" href={STYLESHEET_HREF} />
                        <script>{StaticRaw(THEME_BOOT)}</script>
                    </head>
                    <body class="min-h-screen bg-[#f7f4ee] text-[#18181b] font-sans antialiased dark:bg-[#0b0b0d] dark:text-zinc-100">
                        {proa_core::to_async(LandingHeader {
                            active: MarketingNavActive::for_path(self.meta.path),
                        })}
                        {proa_core::to_async(self.content)}
                        {SearchModal}
                        <script>{StaticRaw(ECOSYSTEM_SCRIPT)}</script>
                        {proa_core::to_async(RsjsRuntimeTags)}
                    </body>
                </html>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for EcosystemMockLogo {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                class={self.class}
                viewBox="0 0 64 64"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
                aria-label="Proa"
            >
                <path d="M13 36.5C17.6 25.7 25.5 18 36.2 13L34.8 33.5L13 36.5Z" fill="#dd7627" />
                <path
                    d="M34.6 14.5L30.9 44.6"
                    stroke="#18181b"
                    stroke-width="2.8"
                    stroke-linecap="round"
                />
                <path
                    d="M10 39.5C16.2 39.8 23.4 39.2 31.3 37.9C41.2 36.3 49 33.7 54 30.2C50.9 39.8 42.3 46.5 30 47.8C18.7 49 11.5 46 10 39.5Z"
                    fill="#18181b"
                />
                <path
                    d="M23.5 50.6L29.4 43.8"
                    stroke="#18181b"
                    stroke-width="2.4"
                    stroke-linecap="round"
                />
                <path
                    d="M40.9 49.7L35.3 42.4"
                    stroke="#18181b"
                    stroke-width="2.4"
                    stroke-linecap="round"
                />
                <path
                    d="M19.8 52.2C23.3 51.7 27.2 51.2 31.4 50.7C36.1 50.1 39.8 49.4 42.8 48.5C40.7 54 35.5 56.5 27.4 56C24 55.8 21.4 54.5 19.8 52.2Z"
                    fill="#18181b"
                    opacity="0.6"
                />
            </svg>
        }
        .render(cx)
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for EcosystemMockFooter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <footer class="border-t border-[#00000014] pb-14 pt-10">
                <div class="mx-auto flex max-w-[1400px] flex-col items-center justify-between gap-6 px-4 sm:flex-row sm:px-6 lg:px-8">
                    <span class="flex items-center text-[16px] font-semibold">
                        {EcosystemMockLogo { class: "mr-[7px] h-[22px] w-[22px]" }}
                        "Proa"
                    </span>
                    <div class="flex flex-wrap justify-center gap-6">
                        <a class="text-[13.5px] text-[#7b746b]" href="/docs">"Docs"</a>
                        <a class="text-[13.5px] text-[#7b746b]" href="/ecosystem">"Ecosystem"</a>
                        <a class="text-[13.5px] text-[#7b746b]" href="/#bench">"Benchmarks"</a>
                        <a
                            class="text-[13.5px] text-[#7b746b]"
                            href="https://github.com/proa-labs/proa"
                            rel="noreferrer noopener"
                            target="_blank"
                        >
                            "GitHub"
                        </a>
                    </div>
                    <span class="font-mono text-[11px] text-[#a59f97]">
                        "© Proa Labs, Inc. · Rust · Islands"
                    </span>
                </div>
            </footer>
        }
        .render(cx)
    }
}
