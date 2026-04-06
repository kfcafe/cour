use std::path::Path;

use crate::config::{AppConfig, ProjectPaths};
use crate::error::{AppError, AppResult};
use crate::index::open_database;
use crate::index::repo::{append_audit_event, get_draft, update_draft_status};

pub fn run_send(config_path: Option<&Path>, draft_id: i64) -> AppResult<String> {
    let config = AppConfig::load(config_path)?;
    let paths = ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    let conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;

    let draft = get_draft(&conn, draft_id)
        .map_err(|err| AppError::Sqlite(err.to_string()))?
        .ok_or_else(|| AppError::Config(format!("draft {draft_id} not found")))?;

    if draft.status != "approved" {
        return Err(AppError::Send(format!(
            "draft {draft_id} is {} and must be approved before send",
            draft.status
        )));
    }

    let _identity = config
        .smtp
        .iter()
        .find(|identity| identity.default.unwrap_or(false))
        .or_else(|| config.smtp.first())
        .ok_or_else(|| AppError::Config("no smtp identity configured".to_string()))?;

    update_draft_status(&conn, draft_id, "sent")
        .map_err(|err| AppError::Sqlite(err.to_string()))?;
    append_audit_event(
        &conn,
        "draft",
        draft_id,
        "draft_send_delegated",
        Some("{\"source\":\"cli\",\"transport\":\"pending\"}"),
    )
    .map_err(|err| AppError::Sqlite(err.to_string()))?;

    Ok(format!(
        "send delegated for draft {draft_id}; SMTP delivery not wired yet"
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::index::repo::{create_draft, update_draft_status};
    use crate::index::schema::initialize_schema;
    use crate::test_support::TestEnvGuard;

    use super::run_send;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-send-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn setup_pending_draft() -> (
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        i64,
    ) {
        let root = temp_dir("pending");
        let config_path = root.join("config.toml");
        fs::write(
            &config_path,
            "accounts = [{ name = \"personal\", email_address = \"ash@example.com\", maildir_root = \"/tmp/mail\", default = true }]\n\n[[smtp]]\nname = \"default\"\nemail_address = \"ash@example.com\"\nhost = \"smtp.example.com\"\nport = 465\ndefault = true\n",
        )
        .expect("write config");

        let state_home = root.join("state");
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

        (root, config_path, state_home, draft_id)
    }

    #[test]
    fn refuses_unapproved_draft() {
        let (root, config_path, state_home, draft_id) = setup_pending_draft();
        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", &state_home);

        let err =
            run_send(Some(&config_path), draft_id).expect_err("pending draft should be rejected");
        assert!(err.to_string().contains("must be approved before send"));

        drop(env);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn marks_approved_draft_as_sent_when_delegated() {
        let (root, config_path, state_home, draft_id) = setup_pending_draft();
        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", &state_home);

        let conn = Connection::open(state_home.join("cour").join("index.db")).expect("open db");
        update_draft_status(&conn, draft_id, "approved").expect("approve draft");
        drop(conn);

        let output = run_send(Some(&config_path), draft_id).expect("send approved draft");
        assert!(output.contains("SMTP delivery not wired yet"));

        let conn = Connection::open(state_home.join("cour").join("index.db")).expect("reopen db");
        let status: String = conn
            .query_row(
                "SELECT status FROM drafts WHERE id = ?1",
                [draft_id],
                |row| row.get(0),
            )
            .expect("read draft status");
        assert_eq!(status, "sent");

        drop(env);
        let _ = fs::remove_dir_all(root);
    }
}
