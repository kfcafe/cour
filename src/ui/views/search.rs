use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Widget},
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
    render_search_widget(area, frame.buffer_mut(), theme, state);
}

fn render_search_widget(area: Rect, buf: &mut Buffer, theme: &Theme, state: &SearchState) {
    let vertical =
        ratatui::layout::Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let query_text = if state.query.is_empty() {
        if state.editing {
            "Start typing a query".to_string()
        } else {
            "Press / or Enter to search".to_string()
        }
    } else {
        state.query.clone()
    };

    let query_title = if state.editing {
        "Search · editing"
    } else if state.results.is_empty() {
        "Search · idle"
    } else {
        "Search · results"
    };

    let query = Paragraph::new(Line::from(vec![
        Span::styled(
            "/ ",
            if state.editing {
                theme.text_accent
            } else {
                theme.text_dim
            },
        ),
        Span::styled(
            query_text,
            if state.query.is_empty() {
                theme.text_dim
            } else {
                theme.text
            },
        ),
        if state.editing {
            Span::styled(" █", theme.text_accent)
        } else {
            Span::raw("")
        },
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(query_title)
            .border_style(if state.editing {
                theme.border_focused
            } else {
                theme.border
            }),
    );
    query.render(vertical[0], buf);

    if state.query.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(Span::styled(
                "Type a few words to find matching threads.",
                theme.text,
            )),
            Line::from(Span::styled(
                "Press / or Enter to edit the query, then press Enter again to run it.",
                theme.text_dim,
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title("Empty state"))
        .style(theme.text_dim);
        Clear.render(vertical[1], buf);
        empty.render(vertical[1], buf);
        return;
    }

    if state.results.is_empty() {
        let no_results = Paragraph::new(vec![
            Line::from(Span::styled("No results yet.", theme.text)),
            Line::from(Span::styled(
                "Press Enter to run this query or keep refining it.",
                theme.text_dim,
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title("Results"));
        Clear.render(vertical[1], buf);
        no_results.render(vertical[1], buf);
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
    table.render(vertical[1], buf);
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

#[cfg(test)]
mod tests {
    use super::{render_search_widget, SearchState};
    use crate::ui::theme::Theme;
    use ratatui::{buffer::Buffer, layout::Rect};

    #[test]
    fn search_empty_state_prompts_for_query() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 10));
        let theme = Theme::default();
        let state = SearchState::default();

        render_search_widget(buffer.area, &mut buffer, &theme, &state);

        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Press / or Enter to search"));
        assert!(rendered.contains("Type a few words to find matching threads."));
    }
}
