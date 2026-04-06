use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::index::query::{
    list_recent_threads, list_threads_by_state, pending_drafts_count, ThreadListRow,
};
use crate::ui::theme::Theme;

const SECTIONS: &[(&str, &str)] = &[
    ("Needs Reply", "waiting_on_me"),
    ("Urgent", "urgent"),
    ("Waiting on Them", "waiting_on_them"),
    ("Follow Up Due", "follow_up_due"),
    ("Low Value", "low_value"),
];

pub fn render_brief(frame: &mut Frame, area: Rect, theme: &Theme, conn: &Connection) {
    let section_count = SECTIONS.len() + 2; // +recent +drafts
    let mut constraints: Vec<Constraint> = SECTIONS.iter().map(|_| Constraint::Min(3)).collect();
    constraints.push(Constraint::Min(4)); // recent
    constraints.push(Constraint::Length(2)); // drafts count

    let chunks = Layout::vertical(constraints).split(area);

    for (i, (label, state)) in SECTIONS.iter().enumerate() {
        let threads = list_threads_by_state(conn, state).unwrap_or_default();
        render_section(frame, chunks[i], label, &threads, theme);
    }

    let recent = list_recent_threads(conn, 5).unwrap_or_default();
    render_section(
        frame,
        chunks[section_count - 2],
        "Recent Threads",
        &recent,
        theme,
    );

    let drafts = pending_drafts_count(conn).unwrap_or(0);
    let drafts_line = Line::from(vec![
        Span::styled("Pending drafts", theme.text_muted),
        Span::styled(": ", theme.thread_meta),
        Span::styled(format!("{drafts}"), theme.count_badge),
    ]);
    frame.render_widget(Paragraph::new(drafts_line), chunks[section_count - 1]);
}

fn render_section<W>(
    target: &mut W,
    area: Rect,
    title: &str,
    threads: &[ThreadListRow],
    theme: &Theme,
) where
    W: WidgetRef,
{
    let mut lines = vec![Line::from(vec![
        Span::styled(title, theme.section_header),
        Span::styled(
            format!("  {}", section_count_label(threads.len())),
            theme.thread_meta,
        ),
    ])];

    if threads.is_empty() {
        lines.push(Line::from(Span::styled("  · all clear", theme.empty_state)));
    } else {
        for row in threads {
            let subject = row.subject.as_deref().unwrap_or("(no subject)");
            let count = if row.message_count > 1 {
                format!("{} msgs", row.message_count)
            } else {
                "1 msg".to_string()
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  #{:<4} ", row.id), theme.thread_meta),
                Span::styled(
                    subject.to_string(),
                    theme
                        .thread_subject
                        .patch(thread_style(theme, row.state.as_deref())),
                ),
                Span::styled(format!("  {count}"), theme.thread_meta),
            ]));
        }
    }

    target.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn section_count_label(count: usize) -> String {
    match count {
        0 => "clear".to_string(),
        1 => "1 thread".to_string(),
        _ => format!("{count} threads"),
    }
}

trait WidgetRef {
    fn render_widget(&mut self, widget: Paragraph<'_>, area: Rect);
}

impl WidgetRef for Frame<'_> {
    fn render_widget(&mut self, widget: Paragraph<'_>, area: Rect) {
        Frame::render_widget(self, widget, area);
    }
}

impl WidgetRef for Buffer {
    fn render_widget(&mut self, widget: Paragraph<'_>, area: Rect) {
        Widget::render(widget, area, self);
    }
}

fn thread_style(theme: &Theme, state: Option<&str>) -> ratatui::style::Style {
    match state {
        Some("waiting_on_me") => theme.thread_needs_reply,
        Some("waiting_on_them") => theme.thread_waiting,
        Some("low_value") => theme.thread_low_value,
        _ => theme.text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_renders_priority_sections_with_empty_state() {
        let theme = Theme::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 3));

        render_section(
            &mut buffer,
            Rect::new(0, 0, 60, 3),
            "Needs Reply",
            &[],
            &theme,
        );

        let header = buffer.cell((0, 0)).expect("header cell").style();
        assert_eq!(header.fg, theme.section_header.fg);
        assert!(header
            .add_modifier
            .contains(theme.section_header.add_modifier));

        let empty_marker = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "·")
            .expect("empty marker");
        assert_eq!(empty_marker.style().fg, theme.empty_state.fg);

        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Needs Reply"));
        assert!(rendered.contains("clear"));
        assert!(rendered.contains("all clear"));
    }
}
