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
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
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

    render_messages(frame, messages_area, theme, detail, state.scroll_offset);
    render_metadata(frame, meta_area, theme, detail);
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
        Line::default(),
    ];

    for (i, msg) in detail.messages.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(Span::styled(
                "────────────────────────────────────────",
                theme.text_dim,
            )));
        }
        lines.push(Line::from(vec![
            Span::styled("From: ", theme.text_dim),
            Span::styled(
                msg.from_email.as_deref().unwrap_or("unknown").to_string(),
                theme.text_accent,
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Date: ", theme.text_dim),
            Span::styled(
                msg.sent_at.as_deref().unwrap_or("unknown").to_string(),
                theme.text_muted,
            ),
        ]));
        lines.push(Line::default());
        for body_line in msg.body_text.lines() {
            lines.push(Line::from(Span::styled(body_line.to_string(), theme.text)));
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
            Span::styled("State: ", theme.text_dim),
            Span::styled(state.to_string(), theme.text_accent),
        ]),
        Line::from(vec![
            Span::styled("Messages: ", theme.text_dim),
            Span::raw(format!("{}", detail.messages.len())),
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
        Line::from(Span::styled("  unavailable", theme.text_dim)),
        Line::default(),
        Line::from(Span::styled("Latest Ask", theme.text_muted)),
        Line::from(Span::styled("  unavailable", theme.text_dim)),
        Line::default(),
        Line::from(Span::styled("Related Threads", theme.text_muted)),
        Line::from(Span::styled("  unavailable", theme.text_dim)),
    ]);

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}
