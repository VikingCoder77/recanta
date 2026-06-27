# Recanta

[![CI](https://github.com/VikingCoder77/recanta/actions/workflows/ci.yml/badge.svg)](https://github.com/VikingCoder77/recanta/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/VikingCoder77/recanta?sort=semver)](https://github.com/VikingCoder77/recanta/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**To recount, to recall.** A CLI-first, local-first memory and knowledge substrate for
AI agents — the durable layer an agent (or a whole fleet) reads to know what happened
before, what was decided, what you prefer, and how the work is structured.

A fresh agent session runs one compact command and understands the current work, recent
changes, decisions, risks, and the conversation history — without rereading the repo or
loading a large tool surface. Everything stays on your machine, in a single SQLite file.

> Status: **v0.5.0 — released.** Shipped: the core substrate, the local graph **Explorer**
> (`recanta serve`), semantic (vector) **search**, the **MCP bridge** (`recanta mcp`), a
> cross-project **workspace** overview, and a document **auto-updater** (`recanta watch`).
> Retention/`gc` and blast-radius code intelligence are next (see [Roadmap](#roadmap)).

---

## Why Recanta

- **One memory across tools and projects.** Works with Claude Code, Codex, Gemini CLI,
  OpenCode, plain Git, or any shell-capable agent.
- **Local-first, single file.** Everything lives in `.recanta/recanta.db`. No daemon, no
  server, no cloud, no telemetry. Works with no LLM and no embeddings.
- **Privacy by construction.** Secrets are redacted *before* anything is stored — and
  git SHAs, UUIDs, and hashes are never mistaken for secrets.
- **Evidence-backed.** Memories trace back to commits, diffs, documents, and chats.
  Summaries are indexes, not the truth.
- **More than code.** Ingests your documents (Markdown, text, PDF, Word) and imports your
  past agent **sessions**, so the agent doesn't forget what you discussed ten sessions ago.
- **All your projects in one view.** A cross-project **workspace** shows every repo and its
  documents in a single graph — each project keeps its own store and identity.
- **Passive.** Once installed, commits and edits record themselves via Git/harness hooks,
  and watched document folders re-ingest themselves as files change.

## Install

### Download a prebuilt binary (no build required)

Grab the archive for your platform from the
[**latest release**](https://github.com/VikingCoder77/recanta/releases/latest), unpack it,
and put `recanta` on your `PATH`:

```bash
# macOS (Apple Silicon) example
tar -xzf recanta-macos-arm64.tar.gz
sudo mv recanta /usr/local/bin/
recanta --version
```

Builds are published for macOS (arm64 + x86_64), Linux (x86_64), and Windows (x86_64).

### Build from source

Requires Rust 1.80+ (a C toolchain is needed for bundled SQLite and tree-sitter):

```bash
cargo install --path .       # from a clone
# or
cargo build --release        # binary at target/release/recanta
```

## Quickstart

```bash
cd your-project
recanta init                 # create .recanta/ + the store; ignores it in git
recanta install --apply      # wire git + Claude Code hooks; import existing sessions

recanta brief                # compact, budgeted briefing for a fresh session
recanta search "why did we change the trailing stop?"
recanta inspect function MyClass.__init__
recanta index                # build the code graph (functions/classes/imports)
recanta ingest docs/ -r      # bring in PDFs, Word docs, Markdown as searchable memory
recanta import-sessions      # import past Claude Code/Codex/Gemini/OpenCode chats
recanta embed                # optional: enable semantic (vector) search via a local model
recanta watch ~/Documents    # auto-ingest new/changed documents from a folder
recanta serve                # open the Explorer (all your projects in one graph)
recanta status               # project, branch, index/capture state, DB size
```

## Commands

| Command | What it does |
|---|---|
| `init` | Create `.recanta/`, the store, and identity; ignore the store in git |
| `install` / `uninstall` | Non-destructive Git + Claude Code hooks (dry-run by default) |
| `brief` | Token-budgeted fresh-session briefing |
| `search` | Hybrid search across memory, documents, and chat transcripts (FTS + vectors) |
| `inspect` | Show a function/class/file: location, signature, recent changes |
| `remember` | Record a durable memory (decision, task, preference, …) |
| `index` | Build the tree-sitter code graph (Python/JS/TS/Rust) |
| `ingest` | Ingest documents (Markdown/text/PDF/Word/`.doc`) |
| `embed` | Compute embeddings for semantic search (local Ollama / LM Studio / bundled) |
| `import-sessions` | Import agent transcripts (Claude Code, Codex, Gemini, OpenCode) |
| `record-commit` / `record-edit` / `record-event` | Hook targets that record activity |
| `capture` | Manage raw-transcript capture (on by default, redacted) |
| `serve` | Local, read-only **Explorer** web UI (multi-project graph + search) |
| `workspace` | Manage the cross-project overview (`list`/`add`/`remove`/`enable`/`disable`) |
| `watch` | Auto-ingest new/changed documents from watched folders (incremental) |
| `mcp` | **Model Context Protocol** bridge over stdio (≤5 tools, local-only) |
| `changed` | Show changed files and the symbols they touch |
| `status` / `migrate` | Health/identity report; apply schema migrations |

Every query command takes `--budget <chars>` and `--format compact|json|ids-only`.

## How it works

```
Harnesses + Git hooks  →  recanta CLI (short-lived)  →  SQLite (WAL, single file)
                                   │                       memory · code graph · documents ·
   redaction → parse/extract ──────┘                       sessions · FTS5 · vectors
                                   ↓
        read paths: brief · search · inspect · MCP bridge · Explorer (serve)
```

The default execution model is synchronous and daemonless: a hook fires `recanta`, which
redacts, writes to SQLite, and exits. Identity is the repo's root-commit SHA, so memory
follows the project across clones.

## Workspace (one Explorer across all your projects)

Run several projects at once? `recanta init` registers each one in a global workspace
(`~/.recanta/workspace.json`), and `recanta serve` shows **all of them in a single graph**
with a per-project filter — while each project keeps its **own single-file store and
identity** (the registry is just a list of paths, never a merged database).

```bash
recanta init                 # in each project — auto-registers it
recanta workspace list       # see registered projects + on/off state
recanta serve                # one Explorer, all projects, filter by project
recanta serve --single       # just the current project
recanta workspace disable    # opt out: serve shows only the current project
```

New projects show up automatically. Each folder keeps its own ID; you just get one place
to see everything.

### Keeping documents fresh

Point Recanta at your document folders and it ingests new/changed files automatically —
only the differences, since ingestion is content-hash idempotent:

```bash
recanta watch ~/Documents/business   # remembers the folder; watches it
recanta watch                         # watch all saved folders for this project
recanta watch --once                  # one catch-up pass, then exit
recanta serve --watch                 # Explorer + live refresh as docs change
```

## MCP bridge

Agents that speak the **Model Context Protocol** can reach the same local store without
the CLI. The bridge is deliberately tiny — four `recanta_*` tools (`recanta_brief`,
`recanta_search`, `recanta_remember`, `recanta_inspect`), well under the ≤5 cap — and it
re-enters the exact same in-process command logic, so redaction, budgeting, and ranking
are identical to the CLI. stdio transport, no network, no telemetry.

Register it with Claude Code (or any MCP client):

```bash
claude mcp add recanta -- recanta mcp --project /path/to/repo
```

It speaks JSON-RPC 2.0 over stdio (`initialize` → `tools/list` → `tools/call`).

### Works with any MCP harness

Because the bridge is plain MCP-over-stdio, **every MCP-capable harness can use Recanta** —
no per-tool code. Most share the same `mcpServers` config shape; add this block:

```json
{
  "mcpServers": {
    "recanta": {
      "command": "recanta",
      "args": ["mcp", "--project", "/path/to/repo"]
    }
  }
}
```

Where that config lives:

| Harness | Config location |
|---|---|
| **Claude Code** | `claude mcp add …` (above) or `.mcp.json` |
| **Google Antigravity** (IDE/CLI) | `~/.gemini/config/mcp_config.json` |
| **Codex** | `~/.codex/config.toml` (`[mcp_servers.recanta]`) |
| **Cursor** | `~/.cursor/mcp.json` |
| **Windsurf** | `~/.codeium/windsurf/mcp_config.json` |
| **OpenCode** | `opencode.json` (`mcp` block) |
| **VS Code / Copilot** | `.vscode/mcp.json` |

Recanta sits alongside other MCP servers (e.g. Miro's board server) — they don't conflict;
a harness can talk to all of them at once.

## Privacy & capture

- **Redaction always runs** before storage (API keys, tokens, private keys, secret-named
  assignments). Git SHAs, UUIDs, and hashes are allowlisted.
- **Raw transcript capture is on by default** (redacted, local-only) so chats are
  searchable. Turn it off per project with `recanta capture disable`.
- Nothing leaves your machine.

## Roadmap

Shipped in v0.1: the CLI core, SQLite/FTS5 store, redaction, the non-destructive
installer (Git + Claude Code), the tree-sitter code graph, document ingestion, and
multi-harness session import.

v0.3 added the local **Explorer** (`recanta serve`) — a read-only graph + search UI
served from the binary (no Node build, no CDN). v0.4 added local **embeddings/vector**
search (`sqlite-vec` + bundled `fastembed`, degrading to FTS-only). **v0.5 (released)** adds
the thin **MCP** bridge (`recanta mcp`), a cross-project **workspace** overview, and a
document **auto-updater** (`recanta watch` / `serve --watch`). Next: retention/`gc`, an
optional daemon, and blast-radius code intelligence. See `CHANGELOG.md` for details.

## License

[Apache-2.0](LICENSE).
