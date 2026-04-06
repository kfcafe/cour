use rusqlite::Connection;

pub fn initialize_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS accounts (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email_address TEXT NOT NULL,
            maildir_root TEXT NOT NULL,
            sync_command TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS mailboxes (
            id INTEGER PRIMARY KEY,
            account_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            special_use TEXT,
            FOREIGN KEY (account_id) REFERENCES accounts(id)
        );

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY,
            account_id INTEGER NOT NULL,
            mailbox_id INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            message_id_header TEXT,
            in_reply_to TEXT,
            references_json TEXT,
            subject TEXT,
            from_name TEXT,
            from_email TEXT,
            to_json TEXT,
            cc_json TEXT,
            sent_at TEXT,
            received_at TEXT,
            flags_json TEXT,
            body_text TEXT,
            body_html TEXT,
            snippet TEXT,
            parse_hash TEXT,
            file_mtime INTEGER,
            indexed_at TEXT NOT NULL,
            FOREIGN KEY (account_id) REFERENCES accounts(id),
            FOREIGN KEY (mailbox_id) REFERENCES mailboxes(id)
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
            subject,
            body_text,
            snippet,
            content='messages',
            content_rowid='id'
        );

        CREATE TABLE IF NOT EXISTS participants (
            id INTEGER PRIMARY KEY,
            message_id INTEGER NOT NULL,
            role TEXT NOT NULL,
            display_name TEXT,
            email TEXT NOT NULL,
            FOREIGN KEY (message_id) REFERENCES messages(id)
        );

        CREATE TABLE IF NOT EXISTS threads (
            id INTEGER PRIMARY KEY,
            account_id INTEGER NOT NULL,
            subject_canonical TEXT,
            latest_message_at TEXT,
            unread_count INTEGER NOT NULL DEFAULT 0,
            message_count INTEGER NOT NULL DEFAULT 0,
            state TEXT,
            needs_reply INTEGER NOT NULL DEFAULT 0,
            waiting_since TEXT,
            has_draft INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (account_id) REFERENCES accounts(id)
        );

        CREATE TABLE IF NOT EXISTS thread_messages (
            thread_id INTEGER NOT NULL,
            message_id INTEGER NOT NULL,
            ordinal INTEGER NOT NULL,
            PRIMARY KEY (thread_id, message_id),
            FOREIGN KEY (thread_id) REFERENCES threads(id),
            FOREIGN KEY (message_id) REFERENCES messages(id)
        );

        CREATE TABLE IF NOT EXISTS message_ai (
            message_id INTEGER PRIMARY KEY,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            summary TEXT,
            action TEXT,
            urgency_score REAL,
            confidence REAL,
            categories_json TEXT,
            entities_json TEXT,
            deadlines_json TEXT,
            embedding_blob BLOB,
            enriched_at TEXT NOT NULL,
            FOREIGN KEY (message_id) REFERENCES messages(id)
        );

        CREATE TABLE IF NOT EXISTS thread_ai (
            thread_id INTEGER PRIMARY KEY,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            brief_summary TEXT,
            latest_ask TEXT,
            recommended_action TEXT,
            thread_state_hint TEXT,
            related_thread_ids_json TEXT,
            stale_after TEXT,
            enriched_at TEXT NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id)
        );

        CREATE TABLE IF NOT EXISTS drafts (
            id INTEGER PRIMARY KEY,
            thread_id INTEGER,
            status TEXT NOT NULL,
            source TEXT NOT NULL,
            to_json TEXT,
            cc_json TEXT,
            subject TEXT,
            body_text TEXT,
            provider TEXT,
            model TEXT,
            confidence REAL,
            rationale_json TEXT,
            approved_at TEXT,
            sent_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id)
        );

        CREATE TABLE IF NOT EXISTS audit_events (
            id INTEGER PRIMARY KEY,
            entity_type TEXT NOT NULL,
            entity_id INTEGER NOT NULL,
            action TEXT NOT NULL,
            details_json TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sync_runs (
            id INTEGER PRIMARY KEY,
            account_id INTEGER NOT NULL,
            command TEXT NOT NULL,
            exit_code INTEGER,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            stderr_text TEXT,
            stdout_text TEXT,
            FOREIGN KEY (account_id) REFERENCES accounts(id)
        );
        "#,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        env, fs,
        path::{Path, PathBuf},
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use rusqlite::Connection;

    use super::initialize_schema;

    struct TempDatabasePath {
        path: PathBuf,
    }

    impl TempDatabasePath {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "cour-index-schema-{pid}-{unique}.sqlite",
                pid = process::id()
            ));
            Self { path }
        }

        fn as_path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDatabasePath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(format!("{}-wal", self.path.display()));
            let _ = fs::remove_file(format!("{}-shm", self.path.display()));
        }
    }

    #[test]
    fn initializes_schema_with_fts() {
        let temp_db = TempDatabasePath::new();
        let conn = Connection::open(temp_db.as_path()).expect("open temp sqlite db");

        initialize_schema(&conn).expect("initialize schema");

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("prepare sqlite_master query");
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query sqlite_master")
            .collect::<rusqlite::Result<BTreeSet<_>>>()
            .expect("collect table names");

        for expected in [
            "accounts",
            "mailboxes",
            "messages",
            "message_fts",
            "participants",
            "threads",
            "thread_messages",
            "message_ai",
            "thread_ai",
            "drafts",
            "audit_events",
            "sync_runs",
        ] {
            assert!(names.contains(expected), "missing table: {expected}");
        }
    }
}
