use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
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
        Span::styled("Pending drafts: ", theme.text_muted),
        Span::styled(format!("{drafts}"), theme.count_badge),
    ]);
    frame.render_widget(Paragraph::new(drafts_line), chunks[section_count - 1]);
}

fn render_section(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    threads: &[ThreadListRow],
    theme: &Theme,
) {
    let mut lines = vec![Line::from(Span::styled(title, theme.section_header))];

    if threads.is_empty() {
        lines.push(Line::from(Span::styled("  — none", theme.text_dim)));
    } else {
        for row in threads {
            let subject = row.subject.as_deref().unwrap_or("(no subject)");
            let count = if row.message_count > 1 {
                format!("  ({} messages)", row.message_count)
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", row.id), theme.text_dim),
                Span::styled(
                    subject.to_string(),
                    thread_style(theme, row.state.as_deref()),
                ),
                Span::styled(count, theme.text_dim),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn thread_style(theme: &Theme, state: Option<&str>) -> ratatui::style::Style {
    match state {
        Some("waiting_on_me") => theme.thread_needs_reply,
        Some("waiting_on_them") => theme.thread_waiting,
        Some("low_value") => theme.thread_low_value,
        _ => theme.text,
    }
}
