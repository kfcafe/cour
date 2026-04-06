use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadCandidate {
    pub message_row_id: i64,
    pub message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadAssignment {
    pub message_row_id: i64,
    pub thread_key: String,
}

pub fn assign_threads(candidates: &[ThreadCandidate]) -> Vec<ThreadAssignment> {
    let mut root_for_message_id: HashMap<String, String> = HashMap::new();
    let mut subject_roots: HashMap<String, String> = HashMap::new();
    let mut assignments = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let root_key =
            thread_root_for_candidate(candidate, &root_for_message_id, &mut subject_roots)
                .unwrap_or_else(|| format!("message-row:{}", candidate.message_row_id));

        if let Some(message_id) = &candidate.message_id_header {
            root_for_message_id.insert(message_id.clone(), root_key.clone());
        }

        assignments.push(ThreadAssignment {
            message_row_id: candidate.message_row_id,
            thread_key: root_key,
        });
    }

    assignments
}

fn thread_root_for_candidate(
    candidate: &ThreadCandidate,
    root_for_message_id: &HashMap<String, String>,
    subject_roots: &mut HashMap<String, String>,
) -> Option<String> {
    candidate
        .references
        .iter()
        .rev()
        .find_map(|reference| root_for_message_id.get(reference).cloned())
        .or_else(|| {
            candidate
                .in_reply_to
                .as_ref()
                .and_then(|reply_to| root_for_message_id.get(reply_to).cloned())
        })
        .or_else(|| subject_thread_root(candidate.subject.as_deref(), subject_roots))
}

fn subject_thread_root(
    subject: Option<&str>,
    subject_roots: &mut HashMap<String, String>,
) -> Option<String> {
    let normalized = normalize_subject(subject?);
    if normalized.is_empty() {
        return None;
    }

    if let Some(existing) = subject_roots.get(&normalized) {
        return Some(existing.clone());
    }

    let key = format!("subject:{normalized}");
    subject_roots.insert(normalized, key.clone());
    Some(key)
}

pub fn normalize_subject(subject: &str) -> String {
    let mut current = subject.trim();

    loop {
        let next = strip_subject_prefix(current);
        if next == current {
            break;
        }
        current = next;
    }

    current.trim().to_string()
}

fn strip_subject_prefix(subject: &str) -> &str {
    let trimmed = subject.trim_start();
    let bytes = trimmed.as_bytes();
    let mut alpha_end = 0;

    while alpha_end < bytes.len() && bytes[alpha_end].is_ascii_alphabetic() {
        alpha_end += 1;
    }

    if alpha_end == 0 {
        return subject;
    }

    let mut index = alpha_end;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }

    if index == bytes.len() || bytes[index] != b':' {
        return subject;
    }

    let prefix = trimmed[..alpha_end].to_ascii_lowercase();
    if !matches!(prefix.as_str(), "re" | "fw" | "fwd") {
        return subject;
    }

    trimmed[index + 1..].trim_start()
}

#[cfg(test)]
mod tests {
    use super::{assign_threads, normalize_subject, ThreadCandidate};

    #[test]
    fn falls_back_to_subject_when_headers_missing() {
        let candidates = vec![
            ThreadCandidate {
                message_row_id: 1,
                message_id_header: Some("<a@example.com>".to_string()),
                in_reply_to: None,
                references: vec![],
                subject: Some("Project Update".to_string()),
            },
            ThreadCandidate {
                message_row_id: 2,
                message_id_header: Some("<b@example.com>".to_string()),
                in_reply_to: None,
                references: vec![],
                subject: Some("Re: Project Update".to_string()),
            },
        ];

        let assignments = assign_threads(&candidates);
        assert_eq!(assignments[0].thread_key, assignments[1].thread_key);
    }

    #[test]
    fn uses_references_before_subject_fallback() {
        let candidates = vec![
            ThreadCandidate {
                message_row_id: 1,
                message_id_header: Some("<root@example.com>".to_string()),
                in_reply_to: None,
                references: vec![],
                subject: Some("Hello".to_string()),
            },
            ThreadCandidate {
                message_row_id: 2,
                message_id_header: Some("<mid@example.com>".to_string()),
                in_reply_to: Some("<root@example.com>".to_string()),
                references: vec!["<root@example.com>".to_string()],
                subject: Some("Re: Hello".to_string()),
            },
            ThreadCandidate {
                message_row_id: 3,
                message_id_header: Some("<reply@example.com>".to_string()),
                in_reply_to: None,
                references: vec![
                    "<root@example.com>".to_string(),
                    "<mid@example.com>".to_string(),
                ],
                subject: Some("Different Subject".to_string()),
            },
        ];

        let assignments = assign_threads(&candidates);
        assert_eq!(assignments[0].thread_key, assignments[1].thread_key);
        assert_eq!(assignments[0].thread_key, assignments[2].thread_key);
    }

    #[test]
    fn falls_back_to_in_reply_to_when_references_missing() {
        let candidates = vec![
            ThreadCandidate {
                message_row_id: 1,
                message_id_header: Some("<root@example.com>".to_string()),
                in_reply_to: None,
                references: vec![],
                subject: Some("Status".to_string()),
            },
            ThreadCandidate {
                message_row_id: 2,
                message_id_header: Some("<reply@example.com>".to_string()),
                in_reply_to: Some("<root@example.com>".to_string()),
                references: vec![],
                subject: Some("Totally different".to_string()),
            },
        ];

        let assignments = assign_threads(&candidates);
        assert_eq!(assignments[0].thread_key, assignments[1].thread_key);
    }

    #[test]
    fn normalize_subject_strips_common_prefixes() {
        assert_eq!(normalize_subject("Re: Fwd: Hello"), "Hello");
        assert_eq!(
            normalize_subject("  RE2: fw: Project Update  "),
            "Project Update"
        );
    }
}
