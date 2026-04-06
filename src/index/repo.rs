use rusqlite::{params, Connection, OptionalExtension};

use crate::ai::traits::{EmbeddingResult, ExtractionOutput};
use crate::config::AccountConfig;
use crate::maildir::DiscoveredMailbox;
use crate::model::{ParsedMessage, ParsedParticipant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMailbox {
    pub id: i64,
    pub name: String,
    pub path: String,
}

pub fn upsert_account(conn: &Connection, account: &AccountConfig) -> rusqlite::Result<i64> {
    let existing = conn
        .query_row(
            "SELECT id FROM accounts WHERE email_address = ?1",
            params![account.email_address],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    match existing {
        Some(id) => {
            conn.execute(
                "UPDATE accounts SET name = ?1, maildir_root = ?2, sync_command = ?3 WHERE id = ?4",
                params![
                    account.name,
                    account.maildir_root.to_string_lossy(),
                    account.sync_command,
                    id,
                ],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO accounts (name, email_address, maildir_root, sync_command, created_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))",
                params![
                    account.name,
                    account.email_address,
                    account.maildir_root.to_string_lossy(),
                    account.sync_command,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn upsert_mailbox(
    conn: &Connection,
    account_id: i64,
    mailbox: &DiscoveredMailbox,
) -> rusqlite::Result<i64> {
    let mailbox_path = mailbox.path.to_string_lossy().to_string();
    let existing = conn
        .query_row(
            "SELECT id FROM mailboxes WHERE account_id = ?1 AND path = ?2",
            params![account_id, mailbox_path],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    match existing {
        Some(id) => {
            conn.execute(
                "UPDATE mailboxes SET name = ?1, special_use = ?2 WHERE id = ?3",
                params![mailbox.name, mailbox.special_use, id],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO mailboxes (account_id, name, path, special_use)
                 VALUES (?1, ?2, ?3, ?4)",
                params![account_id, mailbox.name, mailbox_path, mailbox.special_use],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn list_mailboxes(conn: &Connection, account_id: i64) -> rusqlite::Result<Vec<StoredMailbox>> {
    let mut stmt =
        conn.prepare("SELECT id, name, path FROM mailboxes WHERE account_id = ?1 ORDER BY name")?;
    let rows = stmt.query_map(params![account_id], |row| {
        Ok(StoredMailbox {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
        })
    })?;

    rows.collect()
}

pub fn upsert_message(
    conn: &Connection,
    account_id: i64,
    mailbox_id: i64,
    parsed: &ParsedMessage,
) -> rusqlite::Result<i64> {
    let file_path = parsed.file_path.to_string_lossy().to_string();
    let existing = conn
        .query_row(
            "SELECT id FROM messages WHERE account_id = ?1 AND file_path = ?2 AND parse_hash = ?3",
            params![account_id, file_path, parsed.parse_hash],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    let message_id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE messages SET
                    mailbox_id = ?1,
                    message_id_header = ?2,
                    in_reply_to = ?3,
                    references_json = ?4,
                    subject = ?5,
                    from_name = ?6,
                    from_email = ?7,
                    to_json = ?8,
                    cc_json = ?9,
                    sent_at = ?10,
                    flags_json = ?11,
                    body_text = ?12,
                    body_html = ?13,
                    snippet = ?14,
                    file_mtime = ?15,
                    indexed_at = datetime('now')
                 WHERE id = ?16",
                params![
                    mailbox_id,
                    parsed.message_id_header,
                    parsed.in_reply_to,
                    serde_json_string(&parsed.references),
                    parsed.subject,
                    parsed.from.as_ref().and_then(|a| a.display_name.clone()),
                    parsed.from.as_ref().map(|a| a.email.clone()),
                    serde_json_string(
                        &parsed
                            .to
                            .iter()
                            .map(|a| a.email.clone())
                            .collect::<Vec<_>>()
                    ),
                    serde_json_string(
                        &parsed
                            .cc
                            .iter()
                            .map(|a| a.email.clone())
                            .collect::<Vec<_>>()
                    ),
                    parsed.sent_at,
                    "[]",
                    parsed.body_text,
                    parsed.body_html,
                    parsed.snippet,
                    parsed.file_mtime,
                    id,
                ],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO messages (
                    account_id, mailbox_id, file_path, message_id_header, in_reply_to,
                    references_json, subject, from_name, from_email, to_json, cc_json,
                    sent_at, received_at, flags_json, body_text, body_html, snippet,
                    parse_hash, file_mtime, indexed_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9, ?10, ?11,
                    ?12, datetime('now'), ?13, ?14, ?15, ?16,
                    ?17, ?18, datetime('now')
                 )",
                params![
                    account_id,
                    mailbox_id,
                    file_path,
                    parsed.message_id_header,
                    parsed.in_reply_to,
                    serde_json_string(&parsed.references),
                    parsed.subject,
                    parsed.from.as_ref().and_then(|a| a.display_name.clone()),
                    parsed.from.as_ref().map(|a| a.email.clone()),
                    serde_json_string(
                        &parsed
                            .to
                            .iter()
                            .map(|a| a.email.clone())
                            .collect::<Vec<_>>()
                    ),
                    serde_json_string(
                        &parsed
                            .cc
                            .iter()
                            .map(|a| a.email.clone())
                            .collect::<Vec<_>>()
                    ),
                    parsed.sent_at,
                    "[]",
                    parsed.body_text,
                    parsed.body_html,
                    parsed.snippet,
                    parsed.parse_hash,
                    parsed.file_mtime,
                ],
            )?;
            conn.last_insert_rowid()
        }
    };

    refresh_participants(conn, message_id, &parsed.participants())?;
    refresh_message_fts(conn, message_id, parsed)?;

    Ok(message_id)
}

fn refresh_participants(
    conn: &Connection,
    message_id: i64,
    participants: &[ParsedParticipant],
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM participants WHERE message_id = ?1",
        params![message_id],
    )?;
    for participant in participants {
        conn.execute(
            "INSERT INTO participants (message_id, role, display_name, email)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                message_id,
                participant.role,
                participant.display_name,
                participant.email,
            ],
        )?;
    }
    Ok(())
}

fn refresh_message_fts(
    conn: &Connection,
    message_id: i64,
    parsed: &ParsedMessage,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO message_fts(rowid, subject, body_text, snippet) VALUES (?1, ?2, ?3, ?4)",
        params![message_id, parsed.subject, parsed.body_text, parsed.snippet],
    )?;
    Ok(())
}

fn serde_json_string(values: &[String]) -> String {
    let mut out = String::from("[");
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(ch),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

fn serde_json_opt_string(values: Option<&[String]>) -> Option<String> {
    values.map(serde_json_string)
}

fn serialize_embedding_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(vector));
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessageAiMetadata {
    pub message_id: i64,
    pub extraction_hash: Option<String>,
    pub embedding_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredThreadAiMetadata {
    pub thread_id: i64,
    pub content_hash: Option<String>,
}

pub struct MessageAiUpsert<'a> {
    pub message_id: i64,
    pub extraction_hash: Option<String>,
    pub extraction: Option<&'a ExtractionOutput>,
    pub embedding_hash: Option<String>,
    pub embedding: Option<&'a EmbeddingResult>,
}

pub struct ThreadAiUpsert<'a> {
    pub thread_id: i64,
    pub content_hash: String,
    pub extraction: &'a ExtractionOutput,
    pub related_thread_ids: &'a [i64],
}

pub fn get_message_ai_metadata(
    conn: &Connection,
    message_id: i64,
) -> rusqlite::Result<Option<StoredMessageAiMetadata>> {
    conn.query_row(
        "SELECT message_id, categories_json, deadlines_json FROM message_ai WHERE message_id = ?1",
        params![message_id],
        |row| {
            Ok(StoredMessageAiMetadata {
                message_id: row.get(0)?,
                extraction_hash: row.get(1)?,
                embedding_hash: row.get(2)?,
            })
        },
    )
    .optional()
}

pub fn get_thread_ai_metadata(
    conn: &Connection,
    thread_id: i64,
) -> rusqlite::Result<Option<StoredThreadAiMetadata>> {
    conn.query_row(
        "SELECT thread_id, stale_after FROM thread_ai WHERE thread_id = ?1",
        params![thread_id],
        |row| {
            Ok(StoredThreadAiMetadata {
                thread_id: row.get(0)?,
                content_hash: row.get(1)?,
            })
        },
    )
    .optional()
}

pub fn list_message_ids_for_thread(
    conn: &Connection,
    thread_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT message_id FROM thread_messages WHERE thread_id = ?1 ORDER BY ordinal ASC, message_id ASC",
    )?;
    let rows = stmt.query_map(params![thread_id], |row| row.get(0))?;
    rows.collect()
}

pub fn upsert_message_ai(conn: &Connection, input: &MessageAiUpsert<'_>) -> rusqlite::Result<()> {
    let existing: Option<(
        Option<String>,
        Option<String>,
        Option<f32>,
        Option<f32>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<Vec<u8>>,
        Option<String>,
    )> = conn
        .query_row(
            "SELECT summary, action, urgency_score, confidence, provider, model,
                    entities_json, deadlines_json, embedding_blob, enriched_at
             FROM message_ai WHERE message_id = ?1",
            params![input.message_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .optional()?;

    let summary = input
        .extraction
        .map(|value| value.summary.clone())
        .or_else(|| existing.as_ref().and_then(|row| row.0.clone()));
    let action = input
        .extraction
        .map(|value| value.action.clone())
        .or_else(|| existing.as_ref().and_then(|row| row.1.clone()));
    let urgency_score = input
        .extraction
        .map(|value| value.urgency_score)
        .or_else(|| existing.as_ref().and_then(|row| row.2));
    let confidence = input
        .extraction
        .map(|value| value.confidence)
        .or_else(|| existing.as_ref().and_then(|row| row.3));
    let provider = input
        .embedding
        .map(|value| value.provider.clone())
        .or_else(|| input.extraction.map(|value| value.provider.clone()))
        .or_else(|| existing.as_ref().and_then(|row| row.4.clone()));
    let model = input
        .embedding
        .map(|value| value.model.clone())
        .or_else(|| input.extraction.map(|value| value.model.clone()))
        .or_else(|| existing.as_ref().and_then(|row| row.5.clone()));
    let entities_json = input
        .extraction
        .map(|value| serde_json_string(&value.entities))
        .or_else(|| existing.as_ref().and_then(|row| row.6.clone()));
    let deadlines_json = input
        .embedding_hash
        .clone()
        .or_else(|| existing.as_ref().and_then(|row| row.7.clone()));
    let embedding_blob = input
        .embedding
        .map(|value| serialize_embedding_blob(&value.vector))
        .or_else(|| existing.as_ref().and_then(|row| row.8.clone()));
    let enriched_at = if input.extraction.is_some() || input.embedding.is_some() {
        None::<String>
    } else {
        existing.as_ref().and_then(|row| row.9.clone())
    };

    conn.execute(
        "INSERT INTO message_ai (
            message_id, provider, model, summary, action, urgency_score, confidence,
            categories_json, entities_json, deadlines_json, embedding_blob, enriched_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, COALESCE(?12, datetime('now'))
         )
         ON CONFLICT(message_id) DO UPDATE SET
            provider = excluded.provider,
            model = excluded.model,
            summary = excluded.summary,
            action = excluded.action,
            urgency_score = excluded.urgency_score,
            confidence = excluded.confidence,
            categories_json = excluded.categories_json,
            entities_json = excluded.entities_json,
            deadlines_json = excluded.deadlines_json,
            embedding_blob = excluded.embedding_blob,
            enriched_at = COALESCE(?12, message_ai.enriched_at)",
        params![
            input.message_id,
            provider,
            model,
            summary,
            action,
            urgency_score,
            confidence,
            input.extraction_hash,
            entities_json,
            deadlines_json,
            embedding_blob,
            enriched_at,
        ],
    )?;

    Ok(())
}

pub fn upsert_thread_ai(conn: &Connection, input: &ThreadAiUpsert<'_>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO thread_ai (
            thread_id, provider, model, brief_summary, latest_ask, recommended_action,
            thread_state_hint, related_thread_ids_json, stale_after, enriched_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, datetime('now')
         )
         ON CONFLICT(thread_id) DO UPDATE SET
            provider = excluded.provider,
            model = excluded.model,
            brief_summary = excluded.brief_summary,
            latest_ask = excluded.latest_ask,
            recommended_action = excluded.recommended_action,
            thread_state_hint = excluded.thread_state_hint,
            related_thread_ids_json = excluded.related_thread_ids_json,
            stale_after = excluded.stale_after,
            enriched_at = datetime('now')",
        params![
            input.thread_id,
            input.extraction.provider,
            input.extraction.model,
            input.extraction.summary,
            input.extraction.latest_ask,
            input.extraction.action,
            input.extraction.thread_state_hint,
            serde_json_string(
                &input
                    .related_thread_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
            ),
            input.content_hash,
        ],
    )?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredDraft {
    pub id: i64,
    pub thread_id: Option<i64>,
    pub status: String,
    pub source: String,
    pub to_json: Option<String>,
    pub cc_json: Option<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub confidence: Option<f32>,
    pub rationale_json: Option<String>,
    pub approved_at: Option<String>,
    pub sent_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn create_draft(
    conn: &Connection,
    thread_id: Option<i64>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: &str,
    body_text: &str,
    source: &str,
    provider: Option<&str>,
    model: Option<&str>,
    confidence: Option<f32>,
    rationale_json: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO drafts (
            thread_id, status, source, to_json, cc_json, subject, body_text,
            provider, model, confidence, rationale_json, created_at, updated_at
         ) VALUES (?1, 'pending', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'), datetime('now'))",
        params![
            thread_id,
            source,
            serde_json_opt_string(Some(&to)),
            serde_json_opt_string(Some(&cc)),
            subject,
            body_text,
            provider,
            model,
            confidence,
            rationale_json,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_draft_status(conn: &Connection, draft_id: i64, status: &str) -> rusqlite::Result<()> {
    let current = get_draft(conn, draft_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;

    let valid = matches!(
        (current.status.as_str(), status),
        ("pending", "approved")
            | ("approved", "sent")
            | ("pending", "discarded")
            | ("approved", "discarded")
    );

    if !valid {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "invalid draft transition: {} -> {}",
            current.status, status
        )));
    }

    conn.execute(
        "UPDATE drafts
         SET status = ?1,
             approved_at = COALESCE(CASE WHEN ?1 = 'approved' THEN datetime('now') END, approved_at),
             sent_at = COALESCE(CASE WHEN ?1 = 'sent' THEN datetime('now') END, sent_at),
             updated_at = datetime('now')
         WHERE id = ?2",
        params![status, draft_id],
    )?;
    Ok(())
}

pub fn get_draft(conn: &Connection, draft_id: i64) -> rusqlite::Result<Option<StoredDraft>> {
    conn.query_row(
        "SELECT id, thread_id, status, source, to_json, cc_json, subject, body_text, provider, model,
                confidence, rationale_json, approved_at, sent_at, created_at, updated_at
         FROM drafts WHERE id = ?1",
        params![draft_id],
        |row| {
            Ok(StoredDraft {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                status: row.get(2)?,
                source: row.get(3)?,
                to_json: row.get(4)?,
                cc_json: row.get(5)?,
                subject: row.get(6)?,
                body_text: row.get(7)?,
                provider: row.get(8)?,
                model: row.get(9)?,
                confidence: row.get(10)?,
                rationale_json: row.get(11)?,
                approved_at: row.get(12)?,
                sent_at: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        },
    )
    .optional()
}

pub fn append_audit_event(
    conn: &Connection,
    entity_type: &str,
    entity_id: i64,
    action: &str,
    details_json: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO audit_events (entity_type, entity_id, action, details_json, created_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        params![entity_type, entity_id, action, details_json],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn record_sync_run(
    conn: &Connection,
    account_id: i64,
    command: &str,
    exit_code: Option<i32>,
    stdout_text: &str,
    stderr_text: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO sync_runs (
            account_id, command, exit_code, started_at, finished_at, stdout_text, stderr_text
         ) VALUES (?1, ?2, ?3, datetime('now'), datetime('now'), ?4, ?5)",
        params![account_id, command, exit_code, stdout_text, stderr_text],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_sync_run(
    conn: &Connection,
    sync_run_id: i64,
) -> rusqlite::Result<Option<(i64, String, Option<i32>, String, String)>> {
    conn.query_row(
        "SELECT account_id, command, exit_code, stdout_text, stderr_text FROM sync_runs WHERE id = ?1",
        params![sync_run_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )
    .optional()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::config::AccountConfig;
    use crate::index::schema::initialize_schema;
    use crate::maildir::DiscoveredMailbox;
    use crate::model::{ParsedAddress, ParsedMessage};

    use super::{
        append_audit_event, create_draft, get_draft, get_sync_run, record_sync_run,
        update_draft_status, upsert_account, upsert_mailbox, upsert_message,
    };

    fn temp_db() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("cour-repo-test-{unique}.sqlite"))
    }

    #[test]
    fn draft_status_transitions_round_trip() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let draft_id = create_draft(
            &conn,
            None,
            vec!["alice@example.com".to_string()],
            vec!["cc@example.com".to_string()],
            "Re: Hello",
            "Draft body",
            "ai",
            Some("openai-compatible"),
            Some("gpt-4o-mini"),
            None,
            Some("[\"reason\"]"),
        )
        .expect("create draft");

        let pending = get_draft(&conn, draft_id)
            .expect("get pending draft")
            .expect("pending draft exists");
        assert_eq!(pending.status, "pending");
        assert_eq!(pending.source, "ai");
        assert_eq!(pending.to_json.as_deref(), Some("[\"alice@example.com\"]"));
        assert_eq!(pending.cc_json.as_deref(), Some("[\"cc@example.com\"]"));
        assert_eq!(pending.subject.as_deref(), Some("Re: Hello"));
        assert_eq!(pending.body_text.as_deref(), Some("Draft body"));
        assert_eq!(pending.provider.as_deref(), Some("openai-compatible"));
        assert_eq!(pending.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(pending.confidence, None);
        assert_eq!(pending.rationale_json.as_deref(), Some("[\"reason\"]"));
        assert!(pending.approved_at.is_none());
        assert!(pending.sent_at.is_none());

        update_draft_status(&conn, draft_id, "approved").expect("approve draft");
        let approved = get_draft(&conn, draft_id)
            .expect("get approved draft")
            .expect("approved draft exists");
        assert_eq!(approved.status, "approved");
        assert!(approved.approved_at.is_some());
        assert!(approved.sent_at.is_none());

        update_draft_status(&conn, draft_id, "sent").expect("send draft");
        let sent = get_draft(&conn, draft_id)
            .expect("get sent draft")
            .expect("sent draft exists");
        assert_eq!(sent.status, "sent");
        assert!(sent.approved_at.is_some());
        assert!(sent.sent_at.is_some());

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn rejects_invalid_draft_transition() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let draft_id = create_draft(
            &conn,
            None,
            vec!["alice@example.com".to_string()],
            vec![],
            "Re: Hello",
            "Draft body",
            "ai",
            Some("openai-compatible"),
            Some("gpt-4o-mini"),
            None,
            Some("[\"reason\"]"),
        )
        .expect("create draft");

        let err =
            update_draft_status(&conn, draft_id, "sent").expect_err("reject invalid transition");
        assert!(err.to_string().contains("invalid draft transition"));

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn records_audit_event_for_draft_approval() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let draft_id = create_draft(
            &conn,
            None,
            vec!["alice@example.com".to_string()],
            vec![],
            "Re: Hello",
            "Draft body",
            "ai",
            Some("openai-compatible"),
            Some("gpt-4o-mini"),
            None,
            Some("[\"reason\"]"),
        )
        .expect("create draft");

        update_draft_status(&conn, draft_id, "approved").expect("approve draft");
        let audit_id = append_audit_event(&conn, "draft", draft_id, "draft_approved", Some("{}"))
            .expect("append audit event");

        let stored: (String, i64, String, Option<String>) = conn
            .query_row(
                "SELECT entity_type, entity_id, action, details_json FROM audit_events WHERE id = ?1",
                [audit_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("read audit event");
        assert_eq!(stored.0, "draft");
        assert_eq!(stored.1, draft_id);
        assert_eq!(stored.2, "draft_approved");
        assert_eq!(stored.3.as_deref(), Some("{}"));

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn reimport_updates_existing_message() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let account = AccountConfig {
            name: "personal".to_string(),
            email_address: "ash@example.com".to_string(),
            maildir_root: "/tmp/mail/personal".into(),
            sync_command: Some("mbsync personal".to_string()),
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
            subject: Some("Hello".to_string()),
            from: Some(ParsedAddress {
                display_name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
            }),
            to: vec![ParsedAddress {
                display_name: Some("Asher".to_string()),
                email: "ash@example.com".to_string(),
            }],
            cc: vec![],
            sent_at: Some("2026-04-20T10:00:00+00:00".to_string()),
            body_text: "hello one".to_string(),
            body_html: None,
            snippet: "hello one".to_string(),
            parse_hash: "hash-1".to_string(),
            file_mtime: 123,
        };

        let id1 = upsert_message(&conn, account_id, mailbox_id, &parsed).expect("insert message");
        let id2 = upsert_message(&conn, account_id, mailbox_id, &parsed).expect("reimport message");
        assert_eq!(id1, id2);

        let message_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .expect("count messages");
        assert_eq!(message_count, 1);

        let participant_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM participants", [], |row| row.get(0))
            .expect("count participants");
        assert_eq!(participant_count, 2);

        let fts_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM message_fts", [], |row| row.get(0))
            .expect("count fts rows");
        assert_eq!(fts_count, 1);

        let draft_id = create_draft(
            &conn,
            None,
            vec!["alice@example.com".to_string()],
            vec![],
            "Re: Hello",
            "Draft body",
            "ai",
            Some("openai-compatible"),
            Some("gpt-4o-mini"),
            None,
            Some("[\"reason\"]"),
        )
        .expect("create draft");

        update_draft_status(&conn, draft_id, "approved").expect("approve draft");
        let draft = get_draft(&conn, draft_id)
            .expect("get draft")
            .expect("draft exists");
        assert_eq!(draft.status, "approved");
        assert_eq!(draft.source, "ai");
        assert_eq!(draft.confidence, None);
        assert_eq!(draft.rationale_json.as_deref(), Some("[\"reason\"]"));
        assert!(draft.approved_at.is_some());

        let audit_id = append_audit_event(&conn, "draft", draft_id, "draft_approved", Some("{}"))
            .expect("append audit event");
        assert!(audit_id > 0);

        let sync_run_id = record_sync_run(&conn, account_id, "mbsync personal", Some(0), "ok", "")
            .expect("record sync run");
        assert!(sync_run_id > 0);

        let sync_run = get_sync_run(&conn, sync_run_id)
            .expect("get sync run")
            .expect("sync run exists");
        assert_eq!(sync_run.0, account_id);
        assert_eq!(sync_run.1, "mbsync personal");
        assert_eq!(sync_run.2, Some(0));
        assert_eq!(sync_run.3, "ok");
        assert_eq!(sync_run.4, "");

        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))
            .expect("count audit events");
        assert_eq!(audit_count, 1);

        let sync_run_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_runs", [], |row| row.get(0))
            .expect("count sync runs");
        assert_eq!(sync_run_count, 1);

        let _ = fs::remove_file(db_path);
    }
}
