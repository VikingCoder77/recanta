# PRD: Recanta — CLI-First Memory & Knowledge Substrate for AI Agents

**Tagline:** *Recanta — to recount, to recall. The memory that endures across sessions, agents, and your whole AI workforce.*
**Domain:** recanta.dev
**Version:** 4.0
**Date:** 2026-06-06
**Status:** Draft for implementation
**Supersedes:** v3.1 (MemoryHub, CLI-First)

**Changes in 4.0:**
- Renamed to **Recanta** (`recanta` CLI, `.recanta/` project dir, `~/.recanta/` global, `recanta.db`, `RECANTA_*` env, `RECANTA:START/END` managed blocks, `recanta_*` MCP tools).
- **Positioning widened** from "coding harnesses" to a memory & knowledge substrate for **any AI agent or fleet** — including an *AI OS* where agents run part of a business (ops, finance, support, knowledge work) via Claude Code, Claude Cowork, or similar. Coding agents are the **beachhead** (sharpest pain, cleanest hooks), not the ceiling. The architecture is already domain-general; only the code-graph module is code-specific, and it is one optional capability among several (see §1, §6.6, §8.10).

**Carried forward from v3.x:**
- MVP scope cut to a genuinely shippable core; everything else mapped to explicit milestones (resolves the v2 contradiction between §17 and §20).
- Daemon demoted from core to optional; default execution model is synchronous CLI → SQLite.
- Vector search and accurate call-graph resolution removed from MVP.
- Concurrency, idempotency, output budgeting, project identity, schema migration, retention/GC, and scope-conflict precedence now specified concretely.
- Harness adapter sections rewritten against **verified** current docs (Claude Code, Codex, Gemini CLI, OpenCode) and reduced to one canonical command-hook template + one OpenCode plugin shim.
- Transcript/prompt capture is now opt-in (privacy + employer-data safety).
- New first-class human-facing feature: **Recanta Explorer** — a local, searchable, interactive graph UI.

**Added in 3.1:**
- §12.1 storage decided for all users: single SQLite file (FTS5 backbone) → `sqlite-vec` default vector layer in the same file → pluggable LanceDB/Qdrant → `fastembed` embedding default with hard FTS-only fallback. RAG framing clarified in §13.
- §6.5 competitive landscape + capability matrix vs **claude-mem, GitNexus, MemPalace**, with the design commitments that win.
- §8.10 + v0.6 milestone: GitNexus-class impact/blast-radius code intelligence, but Git-auto-refreshed and unified with memory.

---

## 1. Product Summary

Recanta is a local-first, harness-agnostic memory and knowledge substrate for AI agents — the durable layer an agent (or a fleet of agents) reads to know what happened before, what was decided, what the user prefers, and how the work is structured.

The **beachhead is coding agents** — Claude Code, Codex CLI, Gemini CLI, OpenCode, Cursor-like IDE agents, and any shell-capable agent — because that is where the pain is sharpest and the integration hooks are cleanest. But the same substrate serves any agent doing real work: an **AI OS** where agents run part of a business (operations, finance, support, research, knowledge work) through Claude Code, Claude Cowork, or similar. Nothing in the core is code-specific; the code-graph module (§8.10) is one optional capability among several, and the memory model (scopes, evidence, decisions, tasks, supersession, the searchable graph) applies equally to business workflows, procedures, accounts, and domain knowledge.

It gives a fresh session reliable continuity across projects and workspaces — commits, edits, decisions, preferences, active tasks, and structure — without rereading everything or loading thousands of tokens of static memory/tool definitions.

Recanta is **CLI-first**, not MCP-first. It consists of:

1. A stable command-line interface usable by any agent or harness that can run shell commands.
2. Local storage (SQLite) plus full-text and (later) vector indexes.
3. Non-destructive hook/adapter installers for existing projects, harnesses, and agent runtimes.
4. A **human-facing searchable graph UI** (Recanta Explorer) served locally.
5. An optional, deliberately tiny MCP bridge for MCP-only clients.
6. Git- and (where relevant) code-graph-aware memory updates after edits, commits, merges, branch changes, sessions, and workflow events.

**Product promise:**

> A new agent session — whether it's writing code or running a slice of your business — can enter an existing workspace, run one compact `recanta brief`, and understand the current work, user preferences, recent changes, intent, risks, and next steps, without reading everything or loading a large tool surface. A human can open the same memory in a browser and search/explore it as a graph.

---

## 2. Problem Statement

AI agents are powerful — coding agents first, and increasingly agents running real business workflows — but their memory is fragmented, static, token-heavy, and tool-specific. The failures are the same whether the agent edits code or runs operations:

- A fresh session does not know what was done before.
- The agent rereads large parts of the repo, wasting tokens and time.
- `CLAUDE.md`, `AGENTS.md`, `.cursorrules` grow large or stale.
- MCP servers expose many tools, adding schema/tool-loading overhead before useful work begins.
- Memories are summarized without raw evidence, causing hallucinated continuity.
- Project decisions and global user preferences get mixed together.
- Code changes aren’t tied to functions, commits, tests, branches, or reasons.
- Existing projects can’t safely add memory hooks without overwriting current setup.
- Multiple harnesses can’t share one reliable memory backend.
- Humans can’t see or audit what the AI “remembers.”

Recanta makes memory explicit, compact, queryable, local, Git-aware, and **human-inspectable**.

---

## 3. Goals

### 3.1 Product Goals
1. One universal memory layer across AI coding tools and projects.
2. Reduce token usage via CLI queries instead of large static context dumps.
3. Integrate into existing projects/harnesses through safe, reversible installers.
4. Keep raw evidence for every memory.
5. Maintain codebase intelligence through incremental code indexing.
6. Track memory across projects while preventing cross-project contamination.
7. Keep user preferences durable and reusable across projects.
8. Update memory automatically after edits, commits, merges, branch changes, session ends.
9. Let humans browse, search, and audit memory as an interactive graph.
10. Provide optional MCP access without a large MCP tool surface.

### 3.2 Technical Goals
1. CLI works fully without MCP and without a daemon.
2. MCP bridge exposes ≤ 5 tools (or 1 dispatch tool).
3. Git hooks and harness hooks call the same canonical event API.
4. Ingestion is auditable and idempotent; async only where a daemon is enabled.
5. Retrieval is token-budgeted by default with a deterministic, versioned output schema.
6. Every memory item has scope, source, confidence, provenance, validity, and status.
7. Index freshness is visible and enforced.
8. Installation is reversible and non-destructive.
9. The Explorer UI is read-only by default and never weakens the local-first guarantee.

---

## 4. Non-Goals (v1)

Recanta will not: replace Git; replace project-management tools; replace enterprise RAG vector DBs; require cloud hosting; require a specific AI model; require MCP; edit code autonomously; become a full IDE; guarantee perfect multi-language understanding; or store secrets/API keys/tokens/credentials.

---

## 5. Non-Negotiable Principles

1. **CLI-first** — every core feature is available through `recanta` commands.
2. **Local-first** — no source, transcript, embedding, diff, or memory leaves the machine unless explicitly configured.
3. **Raw evidence preserved** — every memory traces back to files, diffs, commits, transcripts, tool events, or user notes.
4. **Summaries are indexes, not truth** — they speed retrieval but never replace evidence.
5. **Token-budgeted output** — every query command enforces a strict, defined output limit (see §14).
6. **Non-destructive installation** — existing configs are backed up and patched with managed blocks, never blindly overwritten.
7. **Harness-agnostic core** — all harnesses use the same backend.
8. **Git-aware** — code memory is tied to commit, branch, worktree, dirty state, and diff provenance.
9. **Project isolation by default** — cross-project memory is explicit, scoped, and safe.
10. **User preferences are durable** — global preferences carry across projects.
11. **Privacy before storage** — secret redaction and capture policy apply *before* storage, embeddings, summarization, and retrieval.
12. **Explainable retrieval** — returned memory includes source IDs and confidence unless compact mode disables evidence.
13. **Working functions are protected** — when updating code, the agent queries function history and returns full changed functions where relevant.
14. **Human-inspectable** — anything the AI can store, a human can search, view, and (in edit mode) correct or delete.

---

## 6. Target Users

**Primary:** A developer, founder, or AI-assisted builder using multiple AI coding agents who wants one persistent, cross-tool, cross-project memory layer.

**Secondary:** Multi-agent setups; AI dev agencies; small teams running Claude Code/Codex/OpenCode/Gemini in parallel; enterprises needing local, auditable AI coding memory; developers maintaining many repos with recurring conventions.

**Emerging (AI-OS):** Founders and operators running part of a business on agents (Claude Code / Claude Cowork / fleets) who need a durable, auditable memory their agent workforce shares across sessions and handoffs — decisions, procedures, commitments, and account knowledge that must not be forgotten or contradicted (see §6.6).

---

## 6.5 Competitive Landscape — and how Recanta beats claude-mem, GitNexus, and MemPalace

The current leaders each nail **one** axis and miss the others. Recanta's thesis is that the three axes — **code intelligence**, **cross-session memory**, and **durable user preferences** — must be unified on a zero-dependency, privacy-first, Git-aware, evidence-backed, human-inspectable base. No competitor does all of that.

**claude-mem** (~80K★, AGPL-3.0) — automatic Claude Code session memory. Five lifecycle hooks auto-capture every tool use/file read/edit; a background worker compresses observations with Claude's agent-sdk into a taxonomy; stored in SQLite + Chroma. *Weaknesses:* heavy dependency chain (Bun + Python/uv + background worker + Chroma + HTTP port 37777); LLM-dependent compression (token cost, requires a model); **captures everything** (privacy/employer-data hazard, no opt-in); memory is lossy AI **summary**, not evidence-backed; not commit/branch/symbol-anchored; web viewer is a table of observations, not a searchable graph; AGPL restricts commercial reuse.

**GitNexus** (~30K★, ISC) — best-in-class code **structure** graph. Tree-sitter → knowledge graph in LadybugDB, exposed as 11 MCP tools (calls, imports, inheritance, execution flow, blast-radius/impact, detect-changes, rename, cypher); Sigma.js web explorer. *Weaknesses:* it is **only** a code graph — no cross-session memory, no user preferences, no decisions/tasks over time, no record of *why*; **11 MCP tools** bloat the agent tool surface (the exact problem we attack); **manual re-index** required after merges; dynamic imports don't resolve; web UI struggles past ~10K files.

**MemPalace** (~20K★, MIT) — verbatim conversation recall via a "memory palace" hierarchy; ChromaDB + SQLite, offline; 19 MCP tools. *Weaknesses:* Python-only; **store-everything-verbatim** (privacy hazard + unbounded growth + no redaction); not code/Git-aware (conversation memory, not code intelligence); 19 MCP tools; and its headline 96.6%/100% LongMemEval scores were independently shown to be just **ChromaDB's default embeddings on raw text** — the palace structure wasn't in the benchmark at all.

### Where Recanta wins (the design commitments that deliver it)

1. **Unifies all three axes.** Code graph **+** session memory **+** durable preferences, with code changes tied to the *reasons* (decisions/evidence). None of the three do this.
2. **One portable SQLite file, zero mandatory runtime/daemon.** Beats claude-mem's Bun+Python+Chroma+worker+port and MemPalace's Python+Chroma. `sqlite-vec` keeps vectors in the same file (§12.1).
3. **Works with no LLM and no embeddings.** v0.1 is FTS + structured retrieval; embeddings are optional/local with an FTS-only fallback. claude-mem needs an LLM to compress; GitNexus needs an LLM key; MemPalace needs an embedding stack.
4. **Evidence-backed, not lossy summary (claude-mem) and not store-everything (MemPalace).** The deliberate middle: selective, durable memories that always trace to raw evidence (diffs/commits/events), with supersession and confidence. "Summaries are indexes, not truth."
5. **Privacy-first by construction.** Opt-in capture + pre-storage redaction (§8.12). claude-mem captures everything; MemPalace stores everything verbatim — both are compliance/employer-data hazards. This is Recanta's enterprise wedge.
6. **Git-anchored and auto-refreshed.** Memory and code graph update automatically on commit/merge via hooks. GitNexus requires manual re-index; claude-mem is session-anchored, not commit/branch-anchored.
7. **Harness-native, not MCP-tool-heavy.** CLI-first with one canonical hook template across four harnesses; MCP is optional and capped at ≤5 tools. GitNexus ships 11 MCP tools, MemPalace 19 — exactly the surface bloat we exist to remove.
8. **Human-inspectable graph that includes memory, not just code.** The Explorer (§15) shows symbols **and** decisions/tasks/memories with an evidence browser and Git+memory timeline — beyond claude-mem's table and beyond GitNexus's code-only graph.
9. **Permissive licensing intent.** Targets a permissive license (vs claude-mem's AGPL) to allow commercial/embedded use.
10. **Benchmark honesty.** We will not ship a misleading headline number (the trap MemPalace fell into). Recanta commits to reproducible evals — LongMemEval for recall **and** a code-memory eval (post-edit symbol recall, decision retrieval, stale-index detection) — with methodology published.

| Capability | claude-mem | GitNexus | MemPalace | **Recanta** |
|---|---|---|---|---|
| Cross-session memory | ✅ | ❌ | ✅ (conversation) | ✅ |
| Durable user preferences | partial | ❌ | partial | ✅ |
| Git/commit/branch/symbol code graph | ❌ | ✅ (manual refresh) | ❌ | ✅ (auto-refresh) |
| Evidence/provenance + supersession | ❌ (summary) | ❌ | ❌ (verbatim) | ✅ |
| Privacy: opt-in capture + redaction | ❌ | n/a | ❌ | ✅ |
| Runs with no LLM/embeddings | ❌ | ❌ | ❌ | ✅ (FTS baseline) |
| Single-file, zero-daemon | ❌ | ❌ | ❌ | ✅ |
| Agent surface | hooks | 11 MCP tools | 19 MCP tools | CLI + ≤5 MCP |
| Human searchable graph (incl. memory) | table | code-only | ❌ | ✅ |
| License | AGPL-3.0 | ISC | MIT | permissive (intended) |

---

## 6.6 The AI-OS use case (why the substrate must generalize beyond code)

Coding is the beachhead, but the destination is broader: organizations are starting to run **parts of a business on agents** — a Claude Code or Claude Cowork instance (or a fleet) that handles operations, support triage, finance prep, research, or internal knowledge work. In that world the failure mode is identical to the coding one, just higher-stakes: the agent forgets what was decided last week, re-derives policy from scratch, contradicts a prior commitment, or loses the thread between handoffs. A business cannot run on an agent with amnesia.

Recanta is the shared, durable, auditable memory those agents rely on. The model already generalizes:

- **Scopes** extend naturally: `user` (the operator's standing preferences), `project`/`workspace` (a business unit, client, or initiative), and domain scopes in place of `repo`/`symbol`.
- **Memory types** are not code-specific: decisions, procedures, commitments, account/customer facts, policies, risks, and tasks are first-class.
- **Evidence & supersession** matter more in business contexts, not less — every remembered policy or commitment traces to its source, and superseded facts are retired, not silently overwritten.
- **The Explorer** lets a human owner see and audit exactly what the agent workforce "knows" and correct it — essential when agents act on the business's behalf.
- **Events** come from workflow/tool hooks the same way they come from Git hooks; Git is simply the richest event source for the coding beachhead.
- **Privacy/capture policy** (opt-in, redaction) becomes a governance requirement when agents touch customer and financial data.

The only code-specific component is the code-graph module (§8.10); everything else is a general agent-memory substrate. This is also a competitive wedge: claude-mem, GitNexus, and MemPalace are each pinned to one mode (code observations, code structure, or chat transcripts), whereas Recanta is the substrate underneath an agent **workforce**.

---

## 7. Architecture

```text
Harnesses / Existing Tools
  ├─ Claude Code   (command/HTTP/MCP/prompt/agent hooks + CLAUDE.md)
  ├─ Codex CLI     (command hooks: hooks.json or inline [hooks] + AGENTS.md)
  ├─ Gemini CLI    (command hooks: settings.json / extension hooks.json)
  ├─ OpenCode      (JS/TS plugin module shelling out to the CLI)
  ├─ Cursor-like   (instruction files + Git hooks)
  ├─ Generic shell agents (instruction file + Git hooks + wrapper)
  └─ Optional MCP clients (thin bridge)
        ↓
recanta CLI  (short-lived process; the primary integration surface)
  init · brief · search · inspect · remember · record-event · record-edit
  record-commit · changed · index · status · install · uninstall · serve · gc
        ↓
ingestion (in-process by default; daemon optional)
  redaction/capture-policy → diff parse → symbol extract → change tracking
  → summarize (optional) → embed (optional) → supersession → retrieval planner
        ↓
local storage (SQLite, WAL)
  raw event log (append-only, idempotent) · git timeline · memory items
  code symbols + relationships · symbol changes · hook installations
  FTS5 index · (optional) vector index · audit/provenance · schema_version
        ↓
read paths
  CLI queries  ·  MCP bridge (optional)  ·  Recanta Explorer (local web UI)
```

**Execution model (important):** The default path is **synchronous and daemonless**. A hook fires `recanta record-commit`, a short-lived process redacts, parses, writes to SQLite, and exits. Reads (`brief`, `search`, `inspect`) are short-lived processes over the same DB. A long-running daemon is **optional**, added only when background embedding, file-watching, or the Explorer server are enabled (`recanta serve` / `--daemon`). This avoids daemon lifecycle/IPC complexity in the core product.

---

## 8. Key Capabilities

### 8.1 Fresh Session Briefing
```bash
recanta brief --budget 1200
recanta brief --task "fix separate paper dirs" --budget 1500
```
Returns, compactly and task-aware: relevant global user preferences; project purpose; current branch + dirty state; active task; recent commits/session activity; active architectural decisions; recently changed files/functions; known risks; and a short list of suggested follow-up queries. The brief must be self-sufficient for the common case (suggested queries are optional, not required follow-ups) and must never dump full history.

### 8.2 CLI Search (hybrid)
```bash
recanta search "where is paper trading state stored?" --budget 800
recanta search "why was trailing stop changed?" --with-evidence
recanta search "database migration conventions" --scope project
```
Retrieval combines FTS5/BM25, (later) vector similarity, code-graph traversal, Git timeline, recency, importance, scope filters, and supersession filters. Default output is compact with source IDs and suggested `inspect` commands. **MVP uses FTS only**; vector is a later milestone (§17) with a defined FTS-only fallback when no embedding model is present.

### 8.3 Function/Class/Module Inspection
```bash
recanta inspect function PaperTrader.__init__ --budget 1000
recanta inspect class PaperTrader
recanta inspect module src/trading/paper_trader.py
```
Returns: current symbol location; current signature; purpose summary; (best-effort) callers/callees; related tests; recent changes; relevant decisions; known risks; last indexed commit; and a staleness flag. Lets agents avoid loading whole files.

### 8.4 Automatic Edit/Commit Memory
```bash
git diff | recanta record-edit --source git-diff --stdin
recanta record-commit --commit HEAD
recanta record-event --type harness.stop --stdin
```
The pipeline extracts changed files/symbols, signature changes, import/dependency changes, added/removed tests, test results if available, commit message + SHA, branch/worktree state, candidate durable memories, and superseded facts. **Ingestion caps:** diffs over a configurable size (default 512 KB) and binary/non-UTF-8 hunks are skipped with a recorded note; ignore rules (§8.12) apply before parsing.

### 8.5 Non-Destructive Hook Installation
```bash
recanta install --project . --harness git,claude-code,codex,gemini-cli --dry-run
recanta install --project . --harness git,claude-code,codex,gemini-cli --apply
```
The installer must:
1. **Detect the real hook mechanism**, not just `.git/hooks/`. It must check `git config core.hooksPath`, Husky (`.husky/`), the `pre-commit` framework (`.pre-commit-config.yaml`), and lefthook (`lefthook.yml`), and integrate with whichever owns the hooks. (Appending to `.git/hooks/post-commit` silently does nothing when `core.hooksPath` is redirected.)
2. Detect existing harness files: `.claude/settings.json`, `.claude/settings.local.json`, `CLAUDE.md`, `AGENTS.md`, `.codex/config.toml`, `.codex/hooks/hooks.json`, `.gemini/settings.json`, `.opencode/plugin/*`, `.cursor/rules/*`, `.cursorrules`.
3. Back up every file it will modify (timestamped, under `.recanta/backups/`).
4. Show a dry-run diff before any change.
5. Insert Recanta content only inside clearly marked managed blocks.
6. Preserve and **chain** existing hooks rather than replacing them.
7. Support uninstall/restore.
8. Record installation metadata in `hook_installations`.
9. Refuse destructive changes unless `--force`.
10. **Warn about folder trust:** Codex and Gemini only run *project-scoped* hooks in trusted folders; the installer must tell the user to trust the project (and note Claude Code’s own trust prompt) or the hooks won’t fire.

Managed-block and Git-chain examples are in §10. Hooks are **fail-open** and never block commits by default.

### 8.6 Harness Adapters (verified surfaces)

> Verified against official docs current as of 2026-06. Harness internals move fast — the installer must **probe the installed version and degrade gracefully**, and ship the capability matrix in §10.5 as data, not assumptions.

**The convergence that simplifies this:** Claude Code, Codex, and Gemini CLI now share a near-identical **command-hook** model — JSON-over-stdin, exit-code semantics (0 ok / 2 block / other = non-blocking error), matcher groups, and `settings.json`/`hooks.json` config. Gemini even exposes `CLAUDE_PROJECT_DIR` as a compatibility alias and offers `gemini hooks migrate --from-claude`. So Recanta ships **one canonical command-hook template** parameterized per harness, plus **one OpenCode plugin shim** (OpenCode uses JS/TS plugin modules, not shell hooks).

- **Claude Code** — config `~/.claude/settings.json`, `.claude/settings.json`, `.claude/settings.local.json`. Events used: `SessionStart` (inject brief via `additionalContext`), `UserPromptSubmit` (optional, opt-in capture), `PostToolUse` matcher `Edit|Write|MultiEdit` (record edits), `Stop`/`SessionEnd` (session summary), `PreCompact` (optional). `SessionStart` + `UserPromptSubmit` stdout is added to context. Handler type: `command`.
- **Codex CLI** — config `~/.codex/config.toml`; project `.codex/` only when trusted. Hooks via `.codex/hooks/hooks.json` or inline `[[hooks.<Event>]]`. Events used: `SessionStart`, `PostToolUse`, `UserPromptSubmit` (opt-in), `Stop`. Only `command` hooks execute (prompt/agent are parsed but skipped). Provide compact `AGENTS.md` block. Note ignored project keys (`notify`, `profile`, etc.).
- **Gemini CLI** — hooks enabled by default (v0.26+). Config `.gemini/settings.json` (project), `~/.gemini/settings.json` (user), `/etc/gemini-cli/settings.json` (system), or extension `hooks/hooks.json`. Same JSON-over-stdin contract (“stdout must be JSON only”). Events used: `SessionStart`, `PostToolUse`/`AfterTool`, `UserPromptSubmit`, `Stop`/`SessionEnd`. Env: `GEMINI_PROJECT_DIR`, `GEMINI_SESSION_ID`, `CLAUDE_PROJECT_DIR` (alias).
- **OpenCode** — JS/TS plugin in `.opencode/plugin/` (project) or `~/.config/opencode/plugin/` (global). Recanta ships a tiny plugin exporting handlers for `session.created` (brief), `tool.execute.after` (record edit), and `session.idle`/`session.deleted` (session end), each shelling out to `recanta` via the plugin `$` helper. No shell-hook model; this shim is the integration.
- **Generic / Cursor-like** — `RECANTA.md` instruction file + Git hooks + optional wrapper (`recanta run -- <agent>`).

### 8.7 Optional Thin MCP Bridge
For MCP-only clients. Allowed MVP tools: `recanta_brief`, `recanta_search`, `recanta_inspect`, `recanta_record`, `recanta_status` — or a single dispatch tool `recanta_run` taking `{command, ...args}`. Never expose dozens of granular tools.

### 8.8 Global User Memory
```bash
recanta remember "When updating code, provide the full function." --scope user
recanta remember "Do not rewrite working code unnecessarily." --scope user --importance high
recanta user-memory list | edit <id> | forget <id>
```
User memory is separate from project memory and injected only when relevant. Conflict precedence is defined in §13.3.

### 8.9 Project Memory
Tracks purpose, active task, architecture decisions, stack/conventions, known issues, module responsibilities, important commands, deployment, test strategy, risky areas, done/next.
```bash
recanta project set-purpose "..."   |  project status
recanta decisions add "CLI is primary; MCP is optional thin bridge"  |  decisions list
recanta task set-current "Implement hook installer"
```

### 8.10 Code Graph Memory (scoped for realism)
Incremental code graph via **tree-sitter** (named explicitly for multi-language breadth).

- **MVP graph (achievable):** `DEFINES` (functions/classes/methods per file), **syntactic** `IMPORTS` (file-level), and `CHANGED_BY` (symbol ↔ commit). Symbol identity is `qualified_name` + body-hash.
- **Deferred / lower-confidence:** `CALLS`/callee resolution requires name/type resolution that tree-sitter alone doesn’t provide (especially in dynamic Python/JS). When produced, it is **heuristic and flagged low-confidence**, never presented as authoritative.
- **Rename/move handling (best-effort):** match on `qualified_name`; if absent, match on signature + body-hash similarity above a threshold; otherwise mark old symbol `deleted` and new `added`. Large refactors may lose fine-grained history — this is an explicit, documented limitation, not a silent failure.

MVP languages: Python, TypeScript, JavaScript, plus config (JSON/YAML/TOML) and Markdown instructions. Later: Go, Rust, Java, C#, PHP, Swift/Kotlin.

Graph entities: Repository, File, Module, Function, Class, Method, Route, Model/Schema, CLI command, Test, Config key, Dependency/import, Commit, Branch, Decision, Task, Bug.
Relationships: `DEFINES`, `IMPORTS`, `CHANGED_BY`, `TESTED_BY`, `EXPLAINS`, `SUPERSEDES`, `DEPENDS_ON`, `IMPLEMENTS`, `RELATED_TO`, `RISKS_BREAKING`, and (low-confidence) `CALLS`.

**Beating GitNexus on code intelligence (roadmap, v0.4 → v0.6).** GitNexus is the bar for deep code-structure graphs (call chains, blast-radius/impact, change detection). Recanta adopts those capabilities — `inspect --impact <symbol>` for blast-radius with confidence scores, and `changed --impact` mapping a Git diff to affected symbols/tests for pre-commit risk — but with three advantages GitNexus structurally lacks: (1) it is **Git-anchored and auto-refreshed** on commit/merge via hooks, so it never needs a manual re-index; (2) the structure graph is **unified with memory** — a blast-radius result links to the decisions, risks, and prior changes explaining *why* the code is shaped that way; and (3) it is **honest about resolution limits** — dynamic imports and reflection are flagged low-confidence rather than presented as complete (the failure mode GitNexus acknowledges). Call-graph accuracy remains best-effort; Recanta never claims a complete call graph it cannot guarantee.

### 8.11 Branch, Worktree & Git Timeline
Handle branch changes, detached HEAD, worktrees, rebases, amended/squashed commits, merges, cherry-picks, dirty work, stashes, deleted branches. Every code memory records: repo ID, worktree path, branch (if any), commit SHA (if any), dirty marker, file path, symbol ID, timestamp.

**Visibility rules (new, previously undefined):**
- Memories default to **repo scope** (visible across branches). `--scope branch` limits visibility to that branch.
- A decision/task recorded on a feature branch becomes **active on the default branch only when a merge is detected** for that branch; until then it is `active` on its branch and not surfaced on others.
- **Squash/rebase orphan handling:** when original SHAs disappear, commit-linked evidence is **retained as historical** (status unchanged, marked `orphaned_sha`) and re-anchored to the surviving commit where derivable. Memory is never deleted just because a SHA vanished.

Staleness warning when index ≠ Git state:
```text
Warning: code graph indexed at 9f31a2c, current HEAD is b128c91. Run: recanta index --update
```

### 8.12 Privacy: Redaction + Capture Policy
Two distinct concerns, both enforced **before** storage/index/embed/summarize/retrieve.

**(a) Secret redaction** — sources: regex/known token formats, `.env`/config conventions, **context-gated** entropy detection, user deny patterns, path denylist. Context-gating is mandatory: entropy alone flags too many false positives (git SHAs, UUIDs, hashes, minified JS, lockfile integrity hashes — and SHAs are core data here). Entropy only triggers when combined with a secret-like context (assignment to `*key/token/secret/password*`, presence in `.env`). Hashes/SHAs/UUIDs are allowlisted. Every redaction records **which pattern fired** (audit + false-positive debugging). On detection: store redacted placeholder only; never embed or summarize the original.
Default ignored paths: `.env*`, `*.pem`, `*.key`, `id_rsa*`, `node_modules/`, `.venv/`, `dist/`, `build/`, `.git/`, cache and generated artifacts (unless explicitly enabled).

**(b) Capture policy (new) — raw prompt/transcript/tool-output capture is OPT-IN and OFF by default.** Prompts and tool outputs routinely contain PII, customer names, internal hostnames, and proprietary code that regex won’t catch. Default behavior captures **derived summaries and structured events**, not raw transcripts. A `capture.raw_transcripts` setting (per user and per project) must be explicitly enabled, has a retention window, and is surfaced in `status`. This also protects against silently ingesting employer/third-party code into a personal store.

### 8.13 Supersession & Contradiction
Each memory has `valid_from`, `valid_to`, `status` (active/superseded/disputed/archived/deleted), `superseded_by`, `confidence`, `source_ids`. Default retrieval prefers active memories and hides superseded ones unless requested.

---

## 9. CLI Specification

### 9.1 Global flags
```bash
--project <path>
--scope user|project|repo|branch|symbol      # unified taxonomy (no "global"; user == cross-project)
--budget <n>                                  # characters by default; see §14
--budget-unit chars|tokens                    # tokens = estimate via configurable ratio or tokenizer
--format compact|markdown|json|ids-only|evidence
--with-evidence | --no-evidence
--quiet | --verbose
--schema-version                              # prints output schema version
```
All commands exit with meaningful status codes. `--format json` emits a **versioned, stable schema** (stable field names; results sorted by rank then by ID for deterministic tie-breaking). The nondeterministic part (LLM summary text) is confined to clearly named fields so structure stays parseable even when summaries vary.

### 9.2 Commands
`init`, `install`, `uninstall`, `brief`, `search`, `inspect`, `remember`, `record-event`, `record-edit`, `record-commit`, `changed`, `index`, `status`, `serve`, `gc`, `user-memory`, `project`, `decisions`, `task`, `evidence`, `migrate`.

Highlights:
- `init --project .` creates `.recanta/project.json` (incl. project UUID + root-commit SHA) and default ignore rules; installs nothing until `install` is run.
- `install` / `uninstall` — see §8.5 and §10. `uninstall` removes only managed blocks unless `--restore-backup <id>`.
- `index --update | --changed-only` — code graph (re)index; `--changed-only` after Git events.
- `status` — daemon (if any), DB health + schema version, current project/branch/dirty, last indexed commit, hook install + chaining mechanism detected, harness adapter status + folder-trust state, capture policy, redaction warnings, queue backlog, DB size + GC suggestion.
- `serve` — starts the read-only Explorer + local API (see §15).
- `gc` — retention/compaction (see §12.3).
- `migrate` — run pending SQLite schema migrations (see §11.3).

---

## 10. Hook System

### 10.1 Hook Manager
Responsible for: detecting the real hook mechanism (incl. `core.hooksPath`/Husky/pre-commit/lefthook); creating dry-run diffs; applying managed blocks; chaining existing hooks; recording backups; safe uninstall; verifying installation; and reporting folder-trust requirements. No installer may silently overwrite a file or silently no-op.

### 10.2 Git Hooks (MVP: post-commit)
`post-commit` (MVP) records commit metadata + changed symbols. `post-checkout`/`post-merge` (later) record branch/merge and trigger `index --changed-only`. Optional non-blocking `pre-commit` records the staged diff. Chaining example:
```bash
#!/usr/bin/env bash
# <existing hook content preserved>
# RECANTA:START (managed)
if command -v recanta >/dev/null 2>&1; then
  recanta record-commit --commit HEAD --project "$(git rev-parse --show-toplevel)" >/dev/null 2>&1 || true
fi
# RECANTA:END
```
Default behavior never blocks commits.

### 10.3 Canonical command-hook template
One template (JSON-over-stdin, exit 0, fail-open) is rendered per harness into its config format (`settings.json` for Claude/Gemini; `hooks.json`/inline `[hooks]` for Codex). Events mapped: session start → `brief`; post-edit tool → `record-edit`; session stop → `record-event --type harness.stop`; optional prompt submit → opt-in capture.

### 10.4 OpenCode plugin shim
A ~30-line TS module shelling out to `recanta` on `session.created`, `tool.execute.after`, and `session.idle`/`session.deleted`.

### 10.5 Capability matrix (ship as data; verified 2026-06)
| Harness | Config location | Hook type | Events Recanta uses | Trust caveat |
|---|---|---|---|---|
| Claude Code | `~/.claude/settings.json`, `.claude/settings.json(.local)` | command/HTTP/MCP/prompt/agent | SessionStart, PostToolUse(Edit\|Write\|MultiEdit), Stop/SessionEnd, UserPromptSubmit*, PreCompact* | trust prompt |
| Codex CLI | `~/.codex/config.toml`, `.codex/hooks/hooks.json` or `[[hooks.*]]` | command only | SessionStart, PostToolUse, Stop, UserPromptSubmit* | project hooks need trusted folder |
| Gemini CLI | `.gemini/settings.json`, `~/.gemini/...`, `/etc/...`, ext `hooks/hooks.json` | command + plugin | SessionStart, PostToolUse/AfterTool, Stop/SessionEnd, UserPromptSubmit* | untrusted folders block project hooks |
| OpenCode | `.opencode/plugin/`, `~/.config/opencode/plugin/` | JS/TS plugin module | session.created, tool.execute.after, session.idle/deleted | n/a |
*opt-in capture only.

---

## 11. Data Model (SQLite)

### 11.1 Core tables
- **users**: id, display_name, created_at
- **projects**: id (UUID), name, root_path, **root_commit_sha** (primary identity), remote_url_hash (hint, nullable), created_at, last_seen_at
- **repositories**: id, project_id, root_path, root_commit_sha, remote_url_hash, default_branch — projects 1:many repositories (monorepo / multi-root support)
- **events** (append-only): id, **idempotency_key** (unique), type, source, harness, project_id, repo_id, branch, commit_sha, payload_redacted, capture_mode (summary|raw), created_at, claimed_at (ingestion lease)
- **memory_items**: id, type (semantic/procedural/episodic/decision/task/warning/bug/code_summary), scope (user/project/repo/branch/symbol), title, content, status, importance, confidence, valid_from, valid_to, source_event_ids, source_commit_shas, superseded_by, created_at, updated_at
- **code_symbols**: id, repo_id, file_path, symbol_type, name, qualified_name, signature, start_line, end_line, body_hash, last_seen_commit, status
- **code_relationships**: id, from_symbol_id, to_symbol_id, relationship_type, confidence, source
- **commits**: sha, repo_id, branch, author_hash, message, timestamp, parent_shas, orphaned_sha (bool)
- **symbol_changes**: id, symbol_id, commit_sha, event_id, change_type (added/modified/deleted/moved/renamed), summary, diff_ref
- **hook_installations**: id, project_id, harness, file_path, mechanism (git-hooks/husky/pre-commit/lefthook/core.hooksPath/settings/plugin), managed_block_id, backup_path, installed_at, version, status
- **redaction_audit**: id, event_id, pattern_id, path, span, created_at
- **schema_version**: version, applied_at

### 11.2 Identity rules
Project/repo identity is the **root-commit SHA** (stable across clones, remotes, SSH/HTTPS). Remote URL and path are hints only. Repos with no commits yet fall back to a generated UUID until the first commit. Two clones of the same repo resolve to the same project by default; `--project` + UUID can force separation. Monorepos = one project, many repositories rows; sub-package scoping via `repo_id` + path prefix.

### 11.3 Migrations
A `schema_version` table plus an ordered migration runner (`recanta migrate`) is mandatory for a long-lived local DB. The CLI refuses to run against a newer schema than it understands and offers `migrate`.

---

## 12. Storage, Concurrency & Retention

### 12.1 Storage — one portable file, no extra runtime

The design rule that follows from "usable for all users": **the entire memory store is a single SQLite file with zero mandatory external runtime, daemon, or second process.** This is also the primary competitive moat (see §6.5) — competitors require Bun + Python + Chroma + a worker (claude-mem) or a Python env + ChromaDB (MemPalace). Recanta requires neither.

Two storage jobs, one file:

**(a) Relational + full-text (the backbone — always present): SQLite + FTS5.**
Zero-config, single file, embedded, available on every OS, FTS5 built in, WAL for the concurrency model (§12.2). The code graph lives in relational tables (no Neo4j, no Docker, no graph-DB dependency). This is non-negotiable and is the whole store for v0.1.

**(b) Vector layer (optional, v0.4+): default `sqlite-vec`, pluggable beyond it.**
When embeddings are enabled, vectors live in the **same SQLite file** via the `sqlite-vec` loadable extension — one file, one backup, transactional consistency, no second process, cross-platform via prebuilt binaries. A pluggable `VectorBackend` interface lets power users swap in alternatives without touching the rest of the system:

| Backend | Role | Process / deps | When |
|---|---|---|---|
| **sqlite-vec** | **default** | none (same SQLite file) | all users, all sizes up to large repos |
| LanceDB | scale option | embedded, no server | very large graphs where a separate columnar store helps |
| Qdrant / pgvector | team/server | server (Docker/managed) | §12.4 team mode only |
| usearch / hnswlib (index file beside DB) | fallback | none | when the SQLite build disables extension loading |
| ChromaDB | optional adapter | Python | only if a user explicitly insists — never the default |

**Caveat (honest):** `sqlite-vec` is a loadable extension; a few locked-down system SQLite builds disable extension loading. Mitigation: bundle a SQLite build/binding that permits extensions (e.g. `better-sqlite3` in Node, or the `sqlite-vec` package which loads into Python's `sqlite3` with `enable_load_extension`). If that ever fails, fall back automatically to a `usearch`/`hnswlib` index file beside the DB. The product never hard-fails on the vector layer.

**Embeddings (the actual hard part for "all users").** Storing vectors is easy everywhere; *generating* them locally on every OS without a GPU is the constraint. Therefore:
- **v0.1 ships with no embeddings at all** — FTS5/BM25 + structured retrieval works identically on every machine with zero ML dependency.
- **v0.4 embeddings are opt-in.** Portable default: a small CPU **ONNX model via `fastembed`** (no GPU, all platforms). Auto-detect and prefer a local provider if present (Ollama / LM Studio / MLX on Apple Silicon). Cloud embeddings only when explicitly configured (breaks local-first → off by default).
- **Hard rule — FTS-only fallback:** if no embedder is available, Recanta silently degrades to FTS + structured retrieval and still works. Embeddings are never required for the product to function. (This is the opposite of claude-mem/GitNexus/MemPalace, all of which require either an LLM key or an embedding stack to deliver their core value.)

### 12.2 Concurrency & idempotency (previously unspecified)
Multiple writers exist: Git hooks, harness hooks, CLI, optional daemon. Rules:
- SQLite in **WAL** mode with `busy_timeout` (default 5s).
- The **event log is append-only** and every event carries an `idempotency_key` (hash of type + commit/file + content digest + coarse timestamp). Duplicate hook fires are no-ops.
- A **single logical ingestion worker** processes events; concurrent ingesters use a row-level **lease** (`claimed_at`) so only one processes a given event. In daemonless mode, ingestion runs inline within the writing command under the same lease discipline.
- Writes are wrapped in transactions; readers never block writers (WAL).

### 12.3 Retention / GC (previously missing)
Unbounded growth is a real risk across years/repos. `recanta gc` provides: event-log rotation (archive/drop raw payloads past a window while keeping derived memories + provenance refs), episodic-memory decay (low-importance, old, superseded items downgraded then archived), and configurable size caps. `status` surfaces DB size and suggests GC. Nothing with active provenance links is deleted without confirmation.

### 12.4 Later team/server mode
Optional: Postgres + pgvector, dedicated graph DB, multi-user auth, shared team memory, remote encrypted sync.

---

## 13. Retrieval

**Is this RAG?** Yes, in the broad sense — `brief`/`search`/`inspect` retrieve relevant context and inject it into the agent (retrieve-then-generate). But it is deliberately **not** vector-DB-centric RAG over chunked documents. Retrieval is (a) **explicit and agent-invoked**, not automatic prompt-stuffing; (b) **hybrid and structured-first** — FTS5/BM25 + Git timeline + code graph, with dense vectors as an *optional* layer (§12.1); and (c) **evidence-linked**, so every result can be traced to its source. This is closer to "structured retrieval-augmented memory" than to classic dense RAG. It also means Recanta behaves well with no embeddings at all (FTS-only), which none of the named competitors do.

### 13.1 Pipeline
1. Parse query → 2. Detect scope → 3. FTS5 search → 4. (later) vector search → 5. graph traversal where relevant → 6. Git recency/branch filters → 7. drop superseded/deleted by default → 8. rank by relevance, importance, recency, confidence, scope → 9. build answer under budget (deterministic truncation, §14) → 10. attach source IDs + suggested next commands.

### 13.2 Progressive disclosure
Default search is compact. The agent (or human) can drill down: `recanta inspect memory <id> --with-evidence`, `recanta evidence <source-id>`, `recanta inspect commit <sha>`. Avoids dumping raw transcripts unless requested.

### 13.3 Scope-conflict precedence (new)
When user-global and project memory conflict: **project wins on code/technical conventions** (style, stack, build); **user wins on interaction/output style** (e.g., “return full functions”). Conflicts are surfaced (not silently resolved) in `brief` and Explorer with both items and the applied precedence.

---

## 14. Output Budgeting (now mechanically defined)

- **Unit:** characters by default (deterministic, tokenizer-agnostic). `--budget-unit tokens` estimates via a configurable chars/token ratio (default 4) or an optional real tokenizer.
- **Truncation:** rank items, fill until the next whole item would exceed budget, then stop. Items are never cut mid-content. A footer reports `… N more items omitted (use --budget <n> or inspect <id>)`.
- **Profiles:** `compact` (agents), `markdown` (humans), `json` (versioned schema), `ids-only`, `evidence`.
- **Testable guarantee:** for any `--budget n`, `len(output) ≤ n` in the chosen unit (the omitted-count footer is included in the accounting).

---

## 15. Recanta Explorer — the human-facing searchable graph

> Answers the question directly: **yes**, Recanta ships a human UI to *see and search* memory as a graph. In v2 this was a vague Phase-7 “dashboard.” In v3 it is a first-class, read-only-by-default feature available early, because it reads the same SQLite the CLI writes and carries low risk.

### 15.1 What it is
A local web app served by `recanta serve --port 7077` (binds to `127.0.0.1`, no telemetry, no auth in local mode). It exposes a **read-only local HTTP API** over the existing DB plus the same retrieval engine as the CLI.

### 15.2 Views
- **Graph canvas** — interactive node-link view of the code + memory graph. Nodes: symbols, files, modules, decisions, tasks, bugs, commits, memories. Edges: the §8.10 relationship types. Click a node → inspector panel (mirrors `recanta inspect`). Filter by scope, status (active/superseded), type, branch, time range, confidence. Layout: force-directed with clustering by module/decision.
- **Search bar** — same hybrid retrieval as `recanta search`, with results highlighted on the graph and listed with source IDs.
- **Evidence browser** — click a memory → see its raw evidence (diffs, commits, events), with redaction clearly marked.
- **Timeline** — Git + memory events over time; scrub to see graph state at a commit.
- **Conflicts panel** — surfaced user/project precedence conflicts (§13.3).
- **Optional edit mode** (off by default) — correct/forget/supersede memories and fix false-positive redactions; all edits are themselves audited.

### 15.3 Tech (aligned to a Vue/Nuxt stack)
- Frontend: Nuxt 3 + Vue 3 + Tailwind (+ PrimeVue for panels/tables if desired).
- Graph rendering: **Cytoscape.js** for rich interaction on graphs up to a few thousand nodes; **Sigma.js (WebGL)** as the large-graph renderer when a repo exceeds the interaction threshold. The API returns a viewport/neighborhood subgraph (not the whole graph) so rendering scales.
- Backend: a thin read-only HTTP layer over SQLite reusing the retrieval planner; ships inside the `serve` command.
- Performance: server-side filtering + neighborhood expansion; never ship the full graph to the browser at once.

### 15.4 Acceptance (Explorer)
- `recanta serve` opens a localhost UI showing the current project’s graph.
- Searching in the UI returns the same items as the equivalent CLI query, highlighted on the graph.
- Clicking a memory shows its evidence and any redaction markers.
- Default mode is read-only; edit mode is explicit and audited.
- No data leaves `127.0.0.1`.

---

## 16. Security & Privacy
- **Local-first default:** local DB/indexes, no telemetry, no cloud embeddings/summarization unless configured.
- **Secret handling:** redact before storage, logs, embeddings, summarization, and search output (§8.12a).
- **Capture policy:** raw transcript/prompt capture opt-in with retention (§8.12b).
- **Explorer:** localhost-bound, no auth in local mode, read-only default.
- **Access control (team mode, later):** auth, project access control, audit log, delete/export controls.

---

## 17. Milestones (replaces the v2 §17/§20 contradiction)

> One source of truth for scope. “MVP” = v0.1 only. Each later version is additive.

### v0.1 — True MVP (ship this first)
1. CLI core: `init`, `remember`, `search`, `brief`, `status`, `inspect file/function`, `record-commit`, `record-edit`.
2. SQLite (WAL) + FTS5 + schema migrations + idempotent append-only event log.
3. User + project + repo + branch + symbol scopes (unified taxonomy).
4. **Conservative** secret redaction with audit; capture defaults to summaries-only.
5. Non-destructive installer with mechanism detection, dry-run, backup, apply, uninstall — Git `post-commit` + **Claude Code** adapter (the canonical command-hook template).
6. Code graph: `DEFINES` + syntactic `IMPORTS` + `CHANGED_BY` for Python/TS/JS via tree-sitter; stale-index detection.
7. Deterministic, budgeted, versioned output (chars).
8. Daemonless synchronous execution.

### v0.2 — Git & multi-harness breadth
`post-checkout`/`post-merge` + branch/merge visibility rules; Codex + Gemini CLI adapters (same template); OpenCode plugin shim; `changed`, `index --changed-only`.

### v0.3 — Explorer (read-only)
`recanta serve` graph UI, search, evidence browser, timeline.

### v0.4 — Retrieval depth
Local embeddings via `fastembed` (ONNX/CPU default) + hybrid ranking, with FTS-only fallback; `sqlite-vec` vector layer in the same file; supersession/decision/task retrieval; heuristic low-confidence `CALLS`.

### v0.5 — Optional surfaces
Thin MCP bridge (≤5 tools); optional daemon (background embedding/file-watch); `gc` retention tooling; Explorer edit mode.

### v0.6 — GitNexus-class code intelligence (unified with memory)
`inspect --impact`/`changed --impact` blast-radius with confidence scores; pre-commit risk surface; deeper (still confidence-scored) call resolution; impact results linked to the decisions/risks that explain the code. All Git-anchored and auto-refreshed.

### Later
Team/server mode, remote encrypted sync, more languages, RBAC.

### Explicitly NOT in v1
Enterprise team sync; browser extension; graph-DB dependency; dozens of MCP tools; autonomous code editing; cloud accounts; fine-grained RBAC; perfect static analysis for all languages.

---

## 18. Acceptance Criteria (made testable)

**CLI**
- `recanta brief --budget 1200` returns a briefing with `len ≤ 1200` chars that includes *all of*: branch, dirty-state, current task (or “none”), ≥1 risk if any risks exist, and ≥1 recent change if any exist.
- `recanta search --format json` returns the versioned schema with results sorted by (rank, id); identical input → identical structure and ordering across runs (summary text fields excepted).
- `recanta inspect function <name>` returns location, signature, summary, recent changes, warnings, last-indexed-commit, and stale flag — without reading the whole file into agent context.
- `recanta status` shows project, branch, dirty state, schema version, index state, detected hook mechanism, harness adapter + folder-trust state, capture mode, and DB size.

**Installer**
- Detects existing hooks/instruction files **and** the active hook mechanism (incl. `core.hooksPath`/Husky/pre-commit/lefthook).
- Dry-run shows exact planned changes; apply backs up first; existing hook behavior preserved; managed blocks are marked and removable; uninstall removes only managed blocks; hooks fail open and never block commits.
- Reports folder-trust requirement for Codex/Gemini project hooks.

**Git memory**
- post-commit records SHA, message, changed files, changed symbols (idempotent on re-fire).
- Branch switch (v0.2) warns if index stale; merge triggers `--changed-only`.
- Rebase/amend/squash retains old commit references as historical evidence (`orphaned_sha`), never orphaning all memory.

**Code graph**
- Detects functions/classes in Python and TS/JS; updates changed symbols after edits/commits; marks deleted symbols `deleted` (not silently removed); `CALLS` (when present) is flagged low-confidence.

**Privacy**
- `.env` contents never stored raw; obvious keys/tokens redacted before storage; redacted values never embedded; search/Explorer output never reveals redacted values; each redaction logs its triggering pattern; git SHAs/UUIDs are **not** redacted.
- Raw transcript capture is off unless `capture.raw_transcripts` is enabled.

**Budget / tokens**
- For any `--budget n --budget-unit <u>`, output ≤ n in unit u, including the omitted-count footer.

**Explorer** — see §15.4.

---

## 19. Risks & Mitigations
- **Installer breaks/no-ops existing setup** → mechanism detection (core.hooksPath/Husky/pre-commit/lefthook), dry-run, backups, managed blocks, chaining, uninstall, fail-open, folder-trust warnings.
- **Stale memory** → track last indexed commit, warn on HEAD mismatch, changed-only reindex after Git events, staleness in `status`/`brief`/`inspect`.
- **Hallucinated facts** → raw evidence, required source IDs, confidence scores, evidence inspection, never high-confidence on summary-only.
- **Cross-project contamination** → root-commit identity, strict scopes, current-project bias, user memory only when relevant, provenance on every item.
- **Token bloat** → budgeted/compact-default output, progressive disclosure, tiny MCP, short managed blocks, self-sufficient brief.
- **Secret leakage / false-positive redaction** → context-gated entropy, allowlisted hashes/SHAs, redaction audit, no embedding raw secrets.
- **Privacy / employer-data ingestion** → capture opt-in + retention; summaries-only default.
- **Concurrency/races** → WAL + busy_timeout, append-only idempotent events, single ingestion lease.
- **Code-graph overreach** → MVP graph syntactic only; CALLS deferred/low-confidence; rename limits documented.
- **DB growth** → `gc` retention/compaction + size surfacing.
- **Harness drift** → capability matrix shipped as data; version probing; graceful degradation; Git hooks as baseline.

---

## 20. Example managed block for instruction files
```md
<!-- RECANTA:START -->
## Recanta
This repo uses Recanta for AI memory and code continuity.
On a fresh session: `recanta brief --budget 1200`
Before modifying an existing symbol: `recanta inspect function <name> --budget 1000`
Rules: don't rewrite working code unnecessarily; when changing a function, return the full updated function; prefer targeted changes; check Recanta for recent decisions/risks before editing critical files.
After significant changes, installed hooks record the edit/commit automatically.
<!-- RECANTA:END -->
```
Keep it compact; never paste full Recanta docs into instruction files.

---

## 21. Final Direction
> **Recanta is a CLI-first, local-first, Git-aware memory and knowledge substrate for AI agents and AI-OS workflows, with a human-facing searchable graph. Coding agents are the beachhead; the substrate serves any agent or fleet. MCP is optional. Hooks are installable, non-destructive adapters. Memory is queryable, scoped, evidence-backed, token-budgeted — and inspectable by people.**

Ship v0.1 narrow and reliable: capture evidence, index code syntactically, search compactly, inspect symbols, install safely, keep memory scoped and current. Add the Explorer early (v0.3) so humans can trust what the AI remembers. Add embeddings, more harnesses, MCP, and the daemon only once the core is solid.

**The winning line over the field:** claude-mem gives you session memory but at the cost of a heavy stack and capturing everything; GitNexus gives you a code graph but no memory and a manual re-index; MemPalace gives you verbatim recall but no code awareness and a privacy/growth problem. Recanta is the only system that unifies code intelligence, session memory, and durable preferences — in **one portable SQLite file, with no required LLM or daemon, privacy-first, Git-auto-refreshed, evidence-backed, and inspectable by people.** That combination is the moat, and every v0.1 commitment exists to protect it.
