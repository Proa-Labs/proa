// Docs search ranking, shared between the browser and the battery.
//
// `public/search.mjs` imports this in the browser and `scripts/search_battery.mjs`
// imports it in Node, so the battery scores the exact configuration the site
// ships. Everything that decides ranking lives in DEFAULT_CONFIG or in the
// small post-processing helpers below; keep it that way so a tuning run can
// override the config without touching the UI.
//
// Orama is loaded through a dynamic import so the `?v=` cache-busting version
// on this module's own URL propagates to the vendored bundle (static assets are
// served with a one-year immutable cache).

const VERSION = new URL(import.meta.url).searchParams.get('v');
const oramaUrl = new URL('./vendor/orama.mjs', import.meta.url);
if (VERSION) oramaUrl.searchParams.set('v', VERSION);
const orama = await import(oramaUrl.href);

export const DEFAULT_CONFIG = {
  // Fields searched, in the order Orama walks them.
  properties: ['title', 'slug', 'keywords', 'description', 'headings', 'content'],
  // Multipliers on the BM25 score per field. Slugs, keywords, and titles say
  // what a page is *about*; content only breaks ties between pages that all
  // mention the term. `keywords` come from page frontmatter: the identifiers
  // and API names a page canonically documents, so "html_sync" lands on HTML
  // rendering even though thirty pages mention it. The title multiplier is
  // deliberately modest: titles are one to four tokens, so BM25 already
  // scores a title hit highly, and a larger multiplier let a one-word title
  // ("Security") beat a page whose slug and keywords matched every term
  // ("security headers"). Values come from scripts/search_battery.mjs runs
  // over the fixture plus a held-out query set.
  boost: { title: 3, slug: 4, keywords: 6, headings: 3, description: 2, content: 1 },
  // Also run a whole-token pass and scale its scores by exactBonus, so
  // "rails" prefers the page about Rails over the one about Railway while a
  // strong prefix hit can still outrank a weak whole-token hit.
  exactFirst: true,
  exactBonus: 2,
  // Maximum edit distance in the typo fallback pass. The distance actually
  // used grows with the longest term (one edit per four characters), so
  // "sigals" still finds Signals and "hydraton" finds Hydration without a
  // three-letter query fuzzing into unrelated words.
  tolerance: 2,
  // 0 means every query term must match somewhere in the document. The
  // fallback pass relaxes this to 1 (any term) when nothing matches.
  threshold: 0,
  // BM25 parameters: k saturates term frequency, b scales the length
  // penalty (1 = full, 0 = none), d is the lower bound on a term's
  // contribution. Long reference pages should not lose to short pages that
  // mention the term once.
  relevance: { k: 1.2, b: 0.75, d: 0.5 },
  limit: 10,
  // English stemming so "caching" finds "cache" and "handlers" finds
  // "handler". Code identifiers such as html_sync survive because the
  // tokenizer keeps underscores.
  stemming: true,
  stemmerSkipProperties: [],
  // Off: docs queries are short and words like "why" and "when" carry meaning
  // here ("why proa", "when stuck").
  stopWords: false,
  minQueryLength: 2,
  snippetLength: 150,
};

const SCHEMA = {
  title: 'string',
  slug: 'string',
  keywords: 'string[]',
  description: 'string',
  headings: 'string[]',
  content: 'string',
};

/**
 * Build an Orama index from the documents array of `/docs/_search/documents.json`.
 */
export async function buildIndex(documents, config = DEFAULT_CONFIG) {
  const db = orama.create({
    schema: SCHEMA,
    components: {
      tokenizer: {
        language: 'english',
        stemming: config.stemming,
        stemmer: orama.englishStemmer,
        stemmerSkipProperties: config.stemmerSkipProperties,
        stopWords: config.stopWords,
      },
    },
  });
  await orama.insertMultiple(db, documents.map(toRecord));
  return db;
}

/**
 * Search the index. Returns `{ hits, count, pass }` where `pass` names which
 * fallback produced the hits: `prefix` (every term matched as a prefix),
 * `typo` (edit distance allowed), `any` (at least one term matched), or
 * `none`.
 */
export async function searchIndex(db, term, config = DEFAULT_CONFIG) {
  const query = String(term ?? '').trim();
  if (query.length < config.minQueryLength) {
    return { hits: [], count: 0, pass: 'none', query };
  }
  const base = {
    term: query,
    properties: config.properties,
    boost: config.boost,
    relevance: config.relevance,
    limit: config.limit,
  };
  const terms = queryTerms(query);
  const run = (pass) =>
    orama.search(db, {
      ...base,
      exact: pass.exact,
      tolerance: pass.tolerance,
      threshold: pass.threshold,
    });

  // Whole-token hits and prefix hits are pooled and ordered by score, with
  // whole-token hits multiplied by `exactBonus`: "rails" prefers the page
  // about Rails over Railway when the two are close, while a strong prefix
  // match (a keyword or title) still beats a weak whole-token match in body
  // text.
  const merged = new Map();
  const remember = (hit, factor) => {
    const score = hit.score * factor;
    const known = merged.get(hit.document.url);
    if (!known || known.score < score) merged.set(hit.document.url, { hit, score });
  };
  let pass = 'none';
  let count = 0;
  if (config.exactFirst) {
    const exact = await run({ exact: true, tolerance: 0, threshold: config.threshold });
    for (const hit of exact.hits) remember(hit, config.exactBonus);
    if (exact.count > 0) {
      pass = 'exact';
      count = exact.count;
    }
  }
  const prefix = await run({ exact: false, tolerance: 0, threshold: config.threshold });
  for (const hit of prefix.hits) remember(hit, 1);
  if (prefix.count > 0) {
    if (pass === 'none') pass = 'prefix';
    count = Math.max(count, prefix.count);
  }

  // Nothing matched every term as typed: allow typos, then any single term.
  const tolerance = typoTolerance(terms, config.tolerance);
  const fallbacks = [
    { name: 'typo', exact: false, tolerance, threshold: config.threshold },
    { name: 'any', exact: false, tolerance, threshold: 1 },
  ];
  for (const fallback of fallbacks) {
    if (merged.size > 0) break;
    if (fallback.name === 'typo' && tolerance === 0) continue;
    const result = await run(fallback);
    for (const hit of result.hits) remember(hit, 1);
    if (result.count > 0) {
      pass = fallback.name;
      count = result.count;
    }
  }

  const hits = [...merged.values()]
    .sort((a, b) => b.score - a.score)
    .slice(0, config.limit)
    .map(({ hit, score }) => decorate(hit, score, terms, config));
  return { hits, count, pass, query };
}

/** Edit distance for the typo pass: one per four characters of the longest term, capped. */
function typoTolerance(terms, max) {
  const longest = terms.reduce((n, term) => Math.max(n, term.length), 0);
  return Math.min(max, Math.floor(longest / 4));
}

/**
 * Lowercased alphanumeric query terms (underscores kept, so `html_sync` stays
 * one term).
 */
export function queryTerms(query) {
  return String(query)
    .toLowerCase()
    .split(/[^a-z0-9_]+/)
    .filter(Boolean);
}

function toRecord(doc) {
  const headings = Array.isArray(doc.headings) ? doc.headings : [];
  return {
    id: doc.id,
    url: doc.url,
    title: doc.title ?? '',
    slug: slugWords(doc.url),
    keywords: Array.isArray(doc.keywords) ? doc.keywords.map(String) : [],
    description: doc.description ?? '',
    section: doc.section ?? null,
    headings: headings.map((heading) => heading.title ?? ''),
    headingIds: headings.map((heading) => heading.id ?? ''),
    content: doc.content ?? '',
  };
}

/** `/docs/framework/security-headers` becomes `security headers`. */
function slugWords(url) {
  const parts = String(url ?? '')
    .split('/')
    .filter(Boolean);
  if (parts.length === 0) return '';
  // Drop the collection prefix ("docs"): every page shares it.
  const last = parts.slice(1);
  return last.join(' ').replace(/[-_]+/g, ' ');
}

function decorate(hit, score, terms, config) {
  const doc = hit.document;
  const heading = bestHeading(doc, terms);
  return {
    url: doc.url,
    href: heading ? `${doc.url}#${heading.id}` : doc.url,
    title: doc.title,
    description: doc.description,
    section: doc.section,
    heading: heading ? heading.title : null,
    snippet: snippet(doc, terms, config.snippetLength),
    score,
  };
}

/**
 * The heading to deep-link when it matches the query better than the page
 * title does. Returns null when the page itself is the best target.
 */
function bestHeading(doc, terms) {
  if (terms.length === 0) return null;
  const titleHits = termHits(doc.title, terms);
  let best = null;
  let bestHits = titleHits;
  for (let i = 0; i < doc.headings.length; i += 1) {
    const hits = termHits(doc.headings[i], terms);
    if (hits > bestHits) {
      best = { title: doc.headings[i], id: doc.headingIds[i] };
      bestHits = hits;
    }
  }
  return best && best.id ? best : null;
}

/** Number of query terms that start a word in `text`. */
function termHits(text, terms) {
  const words = queryTerms(text);
  let hits = 0;
  for (const term of terms) {
    if (words.some((word) => word.startsWith(term))) hits += 1;
  }
  return hits;
}

/**
 * A short window of the page content around the first query match, as
 * segments `{ text, match }` so the renderer can wrap matches without ever
 * treating content as HTML. Falls back to the description, then the opening
 * of the page.
 */
function snippet(doc, terms, length) {
  // Search text keeps code-block line breaks; a one-line snippet should not.
  const content = (doc.content || '').replace(/\s+/g, ' ');
  const lower = content.toLowerCase();
  let first = -1;
  for (const term of terms) {
    const at = wordStart(lower, term);
    if (at !== -1 && (first === -1 || at < first)) first = at;
  }
  if (first === -1) {
    const fallback = doc.description || content.slice(0, length);
    return [{ text: clip(fallback, length), match: false }];
  }
  const start = Math.max(0, first - Math.floor(length / 3));
  let end = Math.min(content.length, start + length);
  let windowStart = start;
  if (windowStart > 0) {
    const space = content.indexOf(' ', windowStart);
    if (space !== -1 && space < first) windowStart = space + 1;
  }
  if (end < content.length) {
    const space = content.lastIndexOf(' ', end);
    if (space > first) end = space;
  }
  const text = content.slice(windowStart, end);
  const segments = highlight(text, terms);
  if (windowStart > 0) segments.unshift({ text: '…', match: false });
  if (end < content.length) segments.push({ text: '…', match: false });
  return segments;
}

/** Index of `term` at the start of a word in `lower`, or -1. */
function wordStart(lower, term) {
  let from = 0;
  for (;;) {
    const at = lower.indexOf(term, from);
    if (at === -1) return -1;
    if (at === 0 || !/[a-z0-9_]/.test(lower[at - 1])) return at;
    from = at + 1;
  }
}

function highlight(text, terms) {
  const segments = [];
  const lower = text.toLowerCase();
  let cursor = 0;
  for (;;) {
    let next = -1;
    let nextTerm = '';
    for (const term of terms) {
      const at = wordStart(lower.slice(cursor), term);
      if (at !== -1 && (next === -1 || cursor + at < next)) {
        next = cursor + at;
        nextTerm = term;
      }
    }
    if (next === -1) break;
    if (next > cursor) segments.push({ text: text.slice(cursor, next), match: false });
    // Extend the highlight over the whole matched word so "cach" highlights
    // "caching".
    let wordEnd = next + nextTerm.length;
    while (wordEnd < text.length && /[a-z0-9_]/i.test(text[wordEnd])) wordEnd += 1;
    segments.push({ text: text.slice(next, wordEnd), match: true });
    cursor = wordEnd;
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor), match: false });
  return segments;
}

function clip(text, length) {
  if (text.length <= length) return text;
  const cut = text.lastIndexOf(' ', length);
  return `${text.slice(0, cut > 0 ? cut : length)}…`;
}
