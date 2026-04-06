use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::index::query::{list_threads_by_state, pending_drafts_count, ThreadListRow};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionsState {
    pub needs_reply: Vec<ThreadListRow>,
    pub waiting_on_them: Vec<ThreadListRow>,
    pub pending_drafts: i64,
    pub selected_section: usize,
    pub selected_row: usize,
}

impl ActionsState {
    pub fn refresh(&mut self, conn: &Connection) -> rusqlite::Result<()> {
        self.needs_reply = list_threads_by_state(conn, "waiting_on_me")?;
        self.waiting_on_them = list_threads_by_state(conn, "waiting_on_them")?;
        self.pending_drafts = pending_drafts_count(conn)?;
        self.clamp_selection();
        Ok(())
    }

    pub fn move_section(&mut self, delta: isize) {
        let next = (self.selected_section as isize + delta).rem_euclid(3) as usize;
        self.selected_section = next;
        self.clamp_selection();
    }

    pub fn move_row(&mut self, delta: isize) {
        let len = self.selected_rows().len();
        if len == 0 {
            self.selected_row = 0;
            return;
        }
        let next = (self.selected_row as isize + delta).clamp(0, (len - 1) as isize) as usize;
        self.selected_row = next;
    }

    pub fn selected_thread_id(&self) -> Option<i64> {
        self.selected_rows()
            .get(self.selected_row)
            .map(|row| row.id)
    }

    fn selected_rows(&self) -> &[ThreadListRow] {
        match self.selected_section {
            0 => &self.needs_reply,
            1 => &self.waiting_on_them,
            _ => &[],
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.selected_rows().len();
        if len == 0 {
            self.selected_row = 0;
        } else if self.selected_row >= len {
            self.selected_row = len - 1;
        }
    }
}

pub fn render_actions(frame: &mut Frame, area: Rect, theme: &Theme, state: &ActionsState) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(42),
            Constraint::Percentage(42),
            Constraint::Length(5),
        ])
        .split(area);

    render_thread_section(
        frame,
        sections[0],
        "Needs Reply",
        state.selected_section == 0,
        theme.thread_selected,
        &state.needs_reply,
        state.selected_row,
    );
    render_thread_section(
        frame,
        sections[1],
        "Waiting on Them",
        state.selected_section == 1,
        theme.selection,
        &state.waiting_on_them,
        state.selected_row,
    );

    let drafts_title = if state.selected_section == 2 {
        " Pending Drafts * "
    } else {
        " Pending Drafts "
    };
    let drafts = Paragraph::new(format!("Pending Drafts: {}", state.pending_drafts))
        .style(if state.selected_section == 2 {
            theme.text
        } else {
            theme.text_dim
        })
        .block(Block::default().borders(Borders::ALL).title(drafts_title))
        .wrap(Wrap { trim: true });
    frame.render_widget(drafts, sections[2]);
}

fn render_thread_section(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    active: bool,
    selected_style: Style,
    rows: &[ThreadListRow],
    selected_row: usize,
) {
    let block = Block::default().borders(Borders::ALL).title(if active {
        format!(" {title} * ")
    } else {
        format!(" {title} ")
    });

    if rows.is_empty() {
        let empty = Paragraph::new("— none")
            .style(Style::default())
            .block(block)
            .wrap(Wrap { trim: true });
        frame.render_widget(empty, area);
        return;
    }

    let items = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let style = if active && index == selected_row {
                selected_style
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![Span::raw(format_thread_row(row))])).style(style)
        })
        .collect::<Vec<_>>();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn format_thread_row(row: &ThreadListRow) -> String {
    format!(
        "[{}] {}  ({} messages, {})",
        row.id,
        row.subject.as_deref().unwrap_or("(no subject)"),
        row.message_count,
        format_age(row.latest_message_at.as_deref())
    )
}

fn format_age(latest_message_at: Option<&str>) -> String {
    latest_message_at
        .map(|value| value.replace('T', " ").chars().take(16).collect())
        .unwrap_or_else(|| "unknown age".to_string())
}
