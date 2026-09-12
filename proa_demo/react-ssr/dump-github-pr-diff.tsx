// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Dump the React SSR GitHub PR-diff page HTML to stdout, for parity diffing
// against the Proa renderer. Run: node --import tsx dump-github-pr-diff.tsx
//
// The document shell mirrors wrapInGithubShell in server.tsx, which in turn
// mirrors the Proa page's own <html>/<head> (lang, data-pr-diff-demo, metas,
// inline stylesheet) and the trailing interaction <script>.
import React from "react";
import { renderToString } from "react-dom/server";
import {
  GithubPrDiffPage,
  GITHUB_PR_DIFF_CSS,
  GITHUB_PR_DIFF_JS,
  DEFAULT_GITHUB_PR_DIFF_PROPS,
} from "../react-ssr-shared/src/github-pr-diff";

const body = renderToString(<GithubPrDiffPage data={DEFAULT_GITHUB_PR_DIFF_PROPS} />);
process.stdout.write(
  `<!DOCTYPE html><html lang="en" data-pr-diff-demo><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Proa large PR diff demo</title><style>${GITHUB_PR_DIFF_CSS}</style></head><body>${body}<script>${GITHUB_PR_DIFF_JS}</script></body></html>`,
);
