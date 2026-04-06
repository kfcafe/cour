use crate::config::{AppConfig, ProjectPaths};
use crate::error::{AppError, AppResult};
use crate::index::open_database;
use crate::index::query::{
    get_thread_detail, list_recent_threads, list_threads_by_state, pending_drafts_count,
};

const BRIEF_SECTIONS: [(&str, &str); 8] = [
    ("Needs reply", "waiting_on_me"),
    ("Urgent", "urgent"),
    ("Waiting on me", "waiting_on_me"),
    ("Waiting on them", "waiting_on_them"),
    ("Follow up due", "follow_up_due"),
    ("Payments", "payments"),
    ("Scheduling", "scheduling"),
    ("Low value", "low_value"),
];

pub fn run_brief(config_path: Option<&std::path::Path>) -> AppResult<String> {
    let _config = AppConfig::load(config_path)?;
    let paths = ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    let conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;

    let recent = list_recent_threads(&conn, 5).map_err(|err| AppError::Sqlite(err.to_string()))?;
    let pending_drafts =
        pending_drafts_count(&conn).map_err(|err| AppError::Sqlite(err.to_string()))?;

    let mut out = String::new();
    for (index, (label, state)) in BRIEF_SECTIONS.iter().enumerate() {
        if index > 0 {
            out.push_str("\n\n");
        }

        out.push_str(label);
        out.push('\n');

        let rows =
            list_threads_by_state(&conn, state).map_err(|err| AppError::Sqlite(err.to_string()))?;
        if rows.is_empty() {
            out.push_str("- None\n");
            continue;
        }

        for row in rows {
            out.push_str(&format!(
                "- [{}] {}{}\n",
                row.id,
                row.subject.as_deref().unwrap_or("(no subject)"),
                format_message_count(row.message_count)
            ));
        }
    }

    out.push_str("\n\nRecent threads\n");
    if recent.is_empty() {
        out.push_str("- None\n");
    } else {
        for row in recent {
            let state = row.state.as_deref().unwrap_or("unknown");
            out.push_str(&format!(
                "- [{}] {} ({state}{})\n",
                row.id,
                row.subject.as_deref().unwrap_or("(no subject)"),
                format_message_count(row.message_count)
            ));
        }
    }

    out.push_str(&format!("\nPending drafts: {pending_drafts}\n"));
    Ok(out)
}

pub fn run_thread(config_path: Option<&std::path::Path>, thread_id: i64) -> AppResult<String> {
    let _config = AppConfig::load(config_path)?;
    let paths = ProjectPaths::detect()?;
    std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
    let db_path = paths.state_dir.join("index.db");
    let conn = open_database(&db_path).map_err(|err| AppError::Sqlite(err.to_string()))?;

    let detail = get_thread_detail(&conn, thread_id)
        .map_err(|err| AppError::Sqlite(err.to_string()))?
        .ok_or_else(|| AppError::Config(format!("thread {thread_id} not found")))?;

    let participants = collect_participants(&detail.messages);
    let latest_summary = detail
        .messages
        .last()
        .map(|message| summarize_body(&message.body_text))
        .unwrap_or_else(|| "No messages in thread".to_string());

    let mut out = String::new();
    out.push_str(&format!("Thread {}\n", detail.thread_id));
    out.push_str(&format!(
        "Subject: {}\n",
        detail.subject.as_deref().unwrap_or("(no subject)")
    ));
    out.push_str(&format!(
        "State: {}\n",
        detail.state.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&format!("Messages: {}\n", detail.messages.len()));
    out.push_str(&format!("Summary: {latest_summary}\n\n"));

    out.push_str("Participants\n");
    if participants.is_empty() {
        out.push_str("- None\n");
    } else {
        for participant in participants {
            out.push_str(&format!("- {participant}\n"));
        }
    }

    out.push_str("\nAI metadata\n");
    out.push_str("- Summary: unavailable\n");
    out.push_str("- Latest ask: unavailable\n");
    out.push_str("- Recommended action: unavailable\n");

    out.push_str("\nMessages\n");
    for (index, message) in detail.messages.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "{}{}. {}\n",
            if index + 1 < 10 { "0" } else { "" },
            index + 1,
            message.subject.as_deref().unwrap_or("(no subject)")
        ));
        out.push_str(&format!(
            "From: {}\n",
            message.from_email.as_deref().unwrap_or("unknown")
        ));
        out.push_str(&format!(
            "Sent: {}\n",
            message.sent_at.as_deref().unwrap_or("unknown")
        ));
        out.push_str(&format!("Body: {}\n", summarize_body(&message.body_text)));
    }

    Ok(out)
}

fn format_message_count(message_count: i64) -> String {
    if message_count <= 1 {
        String::new()
    } else {
        format!(", {message_count} messages")
    }
}

fn summarize_body(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "(empty body)".to_string();
    }
    if normalized.chars().count() <= 120 {
        normalized
    } else {
        let shortened: String = normalized.chars().take(117).collect();
        format!("{shortened}...")
    }
}

fn collect_participants(messages: &[crate::index::query::ThreadMessageRow]) -> Vec<String> {
    let mut participants = Vec::new();
    for message in messages {
        if let Some(email) = message.from_email.as_deref() {
            if !participants.iter().any(|existing| existing == email) {
                participants.push(email.to_string());
            }
        }
    }
    participants
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::commands::sync::run_reindex;
    use crate::test_support::TestEnvGuard;

    use super::{run_brief, run_thread};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-brief-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn create_maildir(root: &std::path::Path) {
        fs::create_dir_all(root.join("cur")).expect("create cur");
        fs::create_dir_all(root.join("new")).expect("create new");
        fs::create_dir_all(root.join("tmp")).expect("create tmp");
    }

    fn seed_index() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = temp_dir("seed");
        create_maildir(&root);
        fs::write(
            root.join("cur").join("msg-1"),
            "From: Alice <alice@example.com>\nTo: Ash <ash@example.com>\nSubject: Need Reply\nMessage-ID: <msg-1@example.com>\nDate: Tue, 11 Mar 2026 10:00:00 +0000\n\nPlease reply\n",
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
        drop(env);

        (root, config_path, state_home)
    }

    #[test]
    fn brief_output_includes_needs_reply_section() {
        let (root, config_path, state_home) = seed_index();

        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", &state_home);

        let output = run_brief(Some(&config_path)).expect("run brief");
        assert!(output.contains("Needs reply"));
        assert!(output.contains("Need Reply"));
        assert!(output.contains("Urgent"));
        assert!(output.contains("Low value"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn thread_output_renders_messages() {
        let (root, config_path, state_home) = seed_index();

        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &root);
        env.set_var("XDG_STATE_HOME", &state_home);

        let output = run_thread(Some(&config_path), 1).expect("run thread");
        assert!(output.contains("Thread 1"));
        assert!(output.contains("Participants"));
        assert!(output.contains("alice@example.com"));
        assert!(output.contains("AI metadata"));
        assert!(output.contains("Please reply"));
        let _ = fs::remove_dir_all(root);
    }
}
