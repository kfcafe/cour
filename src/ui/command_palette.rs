use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::ui::{app::Workspace, theme::Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAction {
    SwitchWorkspace(Workspace),
    RefreshCurrent,
    StartSearchEdit,
    ShowStatus(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDefinition {
    pub label: &'static str,
    pub hint: &'static str,
    pub action: CommandAction,
}

const COMMANDS: &[CommandDefinition] = &[
    CommandDefinition {
        label: "Go to Brief",
        hint: "Switch to the brief workspace",
        action: CommandAction::SwitchWorkspace(Workspace::Brief),
    },
    CommandDefinition {
        label: "Go to Inbox",
        hint: "Switch to the inbox workspace",
        action: CommandAction::SwitchWorkspace(Workspace::Inbox),
    },
    CommandDefinition {
        label: "Go to Search",
        hint: "Switch to the search workspace",
        action: CommandAction::SwitchWorkspace(Workspace::Search),
    },
    CommandDefinition {
        label: "Go to Actions",
        hint: "Switch to the actions workspace",
        action: CommandAction::SwitchWorkspace(Workspace::Actions),
    },
    CommandDefinition {
        label: "Go to Drafts",
        hint: "Switch to the drafts workspace",
        action: CommandAction::SwitchWorkspace(Workspace::Drafts),
    },
    CommandDefinition {
        label: "Refresh current workspace",
        hint: "Reload the current list or view",
        action: CommandAction::RefreshCurrent,
    },
    CommandDefinition {
        label: "Start search",
        hint: "Focus the search query input",
        action: CommandAction::StartSearchEdit,
    },
    CommandDefinition {
        label: "Reindex mail",
        hint: "Not wired yet; shows a status message",
        action: CommandAction::ShowStatus("Reindex is not wired into the command palette yet"),
    },
    CommandDefinition {
        label: "Sync accounts",
        hint: "Not wired yet; shows a status message",
        action: CommandAction::ShowStatus("Sync is not wired into the command palette yet"),
    },
];

#[derive(Debug, Default, Clone)]
pub struct CommandPaletteState {
    pub open: bool,
    pub filter: String,
    pub selected: usize,
}

impl CommandPaletteState {
    pub fn open(&mut self) {
        self.open = true;
        self.filter.clear();
        self.selected = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.filter.clear();
        self.selected = 0;
    }

    pub fn filtered_commands(&self) -> Vec<CommandDefinition> {
        let filter = self.filter.trim().to_ascii_lowercase();
        COMMANDS
            .iter()
            .copied()
            .filter(|command| {
                if filter.is_empty() {
                    return true;
                }

                let label = command.label.to_ascii_lowercase();
                let hint = command.hint.to_ascii_lowercase();
                label.contains(&filter) || hint.contains(&filter)
            })
            .collect()
    }

    pub fn move_down(&mut self) {
        let len = self.filtered_commands().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1).min(len - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_command(&self) -> Option<CommandDefinition> {
        self.filtered_commands().get(self.selected).copied()
    }

    pub fn push_filter(&mut self, ch: char) {
        self.filter.push(ch);
        self.selected = 0;
    }

    pub fn pop_filter(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }
}

pub fn render_command_palette(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &CommandPaletteState,
) {
    if !state.open {
        return;
    }

    let popup = centered_rect(area, 68, 60);
    let [input_area, list_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(1),
    ])
    .areas(popup);

    frame.render_widget(Clear, popup);

    let input = Paragraph::new(Line::from(vec![
        Span::styled(": ", theme.text_accent),
        Span::raw(if state.filter.is_empty() {
            ""
        } else {
            &state.filter
        }),
        Span::raw("█"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Command palette")
            .border_style(theme.border_focused),
    );
    frame.render_widget(input, input_area);

    let commands = state.filtered_commands();
    if commands.is_empty() {
        let empty = Paragraph::new("No commands match this filter")
            .style(theme.text_dim)
            .block(Block::default().borders(Borders::ALL).title("Commands"));
        frame.render_widget(empty, list_area);
    } else {
        let items = commands.iter().enumerate().map(|(index, command)| {
            let style = if index == state.selected {
                theme.selection
            } else {
                theme.text
            };
            ListItem::new(Line::from(vec![
                Span::styled(command.label, style),
                Span::styled(format!(" — {}", command.hint), theme.text_dim),
            ]))
        });

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Commands"));
        frame.render_widget(list, list_area);
    }

    let footer = Paragraph::new("Enter run  Esc close  ↑↓/j/k move")
        .style(theme.text_dim)
        .block(Block::default());
    frame.render_widget(footer, footer_area);
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let [_, vertical, _] = Layout::vertical([
        Constraint::Percentage((100 - height_percent) / 2),
        Constraint::Percentage(height_percent),
        Constraint::Percentage((100 - height_percent) / 2),
    ])
    .areas(area);

    let [_, horizontal, _] = Layout::horizontal([
        Constraint::Percentage((100 - width_percent) / 2),
        Constraint::Percentage(width_percent),
        Constraint::Percentage((100 - width_percent) / 2),
    ])
    .areas(vertical);

    horizontal
}

#[cfg(test)]
mod tests {
    use super::CommandPaletteState;

    #[test]
    fn command_palette_filters_by_substring() {
        let mut state = CommandPaletteState::default();
        state.filter = "draft".to_string();

        let commands = state.filtered_commands();
        assert!(!commands.is_empty());
        assert!(commands.iter().all(|command| {
            command.label.to_ascii_lowercase().contains("draft")
                || command.hint.to_ascii_lowercase().contains("draft")
        }));
        assert!(commands
            .iter()
            .any(|command| command.label == "Go to Drafts"));
    }
}
