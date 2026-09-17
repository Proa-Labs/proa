// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Large pull-request diff stress page.
//!
//! This demo intentionally models the GitHub Files changed surface without
//! copying the saved page wholesale. The diff rows are rendered as static HTML
//! bytes, while one tiny delegated script handles file folding, file jumping,
//! and line selection.

use core::fmt::Write as _;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{html_sync, text};
use serde::{Deserialize, Serialize};

const DEFAULT_FILE_COUNT: usize = 16;
const DEFAULT_LINES_PER_FILE: usize = 40;
const MAX_FILE_COUNT: usize = 48;
const MAX_LINES_PER_FILE: usize = 1_200;
const DEMO_URL_CAPACITY: usize = 512;

/// Allocation-free URL value for dynamic links in this stress-page demo.
///
/// Unlike a streaming `text!` value, this retains the complete URL so
/// `AttrValue::render_url_attr` can run its dangerous-scheme check before any
/// bytes are emitted. Every repository route starts with `/` and file target
/// starts with `#`, but exposing the full value keeps the ordinary URL guard
/// active instead of relying on that construction detail.
struct DemoUrl {
    value: heapless::String<DEMO_URL_CAPACITY>,
    valid: bool,
}

impl DemoUrl {
    fn owner(owner: &str) -> Self {
        let mut value = heapless::String::new();
        let valid = value.push('/').is_ok() && value.push_str(owner).is_ok();
        Self { value, valid }
    }

    fn repository(owner: &str, repository: &str, suffix: &str) -> Self {
        let mut value = heapless::String::new();
        let valid = value.push('/').is_ok()
            && value.push_str(owner).is_ok()
            && value.push('/').is_ok()
            && value.push_str(repository).is_ok()
            && value.push_str(suffix).is_ok();
        Self { value, valid }
    }

    fn file_fragment(file_idx: usize) -> Self {
        let mut value = heapless::String::new();
        let valid = write!(&mut value, "#file-{file_idx}").is_ok();
        Self { value, valid }
    }
}

impl AttrValue for DemoUrl {
    fn should_render(&self) -> bool {
        self.valid
    }

    fn url_str(&self) -> Option<&str> {
        self.valid.then_some(self.value.as_str())
    }

    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        if self.valid {
            AttrValue::render_attr_value(&self.value.as_str(), cx)?;
        }
        Ok(())
    }
}

#[path = "github_pr_diff_fixtures.rs"]
mod github_pr_diff_fixtures;

static FIXTURE_FILE_ROWS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();

const FILE_PATHS: &[&str] = &[
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

const FILE_ADDITIONS: &[usize] = &[3, 3, 1, 2, 20, 136, 63, 56, 27, 31, 10, 30, 31, 23, 37, 39];
const FILE_DELETIONS: &[usize] = &[1, 1, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 4];
const DEFAULT_FILE_LINES: &[usize] = &[
    11, 11, 9, 16, 24, 143, 74, 56, 36, 35, 13, 37, 0, 26, 37, 53,
];

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubPrDiffProps {
    #[serde(default = "default_repo_owner")]
    pub repo_owner: String,
    #[serde(default = "default_repo_name")]
    pub repo_name: String,
    #[serde(default = "default_pr_title")]
    pub title: String,
    #[serde(default = "default_pr_number")]
    pub pull_number: usize,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default = "default_issue_count_label")]
    pub issue_count_label: String,
    #[serde(default = "default_pull_request_count_label")]
    pub pull_request_count_label: String,
    #[serde(default = "default_fork_count_label")]
    pub fork_count_label: String,
    #[serde(default = "default_star_count_label")]
    pub star_count_label: String,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
    #[serde(default = "default_head_branch")]
    pub head_branch: String,
    #[serde(default = "default_commit_count")]
    pub commit_count: usize,
    #[serde(default = "default_checks_count")]
    pub checks_count: usize,
    #[serde(default = "default_conversation_count")]
    pub conversation_count: usize,
    #[serde(default = "default_review_comment_count")]
    pub review_comment_count: usize,
    #[serde(default = "default_viewed_file_count")]
    pub viewed_file_count: usize,
    #[serde(default = "default_commit_selector_label")]
    pub commit_selector_label: String,
    #[serde(default = "default_review_action_label")]
    pub review_action_label: String,
    #[serde(default = "default_file_count")]
    pub file_count: usize,
    #[serde(default = "default_lines_per_file")]
    pub lines_per_file: usize,
    #[serde(default)]
    pub files: Vec<GithubPrDiffFileProps>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubPrDiffFileProps {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub additions: usize,
    #[serde(default)]
    pub deletions: usize,
    #[serde(default)]
    pub visible_lines: usize,
    #[serde(default)]
    pub diff_lines: Vec<GithubPrDiffLineProps>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubPrDiffLineProps {
    #[serde(default = "default_diff_line_kind")]
    pub kind: String,
    #[serde(default)]
    pub old_line: Option<usize>,
    #[serde(default)]
    pub new_line: Option<usize>,
    #[serde(default)]
    pub text: String,
}

impl Default for GithubPrDiffProps {
    fn default() -> Self {
        Self {
            repo_owner: default_repo_owner(),
            repo_name: default_repo_name(),
            title: default_pr_title(),
            pull_number: default_pr_number(),
            author: default_author(),
            status: default_status(),
            issue_count_label: default_issue_count_label(),
            pull_request_count_label: default_pull_request_count_label(),
            fork_count_label: default_fork_count_label(),
            star_count_label: default_star_count_label(),
            base_branch: default_base_branch(),
            head_branch: default_head_branch(),
            commit_count: default_commit_count(),
            checks_count: default_checks_count(),
            conversation_count: default_conversation_count(),
            review_comment_count: default_review_comment_count(),
            viewed_file_count: default_viewed_file_count(),
            commit_selector_label: default_commit_selector_label(),
            review_action_label: default_review_action_label(),
            file_count: DEFAULT_FILE_COUNT,
            lines_per_file: DEFAULT_LINES_PER_FILE,
            files: default_files(),
        }
    }
}

impl GithubPrDiffProps {
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str::<Self>(json)
            .unwrap_or_default()
            .clamped()
    }

    pub fn clamped(mut self) -> Self {
        self.file_count = clamp_usize(self.file_count, 0, MAX_FILE_COUNT);
        self.lines_per_file = clamp_usize(self.lines_per_file, 0, MAX_LINES_PER_FILE);
        if !self.files.is_empty() {
            self.files.truncate(MAX_FILE_COUNT);
            self.file_count = self.files.len();
        }
        self.viewed_file_count = clamp_usize(self.viewed_file_count, 0, self.file_count);
        self
    }

    pub fn total_lines(&self) -> usize {
        let mut total = 0;
        let mut file_idx = 0;
        while file_idx < self.total_files() {
            total += self.file_row_count(file_idx);
            file_idx += 1;
        }
        total
    }

    pub fn total_rows(&self) -> usize {
        self.total_lines()
    }

    pub fn total_files(&self) -> usize {
        self.file_count
    }

    fn total_additions(&self) -> usize {
        let mut total = 0;
        let mut file_idx = 0;
        while file_idx < self.total_files() {
            total += self.file_additions(file_idx);
            file_idx += 1;
        }
        total
    }

    fn total_deletions(&self) -> usize {
        let mut total = 0;
        let mut file_idx = 0;
        while file_idx < self.total_files() {
            total += self.file_deletions(file_idx);
            file_idx += 1;
        }
        total
    }

    fn file_path(&self, file_idx: usize) -> &str {
        if let Some(file) = self.file(file_idx) {
            if !file.path.is_empty() {
                return file.path.as_str();
            }
        }
        FILE_PATHS[file_idx % FILE_PATHS.len()]
    }

    fn file_lines(&self, file_idx: usize) -> usize {
        if let Some(file) = self.file(file_idx) {
            if !file.diff_lines.is_empty() {
                return clamp_usize(file.diff_lines.len(), 0, MAX_LINES_PER_FILE);
            }
        }
        if self.lines_per_file == DEFAULT_LINES_PER_FILE {
            if let Some(lines) = github_pr_diff_fixtures::default_diff_lines(file_idx) {
                return lines.len();
            }
        }
        if self.lines_per_file == DEFAULT_LINES_PER_FILE {
            if let Some(file) = self.file(file_idx) {
                return clamp_usize(file.visible_lines, 0, MAX_LINES_PER_FILE);
            }
            if file_idx < DEFAULT_FILE_LINES.len() {
                return DEFAULT_FILE_LINES[file_idx];
            }
        }
        self.lines_per_file
    }

    fn file_row_count(&self, file_idx: usize) -> usize {
        let lines = self.file_lines(file_idx);
        if self.has_explicit_diff_lines(file_idx) || lines == 0 {
            lines
        } else {
            lines + 1
        }
    }

    fn has_explicit_diff_lines(&self, file_idx: usize) -> bool {
        if let Some(file) = self.file(file_idx) {
            if !file.diff_lines.is_empty() {
                return true;
            }
        }
        self.lines_per_file == DEFAULT_LINES_PER_FILE
            && github_pr_diff_fixtures::default_diff_lines(file_idx).is_some()
    }

    fn file_additions(&self, file_idx: usize) -> usize {
        let lines = self.file_lines(file_idx);
        if self.lines_per_file == DEFAULT_LINES_PER_FILE {
            if let Some(file) = self.file(file_idx) {
                return file.additions;
            }
            if file_idx < FILE_ADDITIONS.len() {
                return FILE_ADDITIONS[file_idx];
            }
        }
        lines * 6 / 10
    }

    fn file_deletions(&self, file_idx: usize) -> usize {
        let lines = self.file_lines(file_idx);
        if self.lines_per_file == DEFAULT_LINES_PER_FILE {
            if let Some(file) = self.file(file_idx) {
                return file.deletions;
            }
            if file_idx < FILE_DELETIONS.len() {
                return FILE_DELETIONS[file_idx];
            }
        }
        lines * 2 / 10
    }

    fn file(&self, file_idx: usize) -> Option<&GithubPrDiffFileProps> {
        if self.files.is_empty() {
            None
        } else {
            self.files.get(file_idx % self.files.len())
        }
    }

    fn status_class(&self) -> &'static str {
        if self.status.eq_ignore_ascii_case("closed") {
            "status-pill status-closed"
        } else {
            "status-pill status-open"
        }
    }
}

pub fn default_props_json() -> serde_json::Value {
    let mut value = serde_json::to_value(GithubPrDiffProps::default())
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = value.as_object_mut() {
        object.remove("file_count");
    }
    value
}

fn default_repo_owner() -> String {
    "oven-sh".to_owned()
}

fn default_repo_name() -> String {
    "bun".to_owned()
}

fn default_pr_title() -> String {
    "[tracking] Zig/C++ fork: Rust\u{2192}Zig port progress".to_owned()
}

const fn default_pr_number() -> usize {
    31_136
}

fn default_author() -> String {
    "MaurerAnton".to_owned()
}

fn default_status() -> String {
    "Closed".to_owned()
}

fn default_issue_count_label() -> String {
    "5k+".to_owned()
}

fn default_pull_request_count_label() -> String {
    "1.9k".to_owned()
}

fn default_fork_count_label() -> String {
    "4.6k".to_owned()
}

fn default_star_count_label() -> String {
    "92.2k".to_owned()
}

fn default_base_branch() -> String {
    "oven-sh:main".to_owned()
}

fn default_head_branch() -> String {
    "MaurerAnton:zig-fork".to_owned()
}

const fn default_commit_count() -> usize {
    8
}

const fn default_checks_count() -> usize {
    1
}

const fn default_conversation_count() -> usize {
    9
}

const fn default_review_comment_count() -> usize {
    5
}

const fn default_viewed_file_count() -> usize {
    0
}

fn default_commit_selector_label() -> String {
    "All commits".to_owned()
}

fn default_review_action_label() -> String {
    "Submit comments".to_owned()
}

fn default_diff_line_kind() -> String {
    "context".to_owned()
}

const fn default_file_count() -> usize {
    DEFAULT_FILE_COUNT
}

const fn default_lines_per_file() -> usize {
    DEFAULT_LINES_PER_FILE
}

fn default_files() -> Vec<GithubPrDiffFileProps> {
    let mut files = Vec::with_capacity(FILE_PATHS.len());
    let mut file_idx = 0;
    while file_idx < FILE_PATHS.len() {
        let diff_lines = default_diff_line_props(file_idx);
        let visible_lines = if diff_lines.is_empty() {
            DEFAULT_FILE_LINES[file_idx]
        } else {
            diff_lines.len()
        };
        files.push(GithubPrDiffFileProps {
            path: FILE_PATHS[file_idx].to_owned(),
            additions: FILE_ADDITIONS[file_idx],
            deletions: FILE_DELETIONS[file_idx],
            visible_lines,
            diff_lines,
        });
        file_idx += 1;
    }
    files
}

fn default_diff_line_props(file_idx: usize) -> Vec<GithubPrDiffLineProps> {
    let Some(lines) = github_pr_diff_fixtures::default_diff_lines(file_idx) else {
        return Vec::new();
    };
    lines
        .iter()
        .map(|line| GithubPrDiffLineProps {
            kind: line.kind.prop_kind().to_owned(),
            old_line: line.old_line,
            new_line: line.new_line,
            text: line.text.to_owned(),
        })
        .collect()
}

const fn clamp_usize(value: usize, min: usize, max: usize) -> usize {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GithubPrDiffPage<'a> {
    pub props: &'a GithubPrDiffProps,
}

impl GithubPrDiffPage<'_> {
    fn total_files(&self) -> usize {
        self.props.total_files()
    }

    fn total_lines(&self) -> usize {
        self.props.total_rows()
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GithubPrDiffPage<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en" data-pr-diff-demo>
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>"Proa large PR diff demo"</title>
                    <style>{StaticRaw(CSS)}</style>
                </head>
                <body>
                    <div class="pr-shell" data-pr-shell>
                        {Header {
                            props: self.props,
                            file_count: self.total_files(),
                            line_count: self.total_lines(),
                            additions: self.props.total_additions(),
                            deletions: self.props.total_deletions(),
                        }}
                        <main class="pr-main">
                            {ReviewToolbar {
                                props: self.props,
                                file_count: self.total_files(),
                                line_count: self.total_lines(),
                            }}
                            <div class="pr-layout">
                                {FileTree { props: self.props }}
                                <section class="diffs" aria-label="Pull request diff">
                                    {for file_idx in 0..self.total_files() {
                                        FileSection {
                                            props: self.props,
                                            file_idx,
                                            lines: self.props.file_lines(file_idx),
                                        }
                                    }}
                                </section>
                            </div>
                        </main>
                    </div>
                    <script>{StaticRaw(JS)}</script>
                </body>
            </html>
        }
        .render(cx)
    }
}

struct Header<'a> {
    props: &'a GithubPrDiffProps,
    file_count: usize,
    line_count: usize,
    additions: usize,
    deletions: usize,
}

struct TopAppHeader<'a> {
    props: &'a GithubPrDiffProps,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TopAppHeader<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <header
                aria-label="Global Navigation Menu"
                class="GlobalNav styles-module__appHeader__YzYWk app-header prc-Stack-Stack-UQ9k6"
                data-gap="none"
                data-direction="vertical"
                data-align="stretch"
                data-wrap="nowrap"
                data-justify="start"
                data-padding="none"
            >
                <div
                    class="app-header-top prc-Stack-Stack-UQ9k6"
                    data-direction="horizontal"
                    data-align="center"
                    data-wrap="nowrap"
                    data-justify="center"
                    data-padding="none"
                >
                    <div
                        data-testid="top-nav-left"
                        class="styles-module__left__Fylw7 styles-module__withLocalNavigation__rjTJ_ app-header-left prc-Stack-Stack-UQ9k6"
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
                                class="app-header-button icon-only"
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
                                    class="octicon octicon-three-bars"
                                    viewBox="0 0 16 16"
                                    width="16"
                                    height="16"
                                    fill="currentColor"
                                    display="inline-block"
                                    overflow="visible"
                                    style="vertical-align:text-bottom"
                                >
                                    <path d="M1 2.75A.75.75 0 0 1 1.75 2h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 2.75Zm0 5A.75.75 0 0 1 1.75 7h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 7.75ZM1.75 12h12.5a.75.75 0 0 1 0 1.5H1.75a.75.75 0 0 1 0-1.5Z" />
                                </svg>
                            </button>
                        </div>
                        <a
                            data-component="IconButton"
                            type="button"
                            class="github-mark app-header-home"
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
                                class="octicon octicon-mark-github"
                                viewBox="0 0 24 24"
                                width="32"
                                height="32"
                                fill="currentColor"
                                display="inline-block"
                                overflow="visible"
                                style="vertical-align:text-bottom"
                            >
                                <path d="M10.226 17.284c-2.965-.36-5.054-2.493-5.054-5.256 0-1.123.404-2.336 1.078-3.144-.292-.741-.247-2.314.09-2.965.898-.112 2.111.36 2.83 1.01.853-.269 1.752-.404 2.853-.404 1.1 0 1.999.135 2.807.382.696-.629 1.932-1.1 2.83-.988.315.606.36 2.179.067 2.942.72.854 1.101 2 1.101 3.167 0 2.763-2.089 4.852-5.098 5.234.763.494 1.28 1.572 1.28 2.807v2.336c0 .674.561 1.056 1.235.786 4.066-1.55 7.255-5.615 7.255-10.646C23.5 6.188 18.334 1 11.978 1 5.62 1 .5 6.188.5 12.545c0 4.986 3.167 9.12 7.435 10.669.606.225 1.19-.18 1.19-.786V20.63a2.9 2.9 0 0 1-1.078.224c-1.483 0-2.359-.808-2.987-2.313-.247-.607-.517-.966-1.034-1.033-.27-.023-.359-.135-.359-.27 0-.27.45-.471.898-.471.652 0 1.213.404 1.797 1.235.45.651.921.943 1.483.943.561 0 .92-.202 1.437-.719.382-.381.674-.718.944-.943" />
                            </svg>
                        </a>
                    </div>
                    <div
                        data-testid="top-nav-center"
                        class="styles-module__center__R3QRv styles-module__withLocalNavigation__rjTJ_ app-header-center prc-Stack-Stack-UQ9k6"
                        data-gap="condensed"
                        data-direction="horizontal"
                        data-align="stretch"
                        data-wrap="nowrap"
                        data-justify="start"
                        data-padding="normal"
                    >
                        <nav
                            class="styles-module__contextRegion__VbSp2 prc-Breadcrumbs-BreadcrumbsBase-3Gb-B header-breadcrumbs"
                            aria-label="Breadcrumbs"
                            data-overflow="menu"
                            data-variant="normal"
                            data-component="Breadcrumbs"
                        >
                            <ol class="prc-Breadcrumbs-BreadcrumbsList-BKjpe">
                                <li class="prc-Breadcrumbs-ItemWrapper-k0NLn">
                                    <a
                                        class="styles-module__contextCrumb__IzGIq prc-Breadcrumbs-Item-jcraJ header-breadcrumb"
                                        data-component="Breadcrumbs.Item"
                                        href={DemoUrl::owner(self.props.repo_owner.as_str())}
                                    >
                                        <span>{self.props.repo_owner.as_str()}</span>
                                    </a>
                                </li>
                                <li class="prc-Breadcrumbs-ItemWrapper-k0NLn">
                                    <a
                                        class="styles-module__contextCrumb__IzGIq prc-Breadcrumbs-Item-jcraJ header-breadcrumb"
                                        data-component="Breadcrumbs.Item"
                                        href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "")}
                                    >
                                        <span class="styles-module__contextCrumbLast__tI2e3">
                                            {self.props.repo_name.as_str()}
                                        </span>
                                    </a>
                                </li>
                            </ol>
                        </nav>
                        <div
                            class="Search-module__searchButtonGroup__aetw5 header-search-group prc-ButtonGroup-ButtonGroup-vFUrY"
                            data-component="ButtonGroup"
                        >
                            <div>
                                <button
                                    data-component="Button"
                                    type="button"
                                    aria-label="Search or jump to..."
                                    class="prc-Button-ButtonBase-9n-Xk styles-module__appHeaderButton__axedQ Search-module__searchButton__aiE0a app-header-button header-search"
                                    data-loading="false"
                                    data-size="medium"
                                    data-variant="invisible"
                                >
                                    <span
                                        data-component="buttonContent"
                                        data-align="center"
                                        class="prc-Button-ButtonContent-Iohp5"
                                    >
                                        <span
                                            data-component="leadingVisual"
                                            class="prc-Button-Visual-YNt2F prc-Button-VisualWrap-E4cnq"
                                        >
                                            <svg
                                                data-component="Octicon"
                                                aria-hidden="true"
                                                focusable="false"
                                                class="octicon octicon-search"
                                                viewBox="0 0 16 16"
                                                width="16"
                                                height="16"
                                                fill="currentColor"
                                                display="inline-block"
                                                overflow="visible"
                                                style="vertical-align:text-bottom"
                                            >
                                                <path d="M10.68 11.74a6 6 0 0 1-7.922-8.982 6 6 0 0 1 8.982 7.922l3.04 3.04a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215ZM11.5 7a4.499 4.499 0 1 0-8.997 0A4.499 4.499 0 0 0 11.5 7Z" />
                                            </svg>
                                        </span>
                                        <span data-component="text" class="prc-Button-Label-FWkx3">
                                            <span class="Search-module__placeholder__p9hbG Search-module__text__veSYi Search-module__value__TFoak">
                                                "Type "
                                                <kbd class="Search-module__kbd__WCskr">"/"</kbd>
                                                " to search"
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
                        class="styles-module__right__mlBQg styles-module__withLocalNavigation__rjTJ_ app-header-right prc-Stack-Stack-UQ9k6"
                        data-gap="condensed"
                        data-direction="horizontal"
                        data-align="center"
                        data-wrap="nowrap"
                        data-justify="start"
                        data-padding="normal"
                    >
                        <div
                            data-testid="top-bar-actions"
                            class="app-action-group prc-Stack-Stack-UQ9k6"
                            data-gap="condensed"
                            data-direction="horizontal"
                            data-align="center"
                            data-wrap="nowrap"
                            data-justify="start"
                            data-padding="none"
                        >
                            <div class="dropdown-wrap">
                                <button
                                    type="button"
                                    class="app-header-button icon-with-caret"
                                    aria-label="Create new"
                                    aria-haspopup="true"
                                    aria-expanded="false"
                                    data-menu-button
                                    data-menu-target="create-menu"
                                >
                                    {StaticRaw(ICON_PLUS)}
                                    {StaticRaw(ICON_TRIANGLE_DOWN)}
                                </button>
                                <div class="pr-menu app-menu" id="create-menu" role="menu" hidden>
                                    <button type="button" role="menuitem">"New repository"</button>
                                    <button type="button" role="menuitem">
                                        "Import repository"
                                    </button>
                                    <button type="button" role="menuitem">"New codespace"</button>
                                    <button type="button" role="menuitem">
                                        "New organization"
                                    </button>
                                </div>
                            </div>
                            <div class="hide-sm hide-md app-quick-links">
                                <a
                                    class="app-header-button icon-only"
                                    href="/issues"
                                    aria-label="All issues"
                                >
                                    {StaticRaw(ICON_ISSUE_OPENED)}
                                </a>
                                <a
                                    class="app-header-button icon-only"
                                    href="/pulls"
                                    aria-label="All pull requests"
                                >
                                    {StaticRaw(ICON_GIT_PULL_REQUEST)}
                                </a>
                                <a
                                    class="app-header-button icon-only"
                                    href="/repos"
                                    aria-label="All repositories"
                                >
                                    {StaticRaw(ICON_REPO)}
                                </a>
                            </div>
                        </div>
                        <a
                            class="app-header-button icon-only notification-button"
                            href="/notifications"
                            aria-label="You have unread notifications"
                        >
                            {StaticRaw(ICON_INBOX)}
                        </a>
                        <button
                            type="button"
                            class="avatar-button"
                            aria-label="Open user navigation menu"
                        >
                            <img
                                alt="Demo user"
                                width="32"
                                height="32"
                                src="https://github.githubassets.com/images/gravatars/gravatar-user-420.png"
                            />
                        </button>
                    </div>
                </div>
                <h2 class="prc-src-InternalVisuallyHidden-2YaI6 sr-only">
                    "Repository navigation"
                </h2>
                <nav
                    class="prc-components-UnderlineWrapper-eT-Yj LocalNavigation-module__LocalNavigation__b0Xc0 local-navigation"
                    aria-label="Repository"
                    data-variant="inset"
                    data-overflow-measured="true"
                >
                    <ul class="prc-components-UnderlineItemList-xKlKC local-nav-list">
                        <li class="prc-UnderlineNav-UnderlineNavItem-syRjR">
                            <a
                                href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "")}
                                data-tab-item="code"
                                class="prc-components-UnderlineItem-7fP-n local-nav-item"
                            >
                                <span
                                    data-component="icon"
                                    class="repo-tab-icon"
                                    aria-hidden="true"
                                >
                                    <svg
                                        data-component="Octicon"
                                        aria-hidden="true"
                                        focusable="false"
                                        class="octicon octicon-code"
                                        viewBox="0 0 16 16"
                                        width="16"
                                        height="16"
                                        fill="currentColor"
                                        display="inline-block"
                                        overflow="visible"
                                        style="vertical-align:text-bottom"
                                    >
                                        <path d="m11.28 3.22 4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.749.749 0 0 1-1.275-.326.749.749 0 0 1 .215-.734L13.94 8l-3.72-3.72a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215Zm-6.56 0a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042L2.06 8l3.72 3.72a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215L.47 8.53a.75.75 0 0 1 0-1.06Z" />
                                    </svg>
                                </span>
                                <span data-component="text" data-content="Code">"Code"</span>
                            </a>
                        </li>
                        <li class="prc-UnderlineNav-UnderlineNavItem-syRjR">
                            <a
                                href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "/issues")}
                                data-tab-item="issues"
                                class="prc-components-UnderlineItem-7fP-n local-nav-item"
                            >
                                <span
                                    data-component="icon"
                                    class="repo-tab-icon"
                                    aria-hidden="true"
                                >
                                    {StaticRaw(ICON_ISSUE_OPENED)}
                                </span>
                                <span data-component="text" data-content="Issues">"Issues"</span>
                                <span data-component="counter" class="local-nav-counter">
                                    {self.props.issue_count_label.as_str()}
                                </span>
                            </a>
                        </li>
                        <li class="prc-UnderlineNav-UnderlineNavItem-syRjR">
                            <a
                                href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "/pulls")}
                                aria-current="page"
                                data-tab-item="pull-requests"
                                class="prc-components-UnderlineItem-7fP-n local-nav-item active"
                            >
                                <span
                                    data-component="icon"
                                    class="repo-tab-icon"
                                    aria-hidden="true"
                                >
                                    <svg
                                        data-component="Octicon"
                                        aria-hidden="true"
                                        focusable="false"
                                        class="octicon octicon-git-pull-request"
                                        viewBox="0 0 16 16"
                                        width="16"
                                        height="16"
                                        fill="currentColor"
                                        display="inline-block"
                                        overflow="visible"
                                        style="vertical-align:text-bottom"
                                    >
                                        <path d="M1.5 3.25a2.25 2.25 0 1 1 3 2.122v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.25 2.25 0 0 1 1.5 3.25Zm5.677-.177L9.573.677A.25.25 0 0 1 10 .854V2.5h1A2.5 2.5 0 0 1 13.5 5v5.628a2.251 2.251 0 1 1-1.5 0V5a1 1 0 0 0-1-1h-1v1.646a.25.25 0 0 1-.427.177L7.177 3.427a.25.25 0 0 1 0-.354ZM3.75 2.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm0 9.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm8.25.75a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0Z" />
                                    </svg>
                                </span>
                                <span data-component="text" data-content="Pull requests">
                                    "Pull requests"
                                </span>
                                <span data-component="counter" class="local-nav-counter">
                                    {self.props.pull_request_count_label.as_str()}
                                </span>
                            </a>
                        </li>
                        <li class="prc-UnderlineNav-UnderlineNavItem-syRjR">
                            <a
                                href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "/discussions")}
                                data-tab-item="discussions"
                                class="prc-components-UnderlineItem-7fP-n local-nav-item"
                            >
                                <span
                                    data-component="icon"
                                    class="repo-tab-icon"
                                    aria-hidden="true"
                                >
                                    <svg
                                        data-component="Octicon"
                                        aria-hidden="true"
                                        focusable="false"
                                        class="octicon octicon-comment-discussion"
                                        viewBox="0 0 16 16"
                                        width="16"
                                        height="16"
                                        fill="currentColor"
                                        display="inline-block"
                                        overflow="visible"
                                        style="vertical-align:text-bottom"
                                    >
                                        <path d="M1.75 1h8.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 10.25 10H7.061l-2.574 2.573A1.458 1.458 0 0 1 2 11.543V10h-.25A1.75 1.75 0 0 1 0 8.25v-5.5C0 1.784.784 1 1.75 1ZM1.5 2.75v5.5c0 .138.112.25.25.25h1a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h3.5a.25.25 0 0 0 .25-.25v-5.5a.25.25 0 0 0-.25-.25h-8.5a.25.25 0 0 0-.25.25Zm13 2a.25.25 0 0 0-.25-.25h-.5a.75.75 0 0 1 0-1.5h.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 14.25 12H14v1.543a1.458 1.458 0 0 1-2.487 1.03L9.22 12.28a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215l2.22 2.22v-2.19a.75.75 0 0 1 .75-.75h1a.25.25 0 0 0 .25-.25Z" />
                                    </svg>
                                </span>
                                <span data-component="text" data-content="Discussions">
                                    "Discussions"
                                </span>
                            </a>
                        </li>
                        <li class="prc-UnderlineNav-UnderlineNavItem-syRjR">
                            <a
                                href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "/actions")}
                                data-tab-item="actions"
                                class="prc-components-UnderlineItem-7fP-n local-nav-item"
                            >
                                <span
                                    data-component="icon"
                                    class="repo-tab-icon"
                                    aria-hidden="true"
                                >
                                    <svg
                                        data-component="Octicon"
                                        aria-hidden="true"
                                        focusable="false"
                                        class="octicon octicon-play"
                                        viewBox="0 0 16 16"
                                        width="16"
                                        height="16"
                                        fill="currentColor"
                                        display="inline-block"
                                        overflow="visible"
                                        style="vertical-align:text-bottom"
                                    >
                                        <path d="M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM1.5 8a6.5 6.5 0 1 0 13 0 6.5 6.5 0 0 0-13 0Zm4.879-2.773 4.264 2.559a.25.25 0 0 1 0 .428l-4.264 2.559A.25.25 0 0 1 6 10.559V5.442a.25.25 0 0 1 .379-.215Z" />
                                    </svg>
                                </span>
                                <span data-component="text" data-content="Actions">"Actions"</span>
                            </a>
                        </li>
                        <li class="prc-UnderlineNav-UnderlineNavItem-syRjR">
                            <a
                                href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "/security")}
                                data-tab-item="security-and-quality"
                                class="prc-components-UnderlineItem-7fP-n local-nav-item"
                            >
                                <span
                                    data-component="icon"
                                    class="repo-tab-icon"
                                    aria-hidden="true"
                                >
                                    <svg
                                        data-component="Octicon"
                                        aria-hidden="true"
                                        focusable="false"
                                        class="octicon octicon-shield"
                                        viewBox="0 0 16 16"
                                        width="16"
                                        height="16"
                                        fill="currentColor"
                                        display="inline-block"
                                        overflow="visible"
                                        style="vertical-align:text-bottom"
                                    >
                                        <path d="M7.467.133a1.748 1.748 0 0 1 1.066 0l5.25 1.68A1.75 1.75 0 0 1 15 3.48V7c0 1.566-.32 3.182-1.303 4.682-.983 1.498-2.585 2.813-5.032 3.855a1.697 1.697 0 0 1-1.33 0c-2.447-1.042-4.049-2.357-5.032-3.855C1.32 10.182 1 8.566 1 7V3.48a1.75 1.75 0 0 1 1.217-1.667Zm.61 1.429a.25.25 0 0 0-.153 0l-5.25 1.68a.25.25 0 0 0-.174.238V7c0 1.358.275 2.666 1.057 3.86.784 1.194 2.121 2.34 4.366 3.297a.196.196 0 0 0 .154 0c2.245-.956 3.582-2.104 4.366-3.298C13.225 9.666 13.5 8.36 13.5 7V3.48a.251.251 0 0 0-.174-.237l-5.25-1.68Z" />
                                    </svg>
                                </span>
                                <span data-component="text" data-content="Security and quality">
                                    "Security and quality"
                                </span>
                            </a>
                        </li>
                        <li class="prc-UnderlineNav-UnderlineNavItem-syRjR">
                            <a
                                href={DemoUrl::repository(self.props.repo_owner.as_str(), self.props.repo_name.as_str(), "/pulse")}
                                data-tab-item="insights"
                                class="prc-components-UnderlineItem-7fP-n local-nav-item"
                            >
                                <span
                                    data-component="icon"
                                    class="repo-tab-icon"
                                    aria-hidden="true"
                                >
                                    <svg
                                        data-component="Octicon"
                                        aria-hidden="true"
                                        focusable="false"
                                        class="octicon octicon-graph"
                                        viewBox="0 0 16 16"
                                        width="16"
                                        height="16"
                                        fill="currentColor"
                                        display="inline-block"
                                        overflow="visible"
                                        style="vertical-align:text-bottom"
                                    >
                                        <path d="M1.5 1.75V13.5h13.75a.75.75 0 0 1 0 1.5H.75a.75.75 0 0 1-.75-.75V1.75a.75.75 0 0 1 1.5 0Zm14.28 2.53-5.25 5.25a.75.75 0 0 1-1.06 0L7 7.06 4.28 9.78a.751.751 0 0 1-1.042-.018.751.751 0 0 1-.018-1.042l3.25-3.25a.75.75 0 0 1 1.06 0L10 7.94l4.72-4.72a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042Z" />
                                    </svg>
                                </span>
                                <span data-component="text" data-content="Insights">
                                    "Insights"
                                </span>
                            </a>
                        </li>
                    </ul>
                </nav>
            </header>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Header<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="pr-header">
                {TopAppHeader { props: self.props }}
                <div class="pr-title-block">
                    <div class="title-row">
                        <h1>
                            {self.props.title.as_str()}
                            " "
                            <span>{text!("#{}", self.props.pull_number)}</span>
                        </h1>
                        <button type="button" class="new-issue-button">"New issue"</button>
                    </div>
                    <div class="meta-row">
                        <span class={self.props.status_class()}>
                            <svg aria-hidden="true" viewBox="0 0 16 16">
                                <path d="M4.75 3.5a1.25 1.25 0 1 1-2.5 0 1.25 1.25 0 0 1 2.5 0Zm0 9a1.25 1.25 0 1 1-2.5 0 1.25 1.25 0 0 1 2.5 0ZM13.75 8a1.25 1.25 0 1 1-2.5 0 1.25 1.25 0 0 1 2.5 0ZM4 4.75v6.5h1.5v-6.5H4Zm7.25 2.5H7.5a2 2 0 0 0-2 2v2h1.5v-2a.5.5 0 0 1 .5-.5h3.75v-1.5Z" />
                            </svg>
                            {self.props.status.as_str()}
                        </span>
                        <strong>{self.props.author.as_str()}</strong>
                        <span>" wants to merge "{self.props.commit_count}" commits into "</span>
                        <code>{self.props.base_branch.as_str()}</code>
                        <span>" from "</span>
                        <code>{self.props.head_branch.as_str()}</code>
                    </div>
                </div>
                <div class="tabs">
                    <a href="#">
                        <span class="tab-icon" aria-hidden="true">
                            <svg viewBox="0 0 16 16">
                                <path d="M1.75 2.5h12.5a.75.75 0 0 1 .75.75v7.5a.75.75 0 0 1-.75.75H6.78l-3.15 2.4A.75.75 0 0 1 2.5 13.3v-1.8h-.75A.75.75 0 0 1 1 10.75v-7.5a.75.75 0 0 1 .75-.75Z" />
                            </svg>
                        </span>
                        "Conversation"
                        <span class="tab-count">{self.props.conversation_count}</span>
                    </a>
                    <a href="#">
                        <span class="tab-icon" aria-hidden="true">
                            {StaticRaw(ICON_GIT_COMMIT)}
                        </span>
                        "Commits"
                        <span class="tab-count">{self.props.commit_count}</span>
                    </a>
                    <a href="#">
                        <span class="tab-icon" aria-hidden="true">
                            <svg viewBox="0 0 16 16">
                                <path d="M5.5 11.5 2.75 8.75l1.06-1.06L5.5 9.38l6.69-6.69 1.06 1.06L5.5 11.5Z" />
                            </svg>
                        </span>
                        "Checks"
                        <span class="tab-count">{self.props.checks_count}</span>
                    </a>
                    <a class="active" href="#">
                        <span class="tab-icon" aria-hidden="true">
                            <svg viewBox="0 0 16 16">
                                <path d="M3.75 1h8.5C13.22 1 14 1.78 14 2.75v10.5c0 .97-.78 1.75-1.75 1.75h-8.5A1.75 1.75 0 0 1 2 13.25V2.75C2 1.78 2.78 1 3.75 1Zm0 1.5a.25.25 0 0 0-.25.25v10.5c0 .14.11.25.25.25h8.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25h-8.5ZM5 5h6v1.5H5V5Zm0 3h6v1.5H5V8Z" />
                            </svg>
                        </span>
                        "Files changed"
                        <span class="tab-count">{self.file_count}</span>
                    </a>
                    <div
                        class="diff-summary"
                        aria-label={text!("Lines changed: {} additions and {} deletions", self.additions, self.deletions)}
                    >
                        <span class="add">"+"{self.additions}</span>
                        <span class="del">"-"{self.deletions}</span>
                        <span class="diff-squares">
                            {for idx in 0..5 {
                                let class = if self.deletions == 0 || idx < 4 { "square add-square" } else { "square neutral-square" };
                                html_sync! { <span class={class}></span> }
                            }}
                        </span>
                        <span class="rendered-count sr-only">
                            {self.line_count}
                            " rendered rows"
                        </span>
                    </div>
                </div>
            </div>
        }
        .render(cx)
    }
}

struct ReviewToolbar<'a> {
    props: &'a GithubPrDiffProps,
    file_count: usize,
    line_count: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ReviewToolbar<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section
                data-side-panel-open="false"
                data-file-tree-expanded="true"
                class="review-toolbar PullRequestFilesToolbar-module__toolbar__ztHN6"
                aria-label="Pull Request Toolbar"
            >
                <h2 class="sr-only">"Pull Request Toolbar"</h2>
                <div class="toolbar-left">
                    <div class="tree-toggle-group">
                        <button
                            type="button"
                            class="toolbar-icon"
                            data-file-tree-panel-action="expand"
                            aria-label="Expand file tree"
                            aria-expanded="true"
                            aria-controls="pr-file-tree"
                            hidden
                        >
                            {StaticRaw(ICON_SIDEBAR_COLLAPSE)}
                        </button>
                        <button
                            type="button"
                            class="toolbar-icon"
                            data-file-tree-panel-action="collapse"
                            aria-label="Collapse file tree"
                            aria-expanded="true"
                            aria-controls="pr-file-tree"
                        >
                            {StaticRaw(ICON_SIDEBAR_EXPAND)}
                        </button>
                    </div>
                    <span class={self.props.status_class()}>
                        {StaticRaw(ICON_PULL_REQUEST_CLOSED)}
                        {self.props.status.as_str()}
                    </span>
                    <div class="toolbar-sticky-title">
                        <strong>{self.props.title.as_str()}</strong>
                        <span>{text!("#{}", self.props.pull_number)}</span>
                    </div>
                    <div class="dropdown-wrap">
                        <button
                            type="button"
                            class="toolbar-button toolbar-menu"
                            id="changes-selector-button"
                            aria-haspopup="true"
                            aria-expanded="false"
                            data-menu-button
                            data-menu-target="changes-menu"
                        >
                            <span class="button-leading" aria-hidden="true">
                                {StaticRaw(ICON_VERSIONS)}
                            </span>
                            <span>{self.props.commit_selector_label.as_str()}</span>
                            <span class="button-trailing" aria-hidden="true">
                                {StaticRaw(ICON_TRIANGLE_DOWN)}
                            </span>
                        </button>
                        <div
                            class="pr-menu toolbar-menu-panel"
                            id="changes-menu"
                            role="menu"
                            hidden
                        >
                            <button type="button" role="menuitem">"All commits"</button>
                            <button type="button" role="menuitem">"Latest commit"</button>
                            <button type="button" role="menuitem">
                                "Viewed since last review"
                            </button>
                            <button type="button" role="menuitem">
                                {text!("{} commits in this pull request", self.props.commit_count)}
                            </button>
                        </div>
                    </div>
                    <button type="button" class="toolbar-menu toolbar-link">
                        "File filter"
                        <span>"⌄"</span>
                    </button>
                    <button type="button" class="toolbar-menu toolbar-link">
                        "Conversations"
                        <span>"⌄"</span>
                    </button>
                    <span class="toolbar-muted sr-only">{self.line_count}" rendered rows"</span>
                </div>
                <div class="toolbar-right">
                    <div
                        class="viewed-progress"
                        aria-label={text!("{} of {} files viewed", self.props.viewed_file_count, self.file_count)}
                    >
                        <svg aria-hidden="true" viewBox="0 0 16 16">
                            <circle
                                cx="8"
                                cy="8"
                                fill="transparent"
                                r="6"
                                stroke="#d0d7de"
                                stroke-width="2"
                            />
                            <circle
                                cx="8"
                                cy="8"
                                fill="transparent"
                                r="6"
                                stroke="#8250df"
                                stroke-dasharray="38"
                                stroke-dashoffset="38"
                                stroke-linecap="round"
                                stroke-width="2"
                            />
                        </svg>
                        <span>{self.props.viewed_file_count}" / "{self.file_count}" viewed"</span>
                    </div>
                    <span class="toolbar-divider"></span>
                    <div class="dropdown-wrap">
                        <button
                            type="button"
                            class="toolbar-button review-primary"
                            aria-haspopup="true"
                            aria-expanded="false"
                            data-menu-button
                            data-menu-target="review-menu"
                        >
                            <span>{self.props.review_action_label.as_str()}</span>
                            {StaticRaw(ICON_TRIANGLE_DOWN)}
                        </button>
                        <div
                            class="pr-menu toolbar-menu-panel right-aligned"
                            id="review-menu"
                            role="menu"
                            hidden
                        >
                            <button type="button" role="menuitem">"Comment"</button>
                            <button type="button" role="menuitem">"Approve"</button>
                            <button type="button" role="menuitem">"Request changes"</button>
                            <button type="button" role="menuitem">
                                {text!("Submit {} pending comments", self.props.review_comment_count)}
                            </button>
                        </div>
                    </div>
                    <button
                        data-component="IconButton"
                        type="button"
                        aria-haspopup="true"
                        aria-expanded="false"
                        tabindex="0"
                        class="toolbar-icon boxed prc-Button-ButtonBase-9n-Xk prc-Button-IconButton-fyge7"
                        data-loading="false"
                        data-no-visuals="true"
                        data-size="small"
                        data-variant="default"
                        aria-label="Open diff view settings"
                    >
                        {StaticRaw(ICON_GEAR)}
                    </button>
                    <span class="toolbar-divider"></span>
                    <button type="button" class="toolbar-icon" aria-label="Open overview panel">
                        <svg aria-hidden="true" viewBox="0 0 16 16">
                            <path d="M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z" />
                        </svg>
                    </button>
                    <button
                        type="button"
                        class="toolbar-button comments-button"
                        aria-label={text!("Open comments panel, {} comments", self.props.review_comment_count)}
                    >
                        <svg aria-hidden="true" viewBox="0 0 16 16">
                            <path d="M1.75 1h8.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 10.25 10H7.061l-2.574 2.573A1.458 1.458 0 0 1 2 11.543V10h-.25A1.75 1.75 0 0 1 0 8.25v-5.5C0 1.784.784 1 1.75 1ZM1.5 2.75v5.5c0 .138.112.25.25.25h1a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h3.5a.25.25 0 0 0 .25-.25v-5.5a.25.25 0 0 0-.25-.25h-8.5a.25.25 0 0 0-.25.25Zm13 2a.25.25 0 0 0-.25-.25h-.5a.75.75 0 0 1 0-1.5h.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 14.25 12H14v1.543a1.458 1.458 0 0 1-2.487 1.03L9.22 12.28a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215l2.22 2.22v-2.19a.75.75 0 0 1 .75-.75h1a.25.25 0 0 0 .25-.25Z" />
                        </svg>
                        <span class="comment-count">{self.props.review_comment_count}</span>
                    </button>
                    <button type="button" class="toolbar-icon boxed" aria-label="More options">
                        <svg aria-hidden="true" viewBox="0 0 16 16">
                            <path d="M8 9a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM1.5 9a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm13 0a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z" />
                        </svg>
                    </button>
                </div>
            </section>
            <div class="perf-strip" data-pr-stats>
                <span>
                    "Rows: "
                    <strong data-stat="rows">"..."</strong>
                </span>
                <span>
                    "DOM nodes: "
                    <strong data-stat="nodes">"..."</strong>
                </span>
                <span>
                    "Delegated listeners: "
                    <strong>"1"</strong>
                </span>
                <span data-stat="selection">"No line selected"</span>
            </div>
        }
        .render(cx)
    }
}

struct FileTree<'a> {
    props: &'a GithubPrDiffProps,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FileTree<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <aside class="file-tree" id="pr-file-tree" aria-label="Changed files">
                <div class="file-tree-controls">
                    <label class="tree-filter-label">
                        <span class="sr-only">"Filter changed files"</span>
                        <span class="tree-filter-icon" aria-hidden="true">
                            <svg viewBox="0 0 16 16">
                                <path d="M10.68 11.74a6 6 0 1 1 1.06-1.06l3.29 3.29a.75.75 0 0 1-1.06 1.06l-3.29-3.29ZM6.5 11a4.5 4.5 0 1 0 0-9 4.5 4.5 0 0 0 0 9Z" />
                            </svg>
                        </span>
                        <input type="search" value="" placeholder="Filter changed files" />
                    </label>
                </div>
                <div class="PullRequestFileTree-module__FileTreeScrollable__OW_Y2">
                    <h2 class="sr-only">"File tree"</h2>
                    <ul
                        role="tree"
                        aria-label="File Tree"
                        data-truncate-text="true"
                        class="tmp-pr-lg-3 prc-TreeView-TreeViewRootUlStyles-Mzrmj"
                    >
                        {FileTreeItems { props: self.props }}
                    </ul>
                </div>
            </aside>
        }
        .render(cx)
    }
}

struct FileTreeItems<'a> {
    props: &'a GithubPrDiffProps,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FileTreeItems<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let mut last_top = "";
        let mut last_parent = "";
        for file_idx in 0..self.props.total_files() {
            let path = self.props.file_path(file_idx);
            let top = first_segment(path);
            let parent = parent_segments(path);
            if top != last_top {
                TreeDirectory {
                    label: top,
                    level: 1,
                }
                .render(cx)?;
                last_top = top;
                last_parent = "";
            }
            if !parent.is_empty() && parent != last_parent {
                TreeDirectory {
                    label: parent,
                    level: 2,
                }
                .render(cx)?;
                last_parent = parent;
            }
            TreeFile {
                props: self.props,
                file_idx,
                level: if parent.is_empty() { 2 } else { 3 },
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct TreeDirectory<'a> {
    label: &'a str,
    level: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TreeDirectory<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <li
                role="treeitem"
                aria-expanded="true"
                aria-level={self.level}
                class="PRIVATE_TreeView-item file-tree-directory"
            >
                <button
                    type="button"
                    class="PRIVATE_TreeView-item-container tree-directory-button"
                    style={proa_core::style_ok(text!("--level:{};", self.level))}
                    data-tree-action="toggle-directory"
                    aria-expanded="true"
                >
                    <span class="PRIVATE_TreeView-item-toggle tree-chevron" aria-hidden="true">
                        <svg viewBox="0 0 16 16">
                            <path d="M6.22 3.22a.75.75 0 0 1 1.06 0l4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 1 1-1.06-1.06L9.94 8 6.22 4.28a.75.75 0 0 1 0-1.06Z" />
                        </svg>
                    </span>
                    <span class="tree-icon folder-icon" aria-hidden="true">
                        <svg viewBox="0 0 16 16">
                            <path d="M1.75 1A1.75 1.75 0 0 0 0 2.75v10.5C0 14.216.784 15 1.75 15h12.5A1.75 1.75 0 0 0 16 13.25v-8.5A1.75 1.75 0 0 0 14.25 3H7.5a.25.25 0 0 1-.2-.1L6.16 1.38A.95.95 0 0 0 5.4 1H1.75Z" />
                        </svg>
                    </span>
                    <span class="PRIVATE_TreeView-item-content-text tree-label">{self.label}</span>
                </button>
            </li>
        }
        .render(cx)
    }
}

struct TreeFile<'a> {
    props: &'a GithubPrDiffProps,
    file_idx: usize,
    level: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TreeFile<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let path = self.props.file_path(self.file_idx);

        html_sync! {
            <li
                role="treeitem"
                aria-level={self.level}
                class="PRIVATE_TreeView-item DiffFileTree-module__file-tree-row__PCB1B file-tree-file"
            >
                <a
                    class="PRIVATE_TreeView-item-container file-tree-link"
                    style={proa_core::style_ok(text!("--level:{};", self.level))}
                    href={DemoUrl::file_fragment(self.file_idx)}
                    data-pr-action="jump-file"
                    data-file-index={self.file_idx}
                    title={path}
                >
                    <span class="tree-spacer" aria-hidden="true"></span>
                    <span class="tree-icon file-icon" aria-hidden="true">
                        <svg viewBox="0 0 16 16">
                            <path d="M3.75 1.5a.25.25 0 0 0-.25.25v12.5c0 .14.11.25.25.25h8.5a.25.25 0 0 0 .25-.25V5h-2.75A1.75 1.75 0 0 1 8 3.25V1.5H3.75ZM2 1.75C2 .78 2.78 0 3.75 0h5.59c.46 0 .9.18 1.23.51l2.92 2.92c.33.33.51.77.51 1.24v9.58c0 .97-.78 1.75-1.75 1.75h-8.5A1.75 1.75 0 0 1 2 14.25V1.75Z" />
                        </svg>
                    </span>
                    <span class="PRIVATE_TreeView-item-content-text tree-label">
                        {basename(path)}
                    </span>
                </a>
            </li>
        }
        .render(cx)
    }
}

struct FileSection<'a> {
    props: &'a GithubPrDiffProps,
    file_idx: usize,
    lines: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FileSection<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let additions = self.props.file_additions(self.file_idx);
        let deletions = self.props.file_deletions(self.file_idx);
        let changed = additions + deletions;
        let row_count = self.props.file_row_count(self.file_idx);
        let intrinsic_height = row_count * 24 + 68;
        let custom_lines = self
            .props
            .file(self.file_idx)
            .map(|file| file.diff_lines.as_slice());
        let fixture_lines = if self.props.lines_per_file == DEFAULT_LINES_PER_FILE {
            github_pr_diff_fixtures::default_diff_lines(self.file_idx)
        } else {
            None
        };

        html_sync! {
            <section
                id={text!("file-{}", self.file_idx)}
                class="diff-file"
                data-file-section
                data-file-index={self.file_idx}
                style={proa_core::style_ok(text!("--intrinsic-height:{}px;", intrinsic_height))}
            >
                <div class="file-header DiffFileHeader-module__diff-file-header__UuNN4">
                    <button
                        type="button"
                        class="icon-button"
                        aria-label="Collapse file"
                        data-pr-action="toggle-file"
                        data-file-index={self.file_idx}
                    >
                        <svg aria-hidden="true" viewBox="0 0 16 16">
                            <path d="M12.78 5.22a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 0 1-1.06 0L3.22 6.28a.75.75 0 1 1 1.06-1.06L8 8.94l3.72-3.72a.75.75 0 0 1 1.06 0Z" />
                        </svg>
                    </button>
                    <span class="file-handle" aria-hidden="true">
                        <svg viewBox="0 0 16 16">
                            <path d="M7.25 2.5 5.5 4.25l1.06 1.06.44-.44v2.38H4.62l.44-.44L4 5.75 2.25 7.5 4 9.25l1.06-1.06-.44-.44H7v2.38l-.44-.44L5.5 10.75l1.75 1.75L9 10.75 7.94 9.69l-.44.44V7.75h2.38l-.44.44 1.06 1.06 1.75-1.75-1.75-1.75-1.06 1.06.44.44H7.5V4.87l.44.44L9 4.25 7.25 2.5Z" />
                        </svg>
                    </span>
                    <div class="file-change-map" aria-label={text!("{} changed lines", changed)}>
                        <span class="file-change-count">{changed}</span>
                        <span class="file-change-bars" aria-hidden="true">
                            {for idx in 0..5 {
                                let class = if idx < additions.min(5) {
                                    "change-square add-square"
                                } else if idx < (additions + deletions).min(5) {
                                    "change-square del-square"
                                } else {
                                    "change-square empty-square"
                                };
                                html_sync! { <span class={class}></span> }
                            }}
                        </span>
                    </div>
                    <div class="file-title DiffFileHeader-module__file-path-section__ZcmB1">
                        <h2>{FilePath { props: self.props, file_idx: self.file_idx }}</h2>
                        <span class="copy-icon" aria-hidden="true">
                            <svg viewBox="0 0 16 16">
                                <path d="M0 6.75C0 5.78.78 5 1.75 5h7.5c.97 0 1.75.78 1.75 1.75v7.5c0 .97-.78 1.75-1.75 1.75h-7.5A1.75 1.75 0 0 1 0 14.25v-7.5ZM1.75 6.5a.25.25 0 0 0-.25.25v7.5c0 .14.11.25.25.25h7.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25h-7.5ZM5 1.75C5 .78 5.78 0 6.75 0h7.5C15.22 0 16 .78 16 1.75v7.5c0 .97-.78 1.75-1.75 1.75H13.5V9.5h.75a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25h-7.5a.25.25 0 0 0-.25.25V2.5H5v-.75Z" />
                            </svg>
                        </span>
                    </div>
                    <div class="dropdown-wrap file-more-wrap">
                        <button
                            type="button"
                            class="icon-button file-more"
                            aria-label="More options"
                            aria-haspopup="true"
                            aria-expanded="false"
                            data-menu-button
                            data-menu-target={text!("file-menu-{}", self.file_idx)}
                            data-pr-action="show-stats"
                        >
                            <svg aria-hidden="true" viewBox="0 0 16 16">
                                <path d="M8 9.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM1.5 9.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm13 0a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z" />
                            </svg>
                        </button>
                        <div
                            class="pr-menu file-menu"
                            id={text!("file-menu-{}", self.file_idx)}
                            role="menu"
                            hidden
                        >
                            <button
                                type="button"
                                role="menuitem"
                                data-pr-action="mark-reviewed"
                                data-file-index={self.file_idx}
                            >
                                "Mark as viewed"
                            </button>
                            <button
                                type="button"
                                role="menuitem"
                                data-pr-action="toggle-file"
                                data-file-index={self.file_idx}
                            >
                                "Collapse file"
                            </button>
                            <button type="button" role="menuitem">"Copy file path"</button>
                            <button type="button" role="menuitem">"View file"</button>
                        </div>
                    </div>
                </div>
                <div class="diff-table-wrap">
                    <table
                        class="diff-table"
                        role="grid"
                        aria-label={text!("Diff for file {}", self.file_idx + 1)}
                    >
                        <colgroup>
                            <col class="line-number-col" />
                            <col class="line-number-col" />
                            <col />
                        </colgroup>
                        <tbody>
                            {if let Some(lines) = custom_lines {
                                if !lines.is_empty() {
                                    if prop_lines_match_fixture(self.file_idx, lines) {
                                        FixtureFileRows {
                                            file_idx: self.file_idx,
                                        }
                                    } else {
                                        for line_idx in 0..lines.len() {
                                            PropDiffLine {
                                                file_idx: self.file_idx,
                                                line_idx,
                                                line: &lines[line_idx],
                                            }
                                        }
                                    }
                                } else if fixture_lines.is_some() {
                                    FixtureFileRows {
                                        file_idx: self.file_idx,
                                    }
                                } else {
                                    if self.lines > 0 {
                                        HunkRow { file_idx: self.file_idx }.render(cx)?;
                                    }
                                    for line_idx in 0..self.lines {
                                        DiffLine {
                                            file_idx: self.file_idx,
                                            line_idx,
                                        }
                                    }
                                }
                            } else if fixture_lines.is_some() {
                                FixtureFileRows {
                                    file_idx: self.file_idx,
                                }
                            } else {
                                if self.lines > 0 {
                                    HunkRow { file_idx: self.file_idx }.render(cx)?;
                                }
                                for line_idx in 0..self.lines {
                                    DiffLine {
                                        file_idx: self.file_idx,
                                        line_idx,
                                    }
                                }
                            }}
                        </tbody>
                    </table>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct FilePath<'a> {
    props: &'a GithubPrDiffProps,
    file_idx: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FilePath<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let path = self.props.file_path(self.file_idx);
        let shard = if self.props.files.is_empty() {
            self.file_idx / FILE_PATHS.len()
        } else {
            self.file_idx / self.props.files.len()
        };
        html_sync! {
            {path}
            {if shard > 0 {
                html_sync! {
                    <span class="path-shard">" shard " {shard}</span>
                }
            }}
        }
        .render(cx)
    }
}

struct HunkRow {
    file_idx: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HunkRow {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <tr class="diff-line-row hunk-row">
                <td class="hunk-control" colspan="2">
                    <button
                        type="button"
                        data-pr-action="expand-hunk"
                        data-file-index={self.file_idx}
                    >
                        <svg aria-hidden="true" viewBox="0 0 16 16">
                            <path d="M8 1.75 5.75 4h1.5v3.25H4l1.5-1.5L4.44 4.69 1.13 8l3.31 3.31 1.06-1.06-1.5-1.5h3.25V12h-1.5L8 14.25 10.25 12h-1.5V8.75H12l-1.5 1.5 1.06 1.06L14.87 8l-3.31-3.31-1.06 1.06 1.5 1.5H8.75V4h1.5L8 1.75Z" />
                        </svg>
                    </button>
                </td>
                <td class="code hunk-code">
                    <code>{hunk_label(self.file_idx)}</code>
                </td>
            </tr>
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLineKind {
    Hunk,
    Context,
    Addition,
    Deletion,
}

impl DiffLineKind {
    const fn for_line(line_idx: usize) -> Self {
        match line_idx % 12 {
            3 | 9 => Self::Deletion,
            4 | 5 | 6 | 10 => Self::Addition,
            _ => Self::Context,
        }
    }
}

pub(crate) struct DiffLineFixture {
    pub(crate) kind: DiffLineKind,
    pub(crate) old_line: Option<usize>,
    pub(crate) new_line: Option<usize>,
    pub(crate) text: &'static str,
}

impl DiffLineKind {
    fn from_label(label: &str) -> Self {
        if label.eq_ignore_ascii_case("hunk") {
            Self::Hunk
        } else if label.eq_ignore_ascii_case("addition") || label.eq_ignore_ascii_case("added") {
            Self::Addition
        } else if label.eq_ignore_ascii_case("deletion") || label.eq_ignore_ascii_case("deleted") {
            Self::Deletion
        } else {
            Self::Context
        }
    }

    const fn row_class(self) -> &'static str {
        match self {
            Self::Hunk => "diff-line-row hunk-row",
            Self::Addition => "diff-line-row diff-row is-addition",
            Self::Deletion => "diff-line-row diff-row is-deletion",
            Self::Context => "diff-line-row diff-row",
        }
    }

    const fn code_class(self) -> &'static str {
        match self {
            Self::Addition => "diff-text syntax-highlighted-line addition",
            Self::Deletion => "diff-text syntax-highlighted-line deletion",
            _ => "diff-text syntax-highlighted-line",
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Addition => "+",
            Self::Deletion => "-",
            _ => " ",
        }
    }

    const fn prop_kind(self) -> &'static str {
        match self {
            Self::Hunk => "hunk",
            Self::Addition => "addition",
            Self::Deletion => "deletion",
            Self::Context => "context",
        }
    }
}

struct FixtureFileRows {
    file_idx: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FixtureFileRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if let Some(rows) = fixture_file_rows(self.file_idx) {
            html_sync! {
                {StaticRaw(rows)}
            }
            .render(cx)
        } else {
            Ok(())
        }
    }
}

struct PropDiffLine<'a> {
    file_idx: usize,
    line_idx: usize,
    line: &'a GithubPrDiffLineProps,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PropDiffLine<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        RenderedDiffLine {
            file_idx: self.file_idx,
            line_idx: self.line_idx,
            kind: DiffLineKind::from_label(self.line.kind.as_str()),
            old_line: self.line.old_line,
            new_line: self.line.new_line,
            text: self.line.text.as_str(),
            highlighted_html: None,
        }
        .render(cx)
    }
}

struct RenderedDiffLine<'a> {
    file_idx: usize,
    line_idx: usize,
    kind: DiffLineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    text: &'a str,
    highlighted_html: Option<&'static str>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for RenderedDiffLine<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if self.kind == DiffLineKind::Hunk {
            return html_sync! {
                <tr class="diff-line-row hunk-row">
                    <td class="hunk-control" colspan="2">
                        <button
                            type="button"
                            data-pr-action="expand-hunk"
                            data-file-index={self.file_idx}
                        >
                            <svg aria-hidden="true" viewBox="0 0 16 16">
                                <path d="M8 1.75 5.75 4h1.5v3.25H4l1.5-1.5L4.44 4.69 1.13 8l3.31 3.31 1.06-1.06-1.5-1.5h3.25V12h-1.5L8 14.25 10.25 12h-1.5V8.75H12l-1.5 1.5 1.06 1.06L14.87 8l-3.31-3.31-1.06 1.06 1.5 1.5H8.75V4h1.5L8 1.75Z" />
                            </svg>
                        </button>
                    </td>
                    <td class="code hunk-code">
                        <code>{self.text}</code>
                    </td>
                </tr>
            }
            .render(cx);
        }

        let old_class = line_number_class(self.kind, true, self.old_line.is_some());
        let new_class = line_number_class(self.kind, false, self.new_line.is_some());
        let (comment_side, comment_line) = comment_target(self.old_line, self.new_line);
        html_sync! {
            <tr
                class={self.kind.row_class()}
                data-line
                data-file-index={self.file_idx}
                data-line-index={self.line_idx}
            >
                <td class={old_class}>
                    {if let Some(line) = self.old_line { html_sync! {
                        {line}
                    } }}
                </td>
                <td class={new_class}>
                    {if let Some(line) = self.new_line { html_sync! {
                        {line}
                    } }}
                </td>
                <td class="code diff-text-cell">
                    <button
                        type="button"
                        class="line-comment-button"
                        aria-label="Add comment on this line"
                        data-pr-comment-button
                        data-file-index={self.file_idx}
                        data-line-index={self.line_idx}
                        data-comment-side={comment_side}
                        data-comment-line={comment_line}
                    >
                        <span class="line-comment-plus" aria-hidden="true">"+"</span>
                    </button>
                    <code class={self.kind.code_class()}>
                        <span class="line-marker diff-text-marker">{self.kind.marker()}</span>
                        <span class="code-inner diff-text-inner">
                            {HighlightedLine { text: self.text, html: self.highlighted_html }}
                        </span>
                    </code>
                </td>
            </tr>
        }
        .render(cx)
    }
}

struct HighlightedLine<'a> {
    text: &'a str,
    html: Option<&'static str>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HighlightedLine<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if let Some(html) = self.html {
            html_sync! {
                {StaticRaw(html)}
            }
            .render(cx)
        } else {
            HighlightedCode { text: self.text }.render(cx)
        }
    }
}

struct HighlightedCode<'a> {
    text: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HighlightedCode<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let bytes = self.text.as_bytes();
        let mut i = 0;
        let mut plain_start = 0;

        while i < bytes.len() {
            let b = bytes[i];
            if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                render_code_plain(&self.text[plain_start..i], cx)?;
                render_code_span("pl-c comment", &self.text[i..], cx)?;
                return Ok(());
            } else if b == b'"' || b == b'\'' || b == b'`' {
                let end = scan_string(bytes, i);
                render_code_plain(&self.text[plain_start..i], cx)?;
                render_code_span("pl-s", &self.text[i..end], cx)?;
                i = end;
                plain_start = i;
            } else if is_ascii_ident_start(b) {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ascii_ident_part(bytes[i]) {
                    i += 1;
                }
                let token = &self.text[start..i];
                if let Some(class) = highlighted_identifier_class(token, bytes, i) {
                    render_code_plain(&self.text[plain_start..start], cx)?;
                    render_code_span(class, token, cx)?;
                    plain_start = i;
                }
            } else if b.is_ascii_digit() {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
                {
                    i += 1;
                }
                render_code_plain(&self.text[plain_start..start], cx)?;
                render_code_span("pl-c1", &self.text[start..i], cx)?;
                plain_start = i;
            } else if b.is_ascii() {
                i += 1;
            } else {
                let ch = self.text[i..].chars().next().unwrap_or('\0');
                i += ch.len_utf8();
            }
        }

        render_code_plain(&self.text[plain_start..], cx)
    }
}

fn render_code_plain<B: WriteBuf, L: DataLoader>(
    fragment: &str,
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    if !fragment.is_empty() {
        html_sync! {
            {fragment}
        }
        .render(cx)?;
    }
    Ok(())
}

fn render_code_span<B: WriteBuf, L: DataLoader>(
    class: &'static str,
    fragment: &str,
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    if !fragment.is_empty() {
        html_sync! {
            <span class={class}>{fragment}</span>
        }
        .render(cx)?;
    }
    Ok(())
}

fn fixture_file_rows(file_idx: usize) -> Option<&'static str> {
    FIXTURE_FILE_ROWS
        .get_or_init(build_fixture_file_rows)
        .get(file_idx)
        .copied()
}

fn prop_lines_match_fixture(file_idx: usize, lines: &[GithubPrDiffLineProps]) -> bool {
    let Some(fixtures) = github_pr_diff_fixtures::default_diff_lines(file_idx) else {
        return false;
    };
    if lines.len() != fixtures.len() {
        return false;
    }

    lines.iter().zip(fixtures).all(|(line, fixture)| {
        DiffLineKind::from_label(line.kind.as_str()) == fixture.kind
            && line.old_line == fixture.old_line
            && line.new_line == fixture.new_line
            && line.text == fixture.text
    })
}

fn build_fixture_file_rows() -> Vec<&'static str> {
    // proa-perf-ok: demo fixtures allocate once during process-lifetime pre-rendering
    let mut files = Vec::with_capacity(DEFAULT_FILE_COUNT);
    for file_idx in 0..DEFAULT_FILE_COUNT {
        let Some(lines) = github_pr_diff_fixtures::default_diff_lines(file_idx) else {
            files.push("");
            continue;
        };

        files.push(render_fixture_file_rows(file_idx, lines));
    }
    files
}

fn render_fixture_file_rows(file_idx: usize, lines: &'static [DiffLineFixture]) -> &'static str {
    // proa-perf-ok: demo fixtures allocate once during process-lifetime pre-rendering
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    for (line_idx, line) in lines.iter().enumerate() {
        let highlighted_html = if line.kind == DiffLineKind::Hunk {
            None
        } else {
            Some(render_highlighted_fixture_line(line.text))
        };
        let _ = RenderedDiffLine {
            file_idx,
            line_idx,
            kind: line.kind,
            old_line: line.old_line,
            new_line: line.new_line,
            text: line.text,
            highlighted_html,
        }
        .render(&mut cx);
    }
    let html = String::from_utf8(out).unwrap_or_default();
    leak_fixture_html(html)
}

fn render_highlighted_fixture_line(text: &'static str) -> &'static str {
    if text.is_empty() {
        return "";
    }

    // proa-perf-ok: demo fixtures allocate once during process-lifetime pre-rendering
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    let _ = HighlightedCode { text }.render(&mut cx);
    let html = String::from_utf8(out).unwrap_or_default();
    leak_fixture_html(html)
}

fn leak_fixture_html(html: String) -> &'static str {
    // Static demo fixtures live for the process lifetime so hot renders can stream them as StaticRaw.
    Box::leak(html.into_boxed_str())
}

const fn is_ascii_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic()
}

const fn is_ascii_ident_part(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

fn scan_string(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
        } else if bytes[i] == quote {
            return i + 1;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn highlighted_identifier_class(token: &str, bytes: &[u8], end: usize) -> Option<&'static str> {
    if is_keyword(token) {
        Some("pl-k")
    } else if is_literal(token) {
        Some("pl-c1")
    } else if next_non_ws(bytes, end) == Some(b'(') {
        Some("pl-en")
    } else {
        None
    }
}

fn next_non_ws(bytes: &[u8], mut end: usize) -> Option<u8> {
    while end < bytes.len() {
        if !bytes[end].is_ascii_whitespace() {
            return Some(bytes[end]);
        }
        end += 1;
    }
    None
}

fn is_keyword(token: &str) -> bool {
    matches!(
        token,
        "and"
            | "as"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "else"
            | "export"
            | "fn"
            | "for"
            | "from"
            | "function"
            | "if"
            | "import"
            | "in"
            | "let"
            | "or"
            | "pub"
            | "return"
            | "test"
            | "try"
            | "var"
            | "while"
    )
}

fn is_literal(token: &str) -> bool {
    matches!(token, "false" | "null" | "true" | "undefined")
}

fn comment_target(old_line: Option<usize>, new_line: Option<usize>) -> (&'static str, usize) {
    if let Some(line) = new_line {
        ("R", line)
    } else if let Some(line) = old_line {
        ("L", line)
    } else {
        ("R", 0)
    }
}

const fn line_number_class(kind: DiffLineKind, old_side: bool, has_value: bool) -> &'static str {
    if !has_value {
        if old_side {
            "line-number old empty-diff-line"
        } else {
            "line-number new empty-diff-line"
        }
    } else {
        match kind {
            DiffLineKind::Addition => "line-number new blob-num-addition",
            DiffLineKind::Deletion => "line-number old blob-num-deletion",
            _ => {
                if old_side {
                    "line-number old diff-line-number-neutral"
                } else {
                    "line-number new diff-line-number-neutral"
                }
            }
        }
    }
}

struct DiffLine {
    file_idx: usize,
    line_idx: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DiffLine {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let kind = DiffLineKind::for_line(self.line_idx);
        let (old_line, new_line) = generated_line_numbers(self.file_idx, self.line_idx);
        match (kind, self.line_idx % 12) {
            (DiffLineKind::Hunk, _) => HunkRow {
                file_idx: self.file_idx,
            }
            .render(cx),
            (DiffLineKind::Addition, 4 | 10) => html_sync! {
                <tr
                    class="diff-line-row diff-row is-addition"
                    data-line
                    data-file-index={self.file_idx}
                    data-line-index={self.line_idx}
                >
                    <td class="line-number old empty-diff-line"></td>
                    <td class="line-number new blob-num-addition">{new_line}</td>
                    <td class="code diff-text-cell">
                        <code class="diff-text syntax-highlighted-line addition">
                            <span class="line-marker diff-text-marker">"+"</span>
                            <span class="code-inner diff-text-inner">
                                "    "
                                <span class="pl-k">"const"</span>
                                " "
                                <span class="pl-s1">
                                    "lowered_"
                                    {self.file_idx}
                                    "_"
                                    {self.line_idx}
                                </span>
                                " "
                                <span class="pl-c1">"="</span>
                                " "
                                <span class="pl-k">"try"</span>
                                " "
                                <span class="pl-s1">"parser"</span>
                                <span class="pl-kos">"."</span>
                                <span class="pl-en">"nextToken"</span>
                                <span class="pl-kos">"("</span>
                                <span class="pl-kos">")"</span>
                                <span class="pl-kos">";"</span>
                            </span>
                        </code>
                    </td>
                </tr>
            }
            .render(cx),
            (DiffLineKind::Addition, 5) => html_sync! {
                <tr
                    class="diff-line-row diff-row is-addition"
                    data-line
                    data-file-index={self.file_idx}
                    data-line-index={self.line_idx}
                >
                    <td class="line-number old empty-diff-line"></td>
                    <td class="line-number new blob-num-addition">{new_line}</td>
                    <td class="code diff-text-cell">
                        <code class="diff-text syntax-highlighted-line addition">
                            <span class="line-marker diff-text-marker">"+"</span>
                            <span class="code-inner diff-text-inner">
                                "        "
                                <span class="pl-k">"try"</span>
                                " "
                                <span class="pl-s1">"zig_port"</span>
                                <span class="pl-kos">"."</span>
                                <span class="pl-en">"emitNode"</span>
                                <span class="pl-kos">"("</span>
                                <span class="pl-s1">"ctx"</span>
                                <span class="pl-kos">","</span>
                                " "
                                <span class="pl-s1">
                                    "lowered_"
                                    {self.file_idx}
                                    "_"
                                    {self.line_idx}
                                </span>
                                <span class="pl-kos">")"</span>
                                <span class="pl-kos">";"</span>
                            </span>
                        </code>
                    </td>
                </tr>
            }
            .render(cx),
            (DiffLineKind::Addition, _) => html_sync! {
                <tr
                    class="diff-line-row diff-row is-addition"
                    data-line
                    data-file-index={self.file_idx}
                    data-line-index={self.line_idx}
                >
                    <td class="line-number old empty-diff-line"></td>
                    <td class="line-number new blob-num-addition">{new_line}</td>
                    <td class="code diff-text-cell">
                        <code class="diff-text syntax-highlighted-line addition">
                            <span class="line-marker diff-text-marker">"+"</span>
                            <span class="code-inner diff-text-inner">
                                "        "
                                <span class="pl-s1">"state"</span>
                                <span class="pl-kos">"."</span>
                                <span class="pl-s1">"codegen"</span>
                                <span class="pl-kos">"."</span>
                                <span class="pl-s1">"items"</span>
                                <span class="pl-kos">"["</span>
                                <span class="pl-c1">{self.file_idx}</span>
                                <span class="pl-kos">"]"</span>
                                <span class="pl-kos">"."</span>
                                <span class="pl-en">"appendAssumeCapacity"</span>
                                <span class="pl-kos">"("</span>
                                <span class="pl-s1">"token_"{self.line_idx}</span>
                                <span class="pl-kos">")"</span>
                                <span class="pl-kos">";"</span>
                            </span>
                        </code>
                    </td>
                </tr>
            }
            .render(cx),
            (DiffLineKind::Deletion, 3) => html_sync! {
                <tr
                    class="diff-line-row diff-row is-deletion"
                    data-line
                    data-file-index={self.file_idx}
                    data-line-index={self.line_idx}
                >
                    <td class="line-number old blob-num-deletion">{old_line}</td>
                    <td class="line-number new empty-diff-line"></td>
                    <td class="code diff-text-cell">
                        <code class="diff-text syntax-highlighted-line deletion">
                            <span class="line-marker diff-text-marker">"-"</span>
                            <span class="code-inner diff-text-inner">
                                "    "
                                <span class="pl-k old">"return"</span>
                                " "
                                <span class="pl-s1">"rust_bridge"</span>
                                <span class="pl-kos">"."</span>
                                <span class="pl-en">"lower_property"</span>
                                <span class="pl-kos">"("</span>
                                <span class="pl-s1">"cx"</span>
                                <span class="pl-kos">","</span>
                                " "
                                <span class="pl-s1">"node"</span>
                                <span class="pl-kos">")"</span>
                                <span class="pl-kos">";"</span>
                            </span>
                        </code>
                    </td>
                </tr>
            }
            .render(cx),
            (DiffLineKind::Deletion, _) => html_sync! {
                <tr
                    class="diff-line-row diff-row is-deletion"
                    data-line
                    data-file-index={self.file_idx}
                    data-line-index={self.line_idx}
                >
                    <td class="line-number old blob-num-deletion">{old_line}</td>
                    <td class="line-number new empty-diff-line"></td>
                    <td class="code diff-text-cell">
                        <code class="diff-text syntax-highlighted-line deletion">
                            <span class="line-marker diff-text-marker">"-"</span>
                            <span class="code-inner diff-text-inner">
                                "    "
                                <span class="pl-s1">"legacyRustPort"</span>
                                <span class="pl-kos">"."</span>
                                <span class="pl-en">"recordFallback"</span>
                                <span class="pl-kos">"("</span>
                                <span class="pl-s1">"file_"{self.file_idx}</span>
                                <span class="pl-kos">","</span>
                                " "
                                <span class="pl-s1">"span_"{self.line_idx}</span>
                                <span class="pl-kos">")"</span>
                                <span class="pl-kos">";"</span>
                            </span>
                        </code>
                    </td>
                </tr>
            }
            .render(cx),
            (DiffLineKind::Context, 7 | 11) => html_sync! {
                <tr
                    class="diff-line-row diff-row"
                    data-line
                    data-file-index={self.file_idx}
                    data-line-index={self.line_idx}
                >
                    <td class="line-number old diff-line-number-neutral">{old_line}</td>
                    <td class="line-number new diff-line-number-neutral">{new_line}</td>
                    <td class="code diff-text-cell">
                        <code class="diff-text syntax-highlighted-line">
                            <span class="line-marker diff-text-marker">" "</span>
                            <span class="code-inner diff-text-inner">
                                "    "
                                <span class="pl-c comment">
                                    "// Shared parser state retained around generated port boundary"
                                </span>
                            </span>
                        </code>
                    </td>
                </tr>
            }
            .render(cx),
            (DiffLineKind::Context, _) => html_sync! {
                <tr
                    class="diff-line-row diff-row"
                    data-line
                    data-file-index={self.file_idx}
                    data-line-index={self.line_idx}
                >
                    <td class="line-number old diff-line-number-neutral">{old_line}</td>
                    <td class="line-number new diff-line-number-neutral">{new_line}</td>
                    <td class="code diff-text-cell">
                        <code class="diff-text syntax-highlighted-line">
                            <span class="line-marker diff-text-marker">" "</span>
                            <span class="code-inner diff-text-inner">
                                "    "
                                <span class="pl-k">"if"</span>
                                " "
                                <span class="pl-kos">"("</span>
                                <span class="pl-s1">"token_"{self.line_idx}</span>
                                " "
                                <span class="pl-c1">"=="</span>
                                " "
                                <span class="pl-kos">"."</span>
                                <span class="pl-c1">"identifier"</span>
                                <span class="pl-kos">")"</span>
                                " "
                                <span class="pl-k">"continue"</span>
                                " "
                                <span class="pl-c1">":parse_loop"</span>
                                <span class="pl-kos">";"</span>
                            </span>
                        </code>
                    </td>
                </tr>
            }
            .render(cx),
        }
    }
}

fn generated_line_numbers(file_idx: usize, line_idx: usize) -> (usize, usize) {
    let (mut old_line, mut new_line) = generated_hunk_starts(file_idx);
    let mut idx = 0;
    while idx < line_idx {
        match DiffLineKind::for_line(idx) {
            DiffLineKind::Addition => {
                new_line += 1;
            }
            DiffLineKind::Deletion => {
                old_line += 1;
            }
            _ => {
                old_line += 1;
                new_line += 1;
            }
        }
        idx += 1;
    }
    (old_line, new_line)
}

const fn generated_hunk_starts(file_idx: usize) -> (usize, usize) {
    match file_idx % FILE_PATHS.len() {
        0 => (459, 459),
        1 => (436, 436),
        2 => (318, 318),
        3 => (98, 98),
        _ => (41, 42),
    }
}

fn first_segment(path: &str) -> &str {
    if let Some(pos) = path.find('/') {
        &path[..pos]
    } else {
        path
    }
}

fn parent_segments(path: &str) -> &str {
    if let (Some(first), Some(last)) = (path.find('/'), path.rfind('/')) {
        if last > first {
            return &path[first + 1..last];
        }
    }
    ""
}

fn basename(path: &str) -> &str {
    if let Some(pos) = path.rfind('/') {
        &path[pos + 1..]
    } else {
        path
    }
}

fn hunk_label(file_idx: usize) -> &'static str {
    match file_idx % FILE_PATHS.len() {
        0 => "@@ -459,7 +459,9 @@ pub fn edit(",
        1 => "@@ -436,7 +436,9 @@ EventEmitterPrototype.eventNames = function eventNames() {",
        2 => "@@ -318,7 +318,7 @@ pub fn ParseProperty(",
        3 => "@@ -98,6 +98,8 @@ pub fn nextToken(",
        _ => "@@ -41,7 +42,9 @@ parser/state bridge",
    }
}

const ICON_SIDEBAR_COLLAPSE: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-sidebar-collapse" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M6.823 7.823a.25.25 0 0 1 0 .354l-2.396 2.396A.25.25 0 0 1 4 10.396V5.604a.25.25 0 0 1 .427-.177Z"></path><path d="M1.75 0h12.5C15.216 0 16 .784 16 1.75v12.5A1.75 1.75 0 0 1 14.25 16H1.75A1.75 1.75 0 0 1 0 14.25V1.75C0 .784.784 0 1.75 0ZM1.5 1.75v12.5c0 .138.112.25.25.25H9.5v-13H1.75a.25.25 0 0 0-.25.25ZM11 14.5h3.25a.25.25 0 0 0 .25-.25V1.75a.25.25 0 0 0-.25-.25H11Z"></path></svg>"#;

const ICON_SIDEBAR_EXPAND: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-sidebar-expand" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="m4.177 7.823 2.396-2.396A.25.25 0 0 1 7 5.604v4.792a.25.25 0 0 1-.427.177L4.177 8.177a.25.25 0 0 1 0-.354Z"></path><path d="M0 1.75C0 .784.784 0 1.75 0h12.5C15.216 0 16 .784 16 1.75v12.5A1.75 1.75 0 0 1 14.25 16H1.75A1.75 1.75 0 0 1 0 14.25Zm1.75-.25a.25.25 0 0 0-.25.25v12.5c0 .138.112.25.25.25H9.5v-13Zm12.5 13a.25.25 0 0 0 .25-.25V1.75a.25.25 0 0 0-.25-.25H11v13Z"></path></svg>"#;

const ICON_PULL_REQUEST_CLOSED: &str = r#"<svg data-component="Octicon" focusable="false" aria-label="Pull request" class="octicon octicon-git-pull-request-closed prc-StateLabel-Icon-YICrR" role="img" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M3.25 1A2.25 2.25 0 0 1 4 5.372v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.251 2.251 0 0 1 3.25 1Zm9.5 5.5a.75.75 0 0 1 .75.75v3.378a2.251 2.251 0 1 1-1.5 0V7.25a.75.75 0 0 1 .75-.75Zm-2.03-5.273a.75.75 0 0 1 1.06 0l.97.97.97-.97a.748.748 0 0 1 1.265.332.75.75 0 0 1-.205.729l-.97.97.97.97a.751.751 0 0 1-.018 1.042.751.751 0 0 1-1.042.018l-.97-.97-.97.97a.749.749 0 0 1-1.275-.326.749.749 0 0 1 .215-.734l.97-.97-.97-.97a.75.75 0 0 1 0-1.06ZM2.5 3.25a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0ZM3.25 12a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5Zm9.5 0a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Z"></path></svg>"#;

const ICON_VERSIONS: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-versions" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M7.75 14A1.75 1.75 0 0 1 6 12.25v-8.5C6 2.784 6.784 2 7.75 2h6.5c.966 0 1.75.784 1.75 1.75v8.5A1.75 1.75 0 0 1 14.25 14Zm-.25-1.75c0 .138.112.25.25.25h6.5a.25.25 0 0 0 .25-.25v-8.5a.25.25 0 0 0-.25-.25h-6.5a.25.25 0 0 0-.25.25ZM4.9 3.508a.75.75 0 0 1-.274 1.025.249.249 0 0 0-.126.217v6.5c0 .09.048.173.126.217a.75.75 0 0 1-.752 1.298A1.75 1.75 0 0 1 3 11.25v-6.5c0-.649.353-1.214.874-1.516a.75.75 0 0 1 1.025.274ZM1.625 5.533h.001a.249.249 0 0 0-.126.217v4.5c0 .09.048.173.126.217a.75.75 0 0 1-.752 1.298A1.748 1.748 0 0 1 0 10.25v-4.5a1.748 1.748 0 0 1 .873-1.516.75.75 0 1 1 .752 1.299Z"></path></svg>"#;

const ICON_TRIANGLE_DOWN: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-triangle-down" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="m4.427 7.427 3.396 3.396a.25.25 0 0 0 .354 0l3.396-3.396A.25.25 0 0 0 11.396 7H4.604a.25.25 0 0 0-.177.427Z"></path></svg>"#;

const ICON_GEAR: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-gear" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M8 0a8.2 8.2 0 0 1 .701.031C9.444.095 9.99.645 10.16 1.29l.288 1.107c.018.066.079.158.212.224.231.114.454.243.668.386.123.082.233.09.299.071l1.103-.303c.644-.176 1.392.021 1.82.63.27.385.506.792.704 1.218.315.675.111 1.422-.364 1.891l-.814.806c-.049.048-.098.147-.088.294.016.257.016.515 0 .772-.01.147.038.246.088.294l.814.806c.475.469.679 1.216.364 1.891a7.977 7.977 0 0 1-.704 1.217c-.428.61-1.176.807-1.82.63l-1.102-.302c-.067-.019-.177-.011-.3.071a5.909 5.909 0 0 1-.668.386c-.133.066-.194.158-.211.224l-.29 1.106c-.168.646-.715 1.196-1.458 1.26a8.006 8.006 0 0 1-1.402 0c-.743-.064-1.289-.614-1.458-1.26l-.289-1.106c-.018-.066-.079-.158-.212-.224a5.738 5.738 0 0 1-.668-.386c-.123-.082-.233-.09-.299-.071l-1.103.303c-.644.176-1.392-.021-1.82-.63a8.12 8.12 0 0 1-.704-1.218c-.315-.675-.111-1.422.363-1.891l.815-.806c.05-.048.098-.147.088-.294a6.214 6.214 0 0 1 0-.772c.01-.147-.038-.246-.088-.294l-.815-.806C.635 6.045.431 5.298.746 4.623a7.92 7.92 0 0 1 .704-1.217c.428-.61 1.176-.807 1.82-.63l1.102.302c.067.019.177.011.3-.071.214-.143.437-.272.668-.386.133-.066.194-.158.211-.224l.29-1.106C6.009.645 6.556.095 7.299.03 7.53.01 7.764 0 8 0Zm-.571 1.525c-.036.003-.108.036-.137.146l-.289 1.105c-.147.561-.549.967-.998 1.189-.173.086-.34.183-.5.29-.417.278-.97.423-1.529.27l-1.103-.303c-.109-.03-.175.016-.195.045-.22.312-.412.644-.573.99-.014.031-.021.11.059.19l.815.806c.411.406.562.957.53 1.456a4.709 4.709 0 0 0 0 .582c.032.499-.119 1.05-.53 1.456l-.815.806c-.081.08-.073.159-.059.19.162.346.353.677.573.989.02.03.085.076.195.046l1.102-.303c.56-.153 1.113-.008 1.53.27.161.107.328.204.501.29.447.222.85.629.997 1.189l.289 1.105c.029.109.101.143.137.146a6.6 6.6 0 0 0 1.142 0c.036-.003.108-.036.137-.146l.289-1.105c.147-.561.549-.967.998-1.189.173-.086.34-.183.5-.29.417-.278.97-.423 1.529-.27l1.103.303c.109.029.175-.016.195-.045.22-.313.411-.644.573-.99.014-.031.021-.11-.059-.19l-.815-.806c-.411-.406-.562-.957-.53-1.456a4.709 4.709 0 0 0 0-.582c-.032-.499.119-1.05.53-1.456l.815-.806c.081-.08.073-.159.059-.19a6.464 6.464 0 0 0-.573-.989c-.02-.03-.085-.076-.195-.046l-1.102.303c-.56.153-1.113.008-1.53-.27a4.44 4.44 0 0 0-.501-.29c-.447-.222-.85-.629-.997-1.189l-.289-1.105c-.029-.11-.101-.143-.137-.146a6.6 6.6 0 0 0-1.142 0ZM11 8a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM9.5 8a1.5 1.5 0 1 0-3.001.001A1.5 1.5 0 0 0 9.5 8Z"></path></svg>"#;

const ICON_GIT_COMMIT: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-git-commit fg-muted mr-2 d-none d-sm-inline-block" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align: text-bottom;"><path d="M11.93 8.5a4.002 4.002 0 0 1-7.86 0H.75a.75.75 0 0 1 0-1.5h3.32a4.002 4.002 0 0 1 7.86 0h3.32a.75.75 0 0 1 0 1.5Zm-1.43-.75a2.5 2.5 0 1 0-5 0 2.5 2.5 0 0 0 5 0Z"></path></svg>"#;

const ICON_PLUS: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-plus" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M7.75 2a.75.75 0 0 1 .75.75V7h4.25a.75.75 0 0 1 0 1.5H8.5v4.25a.75.75 0 0 1-1.5 0V8.5H2.75a.75.75 0 0 1 0-1.5H7V2.75A.75.75 0 0 1 7.75 2Z"></path></svg>"#;

const ICON_ISSUE_OPENED: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-issue-opened" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M8 9.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z"></path><path d="M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0ZM1.5 8a6.5 6.5 0 1 0 13 0 6.5 6.5 0 0 0-13 0Z"></path></svg>"#;

const ICON_GIT_PULL_REQUEST: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-git-pull-request" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M1.5 3.25a2.25 2.25 0 1 1 3 2.122v5.256a2.251 2.251 0 1 1-1.5 0V5.372A2.25 2.25 0 0 1 1.5 3.25Zm5.677-.177L9.573.677A.25.25 0 0 1 10 .854V2.5h1A2.5 2.5 0 0 1 13.5 5v5.628a2.251 2.251 0 1 1-1.5 0V5a1 1 0 0 0-1-1h-1v1.646a.25.25 0 0 1-.427.177L7.177 3.427a.25.25 0 0 1 0-.354ZM3.75 2.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm0 9.5a.75.75 0 1 0 0 1.5.75.75.75 0 0 0 0-1.5Zm8.25.75a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0Z"></path></svg>"#;

const ICON_REPO: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-repo" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M2 2.5A2.5 2.5 0 0 1 4.5 0h8.75a.75.75 0 0 1 .75.75v12.5a.75.75 0 0 1-.75.75h-2.5a.75.75 0 0 1 0-1.5h1.75v-2h-8a1 1 0 0 0-.714 1.7.75.75 0 1 1-1.072 1.05A2.495 2.495 0 0 1 2 11.5Zm10.5-1h-8a1 1 0 0 0-1 1v6.708A2.486 2.486 0 0 1 4.5 9h8ZM5 12.25a.25.25 0 0 1 .25-.25h3.5a.25.25 0 0 1 .25.25v3.25a.25.25 0 0 1-.4.2l-1.45-1.087a.249.249 0 0 0-.3 0L5.4 15.7a.25.25 0 0 1-.4-.2Z"></path></svg>"#;

const ICON_INBOX: &str = r#"<svg data-component="Octicon" aria-hidden="true" focusable="false" class="octicon octicon-inbox" viewBox="0 0 16 16" width="16" height="16" fill="currentColor" display="inline-block" overflow="visible" style="vertical-align:text-bottom"><path d="M2.8 2.06A1.75 1.75 0 0 1 4.41 1h7.18c.7 0 1.333.417 1.61 1.06l2.74 6.395c.04.093.06.194.06.295v4.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25v-4.5c0-.101.02-.202.06-.295Zm1.61.44a.25.25 0 0 0-.23.152L1.887 8H4.75a.75.75 0 0 1 .6.3L6.625 10h2.75l1.275-1.7a.75.75 0 0 1 .6-.3h2.863L11.82 2.652a.25.25 0 0 0-.23-.152Zm10.09 7h-2.875l-1.275 1.7a.75.75 0 0 1-.6.3h-3.5a.75.75 0 0 1-.6-.3L4.375 9.5H1.5v3.75c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25Z"></path></svg>"#;

const CSS: &str = r#"
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
"#;

const JS: &str = r#"
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
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::render_to_string;

    fn strip_tags(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut in_tag = false;
        for ch in html.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(ch),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn demo_urls_retain_the_complete_value_for_url_validation() {
        let repository = DemoUrl::repository("proa", "diff-lab", "/issues");
        let fragment = DemoUrl::file_fragment(42);

        assert_eq!(repository.url_str(), Some("/proa/diff-lab/issues"));
        assert_eq!(fragment.url_str(), Some("#file-42"));

        let oversized = "x".repeat(DEMO_URL_CAPACITY);
        let invalid = DemoUrl::owner(&oversized);
        assert!(!invalid.should_render());
        assert_eq!(invalid.url_str(), None);
    }

    #[test]
    fn renders_large_pr_diff_shell() {
        let props = GithubPrDiffProps {
            file_count: 2,
            lines_per_file: 41,
            ..Default::default()
        };
        let props = props.clamped();
        let html = render_to_string(GithubPrDiffPage { props: &props });

        assert!(html.contains("data-pr-diff-demo"));
        assert!(html.contains("tmp-pr-lg-3 prc-TreeView-TreeViewRootUlStyles-Mzrmj"));
        assert!(html.contains("PullRequestFilesToolbar-module__toolbar__ztHN6"));
        assert!(html.contains("data-file-tree-panel-action=\"collapse\""));
        assert!(html.contains("data-file-tree-panel-action=\"expand\""));
        assert!(html.contains("aria-label=\"Expand file tree\" aria-expanded=\"true\" aria-controls=\"pr-file-tree\" hidden"));
        assert!(html.contains("data-tree-action=\"toggle-directory\""));
        assert!(!html.contains("tree-status"));
        assert!(html.contains("class=\"pl-k\""));
        assert!(html.contains("content-visibility: auto"));
        assert!(html.contains("data-line-index=\"40\""));
        assert!(html.contains("document.addEventListener('click'"));
        assert!(html.contains("data-pr-comment-button"));
        assert!(html.contains("data-comment-line"));
        assert!(html.contains("<span class=\"pl-k\">if</span>"));
        assert!(!html.contains("octicon octicon-copilot"));
        assert!(!html.contains("CopilotItems-module__Wrapper__BFG9q"));
        assert!(html.contains("Search-module__value__TFoak"));
        assert!(html.contains("octicon octicon-git-commit"));
        assert!(html.contains("octicon octicon-gear"));
        assert!(!html.contains("</path></path>"));
    }

    #[test]
    fn default_json_exposes_diff_lines_and_derives_file_count() {
        let value = default_props_json();
        let object = value
            .as_object()
            .expect("default props should be an object");
        assert!(!object.contains_key("file_count"));

        let files = value["files"]
            .as_array()
            .expect("default props should include files");
        assert_eq!(files.len(), DEFAULT_FILE_COUNT);

        let first_lines = files[0]["diff_lines"]
            .as_array()
            .expect("default file should expose diff lines");
        assert_eq!(first_lines.len(), 11);
        assert_eq!(first_lines[0]["kind"], "hunk");
        assert_eq!(first_lines[4]["kind"], "deletion");
        assert_eq!(first_lines[4]["old_line"], 462);
        assert_eq!(first_lines[5]["kind"], "addition");
        assert_eq!(first_lines[5]["new_line"], 462);

        let additions: usize = files
            .iter()
            .map(|file| file["additions"].as_u64().unwrap_or(0) as usize)
            .sum();
        let deletions: usize = files
            .iter()
            .map(|file| file["deletions"].as_u64().unwrap_or(0) as usize)
            .sum();
        assert_eq!(additions, 512);
        assert_eq!(deletions, 9);

        let mut with_wrong_count = value;
        with_wrong_count["file_count"] = serde_json::json!(99);
        let props = GithubPrDiffProps::from_json(&with_wrong_count.to_string());
        assert_eq!(props.total_files(), DEFAULT_FILE_COUNT);
    }

    #[test]
    fn edited_default_diff_line_renders_dynamic_content() {
        let mut value = default_props_json();
        value["files"][0]["diff_lines"][4]["text"] =
            serde_json::json!("                                changed <&>");
        let props = GithubPrDiffProps::from_json(&value.to_string());
        let html = render_to_string(GithubPrDiffPage { props: &props });

        let file0_start = html.find(r#"id="file-0""#).expect("file 0 section renders");
        let file0 = &html[file0_start..html.find(r#"id="file-1""#).unwrap_or(html.len())];
        assert!(file0.contains("changed &lt;&amp;&gt;"));
        assert!(!file0.contains("request.version.tag == .github or request.version.tag == .git"));
    }

    #[test]
    fn renders_dynamic_props_from_json() {
        let props = GithubPrDiffProps::from_json(
            r#"{
                "repo_owner": "proa",
                "repo_name": "diff-lab",
                "title": "Dynamic PR",
                "pull_number": 7,
                "author": "alice",
                "issue_count_label": "12",
                "pull_request_count_label": "34",
                "fork_count_label": "56",
                "star_count_label": "78",
                "base_branch": "proa:main",
                "head_branch": "alice:branch",
                "commit_count": 2,
                "checks_count": 3,
                "conversation_count": 4,
                "review_comment_count": 9,
                "viewed_file_count": 5,
                "commit_selector_label": "Latest commit",
                "review_action_label": "Review changes",
                "file_count": 1,
                "lines_per_file": 40,
                "files": [
                    {
                        "path": "crates/demo/src/example.rs",
                        "additions": 8,
                        "deletions": 1,
                        "visible_lines": 3
                    }
                ]
            }"#,
        );

        let html = render_to_string(GithubPrDiffPage { props: &props });

        assert!(html.contains("data-testid=\"top-nav-left\""));
        assert!(html.contains("data-testid=\"top-nav-center\""));
        assert!(html.contains("data-testid=\"top-nav-right\""));
        assert!(html.contains("href=\"/proa\""));
        assert!(html.contains("href=\"/proa/diff-lab\""));
        assert!(html.contains("href=\"/proa/diff-lab/issues\""));
        assert!(html.contains("href=\"/proa/diff-lab/pulls\""));
        assert!(html.contains("href=\"#file-0\""));
        assert!(html.contains("proa"));
        assert!(html.contains("diff-lab"));
        assert!(html.contains("Dynamic PR"));
        assert!(html.contains("#7"));
        assert!(html.contains("alice"));
        assert!(html.contains("data-content=\"Issues\">Issues</span><span data-component=\"counter\" class=\"local-nav-counter\">12</span>"));
        assert!(html.contains("data-content=\"Pull requests\">Pull requests</span><span data-component=\"counter\" class=\"local-nav-counter\">34</span>"));
        assert!(!html.contains("repo-nav"));
        assert!(!html.contains("repo-actions"));
        assert!(html.contains(">Latest commit<"));
        assert!(html.contains(">Review changes<"));
        assert!(html.contains("1 / 1 viewed"));
        assert!(html.contains("Open comments panel, 9 comments"));
        assert!(html.contains("crates/demo/src/example.rs"));
        assert!(html.contains(">example.rs<"));
        assert!(html.contains("+8"));
        assert!(html.contains("-1"));
    }

    #[test]
    fn default_diff_uses_source_line_numbers() {
        let props = GithubPrDiffProps {
            file_count: 1,
            ..Default::default()
        };
        let props = props.clamped();
        let html = render_to_string(GithubPrDiffPage { props: &props });

        assert!(html.contains("@@ -459,7 +459,9 @@ pub fn edit("));
        assert!(html.contains("blob-num-deletion\">462</td>"));
        assert!(html.contains("blob-num-addition\">462</td>"));
        assert!(strip_tags(&html).contains("request.e_string == null"));
        assert!(!html.contains("lowered_0_4"));
    }

    #[test]
    fn renders_custom_json_diff_lines_and_dropdowns() {
        let props = GithubPrDiffProps::from_json(
            r#"{
                "file_count": 1,
                "files": [
                    {
                        "path": "src/lib.rs",
                        "additions": 1,
                        "deletions": 1,
                        "diff_lines": [
                            { "kind": "hunk", "text": "@@ -10,2 +10,2 @@" },
                            { "kind": "deletion", "old_line": 10, "text": "let old = value;" },
                            { "kind": "addition", "new_line": 10, "text": "let new_value = value;" },
                            { "kind": "context", "old_line": 11, "new_line": 11, "text": "return new_value;" }
                        ]
                    }
                ]
            }"#,
        );
        let html = render_to_string(GithubPrDiffPage { props: &props });

        assert!(html.contains("id=\"changes-menu\""));
        assert!(html.contains("data-menu-target=\"review-menu\""));
        assert!(html.contains("id=\"file-menu-0\""));
        assert!(html.contains("@@ -10,2 +10,2 @@"));
        assert!(html.contains("blob-num-deletion\">10</td>"));
        assert!(html.contains("blob-num-addition\">10</td>"));
        assert!(strip_tags(&html).contains("let new_value = value;"));
    }
}
