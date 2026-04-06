use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::index::query::{list_drafts_for_review, DraftReviewRow};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct DraftsState {
    pub drafts: Vec<DraftReviewRow>,
    pub selected: usize,
}

impl DraftsState {
    pub fn refresh(&mut self, conn: &Connection) -> rusqlite::Result<()> {
        self.drafts = list_draft_reviews(conn)?;
        if self.drafts.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.drafts.len().saturating_sub(1));
        }
        Ok(())
    }

    pub fn move_next(&mut self) {
        if !self.drafts.is_empty() {
            self.selected = (self.selected + 1).min(self.drafts.len() - 1);
        }
    }

    pub fn move_previous(&mut self) {
        if !self.drafts.is_empty() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    pub fn selected_draft(&self) -> Option<&DraftReviewRow> {
        self.drafts.get(self.selected)
    }
}

pub fn list_draft_reviews(conn: &Connection) -> rusqlite::Result<Vec<DraftReviewRow>> {
    list_drafts_for_review(conn)
}

pub fn render_drafts(frame: &mut Frame, area: Rect, theme: &Theme, state: &DraftsState) {
    if state.drafts.is_empty() {
        let empty = Paragraph::new("No drafts — use cour draft <thread_id> to generate one")
            .style(theme.text_dim)
            .block(Block::default().borders(Borders::ALL).title(" Drafts "))
            .wrap(Wrap { trim: true });
        frame.render_widget(empty, area);
        return;
    }

    let [list_area, preview_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .areas(area);

    let items = state
        .drafts
        .iter()
        .enumerate()
        .map(|(index, draft)| {
            let selected = index == state.selected;
            let status_style = draft_status_style(theme, draft.approval_status.as_str());
            let subject = draft
                .thread_id
                .map(|thread_id| format!("thread #{thread_id}"))
                .unwrap_or_else(|| "standalone draft".to_string());
            let provider_model = provider_model_label(draft);
            let updated_at = compact_timestamp(&draft.updated_at);
            let next_step = draft_next_step(draft);

            let style = if selected {
                theme.selection
            } else {
                Style::default()
            };
            let body = vec![
                Line::from(vec![
                    Span::styled(format!("[{}] ", draft.id), theme.text_dim),
                    Span::styled(
                        format!("{:<8}", draft.approval_status),
                        status_style.add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(subject, theme.thread_subject),
                ]),
                Line::from(vec![
                    Span::styled("  model ", theme.text_dim),
                    Span::styled(provider_model, theme.text_muted),
                    Span::raw("  "),
                    Span::styled("updated ", theme.text_dim),
                    Span::styled(updated_at, theme.text_muted),
                ]),
                Line::from(vec![
                    Span::styled("  next ", theme.text_dim),
                    Span::styled(next_step, draft_next_step_style(theme, draft)),
                ]),
            ];

            ListItem::new(body).style(style)
        })
        .collect::<Vec<_>>();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(" Drafts "));
    frame.render_widget(list, list_area);

    let selected = state.selected_draft().expect("draft exists when non-empty");
    let preview = build_preview_text(selected, theme);
    let preview_widget = Paragraph::new(preview)
        .block(Block::default().borders(Borders::ALL).title(" Preview "))
        .wrap(Wrap { trim: false });
    frame.render_widget(preview_widget, preview_area);
}

fn build_preview_text(draft: &DraftReviewRow, theme: &Theme) -> Text<'static> {
    let provider = draft.provider.as_deref().unwrap_or("-");
    let model = draft.model.as_deref().unwrap_or("-");
    let latest_event = draft
        .latest_send_audit_at
        .as_deref()
        .or(draft.latest_approval_audit_at.as_deref())
        .or(draft.latest_audit_at.as_deref())
        .unwrap_or("-");

    let lines = vec![
        Line::from(vec![
            Span::styled("status  ", theme.text_dim),
            Span::styled(
                draft.approval_status.clone(),
                draft_status_style(theme, draft.approval_status.as_str())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("next  ", theme.text_dim),
            Span::styled(draft_next_step(draft), draft_next_step_style(theme, draft)),
        ]),
        Line::from(vec![
            Span::styled("source  ", theme.text_dim),
            Span::raw(draft.source.clone()),
            Span::raw("  "),
            Span::styled("thread  ", theme.text_dim),
            Span::raw(
                draft
                    .thread_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled("provider  ", theme.text_dim),
            Span::styled(provider.to_string(), theme.text_accent),
            Span::raw("  "),
            Span::styled("model  ", theme.text_dim),
            Span::raw(model.to_string()),
        ]),
        Line::from(vec![
            Span::styled("created  ", theme.text_dim),
            Span::raw(compact_timestamp(&draft.created_at)),
            Span::raw("  "),
            Span::styled("updated  ", theme.text_dim),
            Span::raw(compact_timestamp(&draft.updated_at)),
        ]),
        Line::from(vec![
            Span::styled("approved ", theme.text_dim),
            Span::raw(compact_optional_timestamp(draft.approved_at.as_deref())),
            Span::raw("  "),
            Span::styled("sent  ", theme.text_dim),
            Span::raw(compact_optional_timestamp(draft.sent_at.as_deref())),
        ]),
        Line::from(vec![
            Span::styled("rationale ", theme.text_dim),
            Span::raw(if draft.has_rationale {
                "present"
            } else {
                "missing"
            }),
            Span::raw("  "),
            Span::styled("latest audit  ", theme.text_dim),
            Span::raw(compact_optional_timestamp(Some(latest_event))),
        ]),
        Line::default(),
        Line::from(Span::styled("Safety", theme.title)),
        Line::from(Span::styled(
            "Nothing sends from the TUI. Review in CLI before approving or sending.",
            theme.warning,
        )),
        Line::default(),
        Line::from(Span::styled("Body", theme.title)),
        Line::from(Span::styled(
            "Draft body is intentionally hidden here to keep review metadata-first.",
            theme.text_dim,
        )),
    ];

    Text::from(lines)
}

fn draft_status_style(theme: &Theme, status: &str) -> Style {
    match status {
        "approved" => theme.text_success,
        "sent" => theme.text_muted,
        "pending" => theme.text_warning,
        _ => theme.text_dim,
    }
}

fn draft_next_step(draft: &DraftReviewRow) -> &'static str {
    match draft.approval_status.as_str() {
        "pending" => "review before approval",
        "approved" => "ready to send from CLI",
        "sent" => "already sent; audit only",
        _ => "review state unknown",
    }
}

fn draft_next_step_style(theme: &Theme, draft: &DraftReviewRow) -> Style {
    match draft.approval_status.as_str() {
        "pending" => theme.text_warning,
        "approved" => theme.text_success,
        "sent" => theme.text_muted,
        _ => theme.text_dim,
    }
}

fn provider_model_label(draft: &DraftReviewRow) -> String {
    match (draft.provider.as_deref(), draft.model.as_deref()) {
        (Some(provider), Some(model)) => format!("{provider}/{model}"),
        (Some(provider), None) => provider.to_string(),
        (None, Some(model)) => model.to_string(),
        (None, None) => draft.source.clone(),
    }
}

fn compact_timestamp(value: &str) -> String {
    value.replace('T', " ").chars().take(16).collect()
}

fn compact_optional_timestamp(value: Option<&str>) -> String {
    value
        .map(compact_timestamp)
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    use super::*;

    #[test]
    fn draft_preview_shows_status_and_provider_metadata() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();
        let state = DraftsState {
            drafts: vec![DraftReviewRow {
                id: 42,
                thread_id: Some(9),
                status: "approved".to_string(),
                source: "ai".to_string(),
                provider: Some("openai-compatible".to_string()),
                model: Some("gpt-4o-mini".to_string()),
                has_rationale: true,
                approval_status: "approved".to_string(),
                created_at: "2026-04-06T10:15:00Z".to_string(),
                updated_at: "2026-04-06T10:20:00Z".to_string(),
                approved_at: Some("2026-04-06T10:22:00Z".to_string()),
                sent_at: None,
                latest_audit_at: Some("2026-04-06T10:22:00Z".to_string()),
                latest_approval_audit_at: Some("2026-04-06T10:22:00Z".to_string()),
                latest_send_audit_at: None,
            }],
            selected: 0,
        };

        terminal
            .draw(|frame| render_drafts(frame, frame.area(), &theme, &state))
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        assert_contains(&buffer, "approved");
        assert_contains(&buffer, "openai-compatible");
        assert_contains(&buffer, "gpt-4o-mini");
        assert_contains(&buffer, "ready to send from CLI");
        assert_contains(&buffer, "Nothing sends from the TUI. Review in CLI");
        assert_contains(&buffer, "before approving or sending.");
    }

    fn assert_contains(buffer: &Buffer, needle: &str) {
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(
            rendered.contains(needle),
            "expected buffer to contain {needle:?}, got: {rendered:?}"
        );
    }
}
