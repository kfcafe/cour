use rusqlite::{params, Connection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalSearchRow {
    pub thread_id: i64,
    pub subject: Option<String>,
    pub snippet: String,
}

pub fn search_lexical_candidates(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> rusqlite::Result<Vec<LexicalSearchRow>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT tm.thread_id, m.subject, snippet(message_fts, 2, '[', ']', '…', 12)
         FROM message_fts
         JOIN messages m ON m.id = message_fts.rowid
         JOIN thread_messages tm ON tm.message_id = m.id
         WHERE message_fts MATCH ?1
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![query, limit as i64], |row| {
        Ok(LexicalSearchRow {
            thread_id: row.get(0)?,
            subject: row.get(1)?,
            snippet: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        })
    })?;

    rows.collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::config::AccountConfig;
    use crate::index::query::rebuild_threads_for_account;
    use crate::index::repo::{upsert_account, upsert_mailbox, upsert_message};
    use crate::index::schema::initialize_schema;
    use crate::maildir::DiscoveredMailbox;
    use crate::model::{ParsedAddress, ParsedMessage};

    use super::search_lexical_candidates;

    fn temp_db() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("cour-search-test-{unique}.sqlite"))
    }

    #[test]
    fn lexical_search_matches_body_text() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let account = AccountConfig {
            name: "personal".to_string(),
            email_address: "ash@example.com".to_string(),
            maildir_root: "/tmp/mail/personal".into(),
            sync_command: None,
            default: Some(true),
        };
        let account_id = upsert_account(&conn, &account).expect("upsert account");
        let mailbox = DiscoveredMailbox {
            name: "Inbox".to_string(),
            path: "/tmp/mail/personal".into(),
            special_use: Some("inbox".to_string()),
        };
        let mailbox_id = upsert_mailbox(&conn, account_id, &mailbox).expect("upsert mailbox");

        let parsed = ParsedMessage {
            file_path: "/tmp/mail/personal/cur/msg-1:2,".into(),
            message_id_header: Some("<msg-1@example.com>".to_string()),
            in_reply_to: None,
            references: vec![],
            subject: Some("Invoice Followup".to_string()),
            from: Some(ParsedAddress {
                display_name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
            }),
            to: vec![ParsedAddress {
                display_name: Some("Ash".to_string()),
                email: "ash@example.com".to_string(),
            }],
            cc: vec![],
            sent_at: Some("2026-04-20T10:00:00+00:00".to_string()),
            body_text: "Please pay the invoice this week".to_string(),
            body_html: None,
            snippet: "Please pay the invoice this week".to_string(),
            parse_hash: "hash-1".to_string(),
            file_mtime: 123,
        };
        upsert_message(&conn, account_id, mailbox_id, &parsed).expect("upsert message");
        rebuild_threads_for_account(&conn, account_id).expect("rebuild threads");

        let rows = search_lexical_candidates(&conn, "invoice", 10).expect("search lexical");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subject.as_deref(), Some("Invoice Followup"));

        let _ = fs::remove_file(db_path);
    }
}
