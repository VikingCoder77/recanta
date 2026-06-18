# Changelog

All notable changes to Recanta are documented here. This project adheres to
[Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-06-18

The Recanta Explorer — a local, read-only web UI to see and search your whole memory.

### Explorer (`recanta serve`)
- A local web UI served from the binary (binds `127.0.0.1`; no auth, no telemetry, no
  write endpoints). **Cytoscape.js is vendored and embedded**, so the graph works fully
  offline — no Node/Nuxt build and no CDN.
- **Full force-directed knowledge graph** of everything: every code symbol + module,
  documents, memories, sessions, and commits, connected by `DEFINES`, internal `IMPORTS`
  (file-dependency graph), `CALLS`, `CHANGED_BY` (symbol↔commit), memory→`EVIDENCE`,
  document→`MENTIONS` (code it references), and document↔document `RELATED` (shared
  significant terms).
- **Node-kind and edge-type filters** to isolate a view — e.g. just the call graph, or
  just file dependencies. Search-to-focus, hover/neighborhood labels, and click-a-node to
  fade to its neighborhood with a detail pane (signature/changes for code, content for
  memories).

### Code graph
- **Heuristic `CALLS` edges** (low-confidence, `source='heuristic'`): extracted at index
  time by matching each call's trailing name to a uniquely-named function/method, with a
  denylist of ubiquitous builtin names (`map`/`get`/`unwrap`/`new`/…) to avoid false hubs.
  `index` now reports the call count. Not a complete/authoritative call graph (proper
  resolution is a later milestone).

## [0.2.0] - 2026-06-13

Git & multi-harness breadth, plus Rust support so Recanta can index itself.

### Code graph
- **Rust** added to the code graph (Python/JS/TS/Rust). Structs, enums, traits,
  functions, methods, mods, and `use`/`extern crate` imports; `impl` blocks scope their
  methods to the implementing type (e.g. `Trader.new`). Language rules are unified in a
  single `classify` model.
- `index --changed-only` — incremental reindex of the committed delta
  (`indexed_commit..HEAD`); much faster than a full reindex after a Git event.
- `changed` command — maps the working-tree diff (or `--against <ref>`) to the indexed
  symbols each file touches, as a pre-commit/review risk surface.
- `inspect class` now matches any type-like kind (class/interface/struct/enum/trait).

### Git & harness hooks
- The installer wires three git hooks: `post-commit` (reindex changed files, then
  record the commit, so new symbols are immediately queryable), `post-checkout`, and
  `post-merge` (reindex on branch switch / merge).
- **Install adapters for Codex, Gemini CLI, and OpenCode** (in addition to Claude Code):
  Codex/Gemini get JSON command-hooks (shared merge/marker logic), OpenCode gets a
  `.opencode/plugin/recanta.ts` shim. Trust-folder and output caveats are surfaced.
  (Best-effort: hook formats vary by tool version.)

### Memory
- **Branch-scope visibility**: a `branch`-scoped memory is shown only on its own branch.

### Deferred
- Merge-driven activation of a branch's memories onto the default branch (the post-merge
  reindex is wired; merge→memory activation is a later refinement).

## [0.1.0] - 2026-06-07

First release: a complete, local-first memory substrate for AI agents. Every command in
the v0.1 surface is implemented; there are no stubs.

### Core
- **CLI + storage**: a single SQLite file (`.recanta/recanta.db`) in WAL mode with a
  forward-only schema migration runner and an idempotent, append-only event log. No
  daemon, no server, no required LLM or embeddings.
- **Project identity** via the Git root-commit SHA (stable across clones), with a UUID
  fallback for repos with no commits.
- **Scopes**: `user` (global, cross-project) plus `project` / `repo` / `branch` / `symbol`.
- **Deterministic, budgeted output**: every query enforces `--budget` (output never
  exceeds it); `--format compact|json|ids-only`, with a versioned JSON schema.

### Memory & retrieval
- `remember` durable memories (decisions, tasks, preferences, warnings, …).
- `search` — FTS5/BM25 across **memory, documents, and chat transcripts** in one ranked
  result set; `brief` — a compact, task-aware fresh-session briefing; `inspect` —
  function/class/file location, signature, and recent changes.

### Code intelligence
- Tree-sitter **code graph** for Python, JavaScript/JSX, TypeScript, and TSX: `DEFINES`,
  file-level `IMPORTS`, and `CHANGED_BY` edges; symbol identity is `qualified_name` +
  body-hash; vanished symbols are marked deleted, never silently dropped. Stale-index
  detection (indexed commit vs HEAD) surfaced in `status` and `inspect`.

### Git & hooks
- `record-commit` / `record-edit` / `record-event` ingest commits, diffs, and harness
  events idempotently.
- **Non-destructive installer** (`install` / `uninstall`): detects the real hook
  mechanism (`core.hooksPath` / Husky / pre-commit / lefthook), dry-runs by default, backs
  up every touched file, edits only inside managed blocks, chains existing hooks, and
  fails open. Wires Git `post-commit` and the Claude Code hook set
  (SessionStart → `brief`, PostToolUse → `record-edit`, Stop → `record-event`).

### General-purpose (AIOS) ingestion
- **Documents**: `ingest` Markdown, text, PDF, Word `.docx`, and legacy `.doc` (via
  `textutil`/`antiword`/`catdoc`) as redacted, searchable evidence; recurses subfolders
  with `-r`.
- **Sessions**: `import-sessions` from **Claude Code, Codex, Gemini CLI, and OpenCode**.
  Extracts a per-session episodic memory and, with capture on, stores the full redacted
  conversation full-text-indexed so old discussions stay recallable. `install` auto-imports
  existing sessions.

### Privacy
- **Secret redaction before storage** (`redact`): known token formats and secret-named
  assignments are redacted with an audit trail; Git SHAs, UUIDs, and hashes are
  allowlisted (never redacted).
- **Capture policy** (`capture enable/disable/status`): raw transcript capture defaults
  **on** but always redacted and local-only; disable per project at any time.

### Not yet included (roadmap)
The human-facing Explorer graph UI (`serve`), local embeddings / vector search
(`sqlite-vec` + `fastembed`), a thin MCP bridge, retention/`gc`, blast-radius code
intelligence, `post-checkout`/`post-merge` hooks, incremental `index --changed-only`, and
dedicated `project`/`decisions`/`task`/`user-memory` management commands (decisions and
tasks are writable today via `remember --type`).

[0.3.0]: https://github.com/nptSolutions/recanta/releases/tag/v0.3.0
[0.2.0]: https://github.com/nptSolutions/recanta/releases/tag/v0.2.0
[0.1.0]: https://github.com/nptSolutions/recanta/releases/tag/v0.1.0
