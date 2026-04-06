use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget, Wrap},
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

    render_filter_rail(frame, filter_area, theme, &state.filter);

    if state.threads.is_empty() {
        let empty = Paragraph::new("No threads — run cour reindex")
            .style(theme.empty_state)
            .block(Block::default().borders(Borders::NONE))
            .wrap(Wrap { trim: true });
        frame.render_widget(empty, list_area);
        return;
    }

    render_thread_list(frame, list_area, theme, state);
}

fn render_filter_rail(frame: &mut Frame, area: Rect, theme: &Theme, filter: &Option<String>) {
    let filter_items: Vec<ListItem> = FILTERS
        .iter()
        .map(|(label, filter_val)| {
            let active = match (filter, filter_val) {
                (None, None) => true,
                (Some(f), Some(v)) => f.as_str() == *v,
                _ => false,
            };
            let style = if active {
                theme.workspace_active
            } else {
                theme.workspace_inactive
            };
            ListItem::new(Line::from(Span::styled(format!(" {label}"), style)))
        })
        .collect();
    let filter_list = List::new(filter_items).block(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(theme.border)
            .title(" Filter "),
    );
    frame.render_widget(filter_list, area);
}

fn render_thread_list<W>(target: &mut W, area: Rect, theme: &Theme, state: &InboxState)
where
    W: WidgetRef,
{
    let items: Vec<ListItem> = state
        .threads
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let row_style = if i == state.selected {
                theme.thread_selected
            } else {
                theme.text
            };
            let tag_style = if i == state.selected {
                theme
                    .thread_selected
                    .patch(thread_state_style(theme, row.state.as_deref()))
            } else {
                theme
                    .thread_tag
                    .patch(thread_state_style(theme, row.state.as_deref()))
            };
            let subject_style = if i == state.selected {
                theme.thread_selected.patch(theme.thread_subject)
            } else {
                theme.thread_subject
            };
            let meta_style = if i == state.selected {
                theme.thread_selected.patch(theme.thread_meta)
            } else {
                theme.thread_meta
            };

            let state_tag = format_state_tag(row.state.as_deref());
            let subject = row.subject.as_deref().unwrap_or("(no subject)");
            let count = if row.message_count > 1 {
                format!("{} msgs", row.message_count)
            } else {
                "1 msg".to_string()
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {state_tag:<12} "), tag_style),
                Span::styled(subject.to_string(), subject_style),
                Span::styled(format!("  {count}"), meta_style),
            ]))
            .style(row_style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::NONE).title(" Inbox "));
    target.render_widget(list, area);
}

fn format_state_tag(state: Option<&str>) -> &'static str {
    match state {
        Some("waiting_on_me") => "Needs reply",
        Some("waiting_on_them") => "Waiting",
        Some("low_value") => "Low value",
        Some("urgent") => "Urgent",
        Some("follow_up_due") => "Follow-up",
        _ => "Open",
    }
}

fn thread_state_style(theme: &Theme, state: Option<&str>) -> Style {
    match state {
        Some("waiting_on_me") => theme.thread_needs_reply,
        Some("waiting_on_them") => theme.thread_waiting,
        Some("low_value") => theme.thread_low_value,
        _ => theme.text_muted,
    }
}

trait WidgetRef {
    fn render_widget(&mut self, widget: List<'_>, area: Rect);
}

impl WidgetRef for Frame<'_> {
    fn render_widget(&mut self, widget: List<'_>, area: Rect) {
        Frame::render_widget(self, widget, area);
    }
}

impl WidgetRef for Buffer {
    fn render_widget(&mut self, widget: List<'_>, area: Rect) {
        Widget::render(widget, area, self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_selected_row_uses_selection_style() {
        let theme = Theme::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 3));
        let state = InboxState {
            threads: vec![ThreadListRow {
                id: 42,
                subject: Some("Follow up with ops".to_string()),
                state: Some("waiting_on_me".to_string()),
                message_count: 3,
                latest_message_at: None,
            }],
            selected: 0,
            filter: None,
        };

        render_thread_list(&mut buffer, Rect::new(0, 0, 60, 3), &theme, &state);

        let selected_cell = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "F")
            .expect("selected row content")
            .style();
        assert_eq!(selected_cell.bg, theme.thread_selected.bg);

        let tag_cell = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "N")
            .expect("state tag cell")
            .style();
        assert_eq!(tag_cell.bg, theme.thread_selected.bg);

        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Needs reply"));
        assert!(rendered.contains("Follow up with ops"));
        assert!(rendered.contains("3 msgs"));
    }
}
