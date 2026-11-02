# Proa Claude Code skills

Project-scoped skills for anyone working in this repo with the Claude
Code CLI. They are automatically discovered by Claude when you run
`claude` from the proa repo; no setup, no installation.

## What's here today

| Skill | Invoke | Auto-invoke? | What it covers inline |
|---|---|---|---|
| `add-component` | `/add-component` | Yes (user asks to add a render component) | Standalone component scaffold; generic children; sync/async selection; direct-write performance rules; escaping and stable root attributes. |
| `add-island` | `/add-island` | Yes (user asks for interactivity or an RSJS component) | Explicit-trait rsjs island scaffold; signal API; analyzed render-body rules; event delegation + microtask batching; common compile-error fixes. |
| `test-component` | `/test-component` | Yes (user asks for tests, snapshots, a11y / visual regression) | Unit-test patterns (render to `Vec<u8>` + assert); variant loops; XSS-escape tests; `insta` snapshot tests; Playwright + axe-core; criterion benches; what to test vs skip. |
| `proa-perf-review` | `/proa-perf-review` | Yes (after Proa Rust changes or perf questions) | Runs `scripts/proa_perf_lint.py` and explains how to fix render-path allocation findings. |
| `proa-markdown` | `/proa-markdown` | Yes (Markdown renderers or exports) | Multiline raw documents, short string fragments, explicit Rust slots, shared content, and render verification; follows the shared Codex/Claude authoring guide. |

## How to add a new skill

1. Create `.claude/skills/<your-skill-name>/SKILL.md`. The directory
   name becomes the slash command (`/<your-skill-name>`).
2. Start the file with YAML frontmatter:

   ```yaml
   ---
   description: One sentence on when this skill applies. Claude reads this to decide whether to auto-invoke.
   disable-model-invocation: true   # set if this is a destructive task; remove for knowledge skills
   ---
   ```

3. Below the frontmatter, write instructions Claude reads when the
   skill is invoked. Plain markdown. Reference files in the repo by
   relative path so Claude can open them.
4. Commit. The skill is available to everyone who clones the repo on
   their next `claude` session.

### When to set `disable-model-invocation: true`

- **Yes**: the skill performs a destructive or hard-to-reverse action
  (a future `/bump-versions`, a future `/yank`). Force
  explicit invocation.
- **No / omit**: the skill is knowledge-shaped (`/add-component`,
  `/add-island`, `/explain-X`). Claude should be able to recognize
  the user's intent from the description and pull the skill in.

### Naming rules

- Lowercase, hyphens for separators (`add-component`, not
  `addComponent` or `AddComponent`).
- Avoid collisions with built-ins (`/help`, `/compact`, `/clear`,
  `/init`, `/exit`, `/login`, `/logout`, `/model`, `/agents`, ...).
  Run `/skills` in a Claude Code session to see the current list.
- Names should hint at what the skill does. `/add-component` is fine;
  `/utility` is not.

## Convention for repo references

When a skill instruction references repo files, use paths relative
to `<repo-root>/.claude/skills/<skill-name>/SKILL.md`. For example,
to reference `docs/agent/components.md` from a skill file, write
`../../../docs/agent/components.md`. Claude resolves these
correctly.

## Things to NOT do in a skill

- **Embed secrets.** SKILL.md is committed to git. No tokens, no
  account IDs that should not be public, no production endpoint
  URLs that imply infrastructure topology you would not want
  competitors to see.
- **Assume Claude will follow the skill verbatim.** Claude reads the
  skill as context, then does what makes sense. Write skills as
  guidance for a senior engineer, not as scripts.
- **Embed long static reference dumps.** If the skill needs more
  than ~200 lines, split: keep the load-bearing instructions in
  SKILL.md and link to longer reference docs in `docs/`.
- **Trigger background work in a skill.** Skills run when invoked.
  They are not cron jobs. Use Claude Code hooks (see
  https://code.claude.com/docs/en/hooks.md) for "run X on tool call Y".

## Files in this directory

The Claude Code skill loader expects exactly:

```
.claude/skills/
├── README.md              ← this file (ignored by the loader)
├── <skill-name>/
│   └── SKILL.md           ← required, frontmatter + body
└── <other-skill>/
    └── SKILL.md
```

Anything else (scripts, fixtures, additional docs) can live
alongside `SKILL.md` inside the skill's directory; Claude will not
auto-load them but the skill instructions can reference them.
