use std::hash::Hasher;

use rusqlite::Connection;

use crate::ai::traits::{Embedder, ExtractMessageInput, Extractor, ExtractorInput};
use crate::error::{AppError, AppResult};
use crate::index::query::{get_thread_detail, ThreadDetailRow};
use crate::index::repo::{
    get_message_ai_metadata, get_thread_ai_metadata, list_message_ids_for_thread,
    upsert_message_ai, upsert_thread_ai, MessageAiUpsert, ThreadAiUpsert,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichmentStats {
    pub messages_enriched: usize,
    pub messages_skipped: usize,
    pub threads_enriched: usize,
    pub threads_skipped: usize,
}

pub async fn run_enrichment(
    conn: &Connection,
    thread_id: i64,
    extractor: Option<&dyn Extractor>,
    embedder: Option<&dyn Embedder>,
) -> AppResult<EnrichmentStats> {
    let detail = get_thread_detail(conn, thread_id)
        .map_err(|err| AppError::Sqlite(err.to_string()))?
        .ok_or_else(|| AppError::Sqlite(format!("thread not found: {thread_id}")))?;

    let mut stats = EnrichmentStats {
        messages_enriched: 0,
        messages_skipped: 0,
        threads_enriched: 0,
        threads_skipped: 0,
    };

    for message in &detail.messages {
        let extraction_input = message_extractor_input(message);
        let extraction_hash = content_hash(&extraction_input);
        let embedding_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            hasher.write(message.body_text.as_bytes());
            format!("{:016x}", hasher.finish())
        };

        let existing = get_message_ai_metadata(conn, message.message_id)
            .map_err(|err| AppError::Sqlite(err.to_string()))?;

        let extraction_unchanged = extractor.is_none()
            || existing
                .as_ref()
                .and_then(|meta| meta.extraction_hash.as_deref())
                == Some(extraction_hash.as_str());
        let embedding_unchanged = embedder.is_none()
            || existing
                .as_ref()
                .and_then(|meta| meta.embedding_hash.as_deref())
                == Some(embedding_hash.as_str());

        if extraction_unchanged && embedding_unchanged {
            stats.messages_skipped += 1;
            continue;
        }

        let extraction = if extraction_unchanged {
            None
        } else {
            Some(
                extractor
                    .expect("extractor checked above")
                    .extract(&extraction_input)
                    .await?,
            )
        };

        let embedding = if embedding_unchanged {
            None
        } else {
            Some(
                embedder
                    .expect("embedder checked above")
                    .embed(&message.body_text)
                    .await?,
            )
        };

        upsert_message_ai(
            conn,
            &MessageAiUpsert {
                message_id: message.message_id,
                extraction_hash: Some(extraction_hash),
                extraction: extraction.as_ref(),
                embedding_hash: Some(embedding_hash),
                embedding: embedding.as_ref(),
            },
        )
        .map_err(|err| AppError::Sqlite(err.to_string()))?;
        stats.messages_enriched += 1;
    }

    let thread_input = thread_extractor_input(&detail);
    let thread_hash = content_hash(&thread_input);
    let existing_thread =
        get_thread_ai_metadata(conn, thread_id).map_err(|err| AppError::Sqlite(err.to_string()))?;
    let thread_unchanged = extractor.is_none()
        || existing_thread
            .as_ref()
            .and_then(|meta| meta.content_hash.as_deref())
            == Some(thread_hash.as_str());

    if thread_unchanged {
        stats.threads_skipped += 1;
    } else {
        let extraction = extractor
            .expect("extractor checked above")
            .extract(&thread_input)
            .await?;
        let related_thread_ids = list_message_ids_for_thread(conn, thread_id)
            .map_err(|err| AppError::Sqlite(err.to_string()))?;
        upsert_thread_ai(
            conn,
            &ThreadAiUpsert {
                thread_id,
                content_hash: thread_hash,
                extraction: &extraction,
                related_thread_ids: &related_thread_ids,
            },
        )
        .map_err(|err| AppError::Sqlite(err.to_string()))?;
        stats.threads_enriched += 1;
    }

    Ok(stats)
}

fn message_extractor_input(message: &crate::index::query::ThreadMessageRow) -> ExtractorInput {
    ExtractorInput {
        subject: message.subject.clone(),
        messages: vec![ExtractMessageInput {
            from_email: message.from_email.clone(),
            subject: message.subject.clone(),
            body_text: message.body_text.clone(),
        }],
    }
}

fn thread_extractor_input(detail: &ThreadDetailRow) -> ExtractorInput {
    ExtractorInput {
        subject: detail.subject.clone(),
        messages: detail
            .messages
            .iter()
            .map(|message| ExtractMessageInput {
                from_email: message.from_email.clone(),
                subject: message.subject.clone(),
                body_text: message.body_text.clone(),
            })
            .collect(),
    }
}

fn content_hash(value: &ExtractorInput) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Some(subject) = &value.subject {
        hasher.write(subject.as_bytes());
    }
    hasher.write_u8(0xff);
    for message in &value.messages {
        if let Some(from_email) = &message.from_email {
            hasher.write(from_email.as_bytes());
        }
        hasher.write_u8(0xfe);
        if let Some(subject) = &message.subject {
            hasher.write(subject.as_bytes());
        }
        hasher.write_u8(0xfd);
        hasher.write(message.body_text.as_bytes());
        hasher.write_u8(0xfc);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use async_trait::async_trait;
    use rusqlite::Connection;

    use crate::ai::traits::{
        Embedder, EmbeddingResult, ExtractionOutput, Extractor, ExtractorInput,
    };
    use crate::config::AccountConfig;
    use crate::error::AppResult;
    use crate::index::query::{list_recent_threads, rebuild_threads_for_account};
    use crate::index::repo::{upsert_account, upsert_mailbox, upsert_message};
    use crate::index::schema::initialize_schema;
    use crate::maildir::DiscoveredMailbox;
    use crate::model::{ParsedAddress, ParsedMessage};

    use super::run_enrichment;

    #[derive(Clone, Default)]
    struct StubCounters {
        extractions: Arc<Mutex<usize>>,
        embeddings: Arc<Mutex<usize>>,
    }

    impl StubCounters {
        fn extraction_count(&self) -> usize {
            *self.extractions.lock().expect("lock extraction count")
        }

        fn embedding_count(&self) -> usize {
            *self.embeddings.lock().expect("lock embedding count")
        }
    }

    struct StubExtractor {
        counters: StubCounters,
    }

    #[async_trait]
    impl Extractor for StubExtractor {
        async fn extract(&self, input: &ExtractorInput) -> AppResult<ExtractionOutput> {
            *self
                .counters
                .extractions
                .lock()
                .expect("lock extraction count") += 1;
            Ok(ExtractionOutput {
                provider: "stub-extractor".to_string(),
                model: "stub-model".to_string(),
                summary: format!("summary for {} messages", input.messages.len()),
                action: "respond".to_string(),
                urgency_score: 0.4,
                confidence: 0.9,
                categories: vec!["mail".to_string()],
                entities: vec!["Alice".to_string()],
                deadlines: vec![],
                thread_state_hint: Some("waiting_on_me".to_string()),
                latest_ask: Some("Please reply".to_string()),
            })
        }
    }

    struct StubEmbedder {
        counters: StubCounters,
    }

    #[async_trait]
    impl Embedder for StubEmbedder {
        async fn embed(&self, text: &str) -> AppResult<EmbeddingResult> {
            *self
                .counters
                .embeddings
                .lock()
                .expect("lock embedding count") += 1;
            Ok(EmbeddingResult {
                provider: "stub-embedder".to_string(),
                model: "stub-model".to_string(),
                vector: vec![text.len() as f32, 1.0],
            })
        }
    }

    fn temp_db() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!(
            "cour-enrichment-test-{pid}-{unique}-{counter}.sqlite"
        ))
    }

    fn setup_thread(conn: &Connection) -> i64 {
        let account = AccountConfig {
            name: "personal".to_string(),
            email_address: "ash@example.com".to_string(),
            maildir_root: "/tmp/mail/personal".into(),
            sync_command: None,
            default: Some(true),
        };
        let account_id = upsert_account(conn, &account).expect("upsert account");
        let mailbox = DiscoveredMailbox {
            name: "Inbox".to_string(),
            path: "/tmp/mail/personal".into(),
            special_use: Some("inbox".to_string()),
        };
        let mailbox_id = upsert_mailbox(conn, account_id, &mailbox).expect("upsert mailbox");

        let messages = vec![
            ParsedMessage {
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
                    display_name: Some("Ash".to_string()),
                    email: "ash@example.com".to_string(),
                }],
                cc: vec![],
                sent_at: Some("2026-04-20T10:00:00+00:00".to_string()),
                body_text: "hello one".to_string(),
                body_html: None,
                snippet: "hello one".to_string(),
                parse_hash: "hash-1".to_string(),
                file_mtime: 123,
            },
            ParsedMessage {
                file_path: "/tmp/mail/personal/cur/msg-2:2,".into(),
                message_id_header: Some("<msg-2@example.com>".to_string()),
                in_reply_to: Some("<msg-1@example.com>".to_string()),
                references: vec!["<msg-1@example.com>".to_string()],
                subject: Some("Re: Hello".to_string()),
                from: Some(ParsedAddress {
                    display_name: Some("Ash".to_string()),
                    email: "ash@example.com".to_string(),
                }),
                to: vec![ParsedAddress {
                    display_name: Some("Alice".to_string()),
                    email: "alice@example.com".to_string(),
                }],
                cc: vec![],
                sent_at: Some("2026-04-20T11:00:00+00:00".to_string()),
                body_text: "reply one".to_string(),
                body_html: None,
                snippet: "reply one".to_string(),
                parse_hash: "hash-2".to_string(),
                file_mtime: 124,
            },
        ];

        for parsed in &messages {
            upsert_message(conn, account_id, mailbox_id, parsed).expect("upsert message");
        }

        rebuild_threads_for_account(conn, account_id).expect("rebuild threads");
        list_recent_threads(conn, 10)
            .expect("list threads")
            .into_iter()
            .next()
            .expect("thread exists")
            .id
    }

    #[tokio::test]
    async fn persists_fresh_enrichment_rows() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");
        let thread_id = setup_thread(&conn);

        let counters = StubCounters::default();
        let extractor = StubExtractor {
            counters: counters.clone(),
        };
        let embedder = StubEmbedder {
            counters: counters.clone(),
        };

        let stats = run_enrichment(&conn, thread_id, Some(&extractor), Some(&embedder))
            .await
            .expect("run enrichment");

        assert_eq!(stats.messages_enriched, 2);
        assert_eq!(stats.messages_skipped, 0);
        assert_eq!(stats.threads_enriched, 1);
        assert_eq!(stats.threads_skipped, 0);
        assert_eq!(counters.extraction_count(), 3);
        assert_eq!(counters.embedding_count(), 2);

        let message_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM message_ai", [], |row| row.get(0))
            .expect("count message ai rows");
        assert_eq!(message_count, 2);

        let (provider, model, confidence): (String, String, Option<f32>) = conn
            .query_row(
                "SELECT provider, model, confidence FROM message_ai ORDER BY message_id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read message ai row");
        assert_eq!(provider, "stub-embedder");
        assert_eq!(model, "stub-model");
        assert_eq!(confidence, Some(0.9));

        let thread_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM thread_ai", [], |row| row.get(0))
            .expect("count thread ai rows");
        assert_eq!(thread_count, 1);

        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn skips_unchanged_content() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");
        let thread_id = setup_thread(&conn);

        let counters = StubCounters::default();
        let extractor = StubExtractor {
            counters: counters.clone(),
        };
        let embedder = StubEmbedder {
            counters: counters.clone(),
        };

        run_enrichment(&conn, thread_id, Some(&extractor), Some(&embedder))
            .await
            .expect("initial enrichment");

        let message_enriched_at_before: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT enriched_at FROM message_ai ORDER BY message_id")
                .expect("prepare message enriched_at query");
            stmt.query_map([], |row| row.get::<_, String>(0))
                .expect("query message enriched_at")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("collect message enriched_at")
        };
        let thread_enriched_at_before: String = conn
            .query_row(
                "SELECT enriched_at FROM thread_ai WHERE thread_id = ?1",
                [thread_id],
                |row| row.get(0),
            )
            .expect("read thread enriched_at");

        let stats = run_enrichment(&conn, thread_id, Some(&extractor), Some(&embedder))
            .await
            .expect("rerun enrichment");

        assert_eq!(stats.messages_enriched, 0);
        assert_eq!(stats.messages_skipped, 2);
        assert_eq!(stats.threads_enriched, 0);
        assert_eq!(stats.threads_skipped, 1);
        assert_eq!(counters.extraction_count(), 3);
        assert_eq!(counters.embedding_count(), 2);

        let message_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM message_ai", [], |row| row.get(0))
            .expect("count message ai rows");
        assert_eq!(message_count, 2);
        let thread_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM thread_ai", [], |row| row.get(0))
            .expect("count thread ai rows");
        assert_eq!(thread_count, 1);

        let message_enriched_at_after: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT enriched_at FROM message_ai ORDER BY message_id")
                .expect("prepare message enriched_at query");
            stmt.query_map([], |row| row.get::<_, String>(0))
                .expect("query message enriched_at")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("collect message enriched_at")
        };
        let thread_enriched_at_after: String = conn
            .query_row(
                "SELECT enriched_at FROM thread_ai WHERE thread_id = ?1",
                [thread_id],
                |row| row.get(0),
            )
            .expect("read thread enriched_at after");

        assert_eq!(message_enriched_at_before, message_enriched_at_after);
        assert_eq!(thread_enriched_at_before, thread_enriched_at_after);

        let _ = fs::remove_file(db_path);
    }
}
