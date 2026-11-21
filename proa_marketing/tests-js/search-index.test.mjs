// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { buildIndex, normalizeQuery, searchIndex } from "../public/search-index.mjs";

function page(slug, fields) {
  return {
    id: slug,
    url: `/docs/${slug}`,
    title: slug,
    description: "",
    section: null,
    headings: [],
    keywords: [],
    content: "",
    ...fields,
  };
}

const ranked = outcome => outcome.hits.map(hit => [hit.url, hit.score]);

test("edge punctuation is trimmed from each piece while tokenizer characters and inner punctuation stay", () => {
  for (const [typed, term] of [
    ["cx.url", "cx.url"],
    ["cx.url()", "cx.url"],
    ["cx.url(", "cx.url"],
    ['cx.url("', "cx.url"],
    ["  cx.url( )  ", "cx.url"],
    ["html!", "html"],
    ["#[rsjs]", "rsjs"],
    ['"og:image"', "og:image"],
    ["foo() bar()", "foo bar"],
    ["cx.head_mut().og_image()", "cx.head_mut().og_image"],
    ['cx.url("public/og.png")', 'cx.url("public/og.png'],
    ["--port", "--port"],
    ["'static", "'static"],
    ["(Élan)", "Élan"],
    ["x()", "x"],
    ["()", ""],
    ["( )", ""],
    ["::", ""],
    [undefined, ""],
  ]) {
    assert.equal(normalizeQuery(typed), term, `normalizeQuery(${JSON.stringify(typed)})`);
  }
});

test("a call typed with or without its parentheses ranks like the bare identifier", async () => {
  const db = await buildIndex([
    page("linking", {
      keywords: ["cx.url"],
      content: 'Use cx.url("/blog") for anchors, forms, and asset attributes.',
    }),
    page("forms", { content: "The cx handle parses urlencoded form bodies." }),
    page("routing", { content: "Read route parameters from cx before rendering." }),
  ]);
  const bare = await searchIndex(db, "cx.url");
  assert.equal(bare.pass, "exact");
  assert.equal(bare.hits[0].url, "/docs/linking");
  assert.ok(bare.hits.some(hit => hit.url === "/docs/forms"), "prefix-only pages still match");
  for (const typed of ["cx.url()", "cx.url(", 'cx.url("', 'cx.url("...")', "  cx.url( )  "]) {
    const outcome = await searchIndex(db, typed);
    assert.equal(outcome.pass, "exact", typed);
    assert.deepEqual(ranked(outcome), ranked(bare), typed);
    assert.equal(outcome.count, bare.count, typed);
    // The text shown back to the reader is what they typed, not the normalized term.
    assert.equal(outcome.query, typed.trim(), typed);
  }
});

test("a pasted call with arguments still has to appear verbatim to earn the whole-token bonus", async () => {
  const db = await buildIndex([
    page("metadata", { content: 'let og = cx.url("public/images/og.png"); then set the image.' }),
    page("cards", { content: "Each cx render writes a png url under public images for og cards." }),
  ]);
  const outcome = await searchIndex(db, 'cx.url("public/images/og.png")');
  assert.equal(outcome.pass, "exact");
  assert.deepEqual(
    outcome.hits.map(hit => hit.url),
    ["/docs/metadata", "/docs/cards"],
  );
});

test("queries with nothing left to search return no hits and echo the typed text", async () => {
  const db = await buildIndex([page("x-ray", { content: "x marks the spot () :: {}" })]);
  for (const typed of ["", " ", "x", "()", "( )", "x()", "::", "{}", '"("']) {
    assert.deepEqual(await searchIndex(db, typed), {
      hits: [],
      count: 0,
      pass: "none",
      query: typed.trim(),
    });
  }
  assert.deepEqual(await searchIndex(db, null), { hits: [], count: 0, pass: "none", query: "" });
});

test("count reports every match beyond the limit and never fewer than the hits shown", async () => {
  const many = Array.from({ length: 12 }, (_, index) =>
    page(`cache-${index}`, { content: `Cache entry ${index} stays warm.` }),
  );
  const capped = await searchIndex(await buildIndex(many), "cache");
  assert.equal(capped.hits.length, 10);
  assert.equal(capped.count, 12);

  // One page matches both terms only as whole tokens and the other only as a
  // prefix. The pooled result holds both, so the total has to say two.
  const pooled = await searchIndex(
    await buildIndex([
      page("fonts", { content: "Self-host google fonts with fontsource files." }),
      page("apis", { content: "Call googleapis for font metadata." }),
    ]),
    "google fonts",
  );
  assert.deepEqual(pooled.hits.map(hit => hit.url).sort(), ["/docs/apis", "/docs/fonts"]);
  assert.equal(pooled.count, 2);
});

test("snippets highlight the identifier a call query names", async () => {
  const db = await buildIndex([
    page("signals", { content: "Create state with signal(0) and read it with count.get() in a handler." }),
  ]);
  const [hit] = (await searchIndex(db, "count.get()")).hits;
  assert.deepEqual(
    hit.snippet.filter(segment => segment.match).map(segment => segment.text),
    ["count", "get"],
  );
});
