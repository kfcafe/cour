use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub bg_elevated: Color,
    pub bg_selected: Color,
    pub bg_status_bar: Color,

    pub text: Style,
    pub text_muted: Style,
    pub text_dim: Style,
    pub text_accent: Style,
    pub text_error: Style,
    pub text_success: Style,
    pub text_warning: Style,

    pub border: Style,
    pub border_focused: Style,
    pub title: Style,

    pub workspace_active: Style,
    pub workspace_inactive: Style,

    pub keyhint_key: Style,
    pub keyhint_desc: Style,

    pub thread_needs_reply: Style,
    pub thread_waiting: Style,
    pub thread_low_value: Style,
    pub thread_selected: Style,
    pub selection: Style,

    pub section_header: Style,
    pub count_badge: Style,

    pub warning: Style,
    pub success: Style,
    pub danger: Style,
}

impl Default for Theme {
    fn default() -> Self {
        let bg = Color::Rgb(22, 22, 30);
        let bg_elevated = Color::Rgb(30, 30, 40);
        let bg_selected = Color::Rgb(40, 40, 55);
        let bg_status_bar = Color::Rgb(18, 18, 24);

        let text_color = Color::Rgb(200, 200, 210);
        let muted_color = Color::Rgb(120, 120, 135);
        let dim_color = Color::Rgb(80, 80, 95);
        let accent = Color::Rgb(100, 140, 220);
        let error = Color::Rgb(220, 100, 100);
        let success = Color::Rgb(100, 200, 130);
        let warning = Color::Rgb(220, 180, 80);
        let border_color = Color::Rgb(55, 55, 70);

        Self {
            bg,
            bg_elevated,
            bg_selected,
            bg_status_bar,

            text: Style::default().fg(text_color),
            text_muted: Style::default().fg(muted_color),
            text_dim: Style::default().fg(dim_color),
            text_accent: Style::default().fg(accent),
            text_error: Style::default().fg(error),
            text_success: Style::default().fg(success),
            text_warning: Style::default().fg(warning),

            border: Style::default().fg(border_color),
            border_focused: Style::default().fg(accent),
            title: Style::default().fg(text_color).add_modifier(Modifier::BOLD),

            workspace_active: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            workspace_inactive: Style::default().fg(muted_color),

            keyhint_key: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            keyhint_desc: Style::default().fg(muted_color),

            thread_needs_reply: Style::default().fg(warning).add_modifier(Modifier::BOLD),
            thread_waiting: Style::default().fg(accent),
            thread_low_value: Style::default().fg(dim_color),
            thread_selected: Style::default().bg(bg_selected).fg(text_color),
            selection: Style::default().bg(bg_selected).fg(text_color),

            section_header: Style::default()
                .fg(text_color)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            count_badge: Style::default().fg(accent).add_modifier(Modifier::BOLD),

            warning: Style::default().fg(warning),
            success: Style::default().fg(success),
            danger: Style::default().fg(error),
        }
    }
}
