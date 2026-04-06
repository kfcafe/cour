use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
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
            Constraint::Length(6),
        ])
        .split(area);

    render_thread_section(
        frame,
        sections[0],
        theme,
        "Needs Reply",
        "Threads that still need your answer.",
        state.selected_section == 0,
        theme.thread_selected,
        &state.needs_reply,
        state.selected_row,
    );
    render_thread_section(
        frame,
        sections[1],
        theme,
        "Waiting on Them",
        "Threads already handed off to someone else.",
        state.selected_section == 1,
        theme.selection,
        &state.waiting_on_them,
        state.selected_row,
    );

    let drafts_block = section_block(theme, "Pending Drafts", state.selected_section == 2);
    let drafts_lines = vec![
        Line::from(vec![
            Span::styled("count  ", theme.text_dim),
            Span::styled(state.pending_drafts.to_string(), theme.count_badge),
        ]),
        Line::from(vec![
            Span::styled("safety ", theme.text_dim),
            Span::styled("drafts stay local until you send from CLI", theme.warning),
        ]),
    ];
    let drafts = Paragraph::new(drafts_lines)
        .style(if state.selected_section == 2 {
            theme.text
        } else {
            theme.text_muted
        })
        .block(drafts_block)
        .wrap(Wrap { trim: true });
    frame.render_widget(drafts, sections[2]);
}

fn render_thread_section(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    subtitle: &str,
    active: bool,
    selected_style: Style,
    rows: &[ThreadListRow],
    selected_row: usize,
) {
    let block = section_block(theme, title, active);

    if rows.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(Span::styled(subtitle, theme.text_dim)),
            Line::default(),
            Line::from(Span::styled("— none", theme.empty_state)),
        ])
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
            let lines = vec![
                Line::from(vec![Span::styled(
                    format_thread_subject(row),
                    if active && index == selected_row {
                        theme.thread_subject.add_modifier(Modifier::BOLD)
                    } else {
                        theme.thread_subject
                    },
                )]),
                Line::from(vec![
                    Span::styled(format!("  #{}", row.id), theme.text_dim),
                    Span::raw("  "),
                    Span::styled(format!("{} messages", row.message_count), theme.text_muted),
                    Span::raw("  "),
                    Span::styled(format_age(row.latest_message_at.as_deref()), theme.text_dim),
                ]),
            ];
            ListItem::new(lines).style(style)
        })
        .collect::<Vec<_>>();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn section_block(theme: &Theme, title: &str, active: bool) -> Block<'static> {
    let title_style = if active {
        theme.border_focused.add_modifier(Modifier::BOLD)
    } else {
        theme.border
    };

    Block::default()
        .borders(Borders::ALL)
        .border_style(if active {
            theme.border_focused
        } else {
            theme.border
        })
        .title(Line::from(vec![
            Span::styled(if active { "● " } else { "○ " }, title_style),
            Span::styled(title.to_string(), title_style),
        ]))
}

fn format_thread_subject(row: &ThreadListRow) -> String {
    row.subject.as_deref().unwrap_or("(no subject)").to_string()
}

fn format_age(latest_message_at: Option<&str>) -> String {
    latest_message_at
        .map(|value| value.replace('T', " ").chars().take(16).collect())
        .unwrap_or_else(|| "unknown age".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_selected_section_cycles_with_tab() {
        let mut state = ActionsState::default();

        state.move_section(1);
        assert_eq!(state.selected_section, 1);

        state.move_section(1);
        assert_eq!(state.selected_section, 2);

        state.move_section(1);
        assert_eq!(state.selected_section, 0);

        state.move_section(-1);
        assert_eq!(state.selected_section, 2);
    }
}
