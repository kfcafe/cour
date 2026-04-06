use std::path::Path;

use crate::ai::draft::persist_generated_drafts;
use crate::ai::traits::{DraftCandidate, DraftGenerationResult};
use crate::config::{AppConfig, ProjectPaths};
use crate::error::{AppError, AppResult};
use crate::index::open_database;
use crate::index::query::get_thread_detail;

pub async fn run_draft(config_path: Option<&Path>, thread_id: i64) -> AppResult<String> {
    let config = AppConfig::load(config_path)?;
    let paths = ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    let conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;

    let detail = get_thread_detail(&conn, thread_id)
        .map_err(|err| AppError::Sqlite(err.to_string()))?
        .ok_or_else(|| AppError::Config(format!("thread {thread_id} not found")))?;

    let generated = generate_candidates(&config, &detail);
    let subject = detail
        .subject
        .clone()
        .unwrap_or_else(|| "(no subject)".to_string());
    let to = infer_recipients(&detail);
    let ids = persist_generated_drafts(
        &conn,
        Some(thread_id),
        to.clone(),
        Vec::new(),
        &subject,
        &generated,
    )?;

    let mut lines = Vec::new();
    if generated.provider == "disabled" {
        lines.push("drafting provider disabled; generated fallback draft".to_string());
    } else {
        lines.push(format!(
            "generated {} draft candidate(s) via {} {}",
            ids.len(),
            generated.provider,
            generated.model
        ));
    }
    lines.push(format!("thread: {}", detail.thread_id));
    lines.push(format!("to: {}", join_recipients(&to)));
    for (index, (draft_id, candidate)) in ids.iter().zip(generated.candidates.iter()).enumerate() {
        lines.push(format!("draft {} [{}]", index + 1, draft_id));
        lines.push(indent_block(&candidate.body));
    }

    Ok(lines.join("\n"))
}

fn generate_candidates(
    config: &AppConfig,
    detail: &crate::index::query::ThreadDetailRow,
) -> DraftGenerationResult {
    if config
        .ai
        .drafting
        .as_ref()
        .and_then(|provider| provider.enabled)
        == Some(false)
        || config.ai.drafting.is_none()
    {
        return fallback_generation_result(detail);
    }

    fallback_generation_result(detail)
}

fn fallback_generation_result(
    detail: &crate::index::query::ThreadDetailRow,
) -> DraftGenerationResult {
    let latest_message = detail.messages.last();
    let latest_from = latest_message
        .and_then(|message| message.from_email.as_deref())
        .unwrap_or("the sender");
    let latest_body = latest_message
        .map(|message| summarize_body(&message.body_text))
        .unwrap_or_else(|| "your message".to_string());

    DraftGenerationResult {
        provider: "disabled".to_string(),
        model: "fallback-template".to_string(),
        candidates: vec![DraftCandidate {
            body: format!(
                "Hi,\n\nThanks for the note. I saw your message about {latest_body}. I'll follow up shortly with a fuller reply.\n\nBest,"
            ),
            rationale: vec![format!("Fallback draft created because AI drafting is unavailable for {latest_from}.")],
            confidence: 0.2,
        }],
    }
}

fn infer_recipients(detail: &crate::index::query::ThreadDetailRow) -> Vec<String> {
    detail
        .messages
        .iter()
        .rev()
        .find_map(|message| message.from_email.clone())
        .map(|email| vec![email])
        .unwrap_or_default()
}

fn summarize_body(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "your message".to_string()
    } else {
        compact.chars().take(72).collect()
    }
}

fn join_recipients(recipients: &[String]) -> String {
    if recipients.is_empty() {
        "(none inferred)".to_string()
    } else {
        recipients.join(", ")
    }
}

fn indent_block(text: &str) -> String {
    text.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::commands::sync::run_reindex;
    use crate::test_support::TestEnvGuard;

    use super::run_draft;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-draft-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn create_maildir(root: &std::path::Path) {
        fs::create_dir_all(root.join("cur")).expect("create cur");
        fs::create_dir_all(root.join("new")).expect("create new");
        fs::create_dir_all(root.join("tmp")).expect("create tmp");
    }

    #[tokio::test]
    async fn draft_generation_degrades_gracefully_when_ai_disabled() {
        let root = temp_dir("disabled");
        create_maildir(&root);
        fs::write(
            root.join("cur").join("msg-1"),
            "From: Alice <alice@example.com>\nTo: Ash <ash@example.com>\nSubject: Need Reply\nMessage-ID: <msg-1@example.com>\nDate: Tue, 11 Mar 2026 10:00:00 +0000\n\nPlease send the contract details\n",
        )
        .expect("write message");
        let config_path = root.join("config.toml");
        fs::write(
            &config_path,
            format!(
                "accounts = [{{ name = \"personal\", email_address = \"ash@example.com\", maildir_root = \"{}\", default = true }}]\n\n[ai.drafting]\nprovider = \"openai-compatible\"\nmodel = \"gpt-4o-mini\"\nenabled = false\n",
                root.display()
            ),
        )
        .expect("write config");

        let state_home = root.join("state");
        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", &state_home);

        run_reindex(Some(&config_path)).expect("reindex seed data");
        let output = run_draft(Some(&config_path), 1).await.expect("run draft");
        assert!(output.contains("drafting provider disabled"));
        assert!(output.contains("alice@example.com"));
        let _ = fs::remove_dir_all(root);
    }
}
