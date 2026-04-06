use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAddress {
    pub display_name: Option<String>,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedParticipant {
    pub role: String,
    pub display_name: Option<String>,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMessage {
    pub file_path: PathBuf,
    pub message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: Option<String>,
    pub from: Option<ParsedAddress>,
    pub to: Vec<ParsedAddress>,
    pub cc: Vec<ParsedAddress>,
    pub sent_at: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
    pub snippet: String,
    pub parse_hash: String,
    pub file_mtime: i64,
}

impl ParsedMessage {
    pub fn participants(&self) -> Vec<ParsedParticipant> {
        let mut participants = Vec::new();

        if let Some(from) = &self.from {
            participants.push(ParsedParticipant {
                role: "from".to_string(),
                display_name: from.display_name.clone(),
                email: from.email.clone(),
            });
        }

        participants.extend(self.to.iter().map(|addr| ParsedParticipant {
            role: "to".to_string(),
            display_name: addr.display_name.clone(),
            email: addr.email.clone(),
        }));

        participants.extend(self.cc.iter().map(|addr| ParsedParticipant {
            role: "cc".to_string(),
            display_name: addr.display_name.clone(),
            email: addr.email.clone(),
        }));

        participants
    }
}
