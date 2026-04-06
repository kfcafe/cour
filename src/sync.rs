use std::process::Stdio;
use std::time::{Duration, SystemTime};

use tokio::process::Command;

use crate::config::{AccountConfig, AppConfig};
use crate::error::{AppError, AppResult};
use crate::index::{open_database, repo::record_sync_run, schema::initialize_schema};
use crate::maildir::{discover_mailboxes, DiscoveredMailbox};
use crate::watch::{start_maildir_watcher, MaildirWatcher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRunResult {
    pub command: Option<String>,
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub exit_code: Option<i32>,
    pub stdout_text: String,
    pub stderr_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncAccountResult {
    pub account_name: String,
    pub mailboxes: Vec<DiscoveredMailbox>,
    pub sync: SyncRunResult,
}

pub struct SyncEngine {
    config: AppConfig,
    watcher: Option<MaildirWatcher>,
}

impl SyncEngine {
    pub fn new(config: AppConfig, watch_enabled: bool, debounce: Duration) -> AppResult<Self> {
        let watcher = start_maildir_watcher(&config.accounts, watch_enabled, debounce)?;
        Ok(Self { config, watcher })
    }

    pub fn watcher(&self) -> Option<&MaildirWatcher> {
        self.watcher.as_ref()
    }

    pub async fn sync_all(&self, db_path: &std::path::Path) -> AppResult<Vec<SyncAccountResult>> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(AppError::Io)?;
        }
        let conn = open_database(db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;
        initialize_schema(&conn).map_err(|err| AppError::Sqlite(err.to_string()))?;

        let mut results = Vec::new();
        for account in &self.config.accounts {
            let sync = run_sync_command(account).await?;
            let mailboxes = discover_mailboxes(&account.maildir_root)?;
            let account_id = crate::index::repo::upsert_account(&conn, account)
                .map_err(|err| AppError::Sqlite(err.to_string()))?;
            let command_text = sync.command.clone().unwrap_or_default();
            record_sync_run(
                &conn,
                account_id,
                &command_text,
                sync.exit_code,
                &sync.stdout_text,
                &sync.stderr_text,
            )
            .map_err(|err| AppError::Sqlite(err.to_string()))?;
            results.push(SyncAccountResult {
                account_name: account.name.clone(),
                mailboxes,
                sync,
            });
        }

        Ok(results)
    }
}

pub async fn run_sync_command(account: &AccountConfig) -> AppResult<SyncRunResult> {
    let started_at = SystemTime::now();

    let Some(command) = account.sync_command.clone() else {
        let finished_at = SystemTime::now();
        return Ok(SyncRunResult {
            command: None,
            started_at,
            finished_at,
            exit_code: Some(0),
            stdout_text: String::new(),
            stderr_text: "sync command not configured".to_string(),
        });
    };

    let output = Command::new("sh")
        .arg("-lc")
        .arg(&command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| AppError::SyncCommand(err.to_string()))?;

    let finished_at = SystemTime::now();
    let result = SyncRunResult {
        command: Some(command.clone()),
        started_at,
        finished_at,
        exit_code: output.status.code(),
        stdout_text: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr_text: String::from_utf8_lossy(&output.stderr).to_string(),
    };

    if output.status.success() {
        Ok(result)
    } else {
        Err(AppError::SyncCommand(format!(
            "command `{command}` failed with status {:?}: {}",
            result.exit_code, result.stderr_text
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{AccountConfig, AppConfig};

    use super::{run_sync_command, SyncEngine};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-sync-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn create_maildir(path: &std::path::Path) {
        fs::create_dir_all(path.join("cur")).expect("create cur");
        fs::create_dir_all(path.join("new")).expect("create new");
        fs::create_dir_all(path.join("tmp")).expect("create tmp");
        fs::create_dir_all(path.join(".Sent").join("cur")).expect("create sent cur");
        fs::create_dir_all(path.join(".Sent").join("new")).expect("create sent new");
        fs::create_dir_all(path.join(".Sent").join("tmp")).expect("create sent tmp");
    }

    #[tokio::test]
    async fn captures_sync_command_output() {
        let account = AccountConfig {
            name: "personal".to_string(),
            email_address: "ash@example.com".to_string(),
            maildir_root: "/tmp/mail/personal".into(),
            sync_command: Some("printf 'hello-sync'".to_string()),
            default: Some(true),
        };

        let result = run_sync_command(&account).await.expect("run sync command");
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout_text.contains("hello-sync"));
    }

    #[tokio::test]
    async fn sync_engine_records_run_and_discovers_mailboxes() {
        let root = temp_dir("engine");
        create_maildir(&root);
        let config = AppConfig {
            accounts: vec![AccountConfig {
                name: "personal".to_string(),
                email_address: "ash@example.com".to_string(),
                maildir_root: root.clone(),
                sync_command: Some("printf ok".to_string()),
                default: Some(true),
            }],
            ai: Default::default(),
            smtp: Vec::new(),
        };

        let db_path = root.join("state").join("index.db");
        let engine = SyncEngine::new(config, false, Duration::from_millis(50)).expect("engine");
        let results = engine.sync_all(&db_path).await.expect("sync all");

        assert_eq!(results.len(), 1);
        assert!(results[0]
            .mailboxes
            .iter()
            .any(|mailbox| mailbox.name == "Inbox"));
        assert!(results[0]
            .mailboxes
            .iter()
            .any(|mailbox| mailbox.name == "Sent"));

        let conn = crate::index::open_database(&db_path).expect("open db");
        let sync_runs: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_runs", [], |row| row.get(0))
            .expect("count sync runs");
        assert_eq!(sync_runs, 1);

        let _ = fs::remove_dir_all(root);
    }
}
