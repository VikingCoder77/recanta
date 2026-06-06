//! Ordered schema migrations. A `schema_version` table plus a forward-only runner is
//! mandatory for a long-lived local DB (PRD §11.3): the CLI refuses to run against a
//! schema newer than it understands and offers `migrate`.
//!
//! Each migration is `(version, sql)` applied in order inside a transaction. Never
//! edit a shipped migration — add a new one.

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

/// All migrations, in application order. `version` must be strictly increasing.
const MIGRATIONS: &[(u32, &str)] = &[(1, MIGRATION_0001)];

/// Highest schema version this binary knows how to produce.
pub fn latest_version() -> u32 {
    MIGRATIONS.iter().map(|(v, _)| *v).max().unwrap_or(0)
}

/// Read the applied schema version (0 if the store is brand new).
pub fn current_version(conn: &Connection) -> Result<u32> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version    INTEGER NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .context("ensuring schema_version table")?;
    let v: Option<u32> = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
        .context("reading schema_version")?;
    Ok(v.unwrap_or(0))
}

/// Apply all pending migrations. Returns the resulting version.
pub fn migrate(conn: &Connection) -> Result<u32> {
    let mut current = current_version(conn)?;
    let latest = latest_version();
    if current > latest {
        bail!(
            "store schema is v{current} but this build only understands v{latest}; \
             upgrade recanta"
        );
    }
    for (version, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        conn.execute_batch("BEGIN;")?;
        let applied = (|| -> Result<()> {
            conn.execute_batch(sql)
                .with_context(|| format!("applying migration v{version}"))?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [version],
            )?;
            Ok(())
        })();
        match applied {
            Ok(()) => {
                conn.execute_batch("COMMIT;")?;
                current = *version;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e);
            }
        }
    }
    Ok(current)
}

/// Guard for read paths: error if the store schema is newer than this binary.
pub fn ensure_compatible(conn: &Connection) -> Result<()> {
    let current = current_version(conn)?;
    let latest = latest_version();
    if current > latest {
        bail!(
            "store schema is v{current} but this build only understands v{latest}; \
             upgrade recanta"
        );
    }
    if current < latest {
        bail!("store schema is v{current}; run `recanta migrate` to reach v{latest}");
    }
    Ok(())
}

/// v0.1 core schema (PRD §11.1). Single project per file; `user`-scope memory lives in
/// the global store added later. FTS5 over memory titles/content is the v0.1 search
/// backbone (PRD §12.1).
const MIGRATION_0001: &str = r#"
CREATE TABLE users (
    id           INTEGER PRIMARY KEY,
    display_name TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE projects (
    id              TEXT PRIMARY KEY,           -- UUID, mirrors project.json
    name            TEXT NOT NULL,
    root_path       TEXT NOT NULL,
    root_commit_sha TEXT,                       -- primary identity when present
    remote_url_hash TEXT,                       -- hint only
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE repositories (
    id              INTEGER PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id),
    root_path       TEXT NOT NULL,
    root_commit_sha TEXT,
    remote_url_hash TEXT,
    default_branch  TEXT
);
CREATE INDEX idx_repositories_project ON repositories(project_id);

-- Append-only, idempotent event log (PRD §12.2). Duplicate hook fires collide on
-- idempotency_key and are no-ops; claimed_at is the single-worker ingestion lease.
CREATE TABLE events (
    id               INTEGER PRIMARY KEY,
    idempotency_key  TEXT NOT NULL UNIQUE,
    type             TEXT NOT NULL,
    source           TEXT,
    harness          TEXT,
    project_id       TEXT REFERENCES projects(id),
    repo_id          INTEGER REFERENCES repositories(id),
    branch           TEXT,
    commit_sha       TEXT,
    payload_redacted TEXT,
    capture_mode     TEXT NOT NULL DEFAULT 'summary',  -- summary | raw
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    claimed_at       TEXT
);
CREATE INDEX idx_events_type ON events(type);
CREATE INDEX idx_events_commit ON events(commit_sha);

CREATE TABLE memory_items (
    id                INTEGER PRIMARY KEY,
    type              TEXT NOT NULL,    -- semantic|procedural|episodic|decision|task|warning|bug|code_summary
    scope             TEXT NOT NULL,    -- user|project|repo|branch|symbol
    title             TEXT NOT NULL,
    content           TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'active',  -- active|superseded|disputed|archived|deleted
    importance        TEXT NOT NULL DEFAULT 'normal',  -- low|normal|high
    confidence        REAL NOT NULL DEFAULT 1.0,
    valid_from        TEXT NOT NULL DEFAULT (datetime('now')),
    valid_to          TEXT,
    source_event_ids  TEXT,             -- JSON array
    source_commit_shas TEXT,            -- JSON array
    superseded_by     INTEGER REFERENCES memory_items(id),
    branch            TEXT,             -- set when scope='branch'
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_memory_scope_status ON memory_items(scope, status);
CREATE INDEX idx_memory_type ON memory_items(type);

-- FTS5 backbone over memory text, kept in sync via triggers (external-content table).
CREATE VIRTUAL TABLE memory_fts USING fts5(
    title, content,
    content='memory_items', content_rowid='id',
    tokenize='unicode61'
);
CREATE TRIGGER memory_ai AFTER INSERT ON memory_items BEGIN
    INSERT INTO memory_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
END;
CREATE TRIGGER memory_ad AFTER DELETE ON memory_items BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, title, content)
        VALUES ('delete', old.id, old.title, old.content);
END;
CREATE TRIGGER memory_au AFTER UPDATE ON memory_items BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, title, content)
        VALUES ('delete', old.id, old.title, old.content);
    INSERT INTO memory_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
END;

CREATE TABLE code_symbols (
    id               INTEGER PRIMARY KEY,
    repo_id          INTEGER REFERENCES repositories(id),
    file_path        TEXT NOT NULL,
    symbol_type      TEXT NOT NULL,    -- function|class|method|module|...
    name             TEXT NOT NULL,
    qualified_name   TEXT NOT NULL,
    signature        TEXT,
    start_line       INTEGER,
    end_line         INTEGER,
    body_hash        TEXT,             -- identity = qualified_name + body_hash
    last_seen_commit TEXT,
    status           TEXT NOT NULL DEFAULT 'active'  -- active|deleted
);
CREATE INDEX idx_symbols_repo_qname ON code_symbols(repo_id, qualified_name);
CREATE INDEX idx_symbols_file ON code_symbols(file_path);

CREATE TABLE code_relationships (
    id                INTEGER PRIMARY KEY,
    from_symbol_id    INTEGER NOT NULL REFERENCES code_symbols(id),
    to_symbol_id      INTEGER REFERENCES code_symbols(id),
    relationship_type TEXT NOT NULL,   -- DEFINES|IMPORTS|CHANGED_BY|CALLS(low-conf)|...
    confidence        REAL NOT NULL DEFAULT 1.0,
    source            TEXT
);
CREATE INDEX idx_rel_from ON code_relationships(from_symbol_id);
CREATE INDEX idx_rel_to ON code_relationships(to_symbol_id);

CREATE TABLE commits (
    sha          TEXT PRIMARY KEY,
    repo_id      INTEGER REFERENCES repositories(id),
    branch       TEXT,
    author_hash  TEXT,
    message      TEXT,
    timestamp    TEXT,
    parent_shas  TEXT,                 -- JSON array
    orphaned_sha INTEGER NOT NULL DEFAULT 0   -- bool: SHA vanished via squash/rebase
);

CREATE TABLE symbol_changes (
    id          INTEGER PRIMARY KEY,
    symbol_id   INTEGER REFERENCES code_symbols(id),
    commit_sha  TEXT REFERENCES commits(sha),
    event_id    INTEGER REFERENCES events(id),
    change_type TEXT NOT NULL,         -- added|modified|deleted|moved|renamed
    summary     TEXT,
    diff_ref    TEXT
);
CREATE INDEX idx_symbol_changes_symbol ON symbol_changes(symbol_id);

CREATE TABLE hook_installations (
    id               INTEGER PRIMARY KEY,
    project_id       TEXT REFERENCES projects(id),
    harness          TEXT NOT NULL,
    file_path        TEXT NOT NULL,
    mechanism        TEXT NOT NULL,    -- git-hooks|husky|pre-commit|lefthook|core.hooksPath|settings|plugin
    managed_block_id TEXT,
    backup_path      TEXT,
    installed_at     TEXT NOT NULL DEFAULT (datetime('now')),
    version          TEXT,
    status           TEXT NOT NULL DEFAULT 'installed'
);

CREATE TABLE redaction_audit (
    id         INTEGER PRIMARY KEY,
    event_id   INTEGER REFERENCES events(id),
    pattern_id TEXT NOT NULL,
    path       TEXT,
    span       TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn
    }

    #[test]
    fn migrate_reaches_latest_and_is_idempotent() {
        let conn = fresh();
        assert_eq!(current_version(&conn).unwrap(), 0);
        let v1 = migrate(&conn).unwrap();
        assert_eq!(v1, latest_version());
        // Re-running applies nothing and stays at the same version.
        let v2 = migrate(&conn).unwrap();
        assert_eq!(v2, latest_version());
        // Exactly one row recorded per applied migration.
        let rows: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, latest_version());
    }

    #[test]
    fn memory_fts_tracks_memory_items() {
        let conn = fresh();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO memory_items (type, scope, title, content)
             VALUES ('decision', 'project', 'Use SQLite', 'single portable file')",
            [],
        )
        .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH 'portable'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1, "FTS index should surface the inserted memory");
    }

    #[test]
    fn rejects_newer_schema() {
        let conn = fresh();
        migrate(&conn).unwrap();
        // Simulate a store written by a future build.
        conn.execute("INSERT INTO schema_version (version) VALUES (9999)", [])
            .unwrap();
        assert!(ensure_compatible(&conn).is_err());
        assert!(migrate(&conn).is_err());
    }
}
