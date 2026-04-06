use std::path::Path;

use crate::config::{AppConfig, ProjectPaths};
use crate::error::{AppError, AppResult};
use crate::index::open_database;
use crate::index::repo::{append_audit_event, get_draft, update_draft_status};

pub fn run_approve(config_path: Option<&Path>, draft_id: i64) -> AppResult<String> {
    let _config = AppConfig::load(config_path)?;
    let paths = ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    let conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;

    let draft = get_draft(&conn, draft_id)
        .map_err(|err| AppError::Sqlite(err.to_string()))?
        .ok_or_else(|| AppError::Config(format!("draft {draft_id} not found")))?;

    if draft.status == "approved" {
        return Ok(format!("draft {draft_id} already approved"));
    }

    update_draft_status(&conn, draft_id, "approved")
        .map_err(|err| AppError::Sqlite(err.to_string()))?;
    append_audit_event(
        &conn,
        "draft",
        draft_id,
        "draft_approved",
        Some("{\"source\":\"cli\"}"),
    )
    .map_err(|err| AppError::Sqlite(err.to_string()))?;

    Ok(format!("approved draft {draft_id}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::index::repo::{create_draft, get_draft};
    use crate::index::schema::initialize_schema;
    use crate::test_support::TestEnvGuard;

    use super::run_approve;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-approve-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    #[test]
    fn approve_updates_status_and_audits() {
        let root = temp_dir("status");
        let config_path = root.join("config.toml");
        fs::write(
            &config_path,
            "accounts = [{ name = \"personal\", email_address = \"ash@example.com\", maildir_root = \"/tmp/mail\", default = true }]\n",
        )
        .expect("write config");

        let state_home = root.join("state");
        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", &state_home);

        fs::create_dir_all(state_home.join("cour")).expect("create state dir");
        let db_path = state_home.join("cour").join("index.db");
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
            Some("[]"),
        )
        .expect("create draft");
        drop(conn);

        let output = run_approve(Some(&config_path), draft_id).expect("approve draft");
        assert!(output.contains("approved draft"));

        let conn = Connection::open(&db_path).expect("reopen db");
        let draft = get_draft(&conn, draft_id)
            .expect("get draft")
            .expect("draft exists");
        assert_eq!(draft.status, "approved");
        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE entity_type = 'draft' AND entity_id = ?1 AND action = 'draft_approved'",
                [draft_id],
                |row| row.get(0),
            )
            .expect("count audit events");
        assert_eq!(audit_count, 1);

        drop(env);
        let _ = fs::remove_dir_all(root);
    }
}
