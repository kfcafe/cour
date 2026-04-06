use ratatui::{
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::index::query::{get_thread_detail, ThreadDetailRow};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct ThreadViewState {
    pub thread_id: Option<i64>,
    pub detail: Option<ThreadDetailRow>,
    pub scroll_offset: usize,
}

impl ThreadViewState {
    pub fn load(&mut self, conn: &Connection, thread_id: i64) {
        self.thread_id = Some(thread_id);
        self.detail = get_thread_detail(conn, thread_id).ok().flatten();
        self.scroll_offset = 0;
    }

    pub fn scroll_down(&mut self) {
        let max_offset = self.max_scroll_offset();
        self.scroll_offset = self.scroll_offset.saturating_add(1).min(max_offset);
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    fn max_scroll_offset(&self) -> usize {
        self.detail
            .as_ref()
            .map(thread_content_height)
            .unwrap_or_default()
            .saturating_sub(1)
    }
}

pub fn render_thread_view(frame: &mut Frame, area: Rect, theme: &Theme, state: &ThreadViewState) {
    let Some(detail) = &state.detail else {
        let empty = Paragraph::new("No thread selected — press i for inbox")
            .style(theme.text_dim)
            .block(Block::default().borders(Borders::NONE));
        frame.render_widget(empty, area);
        return;
    };

    let [messages_area, meta_area] = ratatui::layout::Layout::horizontal([
        Constraint::Percentage(70),
        Constraint::Percentage(30),
    ])
    .areas(area);

    let viewport_height = messages_area.height as usize;
    let max_visible_scroll = state
        .max_scroll_offset()
        .saturating_sub(viewport_height.saturating_sub(1));
    let scroll_offset = state.scroll_offset.min(max_visible_scroll);

    render_messages(frame, messages_area, theme, detail, scroll_offset);
    render_metadata(frame, meta_area, theme, detail);
}

fn thread_content_height(detail: &ThreadDetailRow) -> usize {
    let header_height = 2;
    let message_height: usize = detail
        .messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let separator_height = usize::from(i > 0);
            let body_height = msg.body_text.lines().count().max(1);
            separator_height + 4 + body_height + 1
        })
        .sum();

    header_height + message_height
}

fn render_messages(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    detail: &ThreadDetailRow,
    scroll_offset: usize,
) {
    let subject = detail.subject.as_deref().unwrap_or("(no subject)");
    let mut lines = vec![
        Line::from(Span::styled(format!("Thread: {subject}"), theme.title)),
        Line::from(vec![
            Span::styled("Messages ", theme.text_dim),
            Span::styled(detail.messages.len().to_string(), theme.count_badge),
        ]),
        Line::default(),
    ];

    for (i, msg) in detail.messages.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(Span::styled(
                "· · · · · · · · · · · · · · · · · · · · · · · ·",
                theme.text_dim,
            )));
        }
        lines.push(Line::from(vec![
            Span::styled(
                msg.from_email.as_deref().unwrap_or("unknown").to_string(),
                theme.text_accent,
            ),
            Span::styled("  •  ", theme.text_dim),
            Span::styled(
                msg.sent_at.as_deref().unwrap_or("unknown").to_string(),
                theme.text_muted,
            ),
        ]));
        lines.push(Line::from(Span::styled("", theme.text_dim)));
        for body_line in msg.body_text.lines().filter(|line| !line.trim().is_empty()) {
            lines.push(Line::from(vec![
                Span::styled("  ", theme.text_dim),
                Span::styled(body_line.to_string(), theme.text),
            ]));
        }
        if msg.body_text.lines().all(|line| line.trim().is_empty()) {
            lines.push(Line::from(vec![
                Span::styled("  ", theme.text_dim),
                Span::styled("(empty message)", theme.text_dim),
            ]));
        }
        lines.push(Line::default());
    }

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::RIGHT))
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset as u16, 0));
    frame.render_widget(para, area);
}

fn render_metadata(frame: &mut Frame, area: Rect, theme: &Theme, detail: &ThreadDetailRow) {
    let state = detail.state.as_deref().unwrap_or("unknown");
    let participants: Vec<String> = detail
        .messages
        .iter()
        .filter_map(|m| m.from_email.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut lines = vec![
        Line::from(Span::styled("Metadata", theme.section_header)),
        Line::default(),
        Line::from(vec![
            Span::styled("State", theme.text_dim),
            Span::styled("  ", theme.text_dim),
            Span::styled(state.to_string(), theme.text_accent),
        ]),
        Line::from(vec![
            Span::styled("Messages", theme.text_dim),
            Span::styled("  ", theme.text_dim),
            Span::styled(detail.messages.len().to_string(), theme.count_badge),
        ]),
        Line::default(),
        Line::from(Span::styled("Participants", theme.text_muted)),
    ];

    if participants.is_empty() {
        lines.push(Line::from(Span::styled("  — none", theme.text_dim)));
    } else {
        for p in &participants {
            lines.push(Line::from(Span::styled(format!("  {p}"), theme.text)));
        }
    }

    lines.extend([
        Line::default(),
        Line::from(Span::styled("AI Summary", theme.text_muted)),
        Line::from(Span::styled("  Coming soon", theme.text_dim)),
        Line::default(),
        Line::from(Span::styled("Latest Ask", theme.text_muted)),
        Line::from(Span::styled("  Coming soon", theme.text_dim)),
        Line::default(),
        Line::from(Span::styled("Related Threads", theme.text_muted)),
        Line::from(Span::styled("  Coming soon", theme.text_dim)),
    ]);

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    use super::*;
    use crate::index::query::{ThreadDetailRow, ThreadMessageRow};

    #[test]
    fn thread_view_shows_metadata_panel_and_messages() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();
        let state = ThreadViewState {
            thread_id: Some(7),
            detail: Some(ThreadDetailRow {
                thread_id: 7,
                state: Some("needs_reply".to_string()),
                subject: Some("Quarterly planning".to_string()),
                messages: vec![
                    ThreadMessageRow {
                        message_id: 1,
                        subject: Some("Quarterly planning".to_string()),
                        from_email: Some("alice@example.com".to_string()),
                        body_text: "First update from Alice.".to_string(),
                        sent_at: Some("2026-04-01".to_string()),
                    },
                    ThreadMessageRow {
                        message_id: 2,
                        subject: Some("Re: Quarterly planning".to_string()),
                        from_email: Some("bob@example.com".to_string()),
                        body_text: "Second reply from Bob.".to_string(),
                        sent_at: Some("2026-04-02".to_string()),
                    },
                ],
            }),
            scroll_offset: 0,
        };

        terminal
            .draw(|frame| render_thread_view(frame, frame.area(), &theme, &state))
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        assert_contains(&buffer, "Thread: Quarterly planning");
        assert_contains(&buffer, "alice@example.com");
        assert_contains(&buffer, "Second reply from Bob.");
        assert_contains(&buffer, "Metadata");
        assert_contains(&buffer, "needs_reply");
        assert_contains(&buffer, "Participants");
        assert_contains(&buffer, "bob@example.com");
        assert_contains(&buffer, "Coming soon");
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
