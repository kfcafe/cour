use clap::{Parser, Subcommand};

use crate::config::ConfigArgs;
use crate::error::{AppError, AppResult};

pub mod approve;
pub mod doctor;
pub mod draft;
pub mod read;
pub mod search;
pub mod send;
pub mod sync;

#[derive(Debug, Parser)]
#[command(name = "cour", about = "Local-first AI mail client")]
pub struct Cli {
    #[command(flatten)]
    pub config: ConfigArgs,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    Sync {
        #[arg(long, default_value_t = true)]
        watch: bool,
    },
    Reindex,
    Brief,
    Ask {
        query: String,
    },
    Thread {
        thread_id: String,
    },
    Draft {
        thread_id: String,
    },
    Approve {
        draft_id: String,
    },
    Send {
        draft_id: String,
    },
    Doctor,
    Tui,
    Setup,
}

pub async fn dispatch(cli: Cli) -> AppResult<()> {
    let config_path = cli.config.config.as_deref();
    match cli.command {
        Some(Commands::Sync { watch }) => {
            let output = sync::run_sync(config_path, watch).await?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Reindex) => {
            let output = sync::run_reindex(config_path)?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Brief) => {
            let output = read::run_brief(config_path)?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Ask { query }) => {
            let output = search::run_ask(config_path, &query)?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Thread { thread_id }) => {
            let thread_id = parse_id("thread", &thread_id)?;
            let output = read::run_thread(config_path, thread_id)?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Draft { thread_id }) => {
            let thread_id = parse_id("thread", &thread_id)?;
            let output = draft::run_draft(config_path, thread_id).await?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Approve { draft_id }) => {
            let draft_id = parse_id("draft", &draft_id)?;
            let output = approve::run_approve(config_path, draft_id)?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Send { draft_id }) => {
            let draft_id = parse_id("draft", &draft_id)?;
            let output = send::run_send(config_path, draft_id)?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Doctor) => {
            let output = doctor::run_doctor(config_path)?;
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(())
        }
        Some(Commands::Tui) => tui(config_path),
        Some(Commands::Setup) => setup(config_path),
        None => tui(config_path),
    }
}

fn parse_id(kind: &str, raw: &str) -> AppResult<i64> {
    raw.parse::<i64>()
        .map_err(|err| AppError::Config(format!("invalid {kind} id: {err}")))
}

pub fn tui(config_path: Option<&std::path::Path>) -> AppResult<()> {
    match crate::config::AppConfig::load(config_path) {
        Ok(config) => {
            let paths = crate::config::ProjectPaths::detect()?;
            std::fs::create_dir_all(&paths.state_dir).map_err(AppError::Io)?;
            let db_path = paths.state_dir.join("index.db");
            crate::ui::app::run_app(config, db_path)
        }
        Err(e) => {
            let paths = crate::config::ProjectPaths::detect().ok();
            let config_file = config_path
                .map(std::path::Path::to_path_buf)
                .or_else(|| paths.as_ref().map(|p| p.config_file.clone()));

            if let Some(ref path) = config_file {
                if crate::ui::onboarding::run_onboarding(path).is_ok() {
                    return Ok(());
                }
            }

            eprintln!("cour — local-first AI mail client\n");
            eprintln!("No configuration found.\n");
            if let Some(ref path) = config_file {
                eprintln!("Expected config at: {}\n", path.display());
            }
            eprintln!("Create a minimal config:\n");
            eprintln!("  mkdir -p ~/.config/cour");
            eprintln!("  cat > ~/.config/cour/config.toml << 'EOF'");
            eprintln!("  accounts = [");
            eprintln!("    {{ name = \"personal\", email_address = \"you@example.com\", maildir_root = \"/path/to/Maildir\", default = true }}");
            eprintln!("  ]");
            eprintln!("  EOF\n");
            eprintln!("Then run: cour doctor\n");
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

pub fn setup(config_path: Option<&std::path::Path>) -> AppResult<()> {
    let config_file = config_path
        .map(std::path::Path::to_path_buf)
        .unwrap_or(crate::config::ProjectPaths::detect()?.config_file);
    crate::ui::onboarding::run_onboarding(&config_file)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use clap::CommandFactory;

    use crate::config::ConfigArgs;
    use crate::test_support::TestEnvGuard;

    use super::{dispatch, Cli, Commands};

    #[test]
    fn help_lists_planned_subcommands() {
        let help = Cli::command().render_long_help().to_string();
        for expected in [
            "sync", "reindex", "brief", "ask", "thread", "draft", "approve", "send", "doctor",
            "tui", "setup",
        ] {
            assert!(
                help.contains(expected),
                "missing subcommand {expected} in help: {help}"
            );
        }
    }

    #[test]
    fn enum_contains_brief_variant() {
        let variant = Commands::Brief;
        assert!(matches!(variant, Commands::Brief));
    }

    #[tokio::test]
    async fn default_launch_path_invokes_tui_when_config_exists() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-tui-startup-{unique}"));
        fs::create_dir_all(&root).expect("create root");

        let config_home = root.join("config-home");
        let state_home = root.join("state-home");
        let maildir_root = root.join("Maildir");
        fs::create_dir_all(config_home.join(".config/cour")).expect("create config dir");
        fs::create_dir_all(&state_home).expect("create state dir");
        fs::create_dir_all(&maildir_root).expect("create maildir dir");

        let config_path = config_home.join(".config/cour/config.toml");
        fs::write(
            &config_path,
            format!(
                "accounts = [{{ name = \"personal\", email_address = \"you@example.com\", maildir_root = \"{}\", default = true }}]\n",
                maildir_root.display()
            ),
        )
        .expect("write config");

        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &config_home);
        env.set_var("XDG_STATE_HOME", &state_home);

        let cli = Cli {
            config: ConfigArgs {
                config: Some(PathBuf::from(&config_path)),
            },
            command: None,
        };

        let result = dispatch(cli).await;
        assert!(result.is_err(), "expected tui startup to be attempted");

        let error = result.expect_err("tui startup should fail in test env");
        let message = error.to_string().to_lowercase();
        assert!(
            message.contains("tty")
                || message.contains("terminal")
                || message.contains("raw mode")
                || message.contains("os error"),
            "expected terminal startup failure, got: {message}"
        );

        drop(env);
        let _ = fs::remove_dir_all(root);
    }
}
