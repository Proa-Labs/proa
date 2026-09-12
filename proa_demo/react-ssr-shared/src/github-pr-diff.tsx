// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// GitHub PR diff page component - React equivalent of proa_demo::github_pr_diff.
//
// This is a 1:1 markup port of proa_demo/src/github_pr_diff.rs (the Proa page
// is ground truth): same element tree, same class/aria/data attributes, same
// SVG paths, same synthesized diff content, all as idiomatic JSX (no raw
// innerHTML injection anywhere). The playground passes the same JSON props
// (snake_case, camelCased by the server) to this component that the Proa
// fixture consumes; local defaults mirror GithubPrDiffProps::default() so
// direct GET routes and standalone renderToString benchmarks stay useful.
//
// GITHUB_PR_DIFF_CSS / GITHUB_PR_DIFF_JS are byte-for-byte copies of the Rust
// CSS / JS consts (spliced verbatim; do not reformat). The JS is injected by
// the document shell (server.tsx wrapInGithubShell) before </body>.

import React from "react";
import { FIXTURE_DIFF_LINES, FixtureDiffLine } from "./github-pr-diff-fixtures";

// ---------------------------------------------------------------------------
// Props (camelCase mirror of the Rust serde structs)
// ---------------------------------------------------------------------------

export interface GithubPrDiffLine {
  kind?: string;
  oldLine?: number | null;
  newLine?: number | null;
  text?: string;
}

export interface GithubPrDiffFile {
  path?: string;
  additions?: number;
  deletions?: number;
  visibleLines?: number;
  diffLines?: GithubPrDiffLine[];
}

export interface GithubPrDiffProps {
  repoOwner?: string;
  repoName?: string;
  title?: string;
  pullNumber?: number;
  author?: string;
  status?: string;
  issueCountLabel?: string;
  pullRequestCountLabel?: string;
  forkCountLabel?: string;
  starCountLabel?: string;
  baseBranch?: string;
  headBranch?: string;
  commitCount?: number;
  checksCount?: number;
  conversationCount?: number;
  reviewCommentCount?: number;
  viewedFileCount?: number;
  commitSelectorLabel?: string;
  reviewActionLabel?: string;
  fileCount?: number;
  linesPerFile?: number;
  files?: GithubPrDiffFile[];
}

const DEFAULT_FILE_COUNT = 16;
const DEFAULT_LINES_PER_FILE = 40;
const MAX_FILE_COUNT = 48;
const MAX_LINES_PER_FILE = 1200;

const FILE_PATHS = [
  "src/install/PackageManager/PackageJSONEditor.zig",
  "src/js/node/events.ts",
  "src/js_parser/ast/parseProperty.zig",
  "src/js_parser/lexer.zig",
  "test/bundler/bundler_naming.test.ts",
  "test/cli/install/bun-add.test.ts",
  "test/cli/install/bun-lockb.test.ts",
  "test/js/bun/resolve/resolve-autoinstall-log-dangling.test.ts",
  "test/js/bun/shell/brace.test.ts",
  "test/js/bun/spawn/spawn-kill-signal.test.ts",
  "test/js/bun/util/filesink.test.ts",
  "test/js/node/events/event-emitter.test.ts",
  "test/js/node/fs/fs-readfile-fifo-fixture.js",
  "test/js/node/fs/fs.test.ts",
  "test/regression/issue/30963.test.ts",
  "test/regression/issue/jsx-template-string-crash.test.ts",
];

const FILE_ADDITIONS = [3, 3, 1, 2, 20, 136, 63, 56, 27, 31, 10, 30, 31, 23, 37, 39];
const FILE_DELETIONS = [1, 1, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 4];
const DEFAULT_FILE_LINES = [11, 11, 9, 16, 24, 143, 74, 56, 36, 35, 13, 37, 0, 26, 37, 53];

function clampInt(value: number, min: number, max: number): number {
  if (value < min) return min;
  if (value > max) return max;
  return value;
}

interface ResolvedLine {
  kind: string;
  oldLine: number | null;
  newLine: number | null;
  text: string;
}

interface ResolvedFile {
  path: string;
  additions: number;
  deletions: number;
  visibleLines: number;
  diffLines: ResolvedLine[];
}

interface ResolvedProps {
  repoOwner: string;
  repoName: string;
  title: string;
  pullNumber: number;
  author: string;
  status: string;
  issueCountLabel: string;
  pullRequestCountLabel: string;
  forkCountLabel: string;
  starCountLabel: string;
  baseBranch: string;
  headBranch: string;
  commitCount: number;
  checksCount: number;
  conversationCount: number;
  reviewCommentCount: number;
  viewedFileCount: number;
  commitSelectorLabel: string;
  reviewActionLabel: string;
  fileCount: number;
  linesPerFile: number;
  files: ResolvedFile[];
}

function resolveLine(line: GithubPrDiffLine | undefined): ResolvedLine {
  return {
    kind: line?.kind ?? "context",
    oldLine: line?.oldLine ?? null,
    newLine: line?.newLine ?? null,
    text: line?.text ?? "",
  };
}

function resolveFile(file: GithubPrDiffFile | undefined): ResolvedFile {
  return {
    path: file?.path ?? "",
    additions: file?.additions ?? 0,
    deletions: file?.deletions ?? 0,
    visibleLines: file?.visibleLines ?? 0,
    diffLines: (file?.diffLines ?? []).map(resolveLine),
  };
}

// Mirrors the serde `#[serde(default = ...)]` field defaults plus
// `GithubPrDiffProps::clamped()`.
function resolveProps(data: GithubPrDiffProps | undefined): ResolvedProps {
  const p: ResolvedProps = {
    repoOwner: data?.repoOwner ?? "oven-sh",
    repoName: data?.repoName ?? "bun",
    title: data?.title ?? "[tracking] Zig/C++ fork: Rust→Zig port progress",
    pullNumber: data?.pullNumber ?? 31136,
    author: data?.author ?? "MaurerAnton",
    status: data?.status ?? "Closed",
    issueCountLabel: data?.issueCountLabel ?? "5k+",
    pullRequestCountLabel: data?.pullRequestCountLabel ?? "1.9k",
    forkCountLabel: data?.forkCountLabel ?? "4.6k",
    starCountLabel: data?.starCountLabel ?? "92.2k",
    baseBranch: data?.baseBranch ?? "oven-sh:main",
    headBranch: data?.headBranch ?? "MaurerAnton:zig-fork",
    commitCount: data?.commitCount ?? 8,
    checksCount: data?.checksCount ?? 1,
    conversationCount: data?.conversationCount ?? 9,
    reviewCommentCount: data?.reviewCommentCount ?? 5,
    viewedFileCount: data?.viewedFileCount ?? 0,
    commitSelectorLabel: data?.commitSelectorLabel ?? "All commits",
    reviewActionLabel: data?.reviewActionLabel ?? "Submit comments",
    fileCount: data?.fileCount ?? DEFAULT_FILE_COUNT,
    linesPerFile: data?.linesPerFile ?? DEFAULT_LINES_PER_FILE,
    files: (data?.files ?? []).map(resolveFile),
  };
  p.fileCount = clampInt(p.fileCount, 0, MAX_FILE_COUNT);
  p.linesPerFile = clampInt(p.linesPerFile, 0, MAX_LINES_PER_FILE);
  if (p.files.length > 0) {
    p.files = p.files.slice(0, MAX_FILE_COUNT);
    p.fileCount = p.files.length;
  }
  p.viewedFileCount = clampInt(p.viewedFileCount, 0, p.fileCount);
  return p;
}

export const DEFAULT_GITHUB_PR_DIFF_PROPS: GithubPrDiffProps = {
  repoOwner: "oven-sh",
  repoName: "bun",
  title: "[tracking] Zig/C++ fork: Rust→Zig port progress",
  pullNumber: 31136,
  author: "MaurerAnton",
  status: "Closed",
  issueCountLabel: "5k+",
  pullRequestCountLabel: "1.9k",
  forkCountLabel: "4.6k",
  starCountLabel: "92.2k",
  baseBranch: "oven-sh:main",
  headBranch: "MaurerAnton:zig-fork",
  commitCount: 8,
  checksCount: 1,
  conversationCount: 9,
  reviewCommentCount: 5,
  viewedFileCount: 0,
  commitSelectorLabel: "All commits",
  reviewActionLabel: "Submit comments",
  fileCount: DEFAULT_FILE_COUNT,
  linesPerFile: DEFAULT_LINES_PER_FILE,
  files: FILE_PATHS.map((path, i) => {
    const diffLines: GithubPrDiffLine[] = FIXTURE_DIFF_LINES[i].map((line) => ({
      kind: line.kind,
      oldLine: line.oldLine,
      newLine: line.newLine,
      text: line.text,
    }));
    return {
      path,
      additions: FILE_ADDITIONS[i],
      deletions: FILE_DELETIONS[i],
      visibleLines: diffLines.length > 0 ? diffLines.length : DEFAULT_FILE_LINES[i],
      diffLines,
    };
  }),
};

// ---------------------------------------------------------------------------
// Data helpers mirroring the Rust impl blocks
// ---------------------------------------------------------------------------

function fixtureLines(fileIdx: number): FixtureDiffLine[] | null {
  return fileIdx >= 0 && fileIdx < FIXTURE_DIFF_LINES.length ? FIXTURE_DIFF_LINES[fileIdx] : null;
}

function fileAt(p: ResolvedProps, fileIdx: number): ResolvedFile | null {
  if (p.files.length === 0) return null;
  return p.files[fileIdx % p.files.length] ?? null;
}

function filePath(p: ResolvedProps, fileIdx: number): string {
  const file = fileAt(p, fileIdx);
  if (file && file.path !== "") return file.path;
  return FILE_PATHS[fileIdx % FILE_PATHS.length];
}

function fileLines(p: ResolvedProps, fileIdx: number): number {
  const file = fileAt(p, fileIdx);
  if (file && file.diffLines.length > 0) {
    return clampInt(file.diffLines.length, 0, MAX_LINES_PER_FILE);
  }
  if (p.linesPerFile === DEFAULT_LINES_PER_FILE) {
    const fixture = fixtureLines(fileIdx);
    if (fixture) return fixture.length;
  }
  if (p.linesPerFile === DEFAULT_LINES_PER_FILE) {
    if (file) return clampInt(file.visibleLines, 0, MAX_LINES_PER_FILE);
    if (fileIdx < DEFAULT_FILE_LINES.length) return DEFAULT_FILE_LINES[fileIdx];
  }
  return p.linesPerFile;
}

function hasExplicitDiffLines(p: ResolvedProps, fileIdx: number): boolean {
  const file = fileAt(p, fileIdx);
  if (file && file.diffLines.length > 0) return true;
  return p.linesPerFile === DEFAULT_LINES_PER_FILE && fixtureLines(fileIdx) != null;
}

function fileRowCount(p: ResolvedProps, fileIdx: number): number {
  const lines = fileLines(p, fileIdx);
  if (hasExplicitDiffLines(p, fileIdx) || lines === 0) return lines;
  return lines + 1;
}

function fileAdditions(p: ResolvedProps, fileIdx: number): number {
  const lines = fileLines(p, fileIdx);
  if (p.linesPerFile === DEFAULT_LINES_PER_FILE) {
    const file = fileAt(p, fileIdx);
    if (file) return file.additions;
    if (fileIdx < FILE_ADDITIONS.length) return FILE_ADDITIONS[fileIdx];
  }
  return Math.floor((lines * 6) / 10);
}

function fileDeletions(p: ResolvedProps, fileIdx: number): number {
  const lines = fileLines(p, fileIdx);
  if (p.linesPerFile === DEFAULT_LINES_PER_FILE) {
    const file = fileAt(p, fileIdx);
    if (file) return file.deletions;
    if (fileIdx < FILE_DELETIONS.length) return FILE_DELETIONS[fileIdx];
  }
  return Math.floor((lines * 2) / 10);
}

function totalFiles(p: ResolvedProps): number {
  return p.fileCount;
}

function totalLines(p: ResolvedProps): number {
  let total = 0;
  for (let fileIdx = 0; fileIdx < totalFiles(p); fileIdx++) {
    total += fileRowCount(p, fileIdx);
  }
  return total;
}

function totalAdditions(p: ResolvedProps): number {
  let total = 0;
  for (let fileIdx = 0; fileIdx < totalFiles(p); fileIdx++) {
    total += fileAdditions(p, fileIdx);
  }
  return total;
}

function totalDeletions(p: ResolvedProps): number {
  let total = 0;
  for (let fileIdx = 0; fileIdx < totalFiles(p); fileIdx++) {
    total += fileDeletions(p, fileIdx);
  }
  return total;
}

function statusClass(p: ResolvedProps): string {
  return p.status.toLowerCase() === "closed" ? "status-pill status-closed" : "status-pill status-open";
}

function firstSegment(path: string): string {
  const pos = path.indexOf("/");
  return pos >= 0 ? path.slice(0, pos) : path;
}

function parentSegments(path: string): string {
  const first = path.indexOf("/");
  const last = path.lastIndexOf("/");
  if (first >= 0 && last > first) return path.slice(first + 1, last);
  return "";
}

function basename(path: string): string {
  const pos = path.lastIndexOf("/");
  return pos >= 0 ? path.slice(pos + 1) : path;
}

function hunkLabel(fileIdx: number): string {
  switch (fileIdx % FILE_PATHS.length) {
    case 0:
      return "@@ -459,7 +459,9 @@ pub fn edit(";
    case 1:
      return "@@ -436,7 +436,9 @@ EventEmitterPrototype.eventNames = function eventNames() {";
    case 2:
      return "@@ -318,7 +318,7 @@ pub fn ParseProperty(";
    case 3:
      return "@@ -98,6 +98,8 @@ pub fn nextToken(";
    default:
      return "@@ -41,7 +42,9 @@ parser/state bridge";
  }
}

type DiffLineKind = "hunk" | "context" | "addition" | "deletion";

function kindForLine(lineIdx: number): DiffLineKind {
  switch (lineIdx % 12) {
    case 3:
    case 9:
      return "deletion";
    case 4:
    case 5:
    case 6:
    case 10:
      return "addition";
    default:
      return "context";
  }
}

function kindFromLabel(label: string): DiffLineKind {
  const lower = label.toLowerCase();
  if (lower === "hunk") return "hunk";
  if (lower === "addition" || lower === "added") return "addition";
  if (lower === "deletion" || lower === "deleted") return "deletion";
  return "context";
}

function rowClass(kind: DiffLineKind): string {
  switch (kind) {
    case "hunk":
      return "diff-line-row hunk-row";
    case "addition":
      return "diff-line-row diff-row is-addition";
    case "deletion":
      return "diff-line-row diff-row is-deletion";
    default:
      return "diff-line-row diff-row";
  }
}

function codeClass(kind: DiffLineKind): string {
  switch (kind) {
    case "addition":
      return "diff-text syntax-highlighted-line addition";
    case "deletion":
      return "diff-text syntax-highlighted-line deletion";
    default:
      return "diff-text syntax-highlighted-line";
  }
}

function lineMarker(kind: DiffLineKind): string {
  switch (kind) {
    case "addition":
      return "+";
    case "deletion":
      return "-";
    default:
      return " ";
  }
}

function lineNumberClass(kind: DiffLineKind, oldSide: boolean, hasValue: boolean): string {
  if (!hasValue) {
    return oldSide ? "line-number old empty-diff-line" : "line-number new empty-diff-line";
  }
  if (kind === "addition") return "line-number new blob-num-addition";
  if (kind === "deletion") return "line-number old blob-num-deletion";
  return oldSide ? "line-number old diff-line-number-neutral" : "line-number new diff-line-number-neutral";
}

function commentTarget(oldLine: number | null, newLine: number | null): [string, number] {
  if (newLine != null) return ["R", newLine];
  if (oldLine != null) return ["L", oldLine];
  return ["R", 0];
}

function generatedHunkStarts(fileIdx: number): [number, number] {
  switch (fileIdx % FILE_PATHS.length) {
    case 0:
      return [459, 459];
    case 1:
      return [436, 436];
    case 2:
      return [318, 318];
    case 3:
      return [98, 98];
    default:
      return [41, 42];
  }
}

function generatedLineNumbers(fileIdx: number, lineIdx: number): [number, number] {
  let [oldLine, newLine] = generatedHunkStarts(fileIdx);
  for (let idx = 0; idx < lineIdx; idx++) {
    switch (kindForLine(idx)) {
      case "addition":
        newLine += 1;
        break;
      case "deletion":
        oldLine += 1;
        break;
      default:
        oldLine += 1;
        newLine += 1;
        break;
    }
  }
  return [oldLine, newLine];
}

// ---------------------------------------------------------------------------
// Syntax highlighting (port of the Rust HighlightedCode tokenizer, emitting
// React elements; React handles text escaping)
// ---------------------------------------------------------------------------

const KEYWORDS = new Set([
  "and",
  "as",
  "await",
  "break",
  "const",
  "continue",
  "else",
  "export",
  "fn",
  "for",
  "from",
  "function",
  "if",
  "import",
  "in",
  "let",
  "or",
  "pub",
  "return",
  "test",
  "try",
  "var",
  "while",
]);

const LITERALS = new Set(["false", "null", "true", "undefined"]);

function isIdentStart(c: string): boolean {
  return c === "_" || (c >= "a" && c <= "z") || (c >= "A" && c <= "Z");
}

function isIdentPart(c: string): boolean {
  return isIdentStart(c) || (c >= "0" && c <= "9");
}

function isDigit(c: string): boolean {
  return c >= "0" && c <= "9";
}

function nextNonWs(text: string, end: number): string | null {
  while (end < text.length) {
    const c = text[end];
    if (c !== " " && c !== "\t" && c !== "\n" && c !== "\r" && c !== "\f") return c;
    end += 1;
  }
  return null;
}

function identifierClass(token: string, text: string, end: number): string | null {
  if (KEYWORDS.has(token)) return "pl-k";
  if (LITERALS.has(token)) return "pl-c1";
  if (nextNonWs(text, end) === "(") return "pl-en";
  return null;
}

function scanString(text: string, start: number): number {
  const quote = text[start];
  let i = start + 1;
  while (i < text.length) {
    if (text[i] === "\\") {
      i = Math.min(i + 2, text.length);
    } else if (text[i] === quote) {
      return i + 1;
    } else {
      i += 1;
    }
  }
  return text.length;
}

// Port of the Rust HighlightedCode component; returns React nodes (plain
// text fragments plus <span class="pl-*"> token elements).
function highlightNodes(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  let key = 0;
  const pushPlain = (fragment: string) => {
    if (fragment.length > 0) nodes.push(fragment);
  };
  const pushSpan = (cls: string, fragment: string) => {
    if (fragment.length > 0) {
      nodes.push(
        <span key={key++} className={cls}>
          {fragment}
        </span>,
      );
    }
  };

  let i = 0;
  let plainStart = 0;
  const n = text.length;

  while (i < n) {
    const c = text[i];
    if (c === "/" && i + 1 < n && text[i + 1] === "/") {
      pushPlain(text.slice(plainStart, i));
      pushSpan("pl-c comment", text.slice(i));
      return nodes;
    } else if (c === '"' || c === "'" || c === "`") {
      const end = scanString(text, i);
      pushPlain(text.slice(plainStart, i));
      pushSpan("pl-s", text.slice(i, end));
      i = end;
      plainStart = i;
    } else if (isIdentStart(c)) {
      const start = i;
      i += 1;
      while (i < n && isIdentPart(text[i])) i += 1;
      const token = text.slice(start, i);
      const cls = identifierClass(token, text, i);
      if (cls) {
        pushPlain(text.slice(plainStart, start));
        pushSpan(cls, token);
        plainStart = i;
      }
    } else if (isDigit(c)) {
      const start = i;
      i += 1;
      while (i < n && (isIdentPart(text[i]) || text[i] === ".")) i += 1;
      pushPlain(text.slice(plainStart, start));
      pushSpan("pl-c1", text.slice(start, i));
      plainStart = i;
    } else {
      i += 1;
    }
  }

  pushPlain(text.slice(plainStart));
  return nodes;
}

// ---------------------------------------------------------------------------
// Octicons (JSX ports of the Rust ICON_* raw-string consts, attributes kept
// verbatim)
// ---------------------------------------------------------------------------

function IconSidebarCollapse() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-sidebar-collapse"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M6.823 7.823a.25.25 0 0 1 0 .354l-2.396 2.396A.25.25 0 0 1 4 10.396V5.604a.25.25 0 0 1 .427-.177Z" />
      <path d="M1.75 0h12.5C15.216 0 16 .784 16 1.75v12.5A1.75 1.75 0 0 1 14.25 16H1.75A1.75 1.75 0 0 1 0 14.25V1.75C0 .784.784 0 1.75 0ZM1.5 1.75v12.5c0 .138.112.25.25.25H9.5v-13H1.75a.25.25 0 0 0-.25.25ZM11 14.5h3.25a.25.25 0 0 0 .25-.25V1.75a.25.25 0 0 0-.25-.25H11Z" />
    </svg>
  );
}

function IconSidebarExpand() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-sidebar-expand"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="m4.177 7.823 2.396-2.396A.25.25 0 0 1 7 5.604v4.792a.25.25 0 0 1-.427.177L4.177 8.177a.25.25 0 0 1 0-.354Z" />
      <path d="M0 1.75C0 .784.784 0 1.75 0h12.5C15.216 0 16 .784 16 1.75v12.5A1.75 1.75 0 0 1 14.25 16H1.75A1.75 1.75 0 0 1 0 14.25Zm1.75-.25a.25.25 0 0 0-.25.25v12.5c0 .138.112.25.25.25H9.5v-13Zm12.5 13a.25.25 0 0 0 .25-.25V1.75a.25.25 0 0 0-.25-.25H11v13Z" />
    </svg>
  );
}

function IconPullRequestClosed() {
  return (
    <svg
      data-component="Octicon"
      focusable="false"
      aria-label="Pull request"
      className="octicon octicon-git-pull-request-closed prc-StateLabel-Icon-YICrR"
      role="img"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M3.25 1A2.25 2.25 0 0 1 4 5.372v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.251 2.251 0 0 1 3.25 1Zm9.5 5.5a.75.75 0 0 1 .75.75v3.378a2.251 2.251 0 1 1-1.5 0V7.25a.75.75 0 0 1 .75-.75Zm-2.03-5.273a.75.75 0 0 1 1.06 0l.97.97.97-.97a.748.748 0 0 1 1.265.332.75.75 0 0 1-.205.729l-.97.97.97.97a.751.751 0 0 1-.018 1.042.751.751 0 0 1-1.042.018l-.97-.97-.97.97a.749.749 0 0 1-1.275-.326.749.749 0 0 1 .215-.734l.97-.97-.97-.97a.75.75 0 0 1 0-1.06ZM2.5 3.25a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0ZM3.25 12a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5Zm9.5 0a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Z" />
    </svg>
  );
}

function IconVersions() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-versions"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M7.75 14A1.75 1.75 0 0 1 6 12.25v-8.5C6 2.784 6.784 2 7.75 2h6.5c.966 0 1.75.784 1.75 1.75v8.5A1.75 1.75 0 0 1 14.25 14Zm-.25-1.75c0 .138.112.25.25.25h6.5a.25.25 0 0 0 .25-.25v-8.5a.25.25 0 0 0-.25-.25h-6.5a.25.25 0 0 0-.25.25ZM4.9 3.508a.75.75 0 0 1-.274 1.025.249.249 0 0 0-.126.217v6.5c0 .09.048.173.126.217a.75.75 0 0 1-.752 1.298A1.75 1.75 0 0 1 3 11.25v-6.5c0-.649.353-1.214.874-1.516a.75.75 0 0 1 1.025.274ZM1.625 5.533h.001a.249.249 0 0 0-.126.217v4.5c0 .09.048.173.126.217a.75.75 0 0 1-.752 1.298A1.748 1.748 0 0 1 0 10.25v-4.5a1.748 1.748 0 0 1 .873-1.516.75.75 0 1 1 .752 1.299Z" />
    </svg>
  );
}

function IconTriangleDown() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-triangle-down"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="m4.427 7.427 3.396 3.396a.25.25 0 0 0 .354 0l3.396-3.396A.25.25 0 0 0 11.396 7H4.604a.25.25 0 0 0-.177.427Z" />
    </svg>
  );
}

function IconGear() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-gear"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M8 0a8.2 8.2 0 0 1 .701.031C9.444.095 9.99.645 10.16 1.29l.288 1.107c.018.066.079.158.212.224.231.114.454.243.668.386.123.082.233.09.299.071l1.103-.303c.644-.176 1.392.021 1.82.63.27.385.506.792.704 1.218.315.675.111 1.422-.364 1.891l-.814.806c-.049.048-.098.147-.088.294.016.257.016.515 0 .772-.01.147.038.246.088.294l.814.806c.475.469.679 1.216.364 1.891a7.977 7.977 0 0 1-.704 1.217c-.428.61-1.176.807-1.82.63l-1.102-.302c-.067-.019-.177-.011-.3.071a5.909 5.909 0 0 1-.668.386c-.133.066-.194.158-.211.224l-.29 1.106c-.168.646-.715 1.196-1.458 1.26a8.006 8.006 0 0 1-1.402 0c-.743-.064-1.289-.614-1.458-1.26l-.289-1.106c-.018-.066-.079-.158-.212-.224a5.738 5.738 0 0 1-.668-.386c-.123-.082-.233-.09-.299-.071l-1.103.303c-.644.176-1.392-.021-1.82-.63a8.12 8.12 0 0 1-.704-1.218c-.315-.675-.111-1.422.363-1.891l.815-.806c.05-.048.098-.147.088-.294a6.214 6.214 0 0 1 0-.772c.01-.147-.038-.246-.088-.294l-.815-.806C.635 6.045.431 5.298.746 4.623a7.92 7.92 0 0 1 .704-1.217c.428-.61 1.176-.807 1.82-.63l1.102.302c.067.019.177.011.3-.071.214-.143.437-.272.668-.386.133-.066.194-.158.211-.224l.29-1.106C6.009.645 6.556.095 7.299.03 7.53.01 7.764 0 8 0Zm-.571 1.525c-.036.003-.108.036-.137.146l-.289 1.105c-.147.561-.549.967-.998 1.189-.173.086-.34.183-.5.29-.417.278-.97.423-1.529.27l-1.103-.303c-.109-.03-.175.016-.195.045-.22.312-.412.644-.573.99-.014.031-.021.11.059.19l.815.806c.411.406.562.957.53 1.456a4.709 4.709 0 0 0 0 .582c.032.499-.119 1.05-.53 1.456l-.815.806c-.081.08-.073.159-.059.19.162.346.353.677.573.989.02.03.085.076.195.046l1.102-.303c.56-.153 1.113-.008 1.53.27.161.107.328.204.501.29.447.222.85.629.997 1.189l.289 1.105c.029.109.101.143.137.146a6.6 6.6 0 0 0 1.142 0c.036-.003.108-.036.137-.146l.289-1.105c.147-.561.549-.967.998-1.189.173-.086.34-.183.5-.29.417-.278.97-.423 1.529-.27l1.103.303c.109.029.175-.016.195-.045.22-.313.411-.644.573-.99.014-.031.021-.11-.059-.19l-.815-.806c-.411-.406-.562-.957-.53-1.456a4.709 4.709 0 0 0 0-.582c-.032-.499.119-1.05.53-1.456l.815-.806c.081-.08.073-.159.059-.19a6.464 6.464 0 0 0-.573-.989c-.02-.03-.085-.076-.195-.046l-1.102.303c-.56.153-1.113.008-1.53-.27a4.44 4.44 0 0 0-.501-.29c-.447-.222-.85-.629-.997-1.189l-.289-1.105c-.029-.11-.101-.143-.137-.146a6.6 6.6 0 0 0-1.142 0ZM11 8a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM9.5 8a1.5 1.5 0 1 0-3.001.001A1.5 1.5 0 0 0 9.5 8Z" />
    </svg>
  );
}

function IconGitCommit() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-git-commit fg-muted mr-2 d-none d-sm-inline-block"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M11.93 8.5a4.002 4.002 0 0 1-7.86 0H.75a.75.75 0 0 1 0-1.5h3.32a4.002 4.002 0 0 1 7.86 0h3.32a.75.75 0 0 1 0 1.5Zm-1.43-.75a2.5 2.5 0 1 0-5 0 2.5 2.5 0 0 0 5 0Z" />
    </svg>
  );
}

function IconPlus() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-plus"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M7.75 2a.75.75 0 0 1 .75.75V7h4.25a.75.75 0 0 1 0 1.5H8.5v4.25a.75.75 0 0 1-1.5 0V8.5H2.75a.75.75 0 0 1 0-1.5H7V2.75A.75.75 0 0 1 7.75 2Z" />
    </svg>
  );
}

function IconIssueOpened() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-issue-opened"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M8 9.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z" />
      <path d="M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM1.5 8a6.5 6.5 0 1 0 13 0 6.5 6.5 0 0 0-13 0Z" />
    </svg>
  );
}

function IconGitPullRequest() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-git-pull-request"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M1.5 3.25a2.25 2.25 0 1 1 3 2.122v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.25 2.25 0 0 1 1.5 3.25Zm5.677-.177L9.573.677A.25.25 0 0 1 10 .854V2.5h1A2.5 2.5 0 0 1 13.5 5v5.628a2.251 2.251 0 1 1-1.5 0V5a1 1 0 0 0-1-1h-1v1.646a.25.25 0 0 1-.427.177L7.177 3.427a.25.25 0 0 1 0-.354ZM3.75 2.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm0 9.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm8.25.75a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0Z" />
    </svg>
  );
}

function IconRepo() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-repo"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M2 2.5A2.5 2.5 0 0 1 4.5 0h8.75a.75.75 0 0 1 .75.75v12.5a.75.75 0 0 1-.75.75h-2.5a.75.75 0 0 1 0-1.5h1.75v-2h-8a1 1 0 0 0-.714 1.7.75.75 0 1 1-1.072 1.05A2.495 2.495 0 0 1 2 11.5Zm10.5-1h-8a1 1 0 0 0-1 1v6.708A2.486 2.486 0 0 1 4.5 9h8ZM5 12.25a.25.25 0 0 1 .25-.25h3.5a.25.25 0 0 1 .25.25v3.25a.25.25 0 0 1-.4.2l-1.45-1.087a.249.249 0 0 0-.3 0L5.4 15.7a.25.25 0 0 1-.4-.2Z" />
    </svg>
  );
}

function IconInbox() {
  return (
    <svg
      data-component="Octicon"
      aria-hidden="true"
      focusable="false"
      className="octicon octicon-inbox"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="currentColor"
      display="inline-block"
      overflow="visible"
      style={{ verticalAlign: "text-bottom" }}
    >
      <path d="M2.8 2.06A1.75 1.75 0 0 1 4.41 1h7.18c.7 0 1.333.417 1.61 1.06l2.74 6.395c.04.093.06.194.06.295v4.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25v-4.5c0-.101.02-.202.06-.295Zm1.61.44a.25.25 0 0 0-.23.152L1.887 8H4.75a.75.75 0 0 1 .6.3L6.625 10h2.75l1.275-1.7a.75.75 0 0 1 .6-.3h2.863L11.82 2.652a.25.25 0 0 0-.23-.152Zm10.09 7h-2.875l-1.275 1.7a.75.75 0 0 1-.6.3h-3.5a.75.75 0 0 1-.6-.3L4.375 9.5H1.5v3.75c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25Z" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Components (1:1 ports of the Rust WebRenderSync impls)
// ---------------------------------------------------------------------------

function TopAppHeader({ p }: { p: ResolvedProps }) {
  return (
    <header
      aria-label="Global Navigation Menu"
      className="GlobalNav styles-module__appHeader__YzYWk app-header prc-Stack-Stack-UQ9k6"
      data-gap="none"
      data-direction="vertical"
      data-align="stretch"
      data-wrap="nowrap"
      data-justify="start"
      data-padding="none"
    >
      <div
        className="app-header-top prc-Stack-Stack-UQ9k6"
        data-direction="horizontal"
        data-align="center"
        data-wrap="nowrap"
        data-justify="center"
        data-padding="none"
      >
        <div
          data-testid="top-nav-left"
          className="styles-module__left__Fylw7 styles-module__withLocalNavigation__rjTJ_ app-header-left prc-Stack-Stack-UQ9k6"
          data-gap="condensed"
          data-direction="horizontal"
          data-align="stretch"
          data-wrap="nowrap"
          data-justify="start"
          data-padding="normal"
        >
          <div data-loading-wrapper="true">
            <button
              data-component="IconButton"
              type="button"
              aria-haspopup="dialog"
              className="app-header-button icon-only"
              data-loading="false"
              data-no-visuals="true"
              data-size="medium"
              data-variant="invisible"
              aria-label="Open menu"
            >
              <svg
                data-component="Octicon"
                aria-hidden="true"
                focusable="false"
                className="octicon octicon-three-bars"
                viewBox="0 0 16 16"
                width="16"
                height="16"
                fill="currentColor"
                display="inline-block"
                overflow="visible"
                style={{ verticalAlign: "text-bottom" }}
              >
                <path d="M1 2.75A.75.75 0 0 1 1.75 2h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 2.75Zm0 5A.75.75 0 0 1 1.75 7h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 7.75ZM1.75 12h12.5a.75.75 0 0 1 0 1.5H1.75a.75.75 0 0 1 0-1.5Z"></path>
              </svg>
            </button>
          </div>
          <a
            data-component="IconButton"
            type="button"
            className="github-mark app-header-home"
            data-loading="false"
            data-no-visuals="true"
            data-size="medium"
            data-variant="invisible"
            href="/"
            aria-label="Homepage"
          >
            <svg
              data-component="Octicon"
              aria-hidden="true"
              focusable="false"
              className="octicon octicon-mark-github"
              viewBox="0 0 24 24"
              width="32"
              height="32"
              fill="currentColor"
              display="inline-block"
              overflow="visible"
              style={{ verticalAlign: "text-bottom" }}
            >
              <path d="M10.226 17.284c-2.965-.36-5.054-2.493-5.054-5.256 0-1.123.404-2.336 1.078-3.144-.292-.741-.247-2.314.09-2.965.898-.112 2.111.36 2.83 1.01.853-.269 1.752-.404 2.853-.404 1.1 0 1.999.135 2.807.382.696-.629 1.932-1.1 2.83-.988.315.606.36 2.179.067 2.942.72.854 1.101 2 1.101 3.167 0 2.763-2.089 4.852-5.098 5.234.763.494 1.28 1.572 1.28 2.807v2.336c0 .674.561 1.056 1.235.786 4.066-1.55 7.255-5.615 7.255-10.646C23.5 6.188 18.334 1 11.978 1 5.62 1 .5 6.188.5 12.545c0 4.986 3.167 9.12 7.435 10.669.606.225 1.19-.18 1.19-.786V20.63a2.9 2.9 0 0 1-1.078.224c-1.483 0-2.359-.808-2.987-2.313-.247-.607-.517-.966-1.034-1.033-.27-.023-.359-.135-.359-.27 0-.27.45-.471.898-.471.652 0 1.213.404 1.797 1.235.45.651.921.943 1.483.943.561 0 .92-.202 1.437-.719.382-.381.674-.718.944-.943"></path>
            </svg>
          </a>
        </div>
        <div
          data-testid="top-nav-center"
          className="styles-module__center__R3QRv styles-module__withLocalNavigation__rjTJ_ app-header-center prc-Stack-Stack-UQ9k6"
          data-gap="condensed"
          data-direction="horizontal"
          data-align="stretch"
          data-wrap="nowrap"
          data-justify="start"
          data-padding="normal"
        >
          <nav
            className="styles-module__contextRegion__VbSp2 prc-Breadcrumbs-BreadcrumbsBase-3Gb-B header-breadcrumbs"
            aria-label="Breadcrumbs"
            data-overflow="menu"
            data-variant="normal"
            data-component="Breadcrumbs"
          >
            <ol className="prc-Breadcrumbs-BreadcrumbsList-BKjpe">
              <li className="prc-Breadcrumbs-ItemWrapper-k0NLn">
                <a
                  className="styles-module__contextCrumb__IzGIq prc-Breadcrumbs-Item-jcraJ header-breadcrumb"
                  data-component="Breadcrumbs.Item"
                  href={`/${p.repoOwner}`}
                >
                  <span>{p.repoOwner}</span>
                </a>
              </li>
              <li className="prc-Breadcrumbs-ItemWrapper-k0NLn">
                <a
                  className="styles-module__contextCrumb__IzGIq prc-Breadcrumbs-Item-jcraJ header-breadcrumb"
                  data-component="Breadcrumbs.Item"
                  href={`/${p.repoOwner}/${p.repoName}`}
                >
                  <span className="styles-module__contextCrumbLast__tI2e3">{p.repoName}</span>
                </a>
              </li>
            </ol>
          </nav>
          <div
            className="Search-module__searchButtonGroup__aetw5 header-search-group prc-ButtonGroup-ButtonGroup-vFUrY"
            data-component="ButtonGroup"
          >
            <div>
              <button
                data-component="Button"
                type="button"
                aria-label="Search or jump to..."
                className="prc-Button-ButtonBase-9n-Xk styles-module__appHeaderButton__axedQ Search-module__searchButton__aiE0a app-header-button header-search"
                data-loading="false"
                data-size="medium"
                data-variant="invisible"
              >
                <span data-component="buttonContent" data-align="center" className="prc-Button-ButtonContent-Iohp5">
                  <span data-component="leadingVisual" className="prc-Button-Visual-YNt2F prc-Button-VisualWrap-E4cnq">
                    <svg
                      data-component="Octicon"
                      aria-hidden="true"
                      focusable="false"
                      className="octicon octicon-search"
                      viewBox="0 0 16 16"
                      width="16"
                      height="16"
                      fill="currentColor"
                      display="inline-block"
                      overflow="visible"
                      style={{ verticalAlign: "text-bottom" }}
                    >
                      <path d="M10.68 11.74a6 6 0 0 1-7.922-8.982 6 6 0 0 1 8.982 7.922l3.04 3.04a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215ZM11.5 7a4.499 4.499 0 1 0-8.997 0A4.499 4.499 0 0 0 11.5 7Z"></path>
                    </svg>
                  </span>
                  <span data-component="text" className="prc-Button-Label-FWkx3">
                    <span className="Search-module__placeholder__p9hbG Search-module__text__veSYi Search-module__value__TFoak">
                      {"Type "}
                      <kbd className="Search-module__kbd__WCskr">/</kbd>
                      {" to search"}
                    </span>
                  </span>
                </span>
              </button>
            </div>
            <div></div>
          </div>
        </div>
        <div
          data-testid="top-nav-right"
          className="styles-module__right__mlBQg styles-module__withLocalNavigation__rjTJ_ app-header-right prc-Stack-Stack-UQ9k6"
          data-gap="condensed"
          data-direction="horizontal"
          data-align="center"
          data-wrap="nowrap"
          data-justify="start"
          data-padding="normal"
        >
          <div
            data-testid="top-bar-actions"
            className="app-action-group prc-Stack-Stack-UQ9k6"
            data-gap="condensed"
            data-direction="horizontal"
            data-align="center"
            data-wrap="nowrap"
            data-justify="start"
            data-padding="none"
          >
            <div className="dropdown-wrap">
              <button
                type="button"
                className="app-header-button icon-with-caret"
                aria-label="Create new"
                aria-haspopup="true"
                aria-expanded="false"
                data-menu-button=""
                data-menu-target="create-menu"
              >
                <IconPlus />
                <IconTriangleDown />
              </button>
              <div className="pr-menu app-menu" id="create-menu" role="menu" hidden>
                <button type="button" role="menuitem">
                  New repository
                </button>
                <button type="button" role="menuitem">
                  Import repository
                </button>
                <button type="button" role="menuitem">
                  New codespace
                </button>
                <button type="button" role="menuitem">
                  New organization
                </button>
              </div>
            </div>
            <div className="hide-sm hide-md app-quick-links">
              <a className="app-header-button icon-only" href="/issues" aria-label="All issues">
                <IconIssueOpened />
              </a>
              <a className="app-header-button icon-only" href="/pulls" aria-label="All pull requests">
                <IconGitPullRequest />
              </a>
              <a className="app-header-button icon-only" href="/repos" aria-label="All repositories">
                <IconRepo />
              </a>
            </div>
          </div>
          <a
            className="app-header-button icon-only notification-button"
            href="/notifications"
            aria-label="You have unread notifications"
          >
            <IconInbox />
          </a>
          {/* Known invisible delta: React 19's renderToString hoists a
              <link rel="preload" as="image"> hint for this img to the start
              of the stream; the Proa page does not emit one. */}
          <button type="button" className="avatar-button" aria-label="Open user navigation menu">
            <img
              alt="Demo user"
              width="32"
              height="32"
              src="https://github.githubassets.com/images/gravatars/gravatar-user-420.png"
            />
          </button>
        </div>
      </div>
      <h2 className="prc-src-InternalVisuallyHidden-2YaI6 sr-only">Repository navigation</h2>
      <nav
        className="prc-components-UnderlineWrapper-eT-Yj LocalNavigation-module__LocalNavigation__b0Xc0 local-navigation"
        aria-label="Repository"
        data-variant="inset"
        data-overflow-measured="true"
      >
        <ul className="prc-components-UnderlineItemList-xKlKC local-nav-list">
          <li className="prc-UnderlineNav-UnderlineNavItem-syRjR">
            <a
              href={`/${p.repoOwner}/${p.repoName}`}
              data-tab-item="code"
              className="prc-components-UnderlineItem-7fP-n local-nav-item"
            >
              <span data-component="icon" className="repo-tab-icon" aria-hidden="true">
                <svg
                  data-component="Octicon"
                  aria-hidden="true"
                  focusable="false"
                  className="octicon octicon-code"
                  viewBox="0 0 16 16"
                  width="16"
                  height="16"
                  fill="currentColor"
                  display="inline-block"
                  overflow="visible"
                  style={{ verticalAlign: "text-bottom" }}
                >
                  <path d="m11.28 3.22 4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.749.749 0 0 1-1.275-.326.749.749 0 0 1 .215-.734L13.94 8l-3.72-3.72a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215Zm-6.56 0a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042L2.06 8l3.72 3.72a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215L.47 8.53a.75.75 0 0 1 0-1.06Z"></path>
                </svg>
              </span>
              <span data-component="text" data-content="Code">
                Code
              </span>
            </a>
          </li>
          <li className="prc-UnderlineNav-UnderlineNavItem-syRjR">
            <a
              href={`/${p.repoOwner}/${p.repoName}/issues`}
              data-tab-item="issues"
              className="prc-components-UnderlineItem-7fP-n local-nav-item"
            >
              <span data-component="icon" className="repo-tab-icon" aria-hidden="true">
                <IconIssueOpened />
              </span>
              <span data-component="text" data-content="Issues">
                Issues
              </span>
              <span data-component="counter" className="local-nav-counter">
                {p.issueCountLabel}
              </span>
            </a>
          </li>
          <li className="prc-UnderlineNav-UnderlineNavItem-syRjR">
            <a
              href={`/${p.repoOwner}/${p.repoName}/pulls`}
              aria-current="page"
              data-tab-item="pull-requests"
              className="prc-components-UnderlineItem-7fP-n local-nav-item active"
            >
              <span data-component="icon" className="repo-tab-icon" aria-hidden="true">
                <svg
                  data-component="Octicon"
                  aria-hidden="true"
                  focusable="false"
                  className="octicon octicon-git-pull-request"
                  viewBox="0 0 16 16"
                  width="16"
                  height="16"
                  fill="currentColor"
                  display="inline-block"
                  overflow="visible"
                  style={{ verticalAlign: "text-bottom" }}
                >
                  <path d="M1.5 3.25a2.25 2.25 0 1 1 3 2.122v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.25 2.25 0 0 1 1.5 3.25Zm5.677-.177L9.573.677A.25.25 0 0 1 10 .854V2.5h1A2.5 2.5 0 0 1 13.5 5v5.628a2.251 2.251 0 1 1-1.5 0V5a1 1 0 0 0-1-1h-1v1.646a.25.25 0 0 1-.427.177L7.177 3.427a.25.25 0 0 1 0-.354ZM3.75 2.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm0 9.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm8.25.75a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0Z"></path>
                </svg>
              </span>
              <span data-component="text" data-content="Pull requests">
                Pull requests
              </span>
              <span data-component="counter" className="local-nav-counter">
                {p.pullRequestCountLabel}
              </span>
            </a>
          </li>
          <li className="prc-UnderlineNav-UnderlineNavItem-syRjR">
            <a
              href={`/${p.repoOwner}/${p.repoName}/discussions`}
              data-tab-item="discussions"
              className="prc-components-UnderlineItem-7fP-n local-nav-item"
            >
              <span data-component="icon" className="repo-tab-icon" aria-hidden="true">
                <svg
                  data-component="Octicon"
                  aria-hidden="true"
                  focusable="false"
                  className="octicon octicon-comment-discussion"
                  viewBox="0 0 16 16"
                  width="16"
                  height="16"
                  fill="currentColor"
                  display="inline-block"
                  overflow="visible"
                  style={{ verticalAlign: "text-bottom" }}
                >
                  <path d="M1.75 1h8.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 10.25 10H7.061l-2.574 2.573A1.458 1.458 0 0 1 2 11.543V10h-.25A1.75 1.75 0 0 1 0 8.25v-5.5C0 1.784.784 1 1.75 1ZM1.5 2.75v5.5c0 .138.112.25.25.25h1a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h3.5a.25.25 0 0 0 .25-.25v-5.5a.25.25 0 0 0-.25-.25h-8.5a.25.25 0 0 0-.25.25Zm13 2a.25.25 0 0 0-.25-.25h-.5a.75.75 0 0 1 0-1.5h.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 14.25 12H14v1.543a1.458 1.458 0 0 1-2.487 1.03L9.22 12.28a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215l2.22 2.22v-2.19a.75.75 0 0 1 .75-.75h1a.25.25 0 0 0 .25-.25Z"></path>
                </svg>
              </span>
              <span data-component="text" data-content="Discussions">
                Discussions
              </span>
            </a>
          </li>
          <li className="prc-UnderlineNav-UnderlineNavItem-syRjR">
            <a
              href={`/${p.repoOwner}/${p.repoName}/actions`}
              data-tab-item="actions"
              className="prc-components-UnderlineItem-7fP-n local-nav-item"
            >
              <span data-component="icon" className="repo-tab-icon" aria-hidden="true">
                <svg
                  data-component="Octicon"
                  aria-hidden="true"
                  focusable="false"
                  className="octicon octicon-play"
                  viewBox="0 0 16 16"
                  width="16"
                  height="16"
                  fill="currentColor"
                  display="inline-block"
                  overflow="visible"
                  style={{ verticalAlign: "text-bottom" }}
                >
                  <path d="M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM1.5 8a6.5 6.5 0 1 0 13 0 6.5 6.5 0 0 0-13 0Zm4.879-2.773 4.264 2.559a.25.25 0 0 1 0 .428l-4.264 2.559A.25.25 0 0 1 6 10.559V5.442a.25.25 0 0 1 .379-.215Z"></path>
                </svg>
              </span>
              <span data-component="text" data-content="Actions">
                Actions
              </span>
            </a>
          </li>
          <li className="prc-UnderlineNav-UnderlineNavItem-syRjR">
            <a
              href={`/${p.repoOwner}/${p.repoName}/security`}
              data-tab-item="security-and-quality"
              className="prc-components-UnderlineItem-7fP-n local-nav-item"
            >
              <span data-component="icon" className="repo-tab-icon" aria-hidden="true">
                <svg
                  data-component="Octicon"
                  aria-hidden="true"
                  focusable="false"
                  className="octicon octicon-shield"
                  viewBox="0 0 16 16"
                  width="16"
                  height="16"
                  fill="currentColor"
                  display="inline-block"
                  overflow="visible"
                  style={{ verticalAlign: "text-bottom" }}
                >
                  <path d="M7.467.133a1.748 1.748 0 0 1 1.066 0l5.25 1.68A1.75 1.75 0 0 1 15 3.48V7c0 1.566-.32 3.182-1.303 4.682-.983 1.498-2.585 2.813-5.032 3.855a1.697 1.697 0 0 1-1.33 0c-2.447-1.042-4.049-2.357-5.032-3.855C1.32 10.182 1 8.566 1 7V3.48a1.75 1.75 0 0 1 1.217-1.667Zm.61 1.429a.25.25 0 0 0-.153 0l-5.25 1.68a.25.25 0 0 0-.174.238V7c0 1.358.275 2.666 1.057 3.86.784 1.194 2.121 2.34 4.366 3.297a.196.196 0 0 0 .154 0c2.245-.956 3.582-2.104 4.366-3.298C13.225 9.666 13.5 8.36 13.5 7V3.48a.251.251 0 0 0-.174-.237l-5.25-1.68Z"></path>
                </svg>
              </span>
              <span data-component="text" data-content="Security and quality">
                Security and quality
              </span>
            </a>
          </li>
          <li className="prc-UnderlineNav-UnderlineNavItem-syRjR">
            <a
              href={`/${p.repoOwner}/${p.repoName}/pulse`}
              data-tab-item="insights"
              className="prc-components-UnderlineItem-7fP-n local-nav-item"
            >
              <span data-component="icon" className="repo-tab-icon" aria-hidden="true">
                <svg
                  data-component="Octicon"
                  aria-hidden="true"
                  focusable="false"
                  className="octicon octicon-graph"
                  viewBox="0 0 16 16"
                  width="16"
                  height="16"
                  fill="currentColor"
                  display="inline-block"
                  overflow="visible"
                  style={{ verticalAlign: "text-bottom" }}
                >
                  <path d="M1.5 1.75V13.5h13.75a.75.75 0 0 1 0 1.5H.75a.75.75 0 0 1-.75-.75V1.75a.75.75 0 0 1 1.5 0Zm14.28 2.53-5.25 5.25a.75.75 0 0 1-1.06 0L7 7.06 4.28 9.78a.751.751 0 0 1-1.042-.018.751.751 0 0 1-.018-1.042l3.25-3.25a.75.75 0 0 1 1.06 0L10 7.94l4.72-4.72a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042Z"></path>
                </svg>
              </span>
              <span data-component="text" data-content="Insights">
                Insights
              </span>
            </a>
          </li>
        </ul>
      </nav>
    </header>
  );
}

function PrHeader({
  p,
  fileCount,
  lineCount,
  additions,
  deletions,
}: {
  p: ResolvedProps;
  fileCount: number;
  lineCount: number;
  additions: number;
  deletions: number;
}) {
  return (
    <div className="pr-header">
      <TopAppHeader p={p} />
      <div className="pr-title-block">
        <div className="title-row">
          <h1>
            {p.title}
            {" "}
            <span>{`#${p.pullNumber}`}</span>
          </h1>
          <button type="button" className="new-issue-button">
            New issue
          </button>
        </div>
        <div className="meta-row">
          <span className={statusClass(p)}>
            <svg aria-hidden="true" viewBox="0 0 16 16">
              <path d="M4.75 3.5a1.25 1.25 0 1 1-2.5 0 1.25 1.25 0 0 1 2.5 0Zm0 9a1.25 1.25 0 1 1-2.5 0 1.25 1.25 0 0 1 2.5 0ZM13.75 8a1.25 1.25 0 1 1-2.5 0 1.25 1.25 0 0 1 2.5 0ZM4 4.75v6.5h1.5v-6.5H4Zm7.25 2.5H7.5a2 2 0 0 0-2 2v2h1.5v-2a.5.5 0 0 1 .5-.5h3.75v-1.5Z" />
            </svg>
            {p.status}
          </span>
          <strong>{p.author}</strong>
          <span>
            {" wants to merge "}
            {p.commitCount}
            {" commits into "}
          </span>
          <code>{p.baseBranch}</code>
          <span>{" from "}</span>
          <code>{p.headBranch}</code>
        </div>
      </div>
      <div className="tabs">
        <a href="#">
          <span className="tab-icon" aria-hidden="true">
            <svg viewBox="0 0 16 16">
              <path d="M1.75 2.5h12.5a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-.75.75H6.78l-3.15 2.4A.75.75 0 0 1 2.5 13.3v-1.8h-.75A.75.75 0 0 1 1 10.75v-7.5a.75.75 0 0 1 .75-.75Z" />
            </svg>
          </span>
          {"Conversation"}
          <span className="tab-count">{p.conversationCount}</span>
        </a>
        <a href="#">
          <span className="tab-icon" aria-hidden="true">
            <IconGitCommit />
          </span>
          {"Commits"}
          <span className="tab-count">{p.commitCount}</span>
        </a>
        <a href="#">
          <span className="tab-icon" aria-hidden="true">
            <svg viewBox="0 0 16 16">
              <path d="M5.5 11.5 2.75 8.75l1.06-1.06L5.5 9.38l6.69-6.69 1.06 1.06L5.5 11.5Z" />
            </svg>
          </span>
          {"Checks"}
          <span className="tab-count">{p.checksCount}</span>
        </a>
        <a className="active" href="#">
          <span className="tab-icon" aria-hidden="true">
            <svg viewBox="0 0 16 16">
              <path d="M3.75 1h8.5C13.22 1 14 1.78 14 2.75v10.5c0 .97-.78 1.75-1.75 1.75h-8.5A1.75 1.75 0 0 1 2 13.25V2.75C2 1.78 2.78 1 3.75 1Zm0 1.5a.25.25 0 0 0-.25.25v10.5c0 .14.11.25.25.25h8.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25h-8.5ZM5 5h6v1.5H5V5Zm0 3h6v1.5H5V8Z" />
            </svg>
          </span>
          {"Files changed"}
          <span className="tab-count">{fileCount}</span>
        </a>
        <div
          className="diff-summary"
          aria-label={`Lines changed: ${additions} additions and ${deletions} deletions`}
        >
          <span className="add">
            {"+"}
            {additions}
          </span>
          <span className="del">
            {"-"}
            {deletions}
          </span>
          <span className="diff-squares">
            {[0, 1, 2, 3, 4].map((idx) => (
              <span
                key={idx}
                className={deletions === 0 || idx < 4 ? "square add-square" : "square neutral-square"}
              ></span>
            ))}
          </span>
          <span className="rendered-count sr-only">
            {lineCount}
            {" rendered rows"}
          </span>
        </div>
      </div>
    </div>
  );
}

function ReviewToolbar({
  p,
  fileCount,
  lineCount,
}: {
  p: ResolvedProps;
  fileCount: number;
  lineCount: number;
}) {
  return (
    <>
      <section
        data-side-panel-open="false"
        data-file-tree-expanded="true"
        className="review-toolbar PullRequestFilesToolbar-module__toolbar__ztHN6"
        aria-label="Pull Request Toolbar"
      >
        <h2 className="sr-only">Pull Request Toolbar</h2>
        <div className="toolbar-left">
          <div className="tree-toggle-group">
            <button
              type="button"
              className="toolbar-icon"
              data-file-tree-panel-action="expand"
              aria-label="Expand file tree"
              aria-expanded="true"
              aria-controls="pr-file-tree"
              hidden
            >
              <IconSidebarCollapse />
            </button>
            <button
              type="button"
              className="toolbar-icon"
              data-file-tree-panel-action="collapse"
              aria-label="Collapse file tree"
              aria-expanded="true"
              aria-controls="pr-file-tree"
            >
              <IconSidebarExpand />
            </button>
          </div>
          <span className={statusClass(p)}>
            <IconPullRequestClosed />
            {p.status}
          </span>
          <div className="toolbar-sticky-title">
            <strong>{p.title}</strong>
            <span>{`#${p.pullNumber}`}</span>
          </div>
          <div className="dropdown-wrap">
            <button
              type="button"
              className="toolbar-button toolbar-menu"
              id="changes-selector-button"
              aria-haspopup="true"
              aria-expanded="false"
              data-menu-button=""
              data-menu-target="changes-menu"
            >
              <span className="button-leading" aria-hidden="true">
                <IconVersions />
              </span>
              <span>{p.commitSelectorLabel}</span>
              <span className="button-trailing" aria-hidden="true">
                <IconTriangleDown />
              </span>
            </button>
            <div className="pr-menu toolbar-menu-panel" id="changes-menu" role="menu" hidden>
              <button type="button" role="menuitem">
                All commits
              </button>
              <button type="button" role="menuitem">
                Latest commit
              </button>
              <button type="button" role="menuitem">
                Viewed since last review
              </button>
              <button type="button" role="menuitem">
                {`${p.commitCount} commits in this pull request`}
              </button>
            </div>
          </div>
          <button type="button" className="toolbar-menu toolbar-link">
            {"File filter"}
            <span>{"⌄"}</span>
          </button>
          <button type="button" className="toolbar-menu toolbar-link">
            {"Conversations"}
            <span>{"⌄"}</span>
          </button>
          <span className="toolbar-muted sr-only">
            {lineCount}
            {" rendered rows"}
          </span>
        </div>
        <div className="toolbar-right">
          <div className="viewed-progress" aria-label={`${p.viewedFileCount} of ${fileCount} files viewed`}>
            <svg aria-hidden="true" viewBox="0 0 16 16">
              <circle cx="8" cy="8" fill="transparent" r="6" stroke="#d0d7de" strokeWidth="2"></circle>
              <circle
                cx="8"
                cy="8"
                fill="transparent"
                r="6"
                stroke="#8250df"
                strokeDasharray="38"
                strokeDashoffset="38"
                strokeLinecap="round"
                strokeWidth="2"
              ></circle>
            </svg>
            <span>
              {p.viewedFileCount}
              {" / "}
              {fileCount}
              {" viewed"}
            </span>
          </div>
          <span className="toolbar-divider"></span>
          <div className="dropdown-wrap">
            <button
              type="button"
              className="toolbar-button review-primary"
              aria-haspopup="true"
              aria-expanded="false"
              data-menu-button=""
              data-menu-target="review-menu"
            >
              <span>{p.reviewActionLabel}</span>
              <IconTriangleDown />
            </button>
            <div className="pr-menu toolbar-menu-panel right-aligned" id="review-menu" role="menu" hidden>
              <button type="button" role="menuitem">
                Comment
              </button>
              <button type="button" role="menuitem">
                Approve
              </button>
              <button type="button" role="menuitem">
                Request changes
              </button>
              <button type="button" role="menuitem">
                {`Submit ${p.reviewCommentCount} pending comments`}
              </button>
            </div>
          </div>
          <button
            data-component="IconButton"
            type="button"
            aria-haspopup="true"
            aria-expanded="false"
            tabIndex={0}
            className="toolbar-icon boxed prc-Button-ButtonBase-9n-Xk prc-Button-IconButton-fyge7"
            data-loading="false"
            data-no-visuals="true"
            data-size="small"
            data-variant="default"
            aria-label="Open diff view settings"
          >
            <IconGear />
          </button>
          <span className="toolbar-divider"></span>
          <button type="button" className="toolbar-icon" aria-label="Open overview panel">
            <svg aria-hidden="true" viewBox="0 0 16 16">
              <path d="M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z" />
            </svg>
          </button>
          <button
            type="button"
            className="toolbar-button comments-button"
            aria-label={`Open comments panel, ${p.reviewCommentCount} comments`}
          >
            <svg aria-hidden="true" viewBox="0 0 16 16">
              <path d="M1.75 1h8.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 10.25 10H7.061l-2.574 2.573A1.458 1.458 0 0 1 2 11.543V10h-.25A1.75 1.75 0 0 1 0 8.25v-5.5C0 1.784.784 1 1.75 1ZM1.5 2.75v5.5c0 .138.112.25.25.25h1a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h3.5a.25.25 0 0 0 .25-.25v-5.5a.25.25 0 0 0-.25-.25h-8.5a.25.25 0 0 0-.25.25Zm13 2a.25.25 0 0 0-.25-.25h-.5a.75.75 0 0 1 0-1.5h.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 14.25 12H14v1.543a1.458 1.458 0 0 1-2.487 1.03L9.22 12.28a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215l2.22 2.22v-2.19a.75.75 0 0 1 .75-.75h1a.25.25 0 0 0 .25-.25Z" />
            </svg>
            <span className="comment-count">{p.reviewCommentCount}</span>
          </button>
          <button type="button" className="toolbar-icon boxed" aria-label="More options">
            <svg aria-hidden="true" viewBox="0 0 16 16">
              <path d="M8 9a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM1.5 9a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm13 0a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z" />
            </svg>
          </button>
        </div>
      </section>
      <div className="perf-strip" data-pr-stats="">
        <span>
          {"Rows: "}
          <strong data-stat="rows">...</strong>
        </span>
        <span>
          {"DOM nodes: "}
          <strong data-stat="nodes">...</strong>
        </span>
        <span>
          {"Delegated listeners: "}
          <strong>1</strong>
        </span>
        <span data-stat="selection">No line selected</span>
      </div>
    </>
  );
}

function TreeDirectory({ label, level }: { label: string; level: number }) {
  return (
    <li role="treeitem" aria-expanded="true" aria-level={level} className="PRIVATE_TreeView-item file-tree-directory">
      <button
        type="button"
        className="PRIVATE_TreeView-item-container tree-directory-button"
        style={{ "--level": `${level};` } as React.CSSProperties}
        data-tree-action="toggle-directory"
        aria-expanded="true"
      >
        <span className="PRIVATE_TreeView-item-toggle tree-chevron" aria-hidden="true">
          <svg viewBox="0 0 16 16">
            <path d="M6.22 3.22a.75.75 0 0 1 1.06 0l4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 1 1-1.06-1.06L9.94 8 6.22 4.28a.75.75 0 0 1 0-1.06Z" />
          </svg>
        </span>
        <span className="tree-icon folder-icon" aria-hidden="true">
          <svg viewBox="0 0 16 16">
            <path d="M1.75 1A1.75 1.75 0 0 0 0 2.75v10.5C0 14.216.784 15 1.75 15h12.5A1.75 1.75 0 0 0 16 13.25v-8.5A1.75 1.75 0 0 0 14.25 3H7.5a.25.25 0 0 1-.2-.1L6.16 1.38A.95.95 0 0 0 5.4 1H1.75Z" />
          </svg>
        </span>
        <span className="PRIVATE_TreeView-item-content-text tree-label">{label}</span>
      </button>
    </li>
  );
}

function TreeFile({ p, fileIdx, level }: { p: ResolvedProps; fileIdx: number; level: number }) {
  const path = filePath(p, fileIdx);
  return (
    <li
      role="treeitem"
      aria-level={level}
      className="PRIVATE_TreeView-item DiffFileTree-module__file-tree-row__PCB1B file-tree-file"
    >
      <a
        className="PRIVATE_TreeView-item-container file-tree-link"
        style={{ "--level": `${level};` } as React.CSSProperties}
        href={`#file-${fileIdx}`}
        data-pr-action="jump-file"
        data-file-index={fileIdx}
        title={path}
      >
        <span className="tree-spacer" aria-hidden="true"></span>
        <span className="tree-icon file-icon" aria-hidden="true">
          <svg viewBox="0 0 16 16">
            <path d="M3.75 1.5a.25.25 0 0 0-.25.25v12.5c0 .14.11.25.25.25h8.5a.25.25 0 0 0 .25-.25V5h-2.75A1.75 1.75 0 0 1 8 3.25V1.5H3.75ZM2 1.75C2 .78 2.78 0 3.75 0h5.59c.46 0 .9.18 1.23.51l2.92 2.92c.33.33.51.77.51 1.24v9.58c0 .97-.78 1.75-1.75 1.75h-8.5A1.75 1.75 0 0 1 2 14.25V1.75Z" />
          </svg>
        </span>
        <span className="PRIVATE_TreeView-item-content-text tree-label">{basename(path)}</span>
      </a>
    </li>
  );
}

function FileTreeItems({ p }: { p: ResolvedProps }) {
  const items: React.ReactNode[] = [];
  let lastTop = "";
  let lastParent = "";
  for (let fileIdx = 0; fileIdx < totalFiles(p); fileIdx++) {
    const path = filePath(p, fileIdx);
    const top = firstSegment(path);
    const parent = parentSegments(path);
    if (top !== lastTop) {
      items.push(<TreeDirectory key={`top-${fileIdx}`} label={top} level={1} />);
      lastTop = top;
      lastParent = "";
    }
    if (parent !== "" && parent !== lastParent) {
      items.push(<TreeDirectory key={`parent-${fileIdx}`} label={parent} level={2} />);
      lastParent = parent;
    }
    items.push(<TreeFile key={`file-${fileIdx}`} p={p} fileIdx={fileIdx} level={parent === "" ? 2 : 3} />);
  }
  return <>{items}</>;
}

function FileTree({ p }: { p: ResolvedProps }) {
  return (
    <aside className="file-tree" id="pr-file-tree" aria-label="Changed files">
      <div className="file-tree-controls">
        <label className="tree-filter-label">
          <span className="sr-only">Filter changed files</span>
          <span className="tree-filter-icon" aria-hidden="true">
            <svg viewBox="0 0 16 16">
              <path d="M10.68 11.74a6 6 0 1 1 1.06-1.06l3.29 3.29a.75.75 0 0 1-1.06 1.06l-3.29-3.29ZM6.5 11a4.5 4.5 0 1 0 0-9 4.5 4.5 0 0 0 0 9Z" />
            </svg>
          </span>
          <input type="search" defaultValue="" placeholder="Filter changed files" />
        </label>
      </div>
      <div className="PullRequestFileTree-module__FileTreeScrollable__OW_Y2">
        <h2 className="sr-only">File tree</h2>
        <ul
          role="tree"
          aria-label="File Tree"
          data-truncate-text="true"
          className="tmp-pr-lg-3 prc-TreeView-TreeViewRootUlStyles-Mzrmj"
        >
          <FileTreeItems p={p} />
        </ul>
      </div>
    </aside>
  );
}

const HUNK_EXPAND_PATH =
  "M8 1.75 5.75 4h1.5v3.25H4l1.5-1.5L4.44 4.69 1.13 8l3.31 3.31 1.06-1.06-1.5-1.5h3.25V12h-1.5L8 14.25 10.25 12h-1.5V8.75H12l-1.5 1.5 1.06 1.06L14.87 8l-3.31-3.31-1.06 1.06 1.5 1.5H8.75V4h1.5L8 1.75Z";

function HunkRow({ fileIdx, text }: { fileIdx: number; text: string }) {
  return (
    <tr className="diff-line-row hunk-row">
      <td className="hunk-control" colSpan={2}>
        <button type="button" data-pr-action="expand-hunk" data-file-index={fileIdx}>
          <svg aria-hidden="true" viewBox="0 0 16 16">
            <path d={HUNK_EXPAND_PATH} />
          </svg>
        </button>
      </td>
      <td className="code hunk-code">
        <code>{text}</code>
      </td>
    </tr>
  );
}

// Port of the Rust RenderedDiffLine (non-hunk case).
function RenderedDiffLine({
  fileIdx,
  lineIdx,
  kind,
  oldLine,
  newLine,
  text,
}: {
  fileIdx: number;
  lineIdx: number;
  kind: DiffLineKind;
  oldLine: number | null;
  newLine: number | null;
  text: string;
}) {
  if (kind === "hunk") {
    return <HunkRow fileIdx={fileIdx} text={text} />;
  }
  const oldClass = lineNumberClass(kind, true, oldLine != null);
  const newClass = lineNumberClass(kind, false, newLine != null);
  const [commentSide, commentLine] = commentTarget(oldLine, newLine);
  return (
    <tr className={rowClass(kind)} data-line="" data-file-index={fileIdx} data-line-index={lineIdx}>
      <td className={oldClass}>{oldLine != null ? oldLine : null}</td>
      <td className={newClass}>{newLine != null ? newLine : null}</td>
      <td className="code diff-text-cell">
        <button
          type="button"
          className="line-comment-button"
          aria-label="Add comment on this line"
          data-pr-comment-button=""
          data-file-index={fileIdx}
          data-line-index={lineIdx}
          data-comment-side={commentSide}
          data-comment-line={commentLine}
        >
          <span className="line-comment-plus" aria-hidden="true">
            +
          </span>
        </button>
        <code className={codeClass(kind)}>
          <span className="line-marker diff-text-marker">{lineMarker(kind)}</span>
          <span className="code-inner diff-text-inner">{highlightNodes(text)}</span>
        </code>
      </td>
    </tr>
  );
}

// Port of the Rust DiffLine (synthetic generated rows used when props do not
// carry explicit diff lines and fixtures do not apply). The static code cell
// markup is transcribed verbatim from the html_sync! templates.
function SyntheticDiffLine({ fileIdx, lineIdx }: { fileIdx: number; lineIdx: number }) {
  const kind = kindForLine(lineIdx);
  const [oldLine, newLine] = generatedLineNumbers(fileIdx, lineIdx);
  const mod = lineIdx % 12;

  if (kind === "addition") {
    let inner: React.ReactNode;
    if (mod === 4 || mod === 10) {
      inner = (
        <>
          {"    "}
          <span className="pl-k">const</span>{" "}
          <span className="pl-s1">{`lowered_${fileIdx}_${lineIdx}`}</span>{" "}
          <span className="pl-c1">=</span>{" "}
          <span className="pl-k">try</span>{" "}
          <span className="pl-s1">parser</span>
          <span className="pl-kos">.</span>
          <span className="pl-en">nextToken</span>
          <span className="pl-kos">(</span>
          <span className="pl-kos">)</span>
          <span className="pl-kos">;</span>
        </>
      );
    } else if (mod === 5) {
      inner = (
        <>
          {"        "}
          <span className="pl-k">try</span>{" "}
          <span className="pl-s1">zig_port</span>
          <span className="pl-kos">.</span>
          <span className="pl-en">emitNode</span>
          <span className="pl-kos">(</span>
          <span className="pl-s1">ctx</span>
          <span className="pl-kos">,</span>{" "}
          <span className="pl-s1">{`lowered_${fileIdx}_${lineIdx}`}</span>
          <span className="pl-kos">)</span>
          <span className="pl-kos">;</span>
        </>
      );
    } else {
      inner = (
        <>
          {"        "}
          <span className="pl-s1">state</span>
          <span className="pl-kos">.</span>
          <span className="pl-s1">codegen</span>
          <span className="pl-kos">.</span>
          <span className="pl-s1">items</span>
          <span className="pl-kos">[</span>
          <span className="pl-c1">{fileIdx}</span>
          <span className="pl-kos">]</span>
          <span className="pl-kos">.</span>
          <span className="pl-en">appendAssumeCapacity</span>
          <span className="pl-kos">(</span>
          <span className="pl-s1">{`token_${lineIdx}`}</span>
          <span className="pl-kos">)</span>
          <span className="pl-kos">;</span>
        </>
      );
    }
    return (
      <tr
        className="diff-line-row diff-row is-addition"
        data-line=""
        data-file-index={fileIdx}
        data-line-index={lineIdx}
      >
        <td className="line-number old empty-diff-line"></td>
        <td className="line-number new blob-num-addition">{newLine}</td>
        <td className="code diff-text-cell">
          <code className="diff-text syntax-highlighted-line addition">
            <span className="line-marker diff-text-marker">+</span>
            <span className="code-inner diff-text-inner">{inner}</span>
          </code>
        </td>
      </tr>
    );
  }

  if (kind === "deletion") {
    let inner: React.ReactNode;
    if (mod === 3) {
      inner = (
        <>
          {"    "}
          <span className="pl-k old">return</span>{" "}
          <span className="pl-s1">rust_bridge</span>
          <span className="pl-kos">.</span>
          <span className="pl-en">lower_property</span>
          <span className="pl-kos">(</span>
          <span className="pl-s1">cx</span>
          <span className="pl-kos">,</span>{" "}
          <span className="pl-s1">node</span>
          <span className="pl-kos">)</span>
          <span className="pl-kos">;</span>
        </>
      );
    } else {
      inner = (
        <>
          {"    "}
          <span className="pl-s1">legacyRustPort</span>
          <span className="pl-kos">.</span>
          <span className="pl-en">recordFallback</span>
          <span className="pl-kos">(</span>
          <span className="pl-s1">{`file_${fileIdx}`}</span>
          <span className="pl-kos">,</span>{" "}
          <span className="pl-s1">{`span_${lineIdx}`}</span>
          <span className="pl-kos">)</span>
          <span className="pl-kos">;</span>
        </>
      );
    }
    return (
      <tr
        className="diff-line-row diff-row is-deletion"
        data-line=""
        data-file-index={fileIdx}
        data-line-index={lineIdx}
      >
        <td className="line-number old blob-num-deletion">{oldLine}</td>
        <td className="line-number new empty-diff-line"></td>
        <td className="code diff-text-cell">
          <code className="diff-text syntax-highlighted-line deletion">
            <span className="line-marker diff-text-marker">-</span>
            <span className="code-inner diff-text-inner">{inner}</span>
          </code>
        </td>
      </tr>
    );
  }

  let inner: React.ReactNode;
  if (mod === 7 || mod === 11) {
    inner = (
      <>
        {"    "}
        <span className="pl-c comment">{"// Shared parser state retained around generated port boundary"}</span>
      </>
    );
  } else {
    inner = (
      <>
        {"    "}
        <span className="pl-k">if</span>{" "}
        <span className="pl-kos">(</span>
        <span className="pl-s1">{`token_${lineIdx}`}</span>{" "}
        <span className="pl-c1">==</span>{" "}
        <span className="pl-kos">.</span>
        <span className="pl-c1">identifier</span>
        <span className="pl-kos">)</span>{" "}
        <span className="pl-k">continue</span>{" "}
        <span className="pl-c1">:parse_loop</span>
        <span className="pl-kos">;</span>
      </>
    );
  }
  return (
    <tr className="diff-line-row diff-row" data-line="" data-file-index={fileIdx} data-line-index={lineIdx}>
      <td className="line-number old diff-line-number-neutral">{oldLine}</td>
      <td className="line-number new diff-line-number-neutral">{newLine}</td>
      <td className="code diff-text-cell">
        <code className="diff-text syntax-highlighted-line">
          <span className="line-marker diff-text-marker">{" "}</span>
          <span className="code-inner diff-text-inner">{inner}</span>
        </code>
      </td>
    </tr>
  );
}

function tableRows(p: ResolvedProps, fileIdx: number, lines: number): React.ReactNode[] {
  const file = fileAt(p, fileIdx);
  const customLines = file ? file.diffLines : null;
  const fixture = p.linesPerFile === DEFAULT_LINES_PER_FILE ? fixtureLines(fileIdx) : null;

  if (customLines && customLines.length > 0) {
    // Explicit prop lines (this also covers the fixture fast path in Rust,
    // whose pre-rendered rows are byte-identical to rendering the lines).
    return customLines.map((line, lineIdx) => (
      <RenderedDiffLine
        key={lineIdx}
        fileIdx={fileIdx}
        lineIdx={lineIdx}
        kind={kindFromLabel(line.kind)}
        oldLine={line.oldLine}
        newLine={line.newLine}
        text={line.text}
      />
    ));
  }
  if (fixture) {
    return fixture.map((line, lineIdx) => (
      <RenderedDiffLine
        key={lineIdx}
        fileIdx={fileIdx}
        lineIdx={lineIdx}
        kind={kindFromLabel(line.kind)}
        oldLine={line.oldLine}
        newLine={line.newLine}
        text={line.text}
      />
    ));
  }
  const rows: React.ReactNode[] = [];
  if (lines > 0) {
    rows.push(<HunkRow key="hunk" fileIdx={fileIdx} text={hunkLabel(fileIdx)} />);
  }
  for (let lineIdx = 0; lineIdx < lines; lineIdx++) {
    rows.push(<SyntheticDiffLine key={lineIdx} fileIdx={fileIdx} lineIdx={lineIdx} />);
  }
  return rows;
}

function FileSection({ p, fileIdx, lines }: { p: ResolvedProps; fileIdx: number; lines: number }) {
  const additions = fileAdditions(p, fileIdx);
  const deletions = fileDeletions(p, fileIdx);
  const changed = additions + deletions;
  const rowCount = fileRowCount(p, fileIdx);
  const intrinsicHeight = rowCount * 24 + 68;
  const path = filePath(p, fileIdx);
  const shard =
    p.files.length === 0
      ? Math.floor(fileIdx / FILE_PATHS.length)
      : Math.floor(fileIdx / p.files.length);

  return (
    <section
      id={`file-${fileIdx}`}
      className="diff-file"
      data-file-section=""
      data-file-index={fileIdx}
      style={{ "--intrinsic-height": `${intrinsicHeight}px;` } as React.CSSProperties}
    >
      <div className="file-header DiffFileHeader-module__diff-file-header__UuNN4">
        <button
          type="button"
          className="icon-button"
          aria-label="Collapse file"
          data-pr-action="toggle-file"
          data-file-index={fileIdx}
        >
          <svg aria-hidden="true" viewBox="0 0 16 16">
            <path d="M12.78 5.22a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 0 1-1.06 0L3.22 6.28a.75.75 0 1 1 1.06-1.06L8 8.94l3.72-3.72a.75.75 0 0 1 1.06 0Z" />
          </svg>
        </button>
        <span className="file-handle" aria-hidden="true">
          <svg viewBox="0 0 16 16">
            <path d="M7.25 2.5 5.5 4.25l1.06 1.06.44-.44v2.38H4.62l.44-.44L4 5.75 2.25 7.5 4 9.25l1.06-1.06-.44-.44H7v2.38l-.44-.44L5.5 10.75l1.75 1.75L9 10.75 7.94 9.69l-.44.44V7.75h2.38l-.44.44 1.06 1.06 1.75-1.75-1.75-1.75-1.06 1.06.44.44H7.5V4.87l.44.44L9 4.25 7.25 2.5Z" />
          </svg>
        </span>
        <div className="file-change-map" aria-label={`${changed} changed lines`}>
          <span className="file-change-count">{changed}</span>
          <span className="file-change-bars" aria-hidden="true">
            {[0, 1, 2, 3, 4].map((idx) => (
              <span
                key={idx}
                className={
                  idx < Math.min(additions, 5)
                    ? "change-square add-square"
                    : idx < Math.min(additions + deletions, 5)
                      ? "change-square del-square"
                      : "change-square empty-square"
                }
              ></span>
            ))}
          </span>
        </div>
        <div className="file-title DiffFileHeader-module__file-path-section__ZcmB1">
          <h2>
            {path}
            {shard > 0 ? (
              <span className="path-shard">
                {" shard "}
                {shard}
              </span>
            ) : null}
          </h2>
          <span className="copy-icon" aria-hidden="true">
            <svg viewBox="0 0 16 16">
              <path d="M0 6.75C0 5.78.78 5 1.75 5h7.5c.97 0 1.75.78 1.75 1.75v7.5c0 .97-.78 1.75-1.75 1.75h-7.5A1.75 1.75 0 0 1 0 14.25v-7.5ZM1.75 6.5a.25.25 0 0 0-.25.25v7.5c0 .14.11.25.25.25h7.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25h-7.5ZM5 1.75C5 .78 5.78 0 6.75 0h7.5C15.22 0 16 .78 16 1.75v7.5c0 .97-.78 1.75-1.75 1.75H13.5V9.5h.75a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25h-7.5a.25.25 0 0 0-.25.25V2.5H5v-.75Z" />
            </svg>
          </span>
        </div>
        <div className="dropdown-wrap file-more-wrap">
          <button
            type="button"
            className="icon-button file-more"
            aria-label="More options"
            aria-haspopup="true"
            aria-expanded="false"
            data-menu-button=""
            data-menu-target={`file-menu-${fileIdx}`}
            data-pr-action="show-stats"
          >
            <svg aria-hidden="true" viewBox="0 0 16 16">
              <path d="M8 9.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM1.5 9.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm13 0a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z" />
            </svg>
          </button>
          <div className="pr-menu file-menu" id={`file-menu-${fileIdx}`} role="menu" hidden>
            <button type="button" role="menuitem" data-pr-action="mark-reviewed" data-file-index={fileIdx}>
              Mark as viewed
            </button>
            <button type="button" role="menuitem" data-pr-action="toggle-file" data-file-index={fileIdx}>
              Collapse file
            </button>
            <button type="button" role="menuitem">
              Copy file path
            </button>
            <button type="button" role="menuitem">
              View file
            </button>
          </div>
        </div>
      </div>
      <div className="diff-table-wrap">
        <table className="diff-table" role="grid" aria-label={`Diff for file ${fileIdx + 1}`}>
          <colgroup>
            <col className="line-number-col" />
            <col className="line-number-col" />
            <col />
          </colgroup>
          <tbody>{tableRows(p, fileIdx, lines)}</tbody>
        </table>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export function GithubPrDiffPage({ data }: { data?: GithubPrDiffProps }) {
  const p = resolveProps(data);
  const fileCount = totalFiles(p);
  const lineCount = totalLines(p);
  const sections: React.ReactNode[] = [];
  for (let fileIdx = 0; fileIdx < fileCount; fileIdx++) {
    sections.push(<FileSection key={fileIdx} p={p} fileIdx={fileIdx} lines={fileLines(p, fileIdx)} />);
  }
  // The delegated interaction <script> (GITHUB_PR_DIFF_JS) is appended by the
  // document shell (wrapInGithubShell / the dump script) right before </body>,
  // matching the Proa page's trailing script.
  return (
    <div className="pr-shell" data-pr-shell="">
      <PrHeader
        p={p}
        fileCount={fileCount}
        lineCount={lineCount}
        additions={totalAdditions(p)}
        deletions={totalDeletions(p)}
      />
      <main className="pr-main">
        <ReviewToolbar p={p} fileCount={fileCount} lineCount={lineCount} />
        <div className="pr-layout">
          <FileTree p={p} />
          <section className="diffs" aria-label="Pull request diff">
            {sections}
          </section>
        </div>
      </main>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Byte-for-byte copies of the Rust CSS / JS consts (spliced verbatim from
// proa_demo/src/github_pr_diff.rs; do not reformat)
// ---------------------------------------------------------------------------

export const GITHUB_PR_DIFF_CSS = `
:root {
  color-scheme: light;
  --bg: #ffffff;
  --header: #24292f;
  --repo-bg: #f6f8fa;
  --panel: #ffffff;
  --subtle: #f6f8fa;
  --control: #f6f8fa;
  --border: #d0d7de;
  --border-soft: #d8dee4;
  --text: #24292f;
  --muted: #57606a;
  --accent: #0969da;
  --add-bg: #dafbe1;
  --add-num: #aceebb;
  --del-bg: #ffebe9;
  --del-num: #ffcecb;
  --hunk-bg: #ddf4ff;
}
* { box-sizing: border-box; }
html, body { margin: 0; min-height: 100%; background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif; }
button, a, input { font: inherit; }
a { color: inherit; text-decoration: none; }
.pr-shell { min-height: 100vh; }
.pr-header { position: relative; z-index: 20; border-bottom: 1px solid var(--border-soft); background: var(--bg); }
.app-header { display: flex; flex-direction: column; border-bottom: 1px solid var(--border-soft); background: #ffffff; }
.app-header-top { min-height: 64px; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 16px; }
.app-header-left, .app-header-center, .app-header-right, .app-action-group, .button-group, .app-quick-links { display: flex; align-items: center; gap: 8px; min-width: 0; }
.app-header-left { flex: 0 0 auto; }
.app-header-center { flex: 1 1 auto; justify-content: flex-start; overflow: hidden; }
.github-mark { width: 32px; height: 32px; display: inline-flex; align-items: center; justify-content: center; color: #24292f; flex: 0 0 auto; border-radius: 50%; }
.github-mark svg { width: 32px; height: 32px; fill: currentColor; }
.header-breadcrumbs { min-width: 0; max-width: 280px; overflow: hidden; }
.header-breadcrumbs ol { display: flex; align-items: center; gap: 0; margin: 0; padding: 0; list-style: none; min-width: 0; }
.header-breadcrumbs li { display: flex; align-items: center; min-width: 0; }
.header-breadcrumbs li + li::before { content: "/"; color: var(--muted); margin: 0 4px; }
.header-breadcrumb { display: inline-flex; align-items: center; min-width: 0; max-width: 150px; height: 32px; border: 0; border-radius: 6px; padding: 0 4px; color: var(--text); background: transparent; cursor: pointer; }
.header-breadcrumb span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.header-breadcrumbs li:last-child .header-breadcrumb span, .styles-module__contextCrumbLast__tI2e3 { font-weight: 600; }
.header-search-group { flex: 0 1 420px; min-width: 240px; margin-left: auto; align-self: center; }
.header-search-group > div:first-child { width: 100%; }
.app-header-button.header-search { width: 100%; min-width: 240px; height: 32px; justify-content: flex-start; border: 1px solid var(--border); border-radius: 6px; padding: 0 12px; color: var(--muted); background: #f6f8fa; font-size: 14px; }
.header-search .prc-Button-ButtonContent-Iohp5 { justify-content: flex-start; width: 100%; gap: 8px; }
.header-search .Search-module__value__TFoak { display: inline-flex; align-items: center; gap: 2px; color: var(--muted); }
.header-search kbd { display: inline-flex; align-items: center; justify-content: center; min-width: 18px; height: 20px; margin: 0 2px; border: 1px solid var(--border); border-radius: 4px; background: #ffffff; color: var(--muted); font-size: 12px; font-family: inherit; }
.app-header-right { flex: 0 0 auto; }
.app-header-button, .avatar-button { min-width: 32px; height: 32px; display: inline-flex; align-items: center; justify-content: center; gap: 4px; border: 1px solid transparent; border-radius: 6px; padding: 0 7px; color: var(--muted); background: transparent; cursor: pointer; }
.app-header-button:hover, .avatar-button:hover, .header-breadcrumb:hover, .github-mark:hover { background: #f6f8fa; color: var(--text); }
.app-header-button svg { width: 16px; height: 16px; fill: currentColor; flex: 0 0 auto; }
.app-header-button .caret, .app-header-button .octicon-triangle-down { width: 12px; height: 12px; }
.icon-only { padding: 0; }
.icon-with-caret { padding: 0 6px; }
.button-group { gap: 0; border: 1px solid var(--border); border-radius: 6px; }
.button-group .app-header-button { border-radius: 0; }
.button-group > :first-child .app-header-button, .button-group > .app-header-button:first-child { border-radius: 6px 0 0 6px; }
.button-group > :last-child .app-header-button, .button-group > .app-header-button:last-child { border-radius: 0 6px 6px 0; border-left-color: var(--border); }
.app-divider { width: 1px; height: 20px; background: var(--border); margin: 0 4px; }
.avatar-button { padding: 0; border-radius: 50%; overflow: hidden; }
.avatar-button img { width: 32px; height: 32px; display: block; border-radius: 50%; }
.notification-button { position: relative; }
.notification-button::after { content: ""; position: absolute; top: 5px; right: 5px; width: 8px; height: 8px; border: 2px solid #ffffff; border-radius: 50%; background: #0969da; }
.dropdown-wrap { position: relative; display: inline-flex; align-items: center; }
.pr-menu { position: absolute; top: calc(100% + 6px); left: 0; z-index: 100; min-width: 220px; border: 1px solid var(--border); border-radius: 6px; padding: 6px 0; background: #ffffff; box-shadow: 0 8px 24px rgba(140, 149, 159, 0.2); color: var(--text); font-size: 14px; }
.pr-menu[hidden] { display: none; }
.pr-menu.right-aligned, .app-header-right .pr-menu, .file-menu { left: auto; right: 0; }
.pr-menu button { width: 100%; min-height: 32px; display: flex; align-items: center; border: 0; border-radius: 0; padding: 6px 12px; color: var(--text); background: transparent; text-align: left; cursor: pointer; white-space: nowrap; }
.pr-menu button:hover { background: #0969da; color: #ffffff; }
.local-navigation, .meta-row, .tabs, .review-toolbar, .toolbar-left, .toolbar-right, .tree-toggle-group, .toolbar-button, .viewed-progress, .perf-strip, .file-header, .file-title, .file-change-map, .file-change-bars, .diff-summary, .diff-squares, .PRIVATE_TreeView-item-container { display: flex; align-items: center; }
.toolbar-left button, .file-header button, .hunk-row button, .new-issue-button { cursor: pointer; color: var(--text); background: var(--control); border: 1px solid var(--border); border-radius: 6px; }
.slash, .meta-row, .toolbar-muted, .rendered-count { color: var(--muted); }
.badge { border: 1px solid var(--border); border-radius: 999px; padding: 1px 7px; color: var(--muted); font-size: 12px; margin-left: 6px; }
.local-navigation { min-height: 48px; padding: 0 32px; overflow: hidden; }
.local-nav-list { display: flex; align-items: center; gap: 2px; margin: 0; padding: 0; list-style: none; list-style-type: none; min-width: 0; overflow-x: auto; }
.local-nav-list > li { list-style: none; list-style-type: none; }
.local-nav-item { display: inline-flex; align-items: center; gap: 6px; min-height: 48px; color: var(--text); border-bottom: 2px solid transparent; padding: 0 10px; font-size: 14px; white-space: nowrap; }
.local-nav-item.active, .local-nav-item[aria-current="page"] { color: var(--text); border-bottom-color: #fd8c73; font-weight: 600; }
.local-nav-counter, .tab-count { color: var(--text); background: #afb8c133; border-radius: 999px; padding: 1px 6px; font-size: 12px; font-weight: 400; }
.repo-tab-icon { width: 16px; height: 16px; display: inline-flex; color: var(--muted) !important; background: transparent !important; border-radius: 0 !important; padding: 0 !important; }
.repo-tab-icon svg { width: 16px; height: 16px; fill: currentColor; }
.pr-title-block { position: relative; padding: 20px 32px 0; background: #ffffff; }
.title-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; min-width: 0; }
h1 { margin: 0; min-width: 0; font-size: 32px; line-height: 1.25; font-weight: 400; color: var(--text); }
h1 span { color: var(--muted); font-weight: 300; }
.new-issue-button { height: 30px; padding: 0 12px; color: #ffffff; background: #1f883d; border-color: #1f883d; font-size: 12px; font-weight: 600; flex: 0 0 auto; }
.status-pill { display: inline-flex; align-items: center; gap: 4px; height: 32px; border-radius: 999px; padding: 0 12px; color: #fff; font-size: 14px; font-weight: 700; margin-right: 4px; }
.status-pill svg { width: 14px; height: 14px; fill: currentColor; flex: 0 0 auto; }
.status-open { background: #1a7f37; }
.status-closed { background: #cf222e; }
.meta-row { flex-wrap: wrap; gap: 4px; margin-top: 8px; font-size: 14px; color: var(--muted); }
.meta-row strong { color: var(--muted); }
code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace; }
.meta-row code { color: var(--accent); background: #ddf4ff; border-radius: 6px; padding: 2px 5px; font-size: 12px; }
.diff-summary { margin-left: auto; gap: 8px; font-size: 12px; font-weight: 700; }
.diff-squares { gap: 2px; }
.square { width: 8px; height: 8px; border-radius: 1px; background: #d8dee4; }
.add-square { background: #2da44e; }
.neutral-square { background: #8c959f; }
.tabs { gap: 0; margin-top: 16px; padding: 0 32px; border-bottom: 1px solid var(--border); align-items: flex-end; }
.tabs a { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; color: var(--muted); border: 1px solid transparent; border-bottom: 0; padding: 0 14px; font-size: 14px; white-space: nowrap; margin-bottom: -1px; }
.tabs a.active { color: var(--text); border-color: var(--border); border-bottom-color: #ffffff; border-radius: 6px 6px 0 0; background: #ffffff; font-weight: 500; }
.tab-icon { width: 16px; height: 16px; display: inline-flex; color: var(--muted); background: transparent !important; border-radius: 0 !important; padding: 0 !important; }
.tab-icon svg { width: 16px; height: 16px; fill: currentColor; }
.toolbar-left button:hover, .file-header button:hover { filter: brightness(0.97); color: var(--text); }
.new-issue-button:hover { filter: brightness(0.95); color: #ffffff; }
.pr-main { padding: 0 32px 40px; }
.pr-layout { display: grid; grid-template-columns: minmax(260px, 296px) minmax(0, 1fr); gap: 16px; }
.file-tree { position: sticky; top: 64px; align-self: start; max-height: calc(100vh - 80px); overflow: hidden; background: var(--panel); }
.pr-shell.file-tree-collapsed .pr-layout { grid-template-columns: minmax(0, 1fr); }
.pr-shell.file-tree-collapsed .file-tree { display: none; }
.file-tree-controls { display: grid; grid-template-columns: 1fr; gap: 8px; padding: 0 0 8px; background: var(--panel); }
.tree-filter-label { position: relative; display: block; }
.tree-filter-label input { width: 100%; height: 32px; border: 1px solid var(--border); border-radius: 6px; padding: 0 10px 0 32px; color: var(--text); background: #ffffff; outline: none; }
.tree-filter-label input:focus { border-color: #0969da; box-shadow: 0 0 0 3px #0969da33; }
.tree-filter-icon { position: absolute; left: 10px; top: 8px; width: 16px; height: 16px; color: var(--muted); pointer-events: none; }
.tree-filter-icon svg { width: 16px; height: 16px; fill: currentColor; }
.PullRequestFileTree-module__FileTreeScrollable__OW_Y2 { max-height: calc(100vh - 120px); overflow: auto; padding: 4px 0; }
.tmp-pr-lg-3.prc-TreeView-TreeViewRootUlStyles-Mzrmj { display: grid; gap: 1px; margin: 0; padding: 0; list-style: none; font-size: 14px; line-height: 20px; }
.PRIVATE_TreeView-item { min-width: 0; list-style: none; }
.PRIVATE_TreeView-item-container { min-width: 0; gap: 6px; min-height: 32px; width: 100%; border: 0; border-radius: 6px; padding: 5px 8px 5px calc(4px + (var(--level) - 1) * 16px); color: var(--text); background: transparent; text-align: left; }
.tree-directory-button { cursor: pointer; }
.file-tree-link { color: var(--muted); }
.PRIVATE_TreeView-item-container:hover, .file-tree-file:first-of-type .file-tree-link { background: var(--subtle); color: var(--text); }
.file-tree-directory .PRIVATE_TreeView-item-container { color: var(--text); font-weight: 400; }
.tree-chevron, .tree-spacer { width: 14px; height: 14px; display: inline-flex; flex: 0 0 auto; color: var(--muted); }
.tree-chevron { transform: rotate(90deg); }
.file-tree-directory.is-collapsed .tree-chevron { transform: rotate(0deg); }
.tree-icon, .file-kind, .file-handle { width: 16px; height: 16px; display: inline-flex; flex: 0 0 auto; color: var(--muted); }
.folder-icon { color: #54aeef; }
.tree-icon svg, .file-kind svg, .file-handle svg, .icon-button svg, .file-more svg, .tree-chevron svg { width: 16px; height: 16px; fill: currentColor; }
.tree-label, .file-path { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
.sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); border: 0; }
.add { color: #1a7f37; }
.del { color: #cf222e; }
.diffs { min-width: 0; }
.review-toolbar { position: sticky; top: 0; z-index: 30; justify-content: space-between; gap: 16px; min-height: 48px; border-bottom: 1px solid var(--border); padding: 8px 0; margin-bottom: 16px; background: #ffffff; color: var(--muted); font-size: 13px; }
.review-toolbar.is-stuck { box-shadow: 0 1px 2px rgba(31, 35, 40, 0.08); }
.toolbar-left, .toolbar-right { gap: 8px; min-width: 0; }
.toolbar-right { flex: 0 0 auto; }
.tree-toggle-group { gap: 0; }
.tree-toggle-group [hidden] { display: none !important; }
.review-toolbar .status-pill, .toolbar-sticky-title { display: none; }
.review-toolbar.is-stuck .status-pill { display: inline-flex; height: 28px; font-size: 12px; padding: 0 10px; }
.review-toolbar.is-stuck .toolbar-sticky-title { display: flex; align-items: center; gap: 6px; min-width: 0; max-width: 420px; color: var(--text); }
.toolbar-sticky-title strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; }
.toolbar-sticky-title span { color: var(--muted); font-size: 14px; }
.review-toolbar .toolbar-link { height: 32px; padding: 0 4px; font-size: 13px; font-weight: 600; color: var(--muted); background: transparent; border-color: transparent; }
.review-toolbar .toolbar-button { gap: 6px; height: 32px; padding: 0 10px; border: 1px solid var(--border); border-radius: 6px; background: #ffffff; color: var(--text); font-size: 13px; font-weight: 600; cursor: pointer; }
.review-toolbar .toolbar-link, .review-toolbar .toolbar-button { flex: 0 0 auto; white-space: nowrap; }
.toolbar-menu { display: inline-flex; align-items: center; gap: 5px; }
.toolbar-menu > span { font-size: 13px; }
.toolbar-menu .button-leading, .toolbar-menu .button-trailing { width: 16px; height: 16px; display: inline-flex; align-items: center; justify-content: center; color: inherit; }
.toolbar-menu .button-trailing svg { width: 12px; height: 12px; }
.toolbar-menu-panel { min-width: 240px; }
.review-toolbar .toolbar-icon { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 32px; padding: 0; color: var(--muted); border: 0; border-radius: 6px; background: transparent; cursor: pointer; }
.review-toolbar .toolbar-icon.boxed { border: 1px solid var(--border); background: #ffffff; color: var(--text); }
.toolbar-icon svg, .toolbar-button svg, .viewed-progress svg, .comments-button svg { width: 16px; height: 16px; fill: currentColor; flex: 0 0 auto; }
.viewed-progress { gap: 6px; color: var(--muted); white-space: nowrap; }
.viewed-progress svg { transform: rotate(-90deg); }
.toolbar-divider { width: 1px; height: 20px; background: var(--border); }
.review-toolbar .review-primary { color: #ffffff; background: #1f883d; border-color: #1f883d; }
.comments-button { padding: 0 8px; }
.comment-count { min-width: 20px; padding: 1px 6px; border-radius: 999px; background: #afb8c133; color: var(--text); font-size: 12px; text-align: center; }
.perf-strip { display: none; position: sticky; top: 48px; z-index: 10; gap: 16px; padding: 7px 12px; border: 1px solid var(--border); border-top: 0; border-radius: 0 0 6px 6px; background: #ffffff; color: var(--muted); font-size: 12px; }
.perf-strip strong { color: var(--text); }
.diff-file { content-visibility: auto; contain-intrinsic-size: auto var(--intrinsic-height); border: 1px solid var(--border); border-radius: 6px; background: var(--panel); margin-top: 0; overflow: clip; scroll-margin-top: 72px; }
.diff-file + .diff-file { margin-top: 14px; }
.diff-file.is-collapsed .diff-table { display: none; }
.diff-file.is-reviewed .file-header { background: #f0fff4; }
.file-header { position: sticky; top: 48px; z-index: 5; gap: 8px; min-height: 40px; padding: 6px 10px; border-bottom: 1px solid var(--border); background: var(--subtle); }
.file-change-map { gap: 7px; color: var(--muted); font-size: 12px; font-weight: 600; flex: 0 0 auto; }
.file-change-bars { gap: 2px; }
.change-square { width: 8px; height: 8px; border-radius: 1px; border: 1px solid var(--border-soft); background: #ebedef; }
.change-square.add-square { background: #2da44e; border-color: #2da44e; }
.del-square { background: #cf222e; border-color: #cf222e; }
.empty-square { background: #ebedef; }
.file-title { min-width: 0; flex: 1; gap: 6px; padding-left: 2px; }
.file-header h2 { min-width: 0; flex: 0 1 auto; max-width: 100%; margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); font-size: 13px; font-weight: 600; }
.copy-icon { width: 16px; height: 16px; display: inline-flex; color: var(--muted); flex: 0 0 auto; }
.copy-icon svg { width: 16px; height: 16px; fill: currentColor; }
.icon-button { width: 26px; height: 28px; display: inline-flex; align-items: center; justify-content: center; padding: 0; background: transparent !important; color: var(--muted); border-color: transparent !important; }
.file-more-wrap { margin-left: auto; }
.file-menu { min-width: 180px; }
.path-shard { color: var(--muted); }
.diff-table-wrap { background: var(--panel); }
.diff-table { width: 100%; border-collapse: collapse; table-layout: fixed; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace; font-size: 12px; line-height: 20px; }
.line-number-col { width: 50px; }
.diff-row { cursor: default; }
.diff-row:hover .code, .diff-row:hover .line-number { filter: brightness(0.98); }
.diff-row.is-selected .code, .diff-row.is-selected .line-number { outline: 1px solid #0969da; outline-offset: -1px; }
.line-number { color: var(--muted); text-align: right; padding: 0 8px; user-select: none; font-variant-numeric: tabular-nums; }
.empty-diff-line { background: var(--subtle); }
.diff-line-number-neutral { background: var(--panel); }
.blob-num-addition { background: var(--add-num); color: var(--text); }
.blob-num-deletion { background: var(--del-num); color: var(--text); }
.code { color: var(--text); white-space: pre; overflow: hidden; text-overflow: ellipsis; padding: 0; }
.diff-text-cell { position: relative; overflow: visible; }
.line-comment-button { position: absolute; left: -28px; top: 50%; z-index: 3; width: 24px; height: 24px; display: none; align-items: center; justify-content: center; border: 1px solid #0969da; border-radius: 6px; padding: 0; color: #ffffff; background: #0969da; box-shadow: 0 2px 6px rgba(31, 35, 40, 0.18); cursor: pointer; transform: translateY(-50%); font-size: 18px; font-weight: 600; line-height: 1; }
.line-comment-button:hover, .line-comment-button:focus-visible { background: #0550ae; border-color: #0550ae; outline: none; }
.line-comment-plus { display: block; transform: translateY(-1px); }
.diff-row:hover .line-comment-button, .line-comment-button:focus-visible, .diff-row.is-commenting .line-comment-button { display: inline-flex; }
.diff-text { display: block; position: relative; min-height: 20px; padding: 0 24px; overflow: hidden; text-overflow: ellipsis; }
.diff-text.addition { background: var(--add-bg); }
.diff-text.deletion { background: var(--del-bg); }
.line-marker { position: absolute; left: 8px; color: var(--muted); user-select: none; }
.code-inner { display: inline-block; padding-left: 6px; }
.pl-k { color: #cf222e; }
.pl-k.old { color: #cf222e; }
.pl-s1, .pl-kos { color: #24292f; }
.pl-c1 { color: #0550ae; }
.pl-en { color: #8250df; }
.pl-s { color: #0a3069; }
.pl-c { color: #6e7781; }
.comment { color: #6e7781; }
.hunk-row td { background: var(--hunk-bg); color: var(--muted); padding: 0; }
.hunk-control { background: #b6e3ff !important; text-align: center; }
.hunk-control button { width: 100%; min-height: 20px; display: inline-flex; align-items: center; justify-content: center; color: var(--muted); padding: 0; border: 0; border-radius: 0; background: transparent; }
.hunk-control svg { width: 14px; height: 14px; fill: currentColor; }
.hunk-code code { display: block; overflow: hidden; text-overflow: ellipsis; white-space: pre; padding: 0 22px; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace; font-size: 12px; line-height: 20px; }
.inline-comment-row td { padding: 0; background: #ffffff; border-top: 1px solid var(--border-soft); border-bottom: 1px solid var(--border-soft); }
.inline-comment-shell { display: flex; align-items: flex-start; gap: 8px; padding: 8px 24px 8px 100px; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif; white-space: normal; }
.inline-comment-card { flex: 1 1 auto; min-width: 0; border: 1px solid var(--border); border-radius: 6px; background: #ffffff; box-shadow: 0 1px 2px rgba(31, 35, 40, 0.04); }
.inline-comment-header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 8px 10px; border-bottom: 1px solid var(--border-soft); background: #f6f8fa; }
.inline-comment-title { display: flex; align-items: center; gap: 8px; margin: 0; color: var(--text); font-size: 14px; font-weight: 600; line-height: 24px; }
.inline-comment-avatar { width: 24px; height: 24px; border-radius: 50%; }
.inline-comment-editor { padding: 0; }
.inline-comment-tabs { display: flex; align-items: center; gap: 0; padding: 8px 8px 0; border-bottom: 1px solid var(--border-soft); }
.inline-comment-tab { min-height: 32px; border: 1px solid transparent; border-bottom: 0; border-radius: 6px 6px 0 0; padding: 0 12px; color: var(--muted); background: transparent; font-size: 13px; font-weight: 600; cursor: pointer; }
.inline-comment-tab.is-active { color: var(--text); border-color: var(--border-soft); background: #ffffff; margin-bottom: -1px; }
.inline-comment-toolbar { display: flex; align-items: center; gap: 2px; padding: 8px; color: var(--muted); }
.inline-comment-tool { width: 30px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: 0; border-radius: 6px; color: var(--muted); background: transparent; cursor: pointer; }
.inline-comment-tool:hover { background: #f6f8fa; color: var(--text); }
.inline-comment-tool svg { width: 16px; height: 16px; fill: currentColor; }
.inline-comment-divider { width: 1px; height: 20px; margin: 0 4px; background: var(--border-soft); }
.inline-comment-textarea { display: block; width: calc(100% - 16px); min-height: 120px; margin: 0 8px 8px; border: 1px solid var(--border); border-radius: 6px; padding: 8px; color: var(--text); background: #ffffff; font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif; resize: vertical; }
.inline-comment-textarea:focus { border-color: #0969da; outline: none; box-shadow: 0 0 0 3px #0969da33; }
.inline-comment-footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 8px; border-top: 1px solid var(--border-soft); }
.inline-comment-upload { display: inline-flex; align-items: center; gap: 6px; border: 0; color: var(--muted); background: transparent; font-size: 12px; cursor: pointer; }
.inline-comment-upload svg { width: 16px; height: 16px; fill: currentColor; }
.inline-comment-actions { display: flex; align-items: center; gap: 8px; }
.inline-comment-action { min-height: 30px; border: 1px solid var(--border); border-radius: 6px; padding: 0 10px; color: var(--text); background: #f6f8fa; font-size: 12px; font-weight: 600; cursor: pointer; }
.inline-comment-action.primary { color: #ffffff; border-color: #1f883d; background: #1f883d; }
.inline-comment-action[disabled] { color: var(--muted); background: #f6f8fa; cursor: default; opacity: 0.65; }
.inline-comment-close { flex: 0 0 auto; width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; margin-top: 2px; border: 1px solid var(--border); border-radius: 6px; padding: 0; color: var(--muted); background: #ffffff; cursor: pointer; }
.inline-comment-close svg { width: 16px; height: 16px; fill: currentColor; }
.persisted-comment-row .inline-comment-card { background: #ffffff; }
.persisted-comment-body { padding: 10px; color: var(--text); font-size: 14px; line-height: 1.5; white-space: pre-wrap; }
@media (max-width: 980px) {
  .pr-main { padding: 0 12px 40px; }
  .pr-layout { grid-template-columns: 1fr; }
  .local-navigation, .pr-title-block { padding-left: 16px; padding-right: 16px; }
  .app-header-top { flex-wrap: wrap; padding: 8px 16px; }
  .app-header-center { order: 3; flex-basis: 100%; }
  .local-nav-list { overflow-x: auto; }
  .diff-summary { position: static; margin-top: 10px; }
  .file-tree { position: static; max-height: 280px; }
  .review-toolbar, .perf-strip, .file-header { position: static; }
  .toolbar-left, .toolbar-right, .review-toolbar { flex-wrap: wrap; }
}
`;

export const GITHUB_PR_DIFF_JS = `
(function () {
  const shell = document.querySelector('[data-pr-shell]');
  const stats = document.querySelector('[data-pr-stats]');
  const toolbar = document.querySelector('.review-toolbar');
  const tree = document.querySelector('[role="tree"]');
  const commentStorageKey = 'proa.github-pr-diff.comments.v1';
  const commentDraftPrefix = 'proa.github-pr-diff.comment-draft.';
  const avatarUrl = 'https://github.githubassets.com/images/gravatars/gravatar-user-420.png';
  let selected = null;

  const toolbarIcons = {
    suggestion: 'M1 1.75C1 .784 1.784 0 2.75 0h7.586c.464 0 .909.184 1.237.513l2.914 2.914c.329.328.513.773.513 1.237v9.586A1.75 1.75 0 0 1 13.25 16H2.75A1.75 1.75 0 0 1 1 14.25Zm1.5 0v12.5c0 .138.112.25.25.25h10.5a.25.25 0 0 0 .25-.25V4.664a.25.25 0 0 0-.073-.177l-2.914-2.914a.25.25 0 0 0-.177-.073ZM8 3.25a.75.75 0 0 1 .75.75v1.5h1.5a.75.75 0 0 1 0 1.5h-1.5v1.5a.75.75 0 0 1-1.5 0V7h-1.5a.75.75 0 0 1 0-1.5h1.5V4A.75.75 0 0 1 8 3.25Zm-3 8a.75.75 0 0 1 .75-.75h4.5a.75.75 0 0 1 0 1.5h-4.5a.75.75 0 0 1-.75-.75Z',
    heading: 'M3.75 2a.75.75 0 0 1 .75.75V7h7V2.75a.75.75 0 0 1 1.5 0v10.5a.75.75 0 0 1-1.5 0V8.5h-7v4.75a.75.75 0 0 1-1.5 0V2.75A.75.75 0 0 1 3.75 2Z',
    bold: 'M4 2h4.5a3.501 3.501 0 0 1 2.852 5.53A3.499 3.499 0 0 1 9.5 14H4a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1Zm1 7v3h4.5a1.5 1.5 0 0 0 0-3Zm3.5-2a1.5 1.5 0 0 0 0-3H5v3Z',
    italic: 'M6 2.75A.75.75 0 0 1 6.75 2h6.5a.75.75 0 0 1 0 1.5h-2.505l-3.858 9H9.25a.75.75 0 0 1 0 1.5h-6.5a.75.75 0 0 1 0-1.5h2.505l3.858-9H6.75A.75.75 0 0 1 6 2.75Z',
    quote: 'M1.75 2.5h10.5a.75.75 0 0 1 0 1.5H1.75a.75.75 0 0 1 0-1.5Zm4 5h8.5a.75.75 0 0 1 0 1.5h-8.5a.75.75 0 0 1 0-1.5Zm0 5h8.5a.75.75 0 0 1 0 1.5h-8.5a.75.75 0 0 1 0-1.5ZM2.5 7.75v6a.75.75 0 0 1-1.5 0v-6a.75.75 0 0 1 1.5 0Z',
    code: 'm11.28 3.22 4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.749.749 0 0 1-1.275-.326.749.749 0 0 1 .215-.734L13.94 8l-3.72-3.72a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215Zm-6.56 0a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042L2.06 8l3.72 3.72a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215L.47 8.53a.75.75 0 0 1 0-1.06Z',
    link: 'm7.775 3.275 1.25-1.25a3.5 3.5 0 1 1 4.95 4.95l-2.5 2.5a3.5 3.5 0 0 1-4.95 0 .751.751 0 0 1 .018-1.042.751.751 0 0 1 1.042-.018 1.998 1.998 0 0 0 2.83 0l2.5-2.5a2.002 2.002 0 0 0-2.83-2.83l-1.25 1.25a.751.751 0 0 1-1.042-.018.751.751 0 0 1-.018-1.042Zm-4.69 9.64a1.998 1.998 0 0 0 2.83 0l1.25-1.25a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042l-1.25 1.25a3.5 3.5 0 1 1-4.95-4.95l2.5-2.5a3.5 3.5 0 0 1 4.95 0 .751.751 0 0 1-.018 1.042.751.751 0 0 1-1.042.018 1.998 1.998 0 0 0-2.83 0l-2.5 2.5a1.998 1.998 0 0 0 0 2.83Z',
    unordered: 'M5.75 2.5h8.5a.75.75 0 0 1 0 1.5h-8.5a.75.75 0 0 1 0-1.5Zm0 5h8.5a.75.75 0 0 1 0 1.5h-8.5a.75.75 0 0 1 0-1.5Zm0 5h8.5a.75.75 0 0 1 0 1.5h-8.5a.75.75 0 0 1 0-1.5ZM2 14a1 1 0 1 1 0-2 1 1 0 0 1 0 2Zm1-6a1 1 0 1 1-2 0 1 1 0 0 1 2 0ZM2 4a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z',
    kebab: 'M8 9a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM1.5 9a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm13 0a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z',
    undo: 'M1.22 6.28a.749.749 0 0 1 0-1.06l3.5-3.5a.749.749 0 1 1 1.06 1.06L3.561 5h7.188l.001.007L10.749 5c.058 0 .116.007.171.019A4.501 4.501 0 0 1 10.5 14H8.796a.75.75 0 0 1 0-1.5H10.5a3 3 0 1 0 0-6H3.561L5.78 8.72a.749.749 0 1 1-1.06 1.06l-3.5-3.5Z',
    paperclip: 'M12.212 3.02a1.753 1.753 0 0 0-2.478.003l-5.83 5.83a3.007 3.007 0 0 0-.88 2.127c0 .795.315 1.551.88 2.116.567.567 1.333.89 2.126.89.79 0 1.548-.321 2.116-.89l5.48-5.48a.75.75 0 0 1 1.061 1.06l-5.48 5.48a4.492 4.492 0 0 1-3.177 1.33c-1.2 0-2.345-.487-3.187-1.33a4.483 4.483 0 0 1-1.32-3.177c0-1.195.475-2.341 1.32-3.186l5.83-5.83a3.25 3.25 0 0 1 5.553 2.297c0 .863-.343 1.691-.953 2.301L7.439 12.39c-.375.377-.884.59-1.416.593a1.998 1.998 0 0 1-1.412-.593a1.992 1.992 0 0 1 0-2.828l5.48-5.48a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042l-5.48 5.48a.492.492 0 0 0 0 .707.499.499 0 0 0 .352.154.51.51 0 0 0 .356-.154l5.833-5.827a1.755 1.755 0 0 0 0-2.481Z'
  };

  function readComments() {
    try {
      const parsed = JSON.parse(localStorage.getItem(commentStorageKey) || '[]');
      return Array.isArray(parsed) ? parsed : [];
    } catch (_) {
      return [];
    }
  }

  function writeComments(comments) {
    try {
      localStorage.setItem(commentStorageKey, JSON.stringify(comments));
    } catch (_) {}
  }

  function draftKey(target) {
    return commentDraftPrefix + target.fileIndex + ':' + target.lineIndex + ':' + target.side + ':' + target.line;
  }

  function readDraft(target) {
    try {
      return localStorage.getItem(draftKey(target)) || '';
    } catch (_) {
      return '';
    }
  }

  function writeDraft(target, value) {
    try {
      localStorage.setItem(draftKey(target), value);
    } catch (_) {}
  }

  function clearDraft(target) {
    try {
      localStorage.removeItem(draftKey(target));
    } catch (_) {}
  }

  function targetFromButton(button) {
    const row = button.closest('[data-line]');
    return {
      row,
      fileIndex: button.getAttribute('data-file-index') || '',
      lineIndex: button.getAttribute('data-line-index') || '',
      side: button.getAttribute('data-comment-side') || 'R',
      line: button.getAttribute('data-comment-line') || '',
      key: (button.getAttribute('data-file-index') || '') + ':' + (button.getAttribute('data-line-index') || '') + ':' + (button.getAttribute('data-comment-side') || 'R') + ':' + (button.getAttribute('data-comment-line') || '')
    };
  }

  function iconButton(label, path) {
    return '<button type="button" class="inline-comment-tool" aria-label="' + label + '"><svg aria-hidden="true" viewBox="0 0 16 16"><path d="' + path + '"></path></svg></button>';
  }

  function commentToolbarHtml() {
    return [
      iconButton('Add a suggestion', toolbarIcons.suggestion),
      iconButton('Heading', toolbarIcons.heading),
      iconButton('Bold', toolbarIcons.bold),
      iconButton('Italic', toolbarIcons.italic),
      iconButton('Quote', toolbarIcons.quote),
      iconButton('Code', toolbarIcons.code),
      iconButton('Link', toolbarIcons.link),
      '<span class="inline-comment-divider"></span>',
      iconButton('Unordered list', toolbarIcons.unordered),
      iconButton('More formatting tools', toolbarIcons.kebab)
    ].join('');
  }

  function closeCommentEditor() {
    document.querySelectorAll('.diff-row.is-commenting').forEach(function (row) {
      row.classList.remove('is-commenting');
    });
    document.querySelectorAll('[data-inline-comment-editor]').forEach(function (row) {
      row.remove();
    });
  }

  function findLastInlineRow(row, key) {
    let target = row;
    let next = target.nextElementSibling;
    while (next && next.classList.contains('inline-comment-row') && next.getAttribute('data-comment-key') === key) {
      target = next;
      next = target.nextElementSibling;
    }
    return target;
  }

  function insertPersistedComment(target, comment) {
    if (!target.row || document.querySelector('[data-comment-id="' + comment.id + '"]')) return;
    const row = document.createElement('tr');
    row.className = 'inline-comment-row persisted-comment-row';
    row.setAttribute('data-comment-key', target.key);
    row.setAttribute('data-comment-id', comment.id);

    const cell = document.createElement('td');
    cell.colSpan = 3;
    cell.innerHTML =
      '<div class="inline-comment-shell" data-inline-markers="true">' +
        '<div class="inline-comment-card">' +
          '<div class="inline-comment-header">' +
            '<h4 class="inline-comment-title"><img class="inline-comment-avatar" alt="demo-user" width="24" height="24"><span data-comment-title></span></h4>' +
          '</div>' +
          '<div class="persisted-comment-body" data-comment-body></div>' +
        '</div>' +
      '</div>';
    row.appendChild(cell);
    cell.querySelector('img').src = avatarUrl;
    cell.querySelector('[data-comment-title]').textContent = (comment.mode === 'review' ? 'Pending review comment on line ' : 'Comment on line ') + target.side + target.line;
    cell.querySelector('[data-comment-body]').textContent = comment.body;
    findLastInlineRow(target.row, target.key).after(row);
  }

  function persistComment(target, body, mode) {
    const text = body.trim();
    if (!text) return false;
    const comments = readComments();
    const comment = {
      id: 'comment-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2),
      key: target.key,
      fileIndex: target.fileIndex,
      lineIndex: target.lineIndex,
      side: target.side,
      line: target.line,
      body: text,
      mode,
      createdAt: new Date().toISOString()
    };
    comments.push(comment);
    writeComments(comments);
    clearDraft(target);
    closeCommentEditor();
    insertPersistedComment(target, comment);
    return true;
  }

  function openCommentEditor(button) {
    const target = targetFromButton(button);
    if (!target.row) return;
    closeCommentEditor();
    target.row.classList.add('is-commenting');

    const row = document.createElement('tr');
    row.className = 'inline-comment-row active-comment-row';
    row.setAttribute('data-inline-comment-editor', 'true');
    row.setAttribute('data-comment-key', target.key);

    const cell = document.createElement('td');
    cell.colSpan = 3;
    cell.innerHTML =
      '<div class="inline-comment-shell InlineMarkers-module__markersDialogActive__gXOXF" aria-hidden="false" data-inline-markers="true">' +
        '<div class="inline-comment-card">' +
          '<div class="inline-comment-header">' +
            '<h4 class="inline-comment-title"><img class="inline-comment-avatar" alt="demo-user" width="24" height="24"><span data-comment-title></span></h4>' +
          '</div>' +
          '<div class="inline-comment-editor">' +
            '<div class="inline-comment-tabs"><button type="button" class="inline-comment-tab is-active">Write</button><button type="button" class="inline-comment-tab">Preview</button></div>' +
            '<div class="inline-comment-toolbar" role="toolbar" aria-label="Formatting tools"></div>' +
            '<textarea class="inline-comment-textarea" rows="5" placeholder="Leave a comment" aria-label="Markdown value"></textarea>' +
            '<div class="inline-comment-footer">' +
              '<button type="button" class="inline-comment-upload"><svg aria-hidden="true" viewBox="0 0 16 16"><path d="' + toolbarIcons.paperclip + '"></path></svg><span>Paste, drop, or click to add files</span></button>' +
              '<div class="inline-comment-actions"><button type="button" class="inline-comment-action" data-comment-cancel>Cancel</button><button type="button" class="inline-comment-action" data-comment-submit>Comment</button><button type="button" class="inline-comment-action primary" data-comment-review>Start a review</button></div>' +
            '</div>' +
          '</div>' +
        '</div>' +
        '<button type="button" class="inline-comment-close" aria-label="Return to code" data-comment-close><svg aria-hidden="true" viewBox="0 0 16 16"><path d="' + toolbarIcons.undo + '"></path></svg></button>' +
      '</div>';
    row.appendChild(cell);

    const textarea = cell.querySelector('textarea');
    const submit = cell.querySelector('[data-comment-submit]');
    const review = cell.querySelector('[data-comment-review]');
    const updateButtons = function () {
      const disabled = textarea.value.trim().length === 0;
      submit.disabled = disabled;
      review.disabled = disabled;
    };

    cell.querySelector('img').src = avatarUrl;
    cell.querySelector('[data-comment-title]').textContent = 'Add a comment on line ' + target.side + target.line;
    cell.querySelector('.inline-comment-toolbar').innerHTML = commentToolbarHtml();
    textarea.value = readDraft(target);
    textarea.addEventListener('input', function () {
      writeDraft(target, textarea.value);
      updateButtons();
    });
    cell.querySelector('[data-comment-cancel]').addEventListener('click', closeCommentEditor);
    cell.querySelector('[data-comment-close]').addEventListener('click', closeCommentEditor);
    submit.addEventListener('click', function () {
      if (!persistComment(target, textarea.value, 'comment')) textarea.focus();
    });
    review.addEventListener('click', function () {
      if (!persistComment(target, textarea.value, 'review')) textarea.focus();
    });

    target.row.after(row);
    updateButtons();
    textarea.focus();
  }

  function restorePersistedComments() {
    readComments().forEach(function (comment) {
      const row = document.querySelector('[data-line][data-file-index="' + comment.fileIndex + '"][data-line-index="' + comment.lineIndex + '"]');
      if (!row) return;
      insertPersistedComment({
        row,
        fileIndex: comment.fileIndex,
        lineIndex: comment.lineIndex,
        side: comment.side,
        line: comment.line,
        key: comment.key
      }, comment);
    });
  }

  function refreshStats() {
    if (!stats) return;
    const rows = document.querySelectorAll('.diff-line-row').length;
    const nodes = document.getElementsByTagName('*').length;
    const rowTarget = stats.querySelector('[data-stat="rows"]');
    const nodeTarget = stats.querySelector('[data-stat="nodes"]');
    if (rowTarget) rowTarget.textContent = rows.toLocaleString();
    if (nodeTarget) nodeTarget.textContent = nodes.toLocaleString();
  }

  function sectionFor(target) {
    const index = target && target.getAttribute('data-file-index');
    if (index == null || index === '') return null;
    return document.querySelector('[data-file-section][data-file-index="' + index + '"]');
  }

  function setSelection(row) {
    if (selected) selected.classList.remove('is-selected');
    selected = row;
    if (selected) selected.classList.add('is-selected');
    const target = stats && stats.querySelector('[data-stat="selection"]');
    if (target && row) {
      target.textContent = 'Selected file ' + row.getAttribute('data-file-index') + ', line ' + row.getAttribute('data-line-index');
    }
  }

  function itemLevel(item) {
    const raw = item && item.getAttribute('aria-level');
    const value = raw ? parseInt(raw, 10) : 1;
    return Number.isFinite(value) ? value : 1;
  }

  function updateTreeVisibility() {
    if (!tree) return;
    const collapsed = [];
    tree.querySelectorAll('[role="treeitem"]').forEach(function (item) {
      const level = itemLevel(item);
      while (collapsed.length && collapsed[collapsed.length - 1] >= level) {
        collapsed.pop();
      }
      item.hidden = collapsed.length > 0;
      if (!item.hidden && item.classList.contains('is-collapsed')) {
        collapsed.push(level);
      }
    });
  }

  function updateStickyToolbar() {
    if (!toolbar) return;
    const rect = toolbar.getBoundingClientRect();
    toolbar.classList.toggle('is-stuck', rect.top <= 0);
  }

  function setFileTreeExpanded(expanded) {
    if (shell) shell.classList.toggle('file-tree-collapsed', !expanded);
    if (toolbar) toolbar.setAttribute('data-file-tree-expanded', expanded ? 'true' : 'false');
    document.querySelectorAll('[data-file-tree-panel-action]').forEach(function (button) {
      const action = button.getAttribute('data-file-tree-panel-action');
      button.hidden = expanded ? action === 'expand' : action === 'collapse';
      button.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    });
  }

  function closeMenus(except) {
    document.querySelectorAll('[data-menu-button][aria-expanded="true"]').forEach(function (button) {
      if (button === except) return;
      button.setAttribute('aria-expanded', 'false');
      const menu = document.getElementById(button.getAttribute('data-menu-target') || '');
      if (menu) menu.hidden = true;
    });
  }

  function toggleMenu(button) {
    const menu = document.getElementById(button.getAttribute('data-menu-target') || '');
    if (!menu) return;
    const opening = button.getAttribute('aria-expanded') !== 'true';
    closeMenus(button);
    button.setAttribute('aria-expanded', opening ? 'true' : 'false');
    menu.hidden = !opening;
  }

  document.addEventListener('click', function (event) {
    const menuButton = event.target.closest('[data-menu-button]');
    if (menuButton) {
      if (menuButton.getAttribute('data-pr-action') === 'show-stats') refreshStats();
      toggleMenu(menuButton);
      event.preventDefault();
      return;
    }

    const panelToggle = event.target.closest('[data-file-tree-panel-action]');
    if (panelToggle) {
      closeMenus(null);
      setFileTreeExpanded(panelToggle.getAttribute('data-file-tree-panel-action') === 'expand');
      event.preventDefault();
      return;
    }

    const treeToggle = event.target.closest('[data-tree-action="toggle-directory"]');
    if (treeToggle) {
      closeMenus(null);
      const item = treeToggle.closest('[role="treeitem"]');
      if (item) {
        const next = item.classList.toggle('is-collapsed');
        item.setAttribute('aria-expanded', next ? 'false' : 'true');
        treeToggle.setAttribute('aria-expanded', next ? 'false' : 'true');
        updateTreeVisibility();
      }
      event.preventDefault();
      return;
    }

    const commentButton = event.target.closest('[data-pr-comment-button]');
    if (commentButton) {
      closeMenus(null);
      openCommentEditor(commentButton);
      event.preventDefault();
      event.stopPropagation();
      return;
    }

    const actionTarget = event.target.closest('[data-pr-action]');
    if (actionTarget) {
      closeMenus(null);
      const action = actionTarget.getAttribute('data-pr-action');
      if (action === 'toggle-file') {
        const section = sectionFor(actionTarget);
        if (section) section.classList.toggle('is-collapsed');
      } else if (action === 'mark-reviewed') {
        const section = sectionFor(actionTarget);
        if (section) section.classList.toggle('is-reviewed');
      } else if (action === 'collapse-all' || action === 'expand-all') {
        document.querySelectorAll('[data-file-section]').forEach(function (section) {
          section.classList.toggle('is-collapsed', action === 'collapse-all');
        });
      } else if (action === 'show-stats') {
        refreshStats();
      }
      if (action !== 'jump-file') event.preventDefault();
      return;
    }

    const menu = event.target.closest('.pr-menu');
    if (menu) {
      closeMenus(null);
      event.preventDefault();
      return;
    }

    closeMenus(null);
    const row = event.target.closest('[data-line]');
    if (row && shell && shell.contains(row)) {
      setSelection(row);
    }
  }, { passive: false });

  window.addEventListener('scroll', updateStickyToolbar, { passive: true });
  window.addEventListener('resize', updateStickyToolbar, { passive: true });
  setFileTreeExpanded(true);
  updateTreeVisibility();
  updateStickyToolbar();
  restorePersistedComments();
  refreshStats();
})();
`;
