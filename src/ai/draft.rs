use crate::ai::traits::DraftGenerationResult;
use crate::error::{AppError, AppResult};
use crate::index::repo::create_draft;

pub fn persist_generated_drafts(
    conn: &rusqlite::Connection,
    thread_id: Option<i64>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: &str,
    generated: &DraftGenerationResult,
) -> AppResult<Vec<i64>> {
    let mut ids = Vec::new();
    for candidate in &generated.candidates {
        let rationale_json = serde_json::to_string(&candidate.rationale)
            .map_err(|err| AppError::Ai(format!("failed to serialize rationale: {err}")))?;
        let id = create_draft(
            conn,
            thread_id,
            to.clone(),
            cc.clone(),
            subject,
            &candidate.body,
            "ai",
            Some(&generated.provider),
            Some(&generated.model),
            None,
            Some(&rationale_json),
        )
        .map_err(|err| AppError::Sqlite(err.to_string()))?;
        ids.push(id);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::ai::traits::{DraftCandidate, DraftGenerationResult};
    use crate::index::repo::get_draft;
    use crate::index::schema::initialize_schema;

    use super::persist_generated_drafts;

    fn temp_db() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("cour-draft-test-{unique}.sqlite"))
    }

    #[test]
    fn stores_multiple_candidate_drafts() {
        let db_path = temp_db();
        let conn = Connection::open(&db_path).expect("open db");
        initialize_schema(&conn).expect("init schema");

        let generated = DraftGenerationResult {
            provider: "openai-compatible".to_string(),
            model: "gpt-4o-mini".to_string(),
            candidates: vec![
                DraftCandidate {
                    body: "Draft one".to_string(),
                    rationale: vec!["reason one".to_string()],
                    confidence: 0.8,
                },
                DraftCandidate {
                    body: "Draft two".to_string(),
                    rationale: vec!["reason two".to_string()],
                    confidence: 0.7,
                },
            ],
        };

        let ids = persist_generated_drafts(
            &conn,
            None,
            vec!["alice@example.com".to_string()],
            vec![],
            "Re: Hello",
            &generated,
        )
        .expect("persist generated drafts");

        assert_eq!(ids.len(), 2);
        let draft = get_draft(&conn, ids[0])
            .expect("get draft")
            .expect("draft exists");
        assert_eq!(draft.source, "ai");
        assert_eq!(draft.provider.as_deref(), Some("openai-compatible"));
        assert_eq!(draft.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(draft.rationale_json.as_deref(), Some("[\"reason one\"]"));

        let _ = fs::remove_file(db_path);
    }
}
