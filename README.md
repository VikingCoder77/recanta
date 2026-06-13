# Recanta

**To recount, to recall.** A CLI-first, local-first memory and knowledge substrate for
AI agents — the durable layer an agent (or a whole fleet) reads to know what happened
before, what was decided, what you prefer, and how the work is structured.

A fresh agent session runs one compact command and understands the current work, recent
changes, decisions, risks, and the conversation history — without rereading the repo or
loading a large tool surface. Everything stays on your machine, in a single SQLite file.

> Status: **v0.1** — the core substrate is complete and usable. The human-facing graph
> Explorer, semantic (vector) search, and an MCP bridge are on the roadmap (see
> [Roadmap](#roadmap)).

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
- **Passive.** Once installed, commits and edits record themselves via Git/harness hooks.

## Install

### Download a prebuilt binary (no build required)

Grab the archive for your platform from the
[**latest release**](https://github.com/nptSolutions/recanta/releases/latest), unpack it,
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
recanta inspect function PaperTrader.__init__
recanta index                # build the code graph (functions/classes/imports)
recanta ingest docs/ -r      # bring in PDFs, Word docs, Markdown as searchable memory
recanta import-sessions      # import past Claude Code/Codex/Gemini/OpenCode chats
recanta status               # project, branch, index/capture state, DB size
```

## Commands

| Command | What it does |
|---|---|
| `init` | Create `.recanta/`, the store, and identity; ignore the store in git |
| `install` / `uninstall` | Non-destructive Git + Claude Code hooks (dry-run by default) |
| `brief` | Token-budgeted fresh-session briefing |
| `search` | Full-text search across memory, documents, and chat transcripts |
| `inspect` | Show a function/class/file: location, signature, recent changes |
| `remember` | Record a durable memory (decision, task, preference, …) |
| `index` | Build the tree-sitter code graph (Python/JS/TS) |
| `ingest` | Ingest documents (Markdown/text/PDF/Word/`.doc`) |
| `import-sessions` | Import agent transcripts (Claude Code, Codex, Gemini, OpenCode) |
| `record-commit` / `record-edit` / `record-event` | Hook targets that record activity |
| `capture` | Manage raw-transcript capture (on by default, redacted) |
| `serve` | Local, read-only **Explorer** web UI (search + code-graph view) |
| `changed` | Show changed files and the symbols they touch |
| `status` / `migrate` | Health/identity report; apply schema migrations |

Every query command takes `--budget <chars>` and `--format compact|json|ids-only`.

## How it works

```
Harnesses + Git hooks  →  recanta CLI (short-lived)  →  SQLite (WAL, single file)
                                   │                       memory · code graph ·
   redaction → parse/extract ──────┘                       documents · sessions · FTS5
                                   ↓
              read paths: brief · search · inspect
```

The default execution model is synchronous and daemonless: a hook fires `recanta`, which
redacts, writes to SQLite, and exits. Identity is the repo's root-commit SHA, so memory
follows the project across clones.

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

The local **Explorer** (`recanta serve`) ships a read-only graph + search UI from the
binary (no Node build, no CDN). Next: local **embeddings/vector** search (`sqlite-vec` +
`fastembed`), a thin **MCP** bridge, retention/`gc`, and blast-radius code intelligence.
See `CHANGELOG.md` for details.

## License

[Apache-2.0](LICENSE).
