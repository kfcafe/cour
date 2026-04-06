use std::path::Path;

use crate::config::{AppConfig, ProjectPaths, ProviderConfig, SmtpIdentityConfig};
use crate::error::{AppError, AppResult};
use crate::index::open_database;

pub fn run_doctor(config_path: Option<&Path>) -> AppResult<String> {
    let config = AppConfig::load(config_path)?;
    let paths = ProjectPaths::detect()?;
    let resolved_config_path = config_path.unwrap_or(paths.config_file.as_path());

    let mut lines = Vec::new();
    let mut problems = 0usize;

    lines.push(format!("config: ok ({})", resolved_config_path.display()));

    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    lines.push(format!("state_dir: ok ({})", paths.state_dir.display()));

    let db_path = paths.state_dir.join("index.db");
    let _conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;
    lines.push(format!("database: ok ({})", db_path.display()));

    for account in &config.accounts {
        if account.maildir_root.exists() {
            lines.push(format!(
                "maildir:{}: ok ({})",
                account.name,
                account.maildir_root.display()
            ));
        } else {
            problems += 1;
            lines.push(format!(
                "maildir:{}: missing ({}) -- create or reconfigure this path",
                account.name,
                account.maildir_root.display()
            ));
        }
    }

    push_provider_diagnostics(
        &mut lines,
        &mut problems,
        "extraction",
        config.ai.extraction.as_ref(),
    );
    push_provider_diagnostics(
        &mut lines,
        &mut problems,
        "embedding",
        config.ai.embedding.as_ref(),
    );
    push_provider_diagnostics(
        &mut lines,
        &mut problems,
        "drafting",
        config.ai.drafting.as_ref(),
    );

    if config.smtp.is_empty() {
        lines.push("smtp: none configured".to_string());
    } else {
        for smtp in &config.smtp {
            push_smtp_diagnostics(&mut lines, &mut problems, smtp);
        }
    }

    lines.push(format!(
        "summary: {} issue(s), network checks skipped",
        problems
    ));

    Ok(lines.join("\n"))
}

fn push_provider_diagnostics(
    lines: &mut Vec<String>,
    problems: &mut usize,
    label: &str,
    provider: Option<&ProviderConfig>,
) {
    let Some(provider) = provider else {
        lines.push(format!("ai:{label}: not configured"));
        return;
    };

    let enabled = provider.enabled.unwrap_or(true);
    if !enabled {
        lines.push(format!(
            "ai:{label}: disabled (provider={}, model={})",
            provider.provider, provider.model
        ));
        return;
    }

    let missing = provider_missing_fields(provider);
    if missing.is_empty() {
        let endpoint = provider.api_url.as_deref().unwrap_or("default endpoint");
        lines.push(format!(
            "ai:{label}: ok (provider={}, model={}, endpoint={endpoint})",
            provider.provider, provider.model
        ));
    } else {
        *problems += 1;
        lines.push(format!(
            "ai:{label}: incomplete ({}) -- set required config fields",
            missing.join(", ")
        ));
    }
}

fn provider_missing_fields(provider: &ProviderConfig) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if provider.provider.trim().is_empty() {
        missing.push("provider");
    }
    if provider.model.trim().is_empty() {
        missing.push("model");
    }
    if provider
        .api_url
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("api_url");
    }
    missing
}

fn push_smtp_diagnostics(lines: &mut Vec<String>, problems: &mut usize, smtp: &SmtpIdentityConfig) {
    let missing = smtp_missing_fields(smtp);
    if missing.is_empty() {
        lines.push(format!(
            "smtp:{}: ok ({}:{}, sender={})",
            smtp.name, smtp.host, smtp.port, smtp.email_address
        ));
    } else {
        *problems += 1;
        lines.push(format!(
            "smtp:{}: incomplete ({}) -- update this identity before sending",
            smtp.name,
            missing.join(", ")
        ));
    }
}

fn smtp_missing_fields(smtp: &SmtpIdentityConfig) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if smtp.name.trim().is_empty() {
        missing.push("name");
    }
    if smtp.email_address.trim().is_empty() {
        missing.push("email_address");
    }
    if smtp.host.trim().is_empty() {
        missing.push("host");
    }
    if smtp.port == 0 {
        missing.push("port");
    }
    if smtp
        .username
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("username");
    }
    if smtp
        .password_env
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("password_env");
    }
    missing
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::test_support::TestEnvGuard;

    use super::run_doctor;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time after epoch")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("cour-doctor-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn with_env<T>(root: &std::path::Path, f: impl FnOnce() -> T) -> T {
        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", root);
        env.set_var("XDG_STATE_HOME", root.join("state"));
        f()
    }

    #[test]
    fn reports_missing_maildir_root() {
        let root = temp_dir("missing-maildir");
        let missing_maildir = root.join("does-not-exist");
        let config_path = root.join("config.toml");
        fs::write(
            &config_path,
            format!(
                concat!(
                    "accounts = [{{ name = \"personal\", email_address = \"ash@example.com\", maildir_root = \"{}\", default = true }}]\n",
                    "[ai.extraction]\n",
                    "provider = \"openai-compatible\"\n",
                    "model = \"gpt-4o-mini\"\n",
                    "api_url = \"https://example.invalid/v1\"\n"
                ),
                missing_maildir.display()
            ),
        )
        .expect("write config");

        let state_dir = root.join("state").join("cour");
        fs::create_dir_all(&state_dir).expect("create state dir");
        let _ = fs::remove_file(state_dir.join("index.db"));
        let _ = fs::remove_file(state_dir.join("index.db-wal"));
        let _ = fs::remove_file(state_dir.join("index.db-shm"));

        let output = with_env(&root, || run_doctor(Some(&config_path))).expect("run doctor");
        assert!(output.contains("maildir:personal: missing"));
        assert!(output.contains("summary: 1 issue(s), network checks skipped"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_healthy_config() {
        let root = temp_dir("healthy-config");
        let maildir = root.join("Maildir");
        fs::create_dir_all(&maildir).expect("create maildir");

        let config_path = root.join("config.toml");
        fs::write(
            &config_path,
            format!(
                concat!(
                    "accounts = [{{ name = \"personal\", email_address = \"ash@example.com\", maildir_root = \"{}\", default = true }}]\n",
                    "[ai.extraction]\n",
                    "provider = \"openai-compatible\"\n",
                    "model = \"gpt-4o-mini\"\n",
                    "api_url = \"https://example.invalid/v1\"\n",
                    "[ai.embedding]\n",
                    "provider = \"ollama\"\n",
                    "model = \"nomic-embed-text\"\n",
                    "api_url = \"http://localhost:11434\"\n",
                    "[[smtp]]\n",
                    "name = \"default\"\n",
                    "email_address = \"ash@example.com\"\n",
                    "host = \"smtp.example.com\"\n",
                    "port = 465\n",
                    "username = \"ash@example.com\"\n",
                    "password_env = \"MAILFOR_SMTP_PASSWORD\"\n"
                ),
                maildir.display()
            ),
        )
        .expect("write config");

        let state_dir = root.join("state").join("cour");
        fs::create_dir_all(&state_dir).expect("create state dir");
        let _ = fs::remove_file(state_dir.join("index.db"));
        let _ = fs::remove_file(state_dir.join("index.db-wal"));
        let _ = fs::remove_file(state_dir.join("index.db-shm"));

        let output = with_env(&root, || run_doctor(Some(&config_path))).expect("run doctor");
        assert!(output.contains("config: ok"));
        assert!(output.contains("state_dir: ok"));
        assert!(output.contains("database: ok"));
        assert!(output.contains("maildir:personal: ok"));
        assert!(output.contains("ai:extraction: ok"));
        assert!(output.contains("ai:embedding: ok"));
        assert!(output.contains("smtp:default: ok"));
        assert!(output.contains("summary: 0 issue(s), network checks skipped"));

        let _ = fs::remove_dir_all(root);
    }
}
