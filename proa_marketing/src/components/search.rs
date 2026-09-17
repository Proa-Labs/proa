// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, RenderOutcome, StaticRaw, WebRender, WriteBuf};
use rsjs::signal;
use rsjs::web::dom;
use rsjs_macros::rsjs;

const SEARCH_INPUT_ICON_HTML: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="search-input-icon" aria-hidden="true"><path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/></svg>"#;

/// The search dialog chrome.
///
/// Ownership is split in two. This island owns the `<dialog>` itself: the
/// trigger buttons (`data-search-open`), Cmd+K, Escape, the backdrop, the
/// ESC button, and clearing plus focusing the input on open. Searching is
/// application-owned JavaScript in `public/search.mjs`, which runs Orama in
/// the browser over the documents the docs build emits, renders into
/// `#search-results`, and handles the arrow keys and Enter on the input.
/// Third-party libraries sit outside the analyzed RSJS surface, so that half
/// cannot live here; see the "Browser APIs" docs page.
///
/// The contract between the two: the island never writes inside
/// `#search-results` or `#search-status` after hydration and never binds the
/// input's value, and it dispatches a `proa:search-open` event on `document`
/// each time it opens (spelled out as a literal below because the RSJS
/// analyzer needs the event name as a string literal).
#[derive(Clone, Copy)]
pub struct SearchModal;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for SearchModal {
    fn render(
        self,
        cx: &mut proa_core::WebContext<L, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), proa_core::WriteError>>> {
        RenderOutcome::pending(async move {
            let dialog = rsjs::node_ref::<rsjs::HtmlDialogElement>();
            let input = rsjs::node_ref::<rsjs::HtmlInputElement>();
            let open = signal(false);
            let close_on_backdrop_click =
                rsjs::event_handler(|event: rsjs::MouseEvent<dom::HtmlDialogElement>| {
                    if event.target_id() == "search-modal" {
                        open.set(false);
                        dialog.close();
                    }
                });
            let open_on_trigger_click =
                rsjs::event_handler(|event: rsjs::MouseEvent<dom::Document>| {
                    if event.closest_attr("data-search-open") == "true" {
                        event.prevent_default();
                        open.set(true);
                        dialog.show_modal();
                        input.set_value("");
                        input.focus();
                        rsjs::document::dispatch_event("proa:search-open");
                    }
                });
            let handle_document_keydown =
                rsjs::event_handler(|event: rsjs::KeyboardEvent<dom::Document>| {
                    if event.is_cmd_or_ctrl("k") {
                        event.prevent_default();
                        open.set(true);
                        dialog.show_modal();
                        input.set_value("");
                        input.focus();
                        rsjs::document::dispatch_event("proa:search-open");
                    }
                    if event.key() == "Escape" {
                        open.set(false);
                        dialog.close();
                    }
                });
            let close_on_click =
                rsjs::event_handler(|event: rsjs::MouseEvent<dom::HtmlButtonElement>| {
                    event.prevent_default();
                    open.set(false);
                    dialog.close();
                });

            proa_macros::html! {
                <dialog
                    id="search-modal"
                    ref={dialog}
                    class="search-modal"
                    aria-label="Search Proa"
                    data-open={open.get()}
                    on_click={close_on_backdrop_click()}
                    on_document_click={open_on_trigger_click()}
                    on_document_keydown={handle_document_keydown()}
                >
                    <h2 class="sr-only">"Search"</h2>
                    <div class="search-input-row">
                        {StaticRaw(SEARCH_INPUT_ICON_HTML)}
                        <input
                            ref={input}
                            type="text"
                            id="search-input"
                            class="search-input"
                            placeholder="Search the docs"
                            autocomplete="off"
                            autocapitalize="off"
                            spellcheck="false"
                            role="combobox"
                            aria-label="Search"
                            aria-autocomplete="list"
                            aria-expanded="false"
                            aria-controls="search-results"
                        />
                        <button
                            type="button"
                            id="search-esc"
                            class="search-esc"
                            aria-label="Close search"
                            tabindex="-1"
                            on_click={close_on_click()}
                        >
                            "ESC"
                        </button>
                    </div>
                    <div id="search-results-wrap" class="search-results-wrap">
                        <div
                            id="search-results"
                            class="search-results"
                            role="listbox"
                            aria-label="Search results"
                        >
                            <div class="search-empty" data-search-hint>
                                "Type at least 2 characters"
                            </div>
                        </div>
                    </div>
                    <div class="search-footer">
                        <span id="search-status" class="search-status" aria-live="polite"></span>
                        <span class="search-keys">
                            <kbd>"↑"</kbd>
                            <kbd>"↓"</kbd>
                            " navigate "
                            <kbd>"↵"</kbd>
                            " open "
                            <kbd>"esc"</kbd>
                            " close"
                        </span>
                    </div>
                </dialog>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
