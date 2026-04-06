use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::index::open_database;
use crate::ingest::ingest_maildir_account;
use crate::sync::SyncEngine;

pub async fn run_sync(config_path: Option<&std::path::Path>, watch: bool) -> AppResult<String> {
    let config = AppConfig::load(config_path)?;
    let paths = crate::config::ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    let engine = SyncEngine::new(config, watch, std::time::Duration::from_millis(250))?;
    let results = engine.sync_all(&db_path).await?;

    let mut lines = Vec::new();
    for result in results {
        lines.push(format!(
            "{}: sync ok (exit={:?}, mailboxes={})",
            result.account_name,
            result.sync.exit_code,
            result.mailboxes.len()
        ));
    }

    if engine.watcher().is_some() {
        lines.push("watch: enabled".to_string());
    } else {
        lines.push("watch: disabled".to_string());
    }

    Ok(lines.join("\n"))
}

pub fn run_reindex(config_path: Option<&std::path::Path>) -> AppResult<String> {
    let config = AppConfig::load(config_path)?;
    let paths = crate::config::ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    drop(open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?);
    let conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;
    crate::index::schema::initialize_schema(&conn)
        .map_err(|err| AppError::Sqlite(err.to_string()))?;

    let mut lines = Vec::new();
    for account in &config.accounts {
        let report = ingest_maildir_account(&conn, account)?;
        lines.push(format!(
            "{}: imported={}, updated={}, skipped={}, failed={}",
            account.name, report.imported, report.updated, report.skipped, report.failed
        ));
    }

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::test_support::TestEnvGuard;

    use super::{run_reindex, run_sync};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-cmd-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn create_maildir(root: &std::path::Path) {
        fs::create_dir_all(root.join("cur")).expect("create cur");
        fs::create_dir_all(root.join("new")).expect("create new");
        fs::create_dir_all(root.join("tmp")).expect("create tmp");
    }

    #[tokio::test]
    async fn sync_handler_reports_success() {
        let root = temp_dir("sync");
        create_maildir(&root);
        let config_path = root.join("config.toml");
        fs::write(
            &config_path,
            format!(
                "accounts = [{{ name = \"personal\", email_address = \"ash@example.com\", maildir_root = \"{}\", sync_command = \"printf sync-ok\", default = true }}]\n",
                root.display()
            ),
        )
        .expect("write config");

        let output = run_sync(Some(&config_path), false)
            .await
            .expect("run sync command");
        assert!(output.contains("personal: sync ok"));
        assert!(output.contains("watch: disabled"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reindex_handler_reports_success() {
        let root = temp_dir("reindex");
        create_maildir(&root);
        fs::write(
            root.join("cur").join("msg-1"),
            "From: Alice <alice@example.com>\nTo: Ash <ash@example.com>\nSubject: Hello\nMessage-ID: <msg-1@example.com>\nDate: Tue, 11 Mar 2026 10:00:00 +0000\n\nBody\n",
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

        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", root.join("state"));

        let output = run_reindex(Some(&config_path)).expect("run reindex");
        assert!(output.contains("imported=1"));

        drop(env);
        let _ = fs::remove_dir_all(root);
    }
}
