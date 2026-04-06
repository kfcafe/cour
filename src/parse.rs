use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::UNIX_EPOCH;

use mail_parser::{Addr, Address, HeaderValue, MessageParser};

use crate::error::{AppError, AppResult};
use crate::model::{ParsedAddress, ParsedMessage};

pub fn parse_message_file(path: &Path) -> AppResult<ParsedMessage> {
    let raw = fs::read(path).map_err(AppError::Io)?;
    let metadata = fs::metadata(path).map_err(AppError::Io)?;
    let file_mtime = metadata
        .modified()
        .map_err(AppError::Io)?
        .duration_since(UNIX_EPOCH)
        .map_err(|err| AppError::Parsing(err.to_string()))?
        .as_secs() as i64;

    let message = MessageParser::default()
        .parse(&raw)
        .ok_or_else(|| AppError::Parsing(format!("failed to parse message {}", path.display())))?;

    let subject = message
        .subject()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let from = parse_first_address(message.from());
    let to = parse_addresses(message.to());
    let cc = parse_addresses(message.cc());
    let body_text = message
        .body_text(0)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let body_html = message.body_html(0).map(|s| s.into_owned());
    let snippet = make_snippet(&body_text, body_html.as_deref());

    Ok(ParsedMessage {
        file_path: path.to_path_buf(),
        message_id_header: message
            .message_id()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned),
        in_reply_to: header_value_as_single_text(message.in_reply_to()),
        references: header_value_as_text_list(message.references()),
        subject,
        from,
        to,
        cc,
        sent_at: message.date().map(|dt| dt.to_rfc3339()),
        body_text,
        body_html,
        snippet,
        parse_hash: compute_parse_hash(&raw),
        file_mtime,
    })
}

fn parse_first_address(header: Option<&Address<'_>>) -> Option<ParsedAddress> {
    parse_addresses(header).into_iter().next()
}

fn parse_addresses(header: Option<&Address<'_>>) -> Vec<ParsedAddress> {
    let mut result = Vec::new();
    let Some(header) = header else {
        return result;
    };

    match header {
        Address::List(items) => {
            for addr in items {
                flatten_addr(addr, &mut result);
            }
        }
        Address::Group(groups) => {
            for group in groups {
                for addr in &group.addresses {
                    flatten_addr(addr, &mut result);
                }
            }
        }
    }

    result
}

fn flatten_addr(addr: &Addr<'_>, out: &mut Vec<ParsedAddress>) {
    let Some(email) = addr.address.as_ref() else {
        return;
    };

    out.push(ParsedAddress {
        display_name: addr
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned),
        email: email.as_ref().to_string(),
    });
}

fn header_value_as_single_text(value: &HeaderValue<'_>) -> Option<String> {
    match value {
        HeaderValue::Text(text) => {
            let trimmed = text.as_ref().trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        HeaderValue::TextList(values) => values
            .iter()
            .map(|v| v.as_ref().trim())
            .find(|s| !s.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn header_value_as_text_list(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::TextList(values) => values
            .iter()
            .map(|v| v.as_ref().trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        HeaderValue::Text(text) => text
            .split_whitespace()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn make_snippet(body_text: &str, body_html: Option<&str>) -> String {
    let source = if body_text.trim().is_empty() {
        body_html.unwrap_or_default()
    } else {
        body_text
    };

    source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

fn compute_parse_hash(raw: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::parse_message_file;

    fn write_temp_message(name: &str, raw: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("cour-{name}-{unique}.eml"));
        fs::write(&path, raw).expect("write temp message");
        path
    }

    #[test]
    fn parses_html_only_message() {
        let path = write_temp_message(
            "html-only",
            "From: Alice <alice@example.com>\nTo: Bob <bob@example.com>\nSubject: Hello\nMessage-ID: <msg-1@example.com>\nDate: Tue, 11 Mar 2026 10:00:00 +0000\nContent-Type: text/html; charset=UTF-8\n\n<p>Hello <b>Bob</b></p>",
        );

        let parsed = parse_message_file(&path).expect("parse html-only message");
        assert_eq!(parsed.subject.as_deref(), Some("Hello"));
        assert_eq!(
            parsed.from.as_ref().map(|a| a.email.as_str()),
            Some("alice@example.com")
        );
        assert_eq!(parsed.to.len(), 1);
        assert!(parsed
            .body_html
            .as_deref()
            .unwrap_or_default()
            .contains("Hello"));
        assert!(!parsed.parse_hash.is_empty());

        let _ = fs::remove_file(path);
    }
}
