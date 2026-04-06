use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension, ToSql};

use crate::threading::{assign_threads, ThreadAssignment, ThreadCandidate};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadListRow {
    pub id: i64,
    pub subject: Option<String>,
    pub state: Option<String>,
    pub message_count: i64,
    pub latest_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMessageRow {
    pub message_id: i64,
    pub subject: Option<String>,
    pub from_email: Option<String>,
    pub body_text: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadDetailRow {
    pub thread_id: i64,
    pub state: Option<String>,
    pub subject: Option<String>,
    pub messages: Vec<ThreadMessageRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DraftReviewRow {
    pub id: i64,
    pub thread_id: Option<i64>,
    pub status: String,
    pub source: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub has_rationale: bool,
    pub approval_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub approved_at: Option<String>,
    pub sent_at: Option<String>,
    pub latest_audit_at: Option<String>,
    pub latest_approval_audit_at: Option<String>,
    pub latest_send_audit_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LexicalSearchFilters {
    pub account_id: Option<i64>,
    pub mailbox_id: Option<i64>,
    pub sent_after: Option<String>,
    pub sent_before: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexicalSearchCandidate {
    pub thread_id: i64,
    pub message_id: i64,
    pub subject: Option<String>,
    pub snippet: String,
    pub lexical_score: f64,
    pub matched_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchScoreBreakdown {
    pub lexical_rank: usize,
    pub lexical_score: f64,
    pub lexical_weighted_score: f64,
    pub semantic_similarity: Option<f64>,
    pub semantic_weighted_score: f64,
    pub blended_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AskSearchResult {
    pub thread_id: i64,
    pub message_id: i64,
    pub subject: Option<String>,
    pub evidence_snippet: String,
    pub matched_at: Option<String>,
    pub summary: Option<String>,
    pub latest_ask: Option<String>,
    pub recommended_action: Option<String>,
    pub score: SearchScoreBreakdown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelatedThreadResult {
    pub thread_id: i64,
    pub subject: Option<String>,
    pub brief_summary: Option<String>,
    pub latest_ask: Option<String>,
    pub recommended_action: Option<String>,
    pub evidence_snippet: String,
    pub matched_at: Option<String>,
    pub score: SearchScoreBreakdown,
}

pub fn rebuild_threads_for_account(conn: &Connection, account_id: i64) -> rusqlite::Result<()> {
    let candidates = load_thread_candidates(conn, account_id)?;
    let assignments = assign_threads(&candidates);

    conn.execute("DELETE FROM thread_messages WHERE thread_id IN (SELECT id FROM threads WHERE account_id = ?1)", params![account_id])?;
    conn.execute(
        "DELETE FROM threads WHERE account_id = ?1",
        params![account_id],
    )?;

    let mut grouped: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for ThreadAssignment {
        message_row_id,
        thread_key,
    } in assignments
    {
        grouped.entry(thread_key).or_default().push(message_row_id);
    }

    for (_thread_key, message_ids) in grouped {
        let thread_id = create_thread(conn, account_id, &message_ids)?;
        for (ordinal, message_id) in message_ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO thread_messages (thread_id, message_id, ordinal) VALUES (?1, ?2, ?3)",
                params![thread_id, message_id, ordinal as i64],
            )?;
        }
    }

    Ok(())
}

fn load_thread_candidates(
    conn: &Connection,
    account_id: i64,
) -> rusqlite::Result<Vec<ThreadCandidate>> {
    let mut stmt = conn.prepare(
        "SELECT id, message_id_header, in_reply_to, references_json, subject
         FROM messages
         WHERE account_id = ?1
         ORDER BY sent_at ASC, id ASC",
    )?;

    let rows = stmt.query_map(params![account_id], |row| {
        let references_json: String = row.get(3)?;
        Ok(ThreadCandidate {
            message_row_id: row.get(0)?,
            message_id_header: row.get(1)?,
            in_reply_to: row.get(2)?,
            references: parse_json_string_list(&references_json),
            subject: row.get(4)?,
        })
    })?;

    rows.collect()
}

fn create_thread(conn: &Connection, account_id: i64, message_ids: &[i64]) -> rusqlite::Result<i64> {
    let summary = summarize_thread(conn, account_id, message_ids)?;

    conn.execute(
        "INSERT INTO threads (
            account_id, subject_canonical, latest_message_at, unread_count,
            message_count, state, needs_reply, waiting_since, has_draft, updated_at
         ) VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
        params![
            account_id,
            summary.subject_canonical,
            summary.latest_message_at,
            summary.message_count,
            summary.state,
            summary.needs_reply,
            summary.waiting_since,
            summary.has_draft,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadMessageSummary {
    subject: Option<String>,
    from_email: Option<String>,
    sent_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadInsertSummary {
    subject_canonical: Option<String>,
    latest_message_at: Option<String>,
    message_count: i64,
    state: Option<String>,
    needs_reply: i64,
    waiting_since: Option<String>,
    has_draft: i64,
}

fn summarize_thread(
    conn: &Connection,
    account_id: i64,
    message_ids: &[i64],
) -> rusqlite::Result<ThreadInsertSummary> {
    let account_email = load_account_email(conn, account_id)?;
    let messages = load_thread_messages(conn, message_ids)?;
    let latest = messages.last().expect("thread must contain messages");

    let has_draft = thread_has_pending_draft(conn, message_ids)?;
    let latest_inbound = messages
        .iter()
        .rev()
        .find(|message| is_inbound_message(message.from_email.as_deref(), account_email.as_str()));

    let has_reply_after_latest_inbound = latest_inbound.is_some_and(|inbound| {
        messages.iter().any(|message| {
            is_outbound_message(message.from_email.as_deref(), account_email.as_str())
                && message.sent_at > inbound.sent_at
        })
    });

    let (state, needs_reply, waiting_since) = if let Some(inbound) = latest_inbound {
        if has_reply_after_latest_inbound {
            (
                Some("waiting_on_them".to_string()),
                0,
                latest.sent_at.clone().or_else(|| inbound.sent_at.clone()),
            )
        } else {
            (
                Some(classify_unanswered_inbound(inbound).to_string()),
                1,
                inbound.sent_at.clone(),
            )
        }
    } else {
        (
            Some("waiting_on_them".to_string()),
            0,
            latest.sent_at.clone(),
        )
    };

    Ok(ThreadInsertSummary {
        subject_canonical: latest.subject.clone(),
        latest_message_at: latest.sent_at.clone(),
        message_count: message_ids.len() as i64,
        state,
        needs_reply,
        waiting_since,
        has_draft: if has_draft { 1 } else { 0 },
    })
}

fn load_account_email(conn: &Connection, account_id: i64) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT email_address FROM accounts WHERE id = ?1",
        params![account_id],
        |row| row.get(0),
    )
}

fn load_thread_messages(
    conn: &Connection,
    message_ids: &[i64],
) -> rusqlite::Result<Vec<ThreadMessageSummary>> {
    let mut messages = Vec::with_capacity(message_ids.len());
    for message_id in message_ids {
        messages.push(conn.query_row(
            "SELECT subject, from_email, sent_at FROM messages WHERE id = ?1",
            params![message_id],
            |row| {
                Ok(ThreadMessageSummary {
                    subject: row.get(0)?,
                    from_email: row.get(1)?,
                    sent_at: row.get(2)?,
                })
            },
        )?);
    }
    Ok(messages)
}

fn thread_has_pending_draft(conn: &Connection, message_ids: &[i64]) -> rusqlite::Result<bool> {
    if message_ids.is_empty() {
        return Ok(false);
    }

    let message_id_set = message_ids.iter().copied().collect::<BTreeSet<_>>();
    let mut stmt = conn.prepare(
        "SELECT d.thread_id, tm.message_id
         FROM drafts d
         JOIN thread_messages tm ON tm.thread_id = d.thread_id
         WHERE d.status IN ('pending', 'approved')",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, i64>(1)?))
    })?;

    for row in rows {
        let (thread_id, message_id) = row?;
        if thread_id.is_some() && message_id_set.contains(&message_id) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn is_inbound_message(from_email: Option<&str>, account_email: &str) -> bool {
    from_email
        .map(|email| !email.eq_ignore_ascii_case(account_email))
        .unwrap_or(false)
}

fn is_outbound_message(from_email: Option<&str>, account_email: &str) -> bool {
    from_email
        .map(|email| email.eq_ignore_ascii_case(account_email))
        .unwrap_or(false)
}

fn classify_unanswered_inbound(message: &ThreadMessageSummary) -> &'static str {
    match message.from_email.as_deref() {
        Some(email) if is_automated_sender(email) => "low_value",
        _ => "waiting_on_me",
    }
}

fn is_automated_sender(email: &str) -> bool {
    let email = email.to_ascii_lowercase();
    email.contains("no-reply")
        || email.contains("noreply")
        || email.contains("newsletter")
        || email.starts_with("notifications@")
}

fn parse_json_string_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Vec::new();
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.trim().is_empty() {
        return Vec::new();
    }

    inner
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn list_recent_threads(
    conn: &Connection,
    limit: usize,
) -> rusqlite::Result<Vec<ThreadListRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, subject_canonical, state, message_count, latest_message_at
         FROM threads
         ORDER BY COALESCE(latest_message_at, updated_at) DESC, id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(ThreadListRow {
            id: row.get(0)?,
            subject: row.get(1)?,
            state: row.get(2)?,
            message_count: row.get(3)?,
            latest_message_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn list_threads_by_state(
    conn: &Connection,
    state: &str,
) -> rusqlite::Result<Vec<ThreadListRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, subject_canonical, state, message_count, latest_message_at
         FROM threads WHERE state = ?1 ORDER BY COALESCE(latest_message_at, updated_at) DESC, id DESC",
    )?;
    let rows = stmt.query_map(params![state], |row| {
        Ok(ThreadListRow {
            id: row.get(0)?,
            subject: row.get(1)?,
            state: row.get(2)?,
            message_count: row.get(3)?,
            latest_message_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn get_thread_detail(
    conn: &Connection,
    thread_id: i64,
) -> rusqlite::Result<Option<ThreadDetailRow>> {
    let thread_meta: Option<(i64, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT id, state, subject_canonical FROM threads WHERE id = ?1",
            params![thread_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;

    let Some((thread_id, state, subject)) = thread_meta else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT m.id, m.subject, m.from_email, m.body_text, m.sent_at
         FROM thread_messages tm
         JOIN messages m ON m.id = tm.message_id
         WHERE tm.thread_id = ?1
         ORDER BY tm.ordinal ASC, m.id ASC",
    )?;
    let messages = stmt
        .query_map(params![thread_id], |row| {
            Ok(ThreadMessageRow {
                message_id: row.get(0)?,
                subject: row.get(1)?,
                from_email: row.get(2)?,
                body_text: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                sent_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Some(ThreadDetailRow {
        thread_id,
        state,
        subject,
        messages,
    }))
}

pub fn search_lexical_candidates(
    conn: &Connection,
    query: &str,
    filters: &LexicalSearchFilters,
    limit: usize,
) -> rusqlite::Result<Vec<LexicalSearchCandidate>> {
    let mut sql = String::from(
        "SELECT tm.thread_id,
                m.id,
                m.subject,
                snippet(message_fts, 1, '[', ']', '…', 12) AS evidence_snippet,
                bm25(message_fts, 5.0, 1.0, 2.0) AS lexical_score,
                m.sent_at
         FROM message_fts
         JOIN messages m ON m.id = message_fts.rowid
         JOIN thread_messages tm ON tm.message_id = m.id
         WHERE message_fts MATCH ?",
    );

    let mut params: Vec<&dyn ToSql> = vec![&query];

    if let Some(account_id) = filters.account_id.as_ref() {
        sql.push_str(" AND m.account_id = ?");
        params.push(account_id);
    }
    if let Some(mailbox_id) = filters.mailbox_id.as_ref() {
        sql.push_str(" AND m.mailbox_id = ?");
        params.push(mailbox_id);
    }
    if let Some(sent_after) = filters.sent_after.as_ref() {
        sql.push_str(" AND m.sent_at >= ?");
        params.push(sent_after);
    }
    if let Some(sent_before) = filters.sent_before.as_ref() {
        sql.push_str(" AND m.sent_at <= ?");
        params.push(sent_before);
    }

    sql.push_str(
        " ORDER BY lexical_score ASC, COALESCE(m.sent_at, '') DESC, m.id DESC
          LIMIT ?",
    );
    let limit_value = limit as i64;
    params.push(&limit_value);

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok(LexicalSearchCandidate {
            thread_id: row.get(0)?,
            message_id: row.get(1)?,
            subject: row.get(2)?,
            snippet: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            lexical_score: row.get(4)?,
            matched_at: row.get(5)?,
        })
    })?;

    rows.collect()
}

fn deserialize_embedding_blob(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.is_empty() || !blob.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return None;
    }

    let mut vector = Vec::with_capacity(blob.len() / std::mem::size_of::<f32>());
    for chunk in blob.chunks_exact(std::mem::size_of::<f32>()) {
        vector.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Some(vector)
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f64> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }

    let mut dot = 0.0_f64;
    let mut left_norm = 0.0_f64;
    let mut right_norm = 0.0_f64;

    for (l, r) in left.iter().zip(right.iter()) {
        let l = f64::from(*l);
        let r = f64::from(*r);
        dot += l * r;
        left_norm += l * l;
        right_norm += r * r;
    }

    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }

    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

pub fn blend_search_results(
    lexical_candidates: &[LexicalSearchCandidate],
    semantic_vectors: &BTreeMap<i64, Vec<f32>>,
    query_embedding: Option<&[f32]>,
) -> Vec<AskSearchResult> {
    const LEXICAL_WEIGHT: f64 = 0.35;
    const SEMANTIC_WEIGHT: f64 = 0.65;

    let lexical_count = lexical_candidates.len().max(1) as f64;
    let mut results = lexical_candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let lexical_rank = index + 1;
            let lexical_rank_score = (lexical_count - index as f64) / lexical_count;
            let semantic_similarity = query_embedding
                .and_then(|query| {
                    semantic_vectors
                        .get(&candidate.thread_id)
                        .map(|vector| (query, vector))
                })
                .and_then(|(query, vector)| cosine_similarity(query, vector));
            let semantic_weighted_score = semantic_similarity.unwrap_or(0.0) * SEMANTIC_WEIGHT;
            let lexical_weighted_score = lexical_rank_score * LEXICAL_WEIGHT;
            AskSearchResult {
                thread_id: candidate.thread_id,
                message_id: candidate.message_id,
                subject: candidate.subject.clone(),
                evidence_snippet: candidate.snippet.clone(),
                matched_at: candidate.matched_at.clone(),
                summary: None,
                latest_ask: None,
                recommended_action: None,
                score: SearchScoreBreakdown {
                    lexical_rank,
                    lexical_score: candidate.lexical_score,
                    lexical_weighted_score,
                    semantic_similarity,
                    semantic_weighted_score,
                    blended_score: lexical_weighted_score + semantic_weighted_score,
                },
            }
        })
        .collect::<Vec<_>>();

    results.sort_by(|left, right| {
        right
            .score
            .blended_score
            .partial_cmp(&left.score.blended_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.score.lexical_rank.cmp(&right.score.lexical_rank))
            .then_with(|| right.matched_at.cmp(&left.matched_at))
    });

    results
}

pub fn ask_search(
    conn: &Connection,
    query: &str,
    filters: &LexicalSearchFilters,
    query_embedding: Option<&[f32]>,
    limit: usize,
) -> rusqlite::Result<Vec<AskSearchResult>> {
    let lexical_candidates = search_lexical_candidates(conn, query, filters, limit)?;
    if lexical_candidates.is_empty() {
        return Ok(Vec::new());
    }

    let semantic_vectors = load_thread_embeddings(
        conn,
        lexical_candidates
            .iter()
            .map(|candidate| candidate.thread_id),
    )?;
    let mut results = blend_search_results(&lexical_candidates, &semantic_vectors, query_embedding);
    hydrate_thread_ai_details(conn, &mut results)?;
    Ok(results)
}

pub fn find_related_threads(
    conn: &Connection,
    thread_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<RelatedThreadResult>> {
    let base = load_thread_embedding(conn, thread_id)?;
    let Some(base_embedding) = base else {
        return Ok(Vec::new());
    };

    let mut stmt = conn.prepare(
        "SELECT t.id,
                t.subject_canonical,
                tai.brief_summary,
                tai.latest_ask,
                tai.recommended_action,
                COALESCE(MAX(m.body_text), ''),
                t.latest_message_at,
                MAX(mai.embedding_blob)
         FROM threads t
         LEFT JOIN thread_ai tai ON tai.thread_id = t.id
         LEFT JOIN thread_messages tm ON tm.thread_id = t.id
         LEFT JOIN messages m ON m.id = tm.message_id
         LEFT JOIN message_ai mai ON mai.message_id = tm.message_id
         WHERE t.id != ?1
         GROUP BY t.id, t.subject_canonical, tai.brief_summary, tai.latest_ask, tai.recommended_action, t.latest_message_at
         HAVING MAX(mai.embedding_blob) IS NOT NULL
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![thread_id, limit as i64 * 4], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Vec<u8>>(7)?,
        ))
    })?;

    let mut related = rows
        .filter_map(|row| {
            let (
                candidate_thread_id,
                subject,
                brief_summary,
                latest_ask,
                recommended_action,
                evidence_snippet,
                matched_at,
                blob,
            ) = row.ok()?;
            let vector = deserialize_embedding_blob(&blob)?;
            let semantic_similarity = cosine_similarity(&base_embedding, &vector)?;
            Some(RelatedThreadResult {
                thread_id: candidate_thread_id,
                subject,
                brief_summary,
                latest_ask,
                recommended_action,
                evidence_snippet,
                matched_at,
                score: SearchScoreBreakdown {
                    lexical_rank: 0,
                    lexical_score: 0.0,
                    lexical_weighted_score: 0.0,
                    semantic_similarity: Some(semantic_similarity),
                    semantic_weighted_score: semantic_similarity,
                    blended_score: semantic_similarity,
                },
            })
        })
        .collect::<Vec<_>>();

    related.sort_by(|left, right| {
        right
            .score
            .blended_score
            .partial_cmp(&left.score.blended_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.matched_at.cmp(&left.matched_at))
    });
    related.truncate(limit);
    Ok(related)
}

fn load_thread_embeddings(
    conn: &Connection,
    thread_ids: impl IntoIterator<Item = i64>,
) -> rusqlite::Result<BTreeMap<i64, Vec<f32>>> {
    let mut embeddings = BTreeMap::new();
    for thread_id in thread_ids {
        if let Some(embedding) = load_thread_embedding(conn, thread_id)? {
            embeddings.insert(thread_id, embedding);
        }
    }
    Ok(embeddings)
}

fn load_thread_embedding(conn: &Connection, thread_id: i64) -> rusqlite::Result<Option<Vec<f32>>> {
    conn.query_row(
        "SELECT mai.embedding_blob
         FROM thread_messages tm
         JOIN message_ai mai ON mai.message_id = tm.message_id
         WHERE tm.thread_id = ?1 AND mai.embedding_blob IS NOT NULL
         ORDER BY tm.ordinal DESC, tm.message_id DESC
         LIMIT 1",
        params![thread_id],
        |row| row.get::<_, Vec<u8>>(0),
    )
    .optional()
    .map(|blob| blob.and_then(|bytes| deserialize_embedding_blob(&bytes)))
}

fn hydrate_thread_ai_details(
    conn: &Connection,
    results: &mut [AskSearchResult],
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT brief_summary, latest_ask, recommended_action FROM thread_ai WHERE thread_id = ?1",
    )?;

    for result in results {
        let details: Option<(Option<String>, Option<String>, Option<String>)> = stmt
            .query_row(params![result.thread_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?;
        if let Some((summary, latest_ask, recommended_action)) = details {
            result.summary = summary;
            result.latest_ask = latest_ask;
            result.recommended_action = recommended_action;
        }
    }

    Ok(())
}

pub fn pending_drafts_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM drafts WHERE status = 'pending'",
        [],
        |row| row.get(0),
    )
}

pub fn list_drafts_for_review(conn: &Connection) -> rusqlite::Result<Vec<DraftReviewRow>> {
    let mut stmt = conn.prepare(
        "SELECT
            d.id,
            d.thread_id,
            d.status,
            d.source,
            d.provider,
            d.model,
            CASE
                WHEN d.rationale_json IS NULL OR TRIM(d.rationale_json) = '' OR TRIM(d.rationale_json) = '[]' THEN 0
                ELSE 1
            END AS has_rationale,
            CASE
                WHEN d.sent_at IS NOT NULL OR d.status = 'sent' THEN 'sent'
                WHEN d.approved_at IS NOT NULL OR d.status = 'approved' THEN 'approved'
                ELSE 'pending'
            END AS approval_status,
            d.created_at,
            d.updated_at,
            d.approved_at,
            d.sent_at,
            MAX(a.created_at) AS latest_audit_at,
            MAX(CASE WHEN a.action = 'draft_approved' THEN a.created_at END) AS latest_approval_audit_at,
            MAX(CASE WHEN a.action = 'draft_sent' THEN a.created_at END) AS latest_send_audit_at
         FROM drafts d
         LEFT JOIN audit_events a ON a.entity_type = 'draft' AND a.entity_id = d.id
         GROUP BY
            d.id,
            d.thread_id,
            d.status,
            d.source,
            d.provider,
            d.model,
            d.rationale_json,
            d.created_at,
            d.updated_at,
            d.approved_at,
            d.sent_at
         ORDER BY d.updated_at DESC, d.id DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(DraftReviewRow {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            status: row.get(2)?,
            source: row.get(3)?,
            provider: row.get(4)?,
            model: row.get(5)?,
            has_rationale: row.get::<_, i64>(6)? != 0,
            approval_status: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            approved_at: row.get(10)?,
            sent_at: row.get(11)?,
            latest_audit_at: row.get(12)?,
            latest_approval_audit_at: row.get(13)?,
            latest_send_audit_at: row.get(14)?,
        })
    })?;

    rows.collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::ai::traits::{EmbeddingResult, ExtractionOutput};
    use crate::config::AccountConfig;
    use crate::index::repo::{
        append_audit_event, create_draft, update_draft_status, upsert_account, upsert_mailbox,
        upsert_message, upsert_message_ai, upsert_thread_ai, MessageAiUpsert, ThreadAiUpsert,
    };
    use crate::index::schema::initialize_schema;
    use crate::maildir::DiscoveredMailbox;
    use crate::model::{ParsedAddress, ParsedMessage};

    use super::{
        ask_search, blend_search_results, find_related_threads, get_thread_detail,
        list_drafts_for_review, list_recent_threads, list_threads_by_state, pending_drafts_count,
        rebuild_threads_for_account, search_lexical_candidates, LexicalSearchCandidate,
        LexicalSearchFilters,
    };

    fn temp_db() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("cour-thread-test-{unique}.sqlite"))
    }

    #[test]
    fn computes_waiting_on_me_for_unanswered_inbound_thread() {
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
            subject: Some("Need Reply".to_string()),
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
            body_text: "Please reply".to_string(),
            body_html: None,
            snippet: "Please reply".to_string(),
            parse_hash: "hash-1".to_string(),
            file_mtime: 123,
        };
        upsert_message(&conn, account_id, mailbox_id, &parsed).expect("upsert message");

        rebuild_threads_for_account(&conn, account_id).expect("rebuild threads");
        let waiting = list_threads_by_state(&conn, "waiting_on_me").expect("list threads");
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].message_count, 1);

        let recent = list_recent_threads(&conn, 10).expect("list recent threads");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].subject.as_deref(), Some("Need Reply"));

        let detail = get_thread_detail(&conn, waiting[0].id)
            .expect("query thread detail")
            .expect("thread detail exists");
        assert_eq!(detail.messages.len(), 1);
        assert_eq!(detail.messages[0].body_text, "Please reply");

        let pending_drafts = pending_drafts_count(&conn).expect("pending draft count");
        assert_eq!(pending_drafts, 0);

        let _ = fs::remove_file(db_path);
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
            file_path: "/tmp/mail/personal/cur/msg-fts-1:2,".into(),
            message_id_header: Some("<msg-fts-1@example.com>".to_string()),
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
            parse_hash: "hash-fts-1".to_string(),
            file_mtime: 123,
        };
        upsert_message(&conn, account_id, mailbox_id, &parsed).expect("upsert message");
        rebuild_threads_for_account(&conn, account_id).expect("rebuild threads");

        let rows = search_lexical_candidates(
            &conn,
            "invoice",
            &LexicalSearchFilters {
                account_id: Some(account_id),
                ..LexicalSearchFilters::default()
            },
            10,
        )
        .expect("search lexical");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subject.as_deref(), Some("Invoice Followup"));
        assert!(rows[0].snippet.contains("[invoice]"));
        assert_eq!(
            rows[0].matched_at.as_deref(),
            Some("2026-04-20T10:00:00+00:00")
        );

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn draft_review_model_includes_provider_and_status() {
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
            Some("[\"reason one\"]"),
        )
        .expect("create draft");

        update_draft_status(&conn, draft_id, "approved").expect("approve draft");
        append_audit_event(&conn, "draft", draft_id, "draft_approved", Some("{}"))
            .expect("append approval audit event");
        append_audit_event(&conn, "draft", draft_id, "draft_reviewed", Some("{}"))
            .expect("append review audit event");

        let rows = list_drafts_for_review(&conn).expect("list drafts for review");
        assert_eq!(rows.len(), 1);

        let row = &rows[0];
        assert_eq!(row.id, draft_id);
        assert_eq!(row.thread_id, None);
        assert_eq!(row.status, "approved");
        assert_eq!(row.source, "ai");
        assert_eq!(row.provider.as_deref(), Some("openai-compatible"));
        assert_eq!(row.model.as_deref(), Some("gpt-4o-mini"));
        assert!(row.has_rationale);
        assert_eq!(row.approval_status, "approved");
        assert!(row.approved_at.is_some());
        assert!(row.sent_at.is_none());
        assert!(row.latest_audit_at.is_some());
        assert!(row.latest_approval_audit_at.is_some());
        assert_eq!(row.latest_send_audit_at, None);
        assert!(!row.created_at.is_empty());
        assert!(!row.updated_at.is_empty());

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn lexical_search_supports_phrase_queries_and_filters() {
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
        let inbox = DiscoveredMailbox {
            name: "Inbox".to_string(),
            path: "/tmp/mail/personal".into(),
            special_use: Some("inbox".to_string()),
        };
        let archive = DiscoveredMailbox {
            name: "Archive".to_string(),
            path: "/tmp/mail/personal/archive".into(),
            special_use: None,
        };
        let inbox_id = upsert_mailbox(&conn, account_id, &inbox).expect("upsert inbox");
        let archive_id = upsert_mailbox(&conn, account_id, &archive).expect("upsert archive");

        let older = ParsedMessage {
            file_path: "/tmp/mail/personal/cur/msg-fts-older:2,".into(),
            message_id_header: Some("<msg-fts-older@example.com>".to_string()),
            in_reply_to: None,
            references: vec![],
            subject: Some("Old logistics".to_string()),
            from: Some(ParsedAddress {
                display_name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
            }),
            to: vec![ParsedAddress {
                display_name: Some("Ash".to_string()),
                email: "ash@example.com".to_string(),
            }],
            cc: vec![],
            sent_at: Some("2026-04-01T09:00:00+00:00".to_string()),
            body_text: "Following up on project phoenix launch timing".to_string(),
            body_html: None,
            snippet: "Following up on project phoenix launch timing".to_string(),
            parse_hash: "hash-fts-older".to_string(),
            file_mtime: 200,
        };
        let newer = ParsedMessage {
            file_path: "/tmp/mail/personal/archive/msg-fts-newer:2,".into(),
            message_id_header: Some("<msg-fts-newer@example.com>".to_string()),
            in_reply_to: None,
            references: vec![],
            subject: Some("Project Phoenix".to_string()),
            from: Some(ParsedAddress {
                display_name: Some("Bob".to_string()),
                email: "bob@example.com".to_string(),
            }),
            to: vec![ParsedAddress {
                display_name: Some("Ash".to_string()),
                email: "ash@example.com".to_string(),
            }],
            cc: vec![],
            sent_at: Some("2026-04-25T15:30:00+00:00".to_string()),
            body_text: "The exact phrase project phoenix appears here.".to_string(),
            body_html: None,
            snippet: "The exact phrase project phoenix appears here.".to_string(),
            parse_hash: "hash-fts-newer".to_string(),
            file_mtime: 300,
        };
        upsert_message(&conn, account_id, inbox_id, &older).expect("upsert older message");
        upsert_message(&conn, account_id, archive_id, &newer).expect("upsert newer message");
        rebuild_threads_for_account(&conn, account_id).expect("rebuild threads");

        let rows = search_lexical_candidates(
            &conn,
            "\"project phoenix\"",
            &LexicalSearchFilters {
                account_id: Some(account_id),
                mailbox_id: Some(archive_id),
                sent_after: Some("2026-04-10T00:00:00+00:00".to_string()),
                sent_before: Some("2026-04-30T23:59:59+00:00".to_string()),
            },
            10,
        )
        .expect("search lexical with phrase");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subject.as_deref(), Some("Project Phoenix"));
        assert!(rows[0].snippet.contains("[project phoenix]"));

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn semantic_reranking_changes_candidate_order() {
        let lexical = vec![
            LexicalSearchCandidate {
                thread_id: 11,
                message_id: 101,
                subject: Some("Invoice status".to_string()),
                snippet: "invoice status update".to_string(),
                lexical_score: -9.0,
                matched_at: Some("2026-04-20T10:00:00+00:00".to_string()),
            },
            LexicalSearchCandidate {
                thread_id: 22,
                message_id: 202,
                subject: Some("Payment next steps".to_string()),
                snippet: "payment plan and settlement".to_string(),
                lexical_score: -7.0,
                matched_at: Some("2026-04-20T11:00:00+00:00".to_string()),
            },
        ];
        let semantic_vectors =
            BTreeMap::from([(11, vec![1.0_f32, 0.0_f32]), (22, vec![0.0_f32, 1.0_f32])]);

        let reranked = blend_search_results(&lexical, &semantic_vectors, Some(&[0.0_f32, 1.0_f32]));

        assert_eq!(reranked.len(), 2);
        assert_eq!(reranked[0].thread_id, 22);
        assert_eq!(reranked[1].thread_id, 11);
        assert!(
            reranked[0].score.semantic_similarity.unwrap()
                > reranked[1].score.semantic_similarity.unwrap()
        );
        assert!(reranked[0].score.blended_score > reranked[1].score.blended_score);
        assert_eq!(reranked[0].score.lexical_rank, 2);
    }

    #[test]
    fn ask_search_returns_score_breakdown_and_thread_context() {
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

        let first = ParsedMessage {
            file_path: "/tmp/mail/personal/cur/msg-semantic-1:2,".into(),
            message_id_header: Some("<msg-semantic-1@example.com>".to_string()),
            in_reply_to: None,
            references: vec![],
            subject: Some("Invoice status".to_string()),
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
            body_text: "Invoice status update".to_string(),
            body_html: None,
            snippet: "Invoice status update".to_string(),
            parse_hash: "hash-semantic-1".to_string(),
            file_mtime: 123,
        };
        let second = ParsedMessage {
            file_path: "/tmp/mail/personal/cur/msg-semantic-2:2,".into(),
            message_id_header: Some("<msg-semantic-2@example.com>".to_string()),
            in_reply_to: None,
            references: vec![],
            subject: Some("Payment next steps".to_string()),
            from: Some(ParsedAddress {
                display_name: Some("Bob".to_string()),
                email: "bob@example.com".to_string(),
            }),
            to: vec![ParsedAddress {
                display_name: Some("Ash".to_string()),
                email: "ash@example.com".to_string(),
            }],
            cc: vec![],
            sent_at: Some("2026-04-20T11:00:00+00:00".to_string()),
            body_text: "Invoice payment plan".to_string(),
            body_html: None,
            snippet: "Invoice payment plan".to_string(),
            parse_hash: "hash-semantic-2".to_string(),
            file_mtime: 124,
        };

        let first_id = upsert_message(&conn, account_id, mailbox_id, &first).expect("upsert first");
        let second_id =
            upsert_message(&conn, account_id, mailbox_id, &second).expect("upsert second");
        rebuild_threads_for_account(&conn, account_id).expect("rebuild threads");

        let thread_ids = list_recent_threads(&conn, 10)
            .expect("list threads")
            .into_iter()
            .map(|row| (row.subject.expect("thread subject"), row.id))
            .collect::<BTreeMap<_, _>>();
        let first_thread_id = *thread_ids.get("Invoice status").expect("first thread");
        let second_thread_id = *thread_ids.get("Payment next steps").expect("second thread");

        let extraction = ExtractionOutput {
            provider: "stub".to_string(),
            model: "test".to_string(),
            summary: "Customer is asking about payment timing".to_string(),
            action: "Respond with plan".to_string(),
            urgency_score: 0.7,
            confidence: 0.9,
            categories: vec!["finance".to_string()],
            entities: vec!["invoice".to_string()],
            deadlines: vec![],
            thread_state_hint: Some("waiting_on_me".to_string()),
            latest_ask: Some("Can you confirm the payment plan?".to_string()),
        };
        upsert_message_ai(
            &conn,
            &MessageAiUpsert {
                message_id: first_id,
                extraction_hash: Some("extract-1".to_string()),
                extraction: Some(&extraction),
                embedding_hash: Some("embed-1".to_string()),
                embedding: Some(&EmbeddingResult {
                    provider: "stub".to_string(),
                    model: "test".to_string(),
                    vector: vec![1.0, 0.0],
                }),
            },
        )
        .expect("store first embedding");
        upsert_message_ai(
            &conn,
            &MessageAiUpsert {
                message_id: second_id,
                extraction_hash: Some("extract-2".to_string()),
                extraction: Some(&extraction),
                embedding_hash: Some("embed-2".to_string()),
                embedding: Some(&EmbeddingResult {
                    provider: "stub".to_string(),
                    model: "test".to_string(),
                    vector: vec![0.0, 1.0],
                }),
            },
        )
        .expect("store second embedding");
        upsert_thread_ai(
            &conn,
            &ThreadAiUpsert {
                thread_id: second_thread_id,
                content_hash: "thread-2".to_string(),
                extraction: &extraction,
                related_thread_ids: &[first_thread_id],
            },
        )
        .expect("store thread ai");

        let results = ask_search(
            &conn,
            "invoice",
            &LexicalSearchFilters {
                account_id: Some(account_id),
                ..LexicalSearchFilters::default()
            },
            Some(&[0.0_f32, 1.0_f32]),
            10,
        )
        .expect("ask search");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].thread_id, second_thread_id);
        assert!(results[0].score.semantic_similarity.is_some());
        assert!(results[0].score.blended_score > results[1].score.blended_score);
        assert_eq!(
            results[0].latest_ask.as_deref(),
            Some("Can you confirm the payment plan?")
        );
        assert_eq!(
            results[0].recommended_action.as_deref(),
            Some("Respond with plan")
        );
        assert!(!results[0].evidence_snippet.is_empty());

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn related_threads_rank_by_embedding_similarity() {
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

        let messages = [
            ("Thread A", "Need help with invoice", vec![1.0_f32, 0.0_f32]),
            ("Thread B", "Payment timing details", vec![0.9_f32, 0.1_f32]),
            ("Thread C", "Travel itinerary", vec![0.0_f32, 1.0_f32]),
        ];
        let extraction = ExtractionOutput {
            provider: "stub".to_string(),
            model: "test".to_string(),
            summary: "summary".to_string(),
            action: "action".to_string(),
            urgency_score: 0.2,
            confidence: 0.8,
            categories: vec![],
            entities: vec![],
            deadlines: vec![],
            thread_state_hint: None,
            latest_ask: Some("latest ask".to_string()),
        };

        for (index, (subject, body, vector)) in messages.iter().enumerate() {
            let parsed = ParsedMessage {
                file_path: format!("/tmp/mail/personal/cur/msg-related-{index}:2,").into(),
                message_id_header: Some(format!("<msg-related-{index}@example.com>")),
                in_reply_to: None,
                references: vec![],
                subject: Some((*subject).to_string()),
                from: Some(ParsedAddress {
                    display_name: Some("Sender".to_string()),
                    email: format!("sender-{index}@example.com"),
                }),
                to: vec![ParsedAddress {
                    display_name: Some("Ash".to_string()),
                    email: "ash@example.com".to_string(),
                }],
                cc: vec![],
                sent_at: Some(format!("2026-04-2{}T10:00:00+00:00", index)),
                body_text: (*body).to_string(),
                body_html: None,
                snippet: (*body).to_string(),
                parse_hash: format!("hash-related-{index}"),
                file_mtime: 200 + index as i64,
            };
            let message_id =
                upsert_message(&conn, account_id, mailbox_id, &parsed).expect("upsert message");
            upsert_message_ai(
                &conn,
                &MessageAiUpsert {
                    message_id,
                    extraction_hash: Some(format!("extract-related-{index}")),
                    extraction: Some(&extraction),
                    embedding_hash: Some(format!("embed-related-{index}")),
                    embedding: Some(&EmbeddingResult {
                        provider: "stub".to_string(),
                        model: "test".to_string(),
                        vector: vector.clone(),
                    }),
                },
            )
            .expect("upsert ai metadata");
        }
        rebuild_threads_for_account(&conn, account_id).expect("rebuild threads");

        let thread_ids = list_recent_threads(&conn, 10)
            .expect("list threads")
            .into_iter()
            .map(|row| (row.subject.expect("subject"), row.id))
            .collect::<BTreeMap<_, _>>();
        for (subject, thread_id) in &thread_ids {
            upsert_thread_ai(
                &conn,
                &ThreadAiUpsert {
                    thread_id: *thread_id,
                    content_hash: format!("content-{subject}"),
                    extraction: &extraction,
                    related_thread_ids: &[],
                },
            )
            .expect("upsert thread ai");
        }

        let base_thread_id = *thread_ids.get("Thread A").expect("thread a id");
        let related = find_related_threads(&conn, base_thread_id, 2).expect("find related");

        assert_eq!(related.len(), 2);
        assert_eq!(related[0].subject.as_deref(), Some("Thread B"));
        assert_eq!(related[1].subject.as_deref(), Some("Thread C"));
        assert!(related[0].score.blended_score > related[1].score.blended_score);

        let _ = fs::remove_file(db_path);
    }
}
