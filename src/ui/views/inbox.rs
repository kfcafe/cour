use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::index::query::{list_recent_threads, list_threads_by_state, ThreadListRow};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct InboxState {
    pub threads: Vec<ThreadListRow>,
    pub selected: usize,
    pub filter: Option<String>,
}

impl InboxState {
    pub fn refresh(&mut self, conn: &Connection) -> rusqlite::Result<()> {
        self.threads = match &self.filter {
            Some(state) => list_threads_by_state(conn, state)?,
            None => list_recent_threads(conn, 100)?,
        };
        self.clamp();
        Ok(())
    }

    pub fn next(&mut self) {
        if !self.threads.is_empty() {
            self.selected = (self.selected + 1).min(self.threads.len() - 1);
        }
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_thread_id(&self) -> Option<i64> {
        self.threads.get(self.selected).map(|r| r.id)
    }

    fn clamp(&mut self) {
        if self.threads.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.threads.len() {
            self.selected = self.threads.len() - 1;
        }
    }
}

const FILTERS: &[(&str, Option<&str>)] = &[
    ("All", None),
    ("Needs Reply", Some("waiting_on_me")),
    ("Waiting", Some("waiting_on_them")),
    ("Low Value", Some("low_value")),
];

pub fn render_inbox(frame: &mut Frame, area: Rect, theme: &Theme, state: &InboxState) {
    let [filter_area, list_area] =
        ratatui::layout::Layout::horizontal([Constraint::Length(16), Constraint::Min(0)])
            .areas(area);

    // filter rail
    let filter_items: Vec<ListItem> = FILTERS
        .iter()
        .map(|(label, filter_val)| {
            let active = match (&state.filter, filter_val) {
                (None, None) => true,
                (Some(f), Some(v)) => f.as_str() == *v,
                _ => false,
            };
            let style = if active {
                theme.workspace_active
            } else {
                theme.text_muted
            };
            ListItem::new(Line::from(Span::styled(format!(" {label}"), style)))
        })
        .collect();
    let filter_list =
        List::new(filter_items).block(Block::default().borders(Borders::RIGHT).title(" Filter "));
    frame.render_widget(filter_list, filter_area);

    // thread list
    if state.threads.is_empty() {
        let empty = Paragraph::new("No threads — run cour reindex")
            .style(theme.text_dim)
            .block(Block::default().borders(Borders::NONE))
            .wrap(Wrap { trim: true });
        frame.render_widget(empty, list_area);
        return;
    }

    let items: Vec<ListItem> = state
        .threads
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let style = if i == state.selected {
                theme.thread_selected
            } else {
                Style::default()
            };
            let state_tag = row.state.as_deref().unwrap_or("?");
            let subject = row.subject.as_deref().unwrap_or("(no subject)");
            let count = if row.message_count > 1 {
                format!(" ({})", row.message_count)
            } else {
                String::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {state_tag:<14} "),
                    thread_state_style(theme, row.state.as_deref()),
                ),
                Span::styled(subject.to_string(), theme.text),
                Span::styled(count, theme.text_dim),
            ]))
            .style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::NONE).title(" Inbox "));
    frame.render_widget(list, list_area);
}

fn thread_state_style(theme: &Theme, state: Option<&str>) -> Style {
    match state {
        Some("waiting_on_me") => theme.thread_needs_reply,
        Some("waiting_on_them") => theme.thread_waiting,
        Some("low_value") => theme.thread_low_value,
        _ => theme.text_muted,
    }
}
