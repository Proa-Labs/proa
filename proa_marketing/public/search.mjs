// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Docs search in the browser, backed by Orama.
//
// Ownership split with the rsjs `SearchModal` island
// (src/components/search.rs):
//
// - The island owns the <dialog>: trigger clicks, Cmd+K, Escape, the
//   backdrop, the ESC button, and focusing/clearing the input on open. Each
//   open dispatches a `proa:search-open` event on `document`.
// - This module owns everything the user sees while typing: it loads the
//   search documents the docs build emits, indexes them with Orama, renders
//   results into #search-results, and handles ArrowUp/ArrowDown/Enter on the
//   input. It never opens or closes the dialog.
//
// The ranking itself lives in ./search-index.mjs so scripts/search_battery.mjs
// can score it in Node without a browser.

const VERSION = new URL(import.meta.url).searchParams.get('v');
const MANIFEST_URL = '/docs/_search/manifest.json';
const DEBOUNCE_MS = 60;

const input = document.getElementById('search-input');
const results = document.getElementById('search-results');
const status = document.getElementById('search-status');

if (input && results) {
  init();
}

function versioned(path) {
  const url = new URL(path, import.meta.url);
  if (VERSION) url.searchParams.set('v', VERSION);
  return url.href;
}

function init() {
  let enginePromise = null;
  let engine = null;
  let hits = [];
  let active = -1;
  let timer = 0;
  let latest = '';

  const hint = results.querySelector('[data-search-hint]');

  document.addEventListener('proa:search-open', () => {
    reset();
    warm();
  });
  // Warm the index before the first keystroke when the user shows intent.
  for (const trigger of document.querySelectorAll('[data-search-open]')) {
    trigger.addEventListener('pointerenter', warm, { once: true });
    trigger.addEventListener('focus', warm, { once: true });
  }
  input.addEventListener('focus', warm, { once: true });
  input.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(() => run(input.value), DEBOUNCE_MS);
  });
  input.addEventListener('keydown', onKeydown);
  // Browsers fire pointermove when content scrolls under a stationary
  // pointer, which would let a keyboard-driven scrollIntoView hand the
  // selection to whatever row ends up under the cursor. Only react when the
  // pointer actually moved.
  let pointerX = -1;
  let pointerY = -1;
  results.addEventListener('pointermove', (event) => {
    if (event.clientX === pointerX && event.clientY === pointerY) return;
    pointerX = event.clientX;
    pointerY = event.clientY;
    const hit = event.target.closest('[data-search-hit]');
    if (!hit) return;
    setActive(Number(hit.getAttribute('data-search-hit')), false);
  });

  function warm() {
    if (enginePromise) return enginePromise;
    enginePromise = load()
      .then((built) => {
        engine = built;
        // The user may already be typing.
        if (latest) run(latest);
        return built;
      })
      .catch((error) => {
        console.error('[search] failed to load the search index', error);
        enginePromise = null;
        setStatus('Search is unavailable right now.');
        throw error;
      });
    return enginePromise;
  }

  async function load() {
    const [{ buildIndex, searchIndex }, documents] = await Promise.all([
      import(versioned('./search-index.mjs')),
      fetchDocuments(),
    ]);
    const db = await buildIndex(documents);
    return { db, searchIndex };
  }

  async function fetchDocuments() {
    const manifest = await fetch(MANIFEST_URL).then(ok).then((response) => response.json());
    const payload = await fetch(manifest.documents).then(ok).then((response) => response.json());
    return payload.documents || [];
  }

  function ok(response) {
    if (!response.ok) throw new Error(`${response.status} ${response.url}`);
    return response;
  }

  async function run(value) {
    const query = value.trim();
    latest = query;
    if (query.length < 2) {
      reset();
      return;
    }
    if (!engine) {
      setStatus('Loading search…');
      warm();
      return;
    }
    const outcome = await engine.searchIndex(engine.db, query);
    // A slower earlier search must not overwrite a newer one.
    if (query !== latest) return;
    render(outcome);
  }

  function reset() {
    hits = [];
    active = -1;
    latest = '';
    clearHits();
    if (hint) hint.hidden = false;
    input.removeAttribute('aria-activedescendant');
    input.setAttribute('aria-expanded', 'false');
    setStatus('');
  }

  function clearHits() {
    for (const node of results.querySelectorAll('[data-search-hit], [data-search-none]')) {
      node.remove();
    }
  }

  function render(outcome) {
    hits = outcome.hits;
    clearHits();
    if (hint) hint.hidden = true;
    if (hits.length === 0) {
      const none = document.createElement('div');
      none.className = 'search-empty';
      none.setAttribute('data-search-none', '');
      none.textContent = `No results for “${outcome.query}”`;
      results.appendChild(none);
      input.setAttribute('aria-expanded', 'false');
      input.removeAttribute('aria-activedescendant');
      setStatus('');
      return;
    }
    const fragment = document.createDocumentFragment();
    hits.forEach((hit, index) => fragment.appendChild(hitElement(hit, index)));
    results.appendChild(fragment);
    input.setAttribute('aria-expanded', 'true');
    setActive(0, false);
    const shown = hits.length;
    const total = outcome.count;
    let text = `${total} result${total === 1 ? '' : 's'}`;
    if (total > shown) text = `Showing ${shown} of ${total}`;
    if (outcome.pass === 'typo') text += ', including close matches';
    if (outcome.pass === 'any') text += ', matching any word';
    setStatus(text);
  }

  function hitElement(hit, index) {
    const anchor = document.createElement('a');
    anchor.className = 'search-hit';
    anchor.id = `search-hit-${index}`;
    anchor.href = hit.href;
    anchor.setAttribute('role', 'option');
    anchor.setAttribute('aria-selected', 'false');
    anchor.setAttribute('data-search-hit', String(index));

    const icon = document.createElement('span');
    icon.className = 'search-hit-icon';
    icon.setAttribute('aria-hidden', 'true');
    anchor.appendChild(icon);

    const body = document.createElement('span');
    body.className = 'search-hit-body';

    const title = document.createElement('span');
    title.className = 'search-hit-title';
    if (hit.section) {
      const section = document.createElement('span');
      section.className = 'search-hit-section';
      section.textContent = hit.section;
      title.appendChild(section);
    }
    title.appendChild(document.createTextNode(hit.title));
    if (hit.heading) {
      const heading = document.createElement('span');
      heading.className = 'search-hit-heading';
      heading.textContent = hit.heading;
      title.appendChild(heading);
    }
    body.appendChild(title);

    const sub = document.createElement('span');
    sub.className = 'search-hit-sub';
    for (const segment of hit.snippet) {
      if (segment.match) {
        const mark = document.createElement('mark');
        mark.textContent = segment.text;
        sub.appendChild(mark);
      } else {
        sub.appendChild(document.createTextNode(segment.text));
      }
    }
    body.appendChild(sub);
    anchor.appendChild(body);
    return anchor;
  }

  function setActive(index, scroll) {
    if (hits.length === 0) return;
    const next = ((index % hits.length) + hits.length) % hits.length;
    const previous = results.querySelector('.search-hit-active');
    if (previous) {
      previous.classList.remove('search-hit-active');
      previous.setAttribute('aria-selected', 'false');
    }
    const current = document.getElementById(`search-hit-${next}`);
    if (!current) return;
    current.classList.add('search-hit-active');
    current.setAttribute('aria-selected', 'true');
    input.setAttribute('aria-activedescendant', current.id);
    if (scroll) current.scrollIntoView({ block: 'nearest' });
    active = next;
  }

  function onKeydown(event) {
    if (hits.length === 0) return;
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        setActive(active + 1, true);
        break;
      case 'ArrowUp':
        event.preventDefault();
        setActive(active - 1, true);
        break;
      case 'Home':
        event.preventDefault();
        setActive(0, true);
        break;
      case 'End':
        event.preventDefault();
        setActive(hits.length - 1, true);
        break;
      case 'Enter': {
        const hit = hits[active >= 0 ? active : 0];
        if (!hit) return;
        event.preventDefault();
        window.location.assign(hit.href);
        break;
      }
      default:
        break;
    }
  }

  function setStatus(text) {
    if (status) status.textContent = text;
  }
}
