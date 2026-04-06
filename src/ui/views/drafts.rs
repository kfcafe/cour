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
            let status_style = draft_status_style(theme, draft.approval_status.as_str());
            let subject = draft
                .thread_id
                .map(|thread_id| format!("thread #{thread_id}"))
                .unwrap_or_else(|| "standalone draft".to_string());
            let provider_model = match (draft.provider.as_deref(), draft.model.as_deref()) {
                (Some(provider), Some(model)) => format!("{provider}/{model}"),
                (Some(provider), None) => provider.to_string(),
                (None, Some(model)) => model.to_string(),
                (None, None) => draft.source.clone(),
            };
            let created_at = compact_timestamp(&draft.created_at);
            let line = Line::from(vec![
                Span::styled(format!("[{}] ", draft.id), theme.text_dim),
                Span::styled(
                    format!("{:<8}", draft.approval_status),
                    status_style.add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(subject, theme.text),
                Span::raw("  "),
                Span::styled(provider_model, theme.text_dim),
                Span::raw("  "),
                Span::styled(created_at, theme.text_dim),
            ]);

            let style = if index == state.selected {
                theme.selection
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
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
    let mut lines = vec![
        Line::from(vec![
            Span::styled("status: ", theme.text_dim),
            Span::styled(
                draft.approval_status.clone(),
                draft_status_style(theme, draft.approval_status.as_str())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("thread: ", theme.text_dim),
            Span::raw(
                draft
                    .thread_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ]),
        Line::from(vec![Span::styled("to: ", theme.text_dim), Span::raw("-")]),
        Line::from(vec![
            Span::styled("provider: ", theme.text_dim),
            Span::raw(draft.provider.clone().unwrap_or_else(|| "-".to_string())),
        ]),
        Line::from(vec![
            Span::styled("model: ", theme.text_dim),
            Span::raw(draft.model.clone().unwrap_or_else(|| "-".to_string())),
        ]),
        Line::from(vec![
            Span::styled("rationale: ", theme.text_dim),
            Span::raw(if draft.has_rationale { "yes" } else { "no" }),
        ]),
        Line::default(),
        Line::from(Span::styled("Body", theme.text)),
        Line::from(Span::styled("────", theme.text_dim)),
    ];

    let body = "Draft body unavailable".to_string();
    lines.extend(body.lines().map(|line| Line::from(line.to_string())));
    Text::from(lines)
}

fn draft_status_style(theme: &Theme, status: &str) -> Style {
    let _ = status;
    theme.text_dim
}

fn compact_timestamp(value: &str) -> String {
    value.replace('T', " ").chars().take(16).collect()
}
