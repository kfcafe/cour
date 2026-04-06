use ratatui::{
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};
use rusqlite::Connection;

use crate::index::query::{ask_search, AskSearchResult, LexicalSearchFilters};
use crate::ui::theme::Theme;

#[derive(Debug, Default, Clone)]
pub struct SearchState {
    pub query: String,
    pub editing: bool,
    pub results: Vec<AskSearchResult>,
    pub selected: usize,
}

impl SearchState {
    pub fn execute(&mut self, conn: &Connection) -> rusqlite::Result<()> {
        self.results = ask_search(
            conn,
            &self.query,
            &LexicalSearchFilters::default(),
            None,
            20,
        )?;
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
        Ok(())
    }

    pub fn move_down(&mut self) {
        if self.results.is_empty() {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1).min(self.results.len() - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_thread_id(&self) -> Option<i64> {
        self.results
            .get(self.selected)
            .map(|result| result.thread_id)
    }
}

pub fn render_search(frame: &mut Frame, area: Rect, theme: &Theme, state: &SearchState) {
    let vertical =
        ratatui::layout::Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let query_text = if state.query.is_empty() {
        if state.editing {
            " ".to_string()
        } else {
            "Type a query…".to_string()
        }
    } else {
        state.query.clone()
    };

    let query = Paragraph::new(Line::from(vec![
        Span::styled("/ ", theme.text_dim),
        Span::raw(query_text),
        if state.editing {
            Span::raw("█")
        } else {
            Span::raw("")
        },
    ]))
    .block(Block::default().borders(Borders::ALL).title("Search"));
    frame.render_widget(query, vertical[0]);

    if state.results.is_empty() {
        let empty = Paragraph::new("Type a query and press Enter to search")
            .style(theme.text_dim)
            .block(Block::default().borders(Borders::ALL).title("Results"));
        frame.render_widget(Clear, vertical[1]);
        frame.render_widget(empty, vertical[1]);
        return;
    }

    let rows = state.results.iter().enumerate().map(|(index, result)| {
        let style = if index == state.selected {
            theme.thread_selected
        } else {
            ratatui::style::Style::default()
        };

        Row::new(vec![
            Cell::from(format!("{}", index + 1)),
            Cell::from(
                result
                    .subject
                    .as_deref()
                    .unwrap_or("(no subject)")
                    .to_string(),
            ),
            Cell::from(truncate_snippet(&result.evidence_snippet, 72)),
            Cell::from(format!("{:.3}", result.score.blended_score)),
        ])
        .style(style)
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Percentage(28),
            Constraint::Percentage(52),
            Constraint::Length(8),
        ],
    )
    .header(Row::new(vec!["#", "Subject", "Evidence", "Score"]).style(theme.text_dim))
    .block(Block::default().borders(Borders::ALL).title("Results"))
    .column_spacing(1);
    frame.render_widget(table, vertical[1]);
}

fn truncate_snippet(snippet: &str, max_chars: usize) -> String {
    let normalized = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let truncated = normalized
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
}
