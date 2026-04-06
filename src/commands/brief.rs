#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::commands::read::run_brief;
    use crate::commands::sync::run_reindex;
    use crate::test_support::TestEnvGuard;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-brief-alias-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn create_maildir(root: &std::path::Path) {
        fs::create_dir_all(root.join("cur")).expect("create cur");
        fs::create_dir_all(root.join("new")).expect("create new");
        fs::create_dir_all(root.join("tmp")).expect("create tmp");
    }

    #[test]
    fn brief_output_includes_needs_reply_section() {
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

        let output = run_brief(Some(&config_path)).expect("run brief");
        assert!(output.contains("Needs reply"));
        assert!(output.contains("Need Reply"));

        drop(env);
        let _ = fs::remove_dir_all(root);
    }
}
