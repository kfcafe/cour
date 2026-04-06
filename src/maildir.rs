use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredMailbox {
    pub name: String,
    pub path: PathBuf,
    pub special_use: Option<String>,
}

pub fn discover_mailboxes(root: &Path) -> AppResult<Vec<DiscoveredMailbox>> {
    let mut mailboxes = Vec::new();
    discover_recursive(root, root, &mut mailboxes)?;
    mailboxes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(mailboxes)
}

fn discover_recursive(
    root: &Path,
    current: &Path,
    mailboxes: &mut Vec<DiscoveredMailbox>,
) -> AppResult<()> {
    if is_maildir_mailbox(current)? {
        mailboxes.push(DiscoveredMailbox {
            name: mailbox_name(root, current),
            path: current.to_path_buf(),
            special_use: special_use_for_path(root, current),
        });
    }

    for entry in fs::read_dir(current).map_err(AppError::Io)? {
        let entry = entry.map_err(AppError::Io)?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name == "cur" || file_name == "new" || file_name == "tmp" {
            continue;
        }

        discover_recursive(root, &path, mailboxes)?;
    }

    Ok(())
}

fn is_maildir_mailbox(path: &Path) -> AppResult<bool> {
    Ok(path.join("cur").is_dir() && path.join("new").is_dir() && path.join("tmp").is_dir())
}

fn mailbox_name(root: &Path, path: &Path) -> String {
    if path == root {
        return "Inbox".to_string();
    }

    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        let raw = component.as_os_str().to_string_lossy();
        let trimmed = raw.trim_start_matches('.');
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    if parts.is_empty() {
        "Inbox".to_string()
    } else {
        parts.join("/")
    }
}

fn special_use_for_path(root: &Path, path: &Path) -> Option<String> {
    let name = mailbox_name(root, path).to_ascii_lowercase();
    match name.as_str() {
        "inbox" => Some("inbox".to_string()),
        "sent" | "sent mail" | "sent messages" => Some("sent".to_string()),
        "drafts" => Some("drafts".to_string()),
        "trash" | "deleted" | "deleted messages" => Some("trash".to_string()),
        "archive" | "all mail" => Some("archive".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::discover_mailboxes;

    fn temp_root() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cour-maildir-test-{unique}"));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    fn create_maildir(path: &Path) {
        fs::create_dir_all(path.join("cur")).expect("create cur");
        fs::create_dir_all(path.join("new")).expect("create new");
        fs::create_dir_all(path.join("tmp")).expect("create tmp");
    }

    #[test]
    fn discovers_nested_maildir_mailboxes() {
        let root = temp_root();
        create_maildir(&root);
        create_maildir(&root.join(".Projects"));
        create_maildir(&root.join(".Projects").join(".ClientA"));
        create_maildir(&root.join(".Sent"));

        let mailboxes = discover_mailboxes(&root).expect("discover mailboxes");
        let names: Vec<_> = mailboxes.iter().map(|m| m.name.as_str()).collect();

        assert!(names.contains(&"Inbox"));
        assert!(names.contains(&"Projects"));
        assert!(names.contains(&"Projects/ClientA"));
        assert!(names.contains(&"Sent"));

        let sent = mailboxes
            .iter()
            .find(|mailbox| mailbox.name == "Sent")
            .expect("find sent mailbox");
        assert_eq!(sent.special_use.as_deref(), Some("sent"));

        let _ = fs::remove_dir_all(&root);
    }
}
