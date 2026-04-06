use std::fs;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::config::AccountConfig;
use crate::error::{AppError, AppResult};
use crate::index::query::rebuild_threads_for_account;
use crate::index::repo::{upsert_account, upsert_mailbox, upsert_message};
use crate::maildir::{discover_mailboxes, DiscoveredMailbox};
use crate::parse::parse_message_file;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestReport {
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub fn ingest_maildir_account(
    conn: &Connection,
    account: &AccountConfig,
) -> AppResult<IngestReport> {
    let account_id =
        upsert_account(conn, account).map_err(|err| AppError::Sqlite(err.to_string()))?;
    let discovered = discover_mailboxes(&account.maildir_root)?;

    let mut report = IngestReport::default();
    for mailbox in discovered {
        ingest_mailbox(conn, account_id, &mailbox, &mut report)?;
    }

    rebuild_threads_for_account(conn, account_id)
        .map_err(|err| AppError::Sqlite(err.to_string()))?;

    Ok(report)
}

fn ingest_mailbox(
    conn: &Connection,
    account_id: i64,
    mailbox: &DiscoveredMailbox,
    report: &mut IngestReport,
) -> AppResult<()> {
    let mailbox_id = upsert_mailbox(conn, account_id, mailbox)
        .map_err(|err| AppError::Sqlite(err.to_string()))?;

    for subdir in ["cur", "new"] {
        let dir = mailbox.path.join(subdir);
        if !dir.is_dir() {
            continue;
        }

        for entry in fs::read_dir(&dir).map_err(AppError::Io)? {
            let entry = entry.map_err(AppError::Io)?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            match ingest_message_file(conn, account_id, mailbox_id, &path) {
                Ok(IngestOutcome::Imported) => report.imported += 1,
                Ok(IngestOutcome::Updated) => report.updated += 1,
                Ok(IngestOutcome::Skipped) => report.skipped += 1,
                Err(_) => report.failed += 1,
            }
        }
    }

    Ok(())
}

enum IngestOutcome {
    Imported,
    Updated,
    Skipped,
}

fn ingest_message_file(
    conn: &Connection,
    account_id: i64,
    mailbox_id: i64,
    path: &Path,
) -> AppResult<IngestOutcome> {
    let parsed = parse_message_file(path)?;

    let existing: Option<(i64, i64, String)> = conn
        .query_row(
            "SELECT id, file_mtime, parse_hash FROM messages WHERE account_id = ?1 AND file_path = ?2 ORDER BY id DESC LIMIT 1",
            rusqlite::params![account_id, path.to_string_lossy().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|err| AppError::Sqlite(err.to_string()))?;

    match existing {
        Some((_id, file_mtime, parse_hash))
            if file_mtime == parsed.file_mtime && parse_hash == parsed.parse_hash =>
        {
            Ok(IngestOutcome::Skipped)
        }
        Some(_) => {
            upsert_message(conn, account_id, mailbox_id, &parsed)
                .map_err(|err| AppError::Sqlite(err.to_string()))?;
            Ok(IngestOutcome::Updated)
        }
        None => {
            upsert_message(conn, account_id, mailbox_id, &parsed)
                .map_err(|err| AppError::Sqlite(err.to_string()))?;
            Ok(IngestOutcome::Imported)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::config::AccountConfig;
    use crate::index::query::{get_thread_detail, list_threads_by_state};
    use crate::index::schema::initialize_schema;

    use super::ingest_maildir_account;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-ingest-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    fn create_maildir(path: &std::path::Path) {
        fs::create_dir_all(path.join("cur")).expect("create cur");
        fs::create_dir_all(path.join("new")).expect("create new");
        fs::create_dir_all(path.join("tmp")).expect("create tmp");
    }

    fn write_message(
        path: &std::path::Path,
        subdir: &str,
        name: &str,
        from: &str,
        to: &str,
        subject: &str,
        date: &str,
        extra_headers: &[(&str, &str)],
        body: &str,
    ) {
        let mut headers = format!(
            "From: {from}\nTo: {to}\nSubject: {subject}\nMessage-ID: <{name}@example.com>\nDate: {date}\n"
        );
        for (key, value) in extra_headers {
            headers.push_str(&format!("{key}: {value}\n"));
        }

        fs::write(path.join(subdir).join(name), format!("{headers}\n{body}\n"))
            .expect("write message");
    }

    #[test]
    fn imports_fixture_maildir_into_sqlite() {
        let root = temp_root("maildir");
        create_maildir(&root);
        write_message(
            &root,
            "cur",
            "msg-1",
            "Alice <alice@example.com>",
            "Ash <ash@example.com>",
            "Hello",
            "Tue, 11 Mar 2026 10:00:00 +0000",
            &[],
            "hello one",
        );

        let db_path = temp_root("db").join("index.sqlite");
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let account = AccountConfig {
            name: "personal".to_string(),
            email_address: "ash@example.com".to_string(),
            maildir_root: root.clone(),
            sync_command: None,
            default: Some(true),
        };

        let report = ingest_maildir_account(&conn, &account).expect("ingest maildir");
        assert_eq!(report.imported, 1);
        assert_eq!(report.failed, 0);

        let messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .expect("count messages");
        assert_eq!(messages, 1);

        let threads: i64 = conn
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
            .expect("count threads");
        assert_eq!(threads, 1);

        let waiting = list_threads_by_state(&conn, "waiting_on_me").expect("list waiting threads");
        assert_eq!(waiting.len(), 1);

        let detail = get_thread_detail(&conn, waiting[0].id)
            .expect("get thread detail")
            .expect("thread detail exists");
        assert_eq!(detail.messages.len(), 1);

        let membership_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM thread_messages", [], |row| row.get(0))
            .expect("count thread membership");
        assert_eq!(membership_count, 1);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn persists_thread_state_transitions_after_ingest() {
        let root = temp_root("thread-state-maildir");
        create_maildir(&root);

        write_message(
            &root,
            "cur",
            "msg-1",
            "Alice <alice@example.com>",
            "Ash <ash@example.com>",
            "Need reply",
            "Tue, 11 Mar 2026 10:00:00 +0000",
            &[],
            "Can you send the update?",
        );
        write_message(
            &root,
            "cur",
            "msg-2",
            "Ash <ash@example.com>",
            "Alice <alice@example.com>",
            "Re: Need reply",
            "Tue, 11 Mar 2026 11:00:00 +0000",
            &[
                ("In-Reply-To", "<msg-1@example.com>"),
                ("References", "<msg-1@example.com>"),
            ],
            "Sent it over.",
        );
        write_message(
            &root,
            "new",
            "msg-3",
            "Newsletter <newsletter@example.com>",
            "Ash <ash@example.com>",
            "Weekly digest",
            "Tue, 11 Mar 2026 12:00:00 +0000",
            &[],
            "Top stories for this week.",
        );

        let db_path = temp_root("thread-state-db").join("index.sqlite");
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let account = AccountConfig {
            name: "personal".to_string(),
            email_address: "ash@example.com".to_string(),
            maildir_root: root.clone(),
            sync_command: None,
            default: Some(true),
        };

        let report = ingest_maildir_account(&conn, &account).expect("ingest maildir");
        assert_eq!(report.imported, 3);
        assert_eq!(report.failed, 0);

        let waiting_on_them =
            list_threads_by_state(&conn, "waiting_on_them").expect("list waiting_on_them");
        assert_eq!(waiting_on_them.len(), 1);
        assert_eq!(waiting_on_them[0].message_count, 2);

        let low_value = list_threads_by_state(&conn, "low_value").expect("list low_value");
        assert_eq!(low_value.len(), 1);
        assert_eq!(low_value[0].message_count, 1);

        let thread_rows: Vec<(String, i64, i64, Option<String>)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT state, needs_reply, has_draft, waiting_since FROM threads ORDER BY id ASC",
                )
                .expect("prepare threads query");
            stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("query threads")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect threads")
        };
        assert_eq!(thread_rows.len(), 2);
        assert!(thread_rows.iter().any(|row| {
            row.0 == "waiting_on_them"
                && row.1 == 0
                && row.2 == 0
                && row.3.as_deref() == Some("2026-03-11T11:00:00Z")
        }));
        assert!(thread_rows.iter().any(|row| {
            row.0 == "low_value"
                && row.1 == 1
                && row.2 == 0
                && row.3.as_deref() == Some("2026-03-11T12:00:00Z")
        }));

        let membership_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM thread_messages", [], |row| row.get(0))
            .expect("count thread membership");
        assert_eq!(membership_count, 3);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(db_path);
    }
}
