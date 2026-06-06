# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Current state

This repository is **pre-implementation**. The only source of truth today is the
product spec, `Recanta_PRD_v4.md`, plus the Apache-2.0 `LICENSE`. There is no code,
build system, test suite, or chosen language toolchain yet. When you add the first
implementation, also add the real build/lint/test commands to this file — do not
invent them before they exist.

Read `Recanta_PRD_v4.md` before doing substantive work. The section references below
(`§8.5`, `§12.1`, etc.) point into that file; it is the authoritative spec and this
file only summarizes the load-bearing parts.

## What Recanta is

A **CLI-first, local-first, Git-aware memory and knowledge substrate for AI agents**.
A fresh agent session runs one compact `recanta brief` to learn the current work,
user preferences, recent changes, decisions, and risks — without rereading the repo
or loading a large MCP tool surface. A human can open the same memory in a local
browser graph UI (the Explorer). Coding agents are the beachhead; the substrate is
domain-general (only the code-graph module is code-specific).

## Non-negotiable principles (these shape every design decision)

These are not aspirational — violating one is a design bug, even if code "works":

- **CLI-first, not MCP-first.** Every core feature is a `recanta` subcommand. The MCP
  bridge is optional and capped at ≤5 tools (or one dispatch tool). The whole point is
  to *avoid* tool-surface bloat (the thing GitNexus/MemPalace do wrong with 11/19 tools).
- **Local-first.** No source, diff, transcript, embedding, or memory leaves the machine
  unless explicitly configured. No telemetry. The Explorer binds `127.0.0.1` only.
- **Single portable SQLite file, zero mandatory daemon/runtime.** This is the moat.
  Default execution is **synchronous and daemonless**: a hook fires a short-lived
  `recanta` process that redacts → writes SQLite → exits. A daemon exists only for
  optional embedding/file-watch/Explorer (`serve`). Do not introduce a required second
  process, port, or external service into the core.
- **Works with no LLM and no embeddings.** v0.1 is FTS5/BM25 + structured retrieval.
  Embeddings (`sqlite-vec` + `fastembed`) are opt-in and must degrade silently to
  FTS-only when absent. Never make the product hard-fail on a missing model/extension.
- **Evidence-backed, not lossy summary and not store-everything.** Every memory traces
  to raw evidence (diff/commit/event/note) with confidence + supersession. "Summaries
  are indexes, not truth." Never present a summary as authoritative fact.
- **Privacy before storage.** Secret redaction *and* capture policy run **before**
  storage/index/embed/summarize/retrieve. Raw transcript/prompt capture is **opt-in,
  off by default** (`capture.raw_transcripts`); default captures derived summaries only.
- **Non-destructive installation.** The installer backs up, shows a dry-run diff, and
  edits only inside `RECANTA:START`/`RECANTA:END` managed blocks. It **chains** existing
  hooks, never overwrites or silently no-ops. Hooks are **fail-open** — they never block
  a commit.
- **Token-budgeted output.** Every query command enforces `--budget`; for any
  `--budget n`, `len(output) ≤ n` in the chosen unit, footer included.

## Architecture (the big picture)

```
Harnesses (Claude Code, Codex, Gemini CLI, OpenCode, generic) + Git hooks
   ↓  (one canonical command-hook template + one OpenCode JS/TS plugin shim)
recanta CLI  — short-lived process, the primary integration surface
   ↓  ingestion (in-process by default): redact → diff-parse → symbol-extract →
      change-track → [summarize] → [embed] → supersession → retrieval planner
   ↓
SQLite (WAL, single file): append-only idempotent event log · git timeline ·
   memory_items · code_symbols + relationships · symbol_changes ·
   hook_installations · redaction_audit · FTS5 · [optional vector] · schema_version
   ↓
read paths: CLI queries · optional thin MCP bridge · Recanta Explorer (local web UI)
```

Key cross-cutting designs that require reading multiple PRD sections to grasp:

- **Scope taxonomy** is `user | project | repo | branch | symbol` (no "global"; `user`
  == cross-project). Scope-conflict precedence (§13.3): **project wins on technical
  conventions, user wins on interaction/output style**; conflicts are surfaced, not
  silently resolved.
- **Project/repo identity is the root-commit SHA** (§11.2), stable across clones and
  remotes. Remote URL and path are only hints. Monorepo = one project, many
  `repositories` rows. No commits yet → fall back to a generated UUID.
- **Concurrency/idempotency** (§12.2): multiple writers (Git hooks, harness hooks, CLI,
  optional daemon) coordinate via WAL + `busy_timeout`, an append-only event log where
  every event has an `idempotency_key` (duplicate hook fires are no-ops), and a
  row-level ingestion **lease** (`claimed_at`) so only one worker processes an event.
- **Code graph** (§8.10) is relational tables in the same SQLite — no graph DB. MVP
  edges are `DEFINES`, syntactic file-level `IMPORTS`, and `CHANGED_BY`, via
  **tree-sitter** for Python/TS/JS. Symbol identity = `qualified_name` + body-hash.
  `CALLS` is deferred and, when produced, **flagged low-confidence** — never presented
  as a complete/authoritative call graph.
- **Git timeline** (§8.11): records repo/worktree/branch/SHA/dirty per memory. Memories
  default to **repo scope** (cross-branch); a branch decision becomes active on the
  default branch **only when a merge is detected**. Squash/rebase that orphans a SHA
  retains evidence as historical (`orphaned_sha`) — memory is never deleted because a
  SHA vanished.
- **Output is deterministic + versioned**: `--format json` emits a stable schema sorted
  by `(rank, id)`; nondeterministic LLM summary text is confined to clearly named fields.

## Naming conventions (do not drift from these)

The v4 rename is strict — use exactly: `recanta` (CLI), `.recanta/` (project dir),
`~/.recanta/` (global), `recanta.db`, `RECANTA_*` (env vars),
`RECANTA:START` / `RECANTA:END` (managed blocks), `recanta_*` (MCP tools).
Earlier drafts used "MemoryHub"; do not reintroduce old names.

## CLI surface (the commands to implement)

`init · install · uninstall · brief · search · inspect · remember · record-event ·
record-edit · record-commit · changed · index · status · serve · gc · user-memory ·
project · decisions · task · evidence · migrate`. Global flags include `--project`,
`--scope`, `--budget` / `--budget-unit`, `--format compact|markdown|json|ids-only|evidence`,
`--with-evidence`. See §9 for the full spec.

## Build scope by milestone — build v0.1 first, narrow and reliable

Do not pull later-milestone features into earlier work. **v0.1 (the true MVP)** is:
CLI core (`init`, `remember`, `search`, `brief`, `status`, `inspect file/function`,
`record-commit`, `record-edit`); SQLite WAL + FTS5 + schema migrations + idempotent
event log; the five scopes; conservative secret redaction with audit (summaries-only
capture); non-destructive installer for **Git post-commit + Claude Code only**; code
graph `DEFINES`/`IMPORTS`/`CHANGED_BY` with stale-index detection; budgeted versioned
output; daemonless. Later milestones (§17) add other harnesses (v0.2), the Explorer
(v0.3), embeddings/vector (v0.4), MCP/daemon/gc (v0.5), and impact analysis (v0.6).

Explicitly **not in v1**: vector search in MVP, autonomous code editing, a graph-DB
dependency, dozens of MCP tools, cloud accounts, team/RBAC.

## Intended stack (from the PRD, not yet committed in code)

The Explorer (v0.3) targets Nuxt 3 + Vue 3 + Tailwind, with Cytoscape.js for graphs
(Sigma.js/WebGL for very large ones), served read-only by `recanta serve --port 7077`.
The core CLI language is **not yet fixed** in the PRD — confirm the choice (and its
implications for the single-file SQLite + extension-loading requirement of `sqlite-vec`,
§12.1) before scaffolding, rather than assuming one.

## Git / commits

- **Never mention "Claude" or "Claude Code" in commit messages, PR titles, or PR bodies**
  for this repository, and do not add a Claude `Co-Authored-By` trailer.
- Remote: `git@github.com:nptSolutions/recanta.git` (origin), default branch `main`.
