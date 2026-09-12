// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! URL-driven CSS filter runtime for the eBay SRP demo.
//!
//! Architecture summary (full rationale lives in the
//! `feat/filter-chips-functional` commit body):
//!
//! 1. Each listing card carries `data-format` and `data-free-shipping`
//!    attrs baked at SSR time by `ebay_srp::Card::render`.
//! 2. The `FilterChips` rsjs island toggles its three local boolean
//!    signals on click; the runtime infers the new state from the
//!    rendered DOM (which branch of each chip's `if` has its inline
//!    `display:none` cleared) and writes a compact `?format=...&shipping=...`
//!    string via `history.replaceState`.
//! 3. The initial `DOMContentLoaded` (plus `popstate` on back/forward)
//!    recomputes a short list of CSS attribute-selector rules and
//!    pushes them into a single `<style id="srp-filter">` element.
//!    Filtering is then pure CSS: sub-millisecond, no per-listing DOM
//!    traversal, scales cleanly to thousands of cards.
//! 4. The query-string format (instead of `#hash`) lets the server
//!    read filter state from the request URL on first paint and seed
//!    the `FilterChips` island with `initial_buy_it_now` /
//!    `initial_auction` / `initial_free_shipping` values matching the
//!    URL. That closes the round-trip URL → chip-visual sync gap that
//!    the original hash-only design left open.
//!
//! Why this remains a separate coordinator: the fixture predates the
//! built-in `rsjs::url` and `rsjs::history` effects and intentionally
//! exercises interoperability with application-owned JavaScript. New
//! islands can call those built-ins from a local `rsjs::event_handler`
//! or put pure portable computation in a helper annotated once with
//! `#[rsjs::client]`; `#[rsjs::client_module]` is optional grouping. There is no separate
//! handler-mode client attribute; event handlers are declared as locals.
//! This coordinator instead observes the DOM state the chips already render
//! and projects it to the URL.
//!
//! ## URL query format
//!
//! `?format=buyitnow,auction&shipping=free`
//!
//! - `format=` carries a comma-separated list of active format slugs
//!   (`buyitnow`, `auction`). When neither chip is on the param is
//!   omitted.
//! - `shipping=free` is present iff the Free shipping chip is on.
//! - When no chips are active the query string is cleared entirely
//!   (`history.replaceState` to `pathname` only).
//!
//! ## Runtime split
//!
//! The script is exposed as a `&'static str` so the playground can
//! splice it into the eBay rsjs page's `</body>` exactly the same
//! way it splices `RSJS_ISLAND_STYLE`. Vanilla JS, no rsjs runtime
//! dependency, IIFE-wrapped to keep the global namespace clean.

/// Vanilla JS coordinator that turns chip-button DOM state into a CSS
/// attribute-selector rule list. Splice this into the page's
/// `</body>` (or `</head>`); both work because the script defers all
/// work to `DOMContentLoaded`.
///
/// The script intentionally does NOT depend on the rsjs runtime, so
/// it survives any future change to how rsjs hydrates the chips.
/// Round-trip URL ↔ chip-visual sync now works in both directions:
/// chip clicks project to the query string here (writes), and the
/// playground's `showcase_ebay_srp_rsjs` route reads the query
/// string and seeds the `FilterChips` island's `initial_*` fields
/// on first paint (reads).
pub const SRP_FILTER_RUNTIME_SCRIPT: &str = r#"<script>
(function () {
  // ---- DOM addressing ---------------------------------------------------
  // The chips island renders three buttons in a fixed order:
  //   [0] Buy It Now    → format=buyitnow
  //   [1] Auction       → format=auction
  //   [2] Free shipping → shipping=free
  // Each button has two child branches (active / inactive) toggled by
  // the rsjs runtime via inline style.display. We treat the
  // active-branch span being visible as "chip is on".
  var CHIPS = [
    { kind: 'format',   value: 'buyitnow' },
    { kind: 'format',   value: 'auction'  },
    { kind: 'shipping', value: 'free'     }
  ];

  function chipButtons() {
    var root = document.querySelector('.srp-filter-chips');
    return root ? root.querySelectorAll('button.filter-chip') : [];
  }

  function isChipActive(button) {
    // The active-state branch is the one carrying the
    // `filter-chip-label--active` class. rsjs hides the inactive
    // branch by setting `display: none` on the chip-label's parent
    // <span> (the conditional-then wrapper). When the chip is on,
    // the active branch's parent has display === '' (or unset).
    var activeLabel = button.querySelector('.filter-chip-label--active');
    if (!activeLabel) return false;
    var parent = activeLabel.parentElement;
    while (parent && parent !== button) {
      if (parent.style && parent.style.display === 'none') return false;
      parent = parent.parentElement;
    }
    return true;
  }

  // ---- URL <-> state ----------------------------------------------------

  function readStateFromDom() {
    var btns = chipButtons();
    var formats = [];
    var freeShipping = false;
    for (var i = 0; i < btns.length && i < CHIPS.length; i++) {
      if (!isChipActive(btns[i])) continue;
      var spec = CHIPS[i];
      if (spec.kind === 'format')   formats.push(spec.value);
      if (spec.kind === 'shipping') freeShipping = true;
    }
    return { formats: formats, freeShipping: freeShipping };
  }

  function readStateFromQuery() {
    var search = (location.search || '').replace(/^\?/, '');
    var formats = [];
    var freeShipping = false;
    if (!search) return { formats: formats, freeShipping: freeShipping };
    var parts = search.split('&');
    for (var i = 0; i < parts.length; i++) {
      var kv = parts[i].split('=');
      if (kv.length !== 2) continue;
      var key = decodeURIComponent(kv[0]);
      var val = decodeURIComponent(kv[1]);
      if (key === 'format') {
        var vals = val.split(',');
        for (var j = 0; j < vals.length; j++) {
          if (vals[j]) formats.push(vals[j]);
        }
      } else if (key === 'shipping' && val === 'free') {
        freeShipping = true;
      }
    }
    return { formats: formats, freeShipping: freeShipping };
  }

  function stateToQuery(state) {
    var parts = [];
    if (state.formats.length) parts.push('format=' + state.formats.join(','));
    if (state.freeShipping)   parts.push('shipping=free');
    return parts.length ? '?' + parts.join('&') : '';
  }

  // ---- CSS rule projection ---------------------------------------------

  function applyState(state) {
    var rules = [];
    if (state.formats.length) {
      // Hide cards whose data-format isn't in the active set. The
      // data attrs live on the inner `<div class="su-card-container">`
      // (so the consolidated `WatchListings60` rsjs island can wrap
      // each card in a literal `<li class="s-card">` without needing
      // dynamic non-reactive attribute support). `:has(> [...])`
      // walks back up to the parent `<li>` so the visual hide stays
      // on the row, not just the inner div.
      var allowed = state.formats.map(function (v) {
        return ':has(> [data-format="' + v + '"])';
      }).join(',');
      rules.push('li.s-card:not(:is(' + allowed + ')){display:none!important}');
    }
    if (state.freeShipping) {
      rules.push('li.s-card:not(:has(> [data-free-shipping="1"])){display:none!important}');
    }
    var style = document.getElementById('srp-filter');
    if (!style) {
      style = document.createElement('style');
      style.id = 'srp-filter';
      document.head.appendChild(style);
    }
    style.textContent = rules.join('\n');
  }

  // ---- Wire-up ---------------------------------------------------------

  function syncFromQuery() {
    applyState(readStateFromQuery());
  }

  function onChipClick(event) {
    var btn = event.target.closest('button.filter-chip');
    if (!btn) return;
    // The rsjs handler runs synchronously and updates inline display
    // styles before this listener fires (we rely on bubbling + later
    // listener registration). Use a microtask so we read the
    // post-click state regardless of listener ordering.
    Promise.resolve().then(function () {
      var state = readStateFromDom();
      var nextQuery = stateToQuery(state);
      var current = location.search || '';
      if (nextQuery !== current) {
        // Use history.replaceState so chip toggles don't reload the
        // page or pollute history. The server reads `?format=...` on
        // subsequent navigations / refreshes to seed the chip islands
        // with the right initial state.
        if (history && history.replaceState) {
          history.replaceState(null, '', location.pathname + nextQuery + location.hash);
        } else {
          location.search = nextQuery;  // pre-history-API fallback (full reload)
        }
        syncFromQuery();
      }
    });
  }

  function init() {
    syncFromQuery();
    var root = document.querySelector('.srp-filter-chips');
    if (root) root.addEventListener('click', onChipClick);
    // popstate fires on back/forward; covers the URL-changed-by-user case.
    window.addEventListener('popstate', syncFromQuery);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init, { once: true });
  } else {
    init();
  }
})();
</script>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_script_is_self_contained() {
        // Sanity: the script must be wrapped in an IIFE inside a
        // <script> tag, depend on no globals besides the standard
        // browser surface, and reference the data attrs the cards
        // emit. If any of these change, update the script.
        assert!(SRP_FILTER_RUNTIME_SCRIPT.starts_with("<script>"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.trim_end().ends_with("</script>"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("(function ()"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("data-format"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("data-free-shipping"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("location.search"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("history.replaceState"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("popstate"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("DOMContentLoaded"));
        assert!(SRP_FILTER_RUNTIME_SCRIPT.contains("srp-filter"));
    }

    #[test]
    fn runtime_script_under_target_size() {
        // Soft cap so we notice if the runtime grows. The current
        // body is ~5 KB raw (well under 2 KB gzipped) which is fine
        // inline; if this trips, either justify it in the commit or
        // move the script to an external file served alongside the
        // rsjs runtime.
        let bytes = SRP_FILTER_RUNTIME_SCRIPT.len();
        assert!(bytes < 8_192, "runtime grew to {bytes} bytes");
    }
}
