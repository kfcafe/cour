use crate::config::{AppConfig, ProjectPaths};
use crate::error::{AppError, AppResult};
use crate::index::open_database;
use crate::index::query::{ask_search, LexicalSearchFilters};

pub fn run_ask(config_path: Option<&std::path::Path>, query: &str) -> AppResult<String> {
    let _config = AppConfig::load(config_path)?;
    let paths = ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    let conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;

    let rows = ask_search(&conn, query, &LexicalSearchFilters::default(), None, 10)
        .map_err(|err| AppError::Sqlite(err.to_string()))?;

    let mut out = String::new();
    out.push_str(&format!("Ask: {query}\n"));
    if rows.is_empty() {
        out.push_str("No matching threads found.\n");
        return Ok(out);
    }

    out.push_str("Ranked threads\n");
    for (index, row) in rows.iter().enumerate() {
        out.push_str(&format!(
            "{}. [{}] {}\n",
            index + 1,
            row.thread_id,
            row.subject.as_deref().unwrap_or("(no subject)")
        ));
        out.push_str(&format!(
            "   Reason: lexical rank {} + semantic {:?} => {:.3}\n",
            row.score.lexical_rank, row.score.semantic_similarity, row.score.blended_score
        ));
        out.push_str(&format!(
            "   Evidence: {}\n",
            sanitize_snippet(&row.evidence_snippet)
        ));
    }

    Ok(out)
}

fn sanitize_snippet(snippet: &str) -> String {
    let normalized = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        "No snippet available".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::commands::sync::run_reindex;
    use crate::test_support::TestEnvGuard;

    use super::run_ask;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-ask-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn create_maildir(root: &std::path::Path) {
        fs::create_dir_all(root.join("cur")).expect("create cur");
        fs::create_dir_all(root.join("new")).expect("create new");
        fs::create_dir_all(root.join("tmp")).expect("create tmp");
    }

    #[test]
    fn ask_output_includes_search_results() {
        let root = temp_dir("seed");
        create_maildir(&root);
        fs::write(
            root.join("cur").join("msg-1"),
            "From: Alice <alice@example.com>\nTo: Ash <ash@example.com>\nSubject: Invoice Followup\nMessage-ID: <msg-1@example.com>\nDate: Tue, 11 Mar 2026 10:00:00 +0000\n\nPlease pay the invoice this week\n",
        )
        .expect("write message");
        let config_path = root.join("config.toml");
        fs::write(
            &config_path,
            format!(
                "accounts = [{{ name = \"personal\", email_address = \"ash@example.com\", maildir_root = \"{}\", default = true }}]\n",
                root.display()
            ),
        )
        .expect("write config");

        let state_home = root.join("state");
        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", &state_home);
        let _ = run_reindex(Some(&config_path)).expect("reindex seed data");

        let output = run_ask(Some(&config_path), "invoice").expect("run ask");
        assert!(output.contains("Ranked threads"));
        assert!(output.contains("Invoice Followup"));
        assert!(output.contains("Reason: lexical rank 1"));
        assert!(output.contains("Evidence:"));
        let _ = fs::remove_dir_all(root);
    }
}
