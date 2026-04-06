use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::CrosstermBackend;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use rusqlite::Connection;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::index::open_database;
use crate::index::schema::initialize_schema;
use crate::ui::command_palette::{render_command_palette, CommandAction, CommandPaletteState};
use crate::ui::theme::Theme;
use crate::ui::views::actions::ActionsState;
use crate::ui::views::brief::render_brief;
use crate::ui::views::drafts::DraftsState;
use crate::ui::views::inbox::InboxState;
use crate::ui::views::search::SearchState;
use crate::ui::views::thread::ThreadViewState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Workspace {
    #[default]
    Brief,
    Inbox,
    Thread,
    Search,
    Actions,
    Drafts,
}

impl Workspace {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Inbox => "inbox",
            Self::Thread => "thread",
            Self::Search => "search",
            Self::Actions => "actions",
            Self::Drafts => "drafts",
        }
    }
}

const ALL_WORKSPACES: &[Workspace] = &[
    Workspace::Brief,
    Workspace::Inbox,
    Workspace::Thread,
    Workspace::Search,
    Workspace::Actions,
    Workspace::Drafts,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FocusMode {
    #[default]
    Navigation,
    EditingSearch,
}

pub struct App {
    pub workspace: Workspace,
    pub running: bool,
    pub theme: Theme,
    pub db_path: PathBuf,
    pub status_message: Option<String>,
    focus_mode: FocusMode,
    previous_workspace: Workspace,

    pub inbox_state: InboxState,
    pub thread_state: ThreadViewState,
    pub search_state: SearchState,
    pub drafts_state: DraftsState,
    pub actions_state: ActionsState,
    pub command_palette_state: CommandPaletteState,
}

impl App {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            workspace: Workspace::Brief,
            running: true,
            theme: Theme::default(),
            db_path,
            status_message: None,
            focus_mode: FocusMode::Navigation,
            previous_workspace: Workspace::Inbox,
            inbox_state: InboxState::default(),
            thread_state: ThreadViewState::default(),
            search_state: SearchState::default(),
            drafts_state: DraftsState::default(),
            actions_state: ActionsState::default(),
            command_palette_state: CommandPaletteState::default(),
        }
    }

    fn open_db(&self) -> Result<Connection, AppError> {
        let conn = open_database(&self.db_path).map_err(|e| AppError::Sqlite(e.to_string()))?;
        initialize_schema(&conn).map_err(|e| AppError::Sqlite(e.to_string()))?;
        Ok(conn)
    }

    fn switch_workspace(&mut self, ws: Workspace) {
        if self.workspace != Workspace::Thread {
            self.previous_workspace = self.workspace;
        }
        self.workspace = ws;
        self.status_message = None;
        self.focus_mode = FocusMode::Navigation;
        self.search_state.editing = false;
        if let Ok(conn) = self.open_db() {
            match ws {
                Workspace::Inbox => {
                    let _ = self.inbox_state.refresh(&conn);
                }
                Workspace::Drafts => {
                    let _ = self.drafts_state.refresh(&conn);
                }
                Workspace::Actions => {
                    let _ = self.actions_state.refresh(&conn);
                }
                Workspace::Search if self.search_state.query.is_empty() => {
                    self.enter_search_edit_mode();
                }
                _ => {}
            }
        }
    }

    fn open_thread(&mut self, thread_id: i64) {
        self.previous_workspace = self.workspace;
        self.workspace = Workspace::Thread;
        self.focus_mode = FocusMode::Navigation;
        self.search_state.editing = false;

        if let Ok(conn) = self.open_db() {
            self.thread_state.load(&conn, thread_id);
        } else {
            self.thread_state.thread_id = Some(thread_id);
            self.thread_state.detail = None;
            self.thread_state.scroll_offset = 0;
        }
    }

    fn enter_search_edit_mode(&mut self) {
        self.focus_mode = FocusMode::EditingSearch;
        self.search_state.editing = true;
    }

    fn exit_search_edit_mode(&mut self) {
        self.focus_mode = FocusMode::Navigation;
        self.search_state.editing = false;
    }

    fn return_from_thread(&mut self) {
        let target = if self.previous_workspace == Workspace::Thread {
            Workspace::Inbox
        } else {
            self.previous_workspace
        };
        self.switch_workspace(target);
    }
}

pub fn run_app(_config: AppConfig, db_path: PathBuf) -> AppResult<()> {
    enable_raw_mode().map_err(AppError::Io)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(AppError::Io)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(AppError::Io)?;

    let mut app = App::new(db_path);

    let result = main_loop(&mut terminal, &mut app);

    disable_raw_mode().map_err(AppError::Io)?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(AppError::Io)?;
    terminal.show_cursor().map_err(AppError::Io)?;

    result
}

fn main_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> AppResult<()> {
    while app.running {
        terminal
            .draw(|frame| render(frame, app))
            .map_err(AppError::Io)?;

        if event::poll(Duration::from_millis(100)).map_err(AppError::Io)? {
            if let Event::Key(key) = event::read().map_err(AppError::Io)? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                handle_key(app, key.code);
            }
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, code: KeyCode) {
    if app.command_palette_state.open {
        handle_command_palette_key(app, code);
        return;
    }

    if code == KeyCode::Char(':') {
        app.command_palette_state.open();
        return;
    }

    if app.workspace == Workspace::Search && app.focus_mode == FocusMode::EditingSearch {
        match code {
            KeyCode::Char(ch) if !ch.is_control() => app.search_state.query.push(ch),
            KeyCode::Backspace => {
                app.search_state.query.pop();
            }
            KeyCode::Enter => {
                if let Ok(conn) = app.open_db() {
                    let _ = app.search_state.execute(&conn);
                }
                app.exit_search_edit_mode();
            }
            KeyCode::Esc => app.exit_search_edit_mode(),
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Char('q') => app.running = false,
        KeyCode::Char('b') => app.switch_workspace(Workspace::Brief),
        KeyCode::Char('i') => app.switch_workspace(Workspace::Inbox),
        KeyCode::Char('s') => app.switch_workspace(Workspace::Search),
        KeyCode::Char('a') => app.switch_workspace(Workspace::Actions),
        KeyCode::Char('d') => app.switch_workspace(Workspace::Drafts),
        KeyCode::Char('?') => {
            app.status_message = Some(help_text(app).to_string());
        }
        _ => handle_workspace_key(app, code),
    }
}

fn handle_command_palette_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.command_palette_state.close(),
        KeyCode::Backspace => app.command_palette_state.pop_filter(),
        KeyCode::Char('j') | KeyCode::Down => app.command_palette_state.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.command_palette_state.move_up(),
        KeyCode::Enter => {
            if let Some(command) = app.command_palette_state.selected_command() {
                execute_command(app, command.action);
            }
            app.command_palette_state.close();
        }
        KeyCode::Char(ch) if !ch.is_control() => app.command_palette_state.push_filter(ch),
        _ => {}
    }
}

fn execute_command(app: &mut App, action: CommandAction) {
    match action {
        CommandAction::SwitchWorkspace(workspace) => app.switch_workspace(workspace),
        CommandAction::RefreshCurrent => refresh_current_workspace(app),
        CommandAction::StartSearchEdit => {
            app.switch_workspace(Workspace::Search);
            app.enter_search_edit_mode();
        }
        CommandAction::ShowStatus(message) => {
            app.status_message = Some(message.to_string());
        }
    }
}

fn refresh_current_workspace(app: &mut App) {
    match app.workspace {
        Workspace::Inbox => {
            if let Ok(conn) = app.open_db() {
                let _ = app.inbox_state.refresh(&conn);
            }
        }
        Workspace::Drafts => {
            if let Ok(conn) = app.open_db() {
                let _ = app.drafts_state.refresh(&conn);
            }
        }
        Workspace::Actions => {
            if let Ok(conn) = app.open_db() {
                let _ = app.actions_state.refresh(&conn);
            }
        }
        Workspace::Search => {
            if app.search_state.query.is_empty() {
                app.enter_search_edit_mode();
            } else if let Ok(conn) = app.open_db() {
                let _ = app.search_state.execute(&conn);
                app.exit_search_edit_mode();
            }
        }
        _ => {
            app.status_message = Some("Nothing to refresh in this workspace yet".to_string());
        }
    }
}

fn handle_workspace_key(app: &mut App, code: KeyCode) {
    match app.workspace {
        Workspace::Inbox => match code {
            KeyCode::Char('j') | KeyCode::Down => app.inbox_state.next(),
            KeyCode::Char('k') | KeyCode::Up => app.inbox_state.previous(),
            KeyCode::Enter => {
                if let Some(id) = app.inbox_state.selected_thread_id() {
                    app.open_thread(id);
                }
            }
            KeyCode::Char('r') => {
                if let Ok(conn) = app.open_db() {
                    let _ = app.inbox_state.refresh(&conn);
                }
            }
            _ => {}
        },
        Workspace::Thread => match code {
            KeyCode::Char('j') | KeyCode::Down => app.thread_state.scroll_down(),
            KeyCode::Char('k') | KeyCode::Up => app.thread_state.scroll_up(),
            KeyCode::Esc => app.return_from_thread(),
            _ => {}
        },
        Workspace::Search => match code {
            KeyCode::Char('/') => app.enter_search_edit_mode(),
            KeyCode::Enter => {
                if let Some(id) = app.search_state.selected_thread_id() {
                    app.open_thread(id);
                } else {
                    app.enter_search_edit_mode();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => app.search_state.move_down(),
            KeyCode::Char('k') | KeyCode::Up => app.search_state.move_up(),
            KeyCode::Esc => app.exit_search_edit_mode(),
            _ => {}
        },
        Workspace::Drafts => match code {
            KeyCode::Char('j') | KeyCode::Down => app.drafts_state.move_next(),
            KeyCode::Char('k') | KeyCode::Up => app.drafts_state.move_previous(),
            KeyCode::Char('r') => {
                if let Ok(conn) = app.open_db() {
                    let _ = app.drafts_state.refresh(&conn);
                }
            }
            _ => {}
        },
        Workspace::Actions => match code {
            KeyCode::Tab => app.actions_state.move_section(1),
            KeyCode::Char('j') | KeyCode::Down => app.actions_state.move_row(1),
            KeyCode::Char('k') | KeyCode::Up => app.actions_state.move_row(-1),
            KeyCode::Enter => {
                if let Some(id) = app.actions_state.selected_thread_id() {
                    app.open_thread(id);
                }
            }
            KeyCode::Char('r') => {
                if let Ok(conn) = app.open_db() {
                    let _ = app.actions_state.refresh(&conn);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn render(frame: &mut ratatui::Frame, app: &App) {
    let theme = &app.theme;
    let area = frame.area();

    // fill background
    frame.render_widget(
        ratatui::widgets::Block::default().style(ratatui::style::Style::default().bg(theme.bg)),
        area,
    );

    let [top_bar, content, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    render_top_bar(frame, top_bar, theme, app.workspace);
    render_content(frame, content, app);
    render_command_palette(frame, content, theme, &app.command_palette_state);
    render_footer(frame, footer, theme, app);
}

fn render_top_bar(frame: &mut ratatui::Frame, area: Rect, theme: &Theme, active: Workspace) {
    let mut spans = vec![Span::styled(" cour ", theme.title)];
    spans.push(Span::styled("│ ", theme.text_dim));

    for ws in ALL_WORKSPACES {
        let style = if *ws == active {
            theme.workspace_active
        } else {
            theme.workspace_inactive
        };
        spans.push(Span::styled(format!(" {} ", ws.label()), style));
    }

    let bar = Paragraph::new(Line::from(spans))
        .style(ratatui::style::Style::default().bg(theme.bg_status_bar));
    frame.render_widget(bar, area);
}

fn render_content(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    match app.workspace {
        Workspace::Brief => {
            if let Ok(conn) = app.open_db() {
                render_brief(frame, area, theme, &conn);
            } else {
                render_placeholder(frame, area, theme, "Could not open database");
            }
        }
        Workspace::Inbox => {
            crate::ui::views::inbox::render_inbox(frame, area, theme, &app.inbox_state);
        }
        Workspace::Thread => {
            crate::ui::views::thread::render_thread_view(frame, area, theme, &app.thread_state);
        }
        Workspace::Search => {
            crate::ui::views::search::render_search(frame, area, theme, &app.search_state);
        }
        Workspace::Drafts => {
            crate::ui::views::drafts::render_drafts(frame, area, theme, &app.drafts_state);
        }
        Workspace::Actions => {
            crate::ui::views::actions::render_actions(frame, area, theme, &app.actions_state);
        }
    }
}

fn render_footer(frame: &mut ratatui::Frame, area: Rect, theme: &Theme, app: &App) {
    let text = if let Some(msg) = &app.status_message {
        msg.as_str()
    } else {
        help_text(app)
    };

    let footer = Paragraph::new(Line::from(Span::styled(text, theme.keyhint_desc)))
        .style(ratatui::style::Style::default().bg(theme.bg_status_bar));
    frame.render_widget(footer, area);
}

fn help_text(app: &App) -> &'static str {
    match (app.workspace, app.focus_mode) {
        (Workspace::Search, FocusMode::EditingSearch) => {
            "Search edit · type query  Enter run  Esc stop editing  b/i/s/a/d switch  : commands"
        }
        (Workspace::Inbox, _) => {
            "Inbox · j/k move  Enter open thread  b/i/s/a/d switch  q quit  : commands  ? help"
        }
        (Workspace::Thread, _) => {
            "Thread · j/k scroll  Esc back  b/i/s/a/d switch  q quit  : commands  ? help"
        }
        (Workspace::Search, _) => {
            "Search · j/k move  Enter open  / edit query  Esc clear focus  b/i/s/a/d switch"
        }
        (Workspace::Actions, _) => {
            "Actions · Tab section  j/k move  Enter open  b/i/s/a/d switch  q quit  : commands"
        }
        (Workspace::Drafts, _) => {
            "Drafts · j/k move  r refresh  b/i/s/a/d switch  q quit  : commands  ? help"
        }
        (Workspace::Brief, _) => "Brief · b/i/s/a/d switch  q quit  : commands  ? help",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::query::ThreadListRow;

    fn test_app() -> App {
        App::new(PathBuf::from("/nonexistent-test.db"))
    }

    #[test]
    fn search_escape_restores_global_workspace_switching() {
        let mut app = test_app();
        app.workspace = Workspace::Search;
        app.search_state.query = "hel".to_string();
        app.enter_search_edit_mode();

        handle_key(&mut app, KeyCode::Esc);
        assert_eq!(app.workspace, Workspace::Search);
        assert!(!app.search_state.editing);
        assert_eq!(app.focus_mode, FocusMode::Navigation);

        handle_key(&mut app, KeyCode::Char('a'));
        assert_eq!(app.workspace, Workspace::Actions);
        assert_eq!(app.search_state.query, "hel");
    }

    #[test]
    fn enter_from_inbox_opens_thread_workspace() {
        let mut app = test_app();
        app.workspace = Workspace::Inbox;
        app.inbox_state.threads = vec![ThreadListRow {
            id: 7,
            subject: Some("Quarterly planning".to_string()),
            state: Some("waiting_on_me".to_string()),
            message_count: 2,
            latest_message_at: None,
        }];
        app.inbox_state.selected = 0;

        let thread_id = app.inbox_state.selected_thread_id().unwrap();
        app.open_thread(thread_id);

        assert_eq!(app.workspace, Workspace::Thread);
        assert_eq!(app.previous_workspace, Workspace::Inbox);
        assert_eq!(app.thread_state.thread_id, Some(7));
    }

    #[test]
    fn thread_escape_returns_to_inbox_workspace() {
        let mut app = test_app();
        app.workspace = Workspace::Inbox;
        app.inbox_state.threads = vec![ThreadListRow {
            id: 9,
            subject: Some("Follow up".to_string()),
            state: Some("waiting_on_them".to_string()),
            message_count: 1,
            latest_message_at: None,
        }];
        app.inbox_state.selected = 0;

        app.previous_workspace = Workspace::Inbox;
        app.workspace = Workspace::Thread;

        handle_key(&mut app, KeyCode::Esc);
        assert_eq!(app.workspace, Workspace::Inbox);
        assert_eq!(app.inbox_state.selected, 0);
    }
}

fn render_placeholder(frame: &mut ratatui::Frame, area: Rect, theme: &Theme, msg: &str) {
    let p = Paragraph::new(msg).style(theme.text_dim);
    frame.render_widget(p, area);
}
