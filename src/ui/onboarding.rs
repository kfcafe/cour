use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::CrosstermBackend;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;

use crate::error::{AppError, AppResult};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingStep {
    Welcome,
    AccountBasics,
    MaildirPath,
    SetupMode,
    SyncCommand,
    AiProviders,
    SmtpIdentity,
    Review,
    Complete,
}

impl OnboardingStep {
    fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::AccountBasics => "Account basics",
            Self::MaildirPath => "Mail storage",
            Self::SetupMode => "Setup style",
            Self::SyncCommand => "Mail download (advanced)",
            Self::AiProviders => "AI features (advanced)",
            Self::SmtpIdentity => "Sending mail (advanced)",
            Self::Review => "Review",
            Self::Complete => "Complete",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DraftAccountConfig {
    pub name: String,
    pub email_address: String,
    pub maildir_root: String,
    pub sync_command: String,
    pub default: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DraftProviderConfig {
    pub provider: String,
    pub model: String,
    pub api_url: String,
    pub api_key_env: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DraftAiConfig {
    pub embedding: DraftProviderConfig,
    pub extraction: DraftProviderConfig,
    pub drafting: DraftProviderConfig,
}

#[derive(Debug, Clone)]
pub struct DraftSmtpIdentity {
    pub name: String,
    pub email_address: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password_env: String,
    pub tls_mode: String,
    pub default: bool,
}

impl Default for DraftSmtpIdentity {
    fn default() -> Self {
        Self {
            name: String::new(),
            email_address: String::new(),
            host: String::new(),
            port: "465".to_string(),
            username: String::new(),
            password_env: String::new(),
            tls_mode: "tls".to_string(),
            default: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DraftConfigModel {
    pub accounts: Vec<DraftAccountConfig>,
    pub ai: DraftAiConfig,
    pub smtp: Vec<DraftSmtpIdentity>,
}

#[derive(Debug, Clone)]
pub struct FieldBuffers {
    pub account_name: String,
    pub email_address: String,
    pub maildir_root: String,
    pub sync_command: String,
    pub ai_provider: String,
    pub ai_model: String,
    pub ai_api_url: String,
    pub ai_api_key_env: String,
    pub smtp_name: String,
    pub smtp_email_address: String,
    pub smtp_host: String,
    pub smtp_port: String,
    pub smtp_username: String,
    pub smtp_password_env: String,
    pub smtp_tls_mode: String,
}

impl Default for FieldBuffers {
    fn default() -> Self {
        Self {
            account_name: String::new(),
            email_address: String::new(),
            maildir_root: String::new(),
            sync_command: String::new(),
            ai_provider: String::new(),
            ai_model: String::new(),
            ai_api_url: String::new(),
            ai_api_key_env: String::new(),
            smtp_name: String::new(),
            smtp_email_address: String::new(),
            smtp_host: String::new(),
            smtp_port: "465".to_string(),
            smtp_username: String::new(),
            smtp_password_env: String::new(),
            smtp_tls_mode: "tls".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OnboardingState {
    pub current_step: OnboardingStep,
    pub field_buffers: FieldBuffers,
    pub validation_errors: Vec<String>,
    pub draft_config: DraftConfigModel,
    pub running: bool,
    pub config_path: PathBuf,
    pub status_message: Option<String>,
    pub selected_field: usize,
    pub advanced_setup: bool,
}

impl OnboardingState {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            current_step: OnboardingStep::Welcome,
            field_buffers: FieldBuffers::default(),
            validation_errors: Vec::new(),
            draft_config: DraftConfigModel::default(),
            running: true,
            config_path,
            status_message: None,
            selected_field: starting_field_for(OnboardingStep::Welcome),
            advanced_setup: false,
        }
    }

    fn next_step(&self) -> Option<OnboardingStep> {
        match self.current_step {
            OnboardingStep::Welcome => Some(OnboardingStep::AccountBasics),
            OnboardingStep::AccountBasics => Some(OnboardingStep::MaildirPath),
            OnboardingStep::MaildirPath => Some(OnboardingStep::SetupMode),
            OnboardingStep::SetupMode => Some(if self.advanced_setup {
                OnboardingStep::SyncCommand
            } else {
                OnboardingStep::Review
            }),
            OnboardingStep::SyncCommand => Some(OnboardingStep::AiProviders),
            OnboardingStep::AiProviders => Some(OnboardingStep::SmtpIdentity),
            OnboardingStep::SmtpIdentity => Some(OnboardingStep::Review),
            OnboardingStep::Review => Some(OnboardingStep::Complete),
            OnboardingStep::Complete => None,
        }
    }

    fn previous_step(&self) -> Option<OnboardingStep> {
        match self.current_step {
            OnboardingStep::Welcome => None,
            OnboardingStep::AccountBasics => Some(OnboardingStep::Welcome),
            OnboardingStep::MaildirPath => Some(OnboardingStep::AccountBasics),
            OnboardingStep::SetupMode => Some(OnboardingStep::MaildirPath),
            OnboardingStep::SyncCommand => Some(OnboardingStep::SetupMode),
            OnboardingStep::AiProviders => Some(OnboardingStep::SyncCommand),
            OnboardingStep::SmtpIdentity => Some(OnboardingStep::AiProviders),
            OnboardingStep::Review => Some(if self.advanced_setup {
                OnboardingStep::SmtpIdentity
            } else {
                OnboardingStep::SetupMode
            }),
            OnboardingStep::Complete => Some(OnboardingStep::Review),
        }
    }

    fn go_back(&mut self) {
        self.validation_errors.clear();
        self.status_message = None;
        if let Some(previous) = self.previous_step() {
            self.current_step = previous;
            self.selected_field = starting_field_for(previous);
        }
    }

    fn move_field(&mut self, direction: isize) {
        let field_count = field_count_for(self.current_step);
        if field_count <= 1 {
            return;
        }

        let max_index = field_count.saturating_sub(1) as isize;
        let next_index = (self.selected_field as isize + direction).clamp(0, max_index) as usize;
        self.selected_field = next_index;
    }

    fn try_advance(&mut self) {
        self.validation_errors.clear();
        self.status_message = None;

        if !self.validate_current_step() {
            return;
        }

        self.capture_current_step();

        match self.next_step() {
            Some(next) => {
                self.current_step = next;
                self.selected_field = starting_field_for(next);
            }
            None => self.running = false,
        }
    }

    fn validate_current_step(&mut self) -> bool {
        match self.current_step {
            OnboardingStep::Welcome => true,
            OnboardingStep::AccountBasics => {
                self.validate_account_basics();
                self.validation_errors.is_empty()
            }
            OnboardingStep::MaildirPath => {
                self.ensure_maildir_step();
                self.validation_errors.is_empty()
            }
            OnboardingStep::SetupMode => true,
            OnboardingStep::SyncCommand => true,
            OnboardingStep::AiProviders => true,
            OnboardingStep::SmtpIdentity => true,
            OnboardingStep::Review => true,
            OnboardingStep::Complete => true,
        }
    }

    fn validate_account_basics(&mut self) {
        let account_name = self.field_buffers.account_name.trim();
        if account_name.is_empty() {
            self.validation_errors
                .push("Account name is required.".to_string());
        }

        let email_address = self.field_buffers.email_address.trim();
        if email_address.is_empty() {
            self.validation_errors
                .push("Email address is required.".to_string());
        } else if !email_address.contains('@') {
            self.validation_errors
                .push("Email address must contain @.".to_string());
        }
    }

    fn ensure_maildir_step(&mut self) {
        match ensure_maildir_root(self.field_buffers.maildir_root.trim()) {
            Ok(()) => {}
            Err(error) => self.validation_errors.push(error),
        }
    }

    fn capture_current_step(&mut self) {
        match self.current_step {
            OnboardingStep::Welcome => {}
            OnboardingStep::AccountBasics => {
                let account_name = self.field_buffers.account_name.trim().to_string();
                let email = self.field_buffers.email_address.trim().to_string();
                let account = self.ensure_account();
                account.name = account_name.clone();
                account.email_address = email.clone();
                account.default = true;

                if self.field_buffers.maildir_root.trim().is_empty() {
                    if let Some(maildir_root) = default_maildir_root(&account_name, &email) {
                        self.field_buffers.maildir_root = maildir_root;
                    }
                }
                if self.field_buffers.smtp_name.trim().is_empty() {
                    self.field_buffers.smtp_name = account_name.clone();
                }
                if self.field_buffers.smtp_email_address.trim().is_empty() {
                    self.field_buffers.smtp_email_address = email.clone();
                }
                if self.field_buffers.smtp_username.trim().is_empty() {
                    self.field_buffers.smtp_username = email.clone();
                }
                prefill_smtp_settings(&email, &mut self.field_buffers);
            }
            OnboardingStep::MaildirPath => {
                let maildir_root = self.field_buffers.maildir_root.trim().to_string();
                let account = self.ensure_account();
                account.maildir_root = maildir_root;
            }
            OnboardingStep::SetupMode => {
                self.advanced_setup = self.selected_field == 1;
                if !self.advanced_setup {
                    self.draft_config.ai = DraftAiConfig::default();
                    self.draft_config.smtp.clear();
                    let account = self.ensure_account();
                    account.sync_command.clear();
                }
            }
            OnboardingStep::SyncCommand => {
                let sync_command = self.field_buffers.sync_command.trim().to_string();
                let account = self.ensure_account();
                account.sync_command = sync_command;
            }
            OnboardingStep::AiProviders => {
                let provider = DraftProviderConfig {
                    provider: self.field_buffers.ai_provider.trim().to_string(),
                    model: self.field_buffers.ai_model.trim().to_string(),
                    api_url: self.field_buffers.ai_api_url.trim().to_string(),
                    api_key_env: self.field_buffers.ai_api_key_env.trim().to_string(),
                    enabled: !(self.field_buffers.ai_provider.trim().is_empty()
                        && self.field_buffers.ai_model.trim().is_empty()),
                };
                self.draft_config.ai.embedding = provider.clone();
                self.draft_config.ai.extraction = provider.clone();
                self.draft_config.ai.drafting = provider;
            }
            OnboardingStep::SmtpIdentity => {
                let smtp_name = self.field_buffers.smtp_name.trim().to_string();
                let smtp_email_address = self.field_buffers.smtp_email_address.trim().to_string();
                let smtp_host = self.field_buffers.smtp_host.trim().to_string();
                let smtp_port = self.field_buffers.smtp_port.trim().to_string();
                let smtp_username = self.field_buffers.smtp_username.trim().to_string();
                let smtp_password_env = self.field_buffers.smtp_password_env.trim().to_string();
                let smtp_tls_mode = self.field_buffers.smtp_tls_mode.trim().to_string();

                if smtp_host.is_empty() {
                    self.draft_config.smtp.clear();
                    return;
                }

                let fallback_name = self.field_buffers.account_name.trim().to_string();
                let smtp = self.ensure_smtp();
                smtp.name = if smtp_name.is_empty() {
                    fallback_name
                } else {
                    smtp_name
                };
                smtp.email_address = smtp_email_address;
                smtp.host = smtp_host;
                smtp.port = smtp_port;
                smtp.username = smtp_username;
                smtp.password_env = smtp_password_env;
                smtp.tls_mode = smtp_tls_mode;
                smtp.default = true;
            }
            OnboardingStep::Review => {
                match write_config_from_onboarding(&self.config_path, &self.draft_config) {
                    Ok(()) => {
                        self.status_message = Some(format!(
                        "Wrote config to {}. Next: cour doctor, cour reindex, then set your AI/SMTP env vars.",
                        self.config_path.display()
                    ));
                    }
                    Err(error) => {
                        self.validation_errors.push(error.to_string());
                        return;
                    }
                }
            }
            OnboardingStep::Complete => {
                self.running = false;
            }
        }
    }

    fn ensure_account(&mut self) -> &mut DraftAccountConfig {
        if self.draft_config.accounts.is_empty() {
            self.draft_config
                .accounts
                .push(DraftAccountConfig::default());
        }
        &mut self.draft_config.accounts[0]
    }

    fn ensure_smtp(&mut self) -> &mut DraftSmtpIdentity {
        if self.draft_config.smtp.is_empty() {
            self.draft_config.smtp.push(DraftSmtpIdentity::default());
        }
        &mut self.draft_config.smtp[0]
    }
}

pub fn run_onboarding(config_path: &Path) -> AppResult<()> {
    enable_raw_mode().map_err(AppError::Io)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(AppError::Io)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(AppError::Io)?;
    let mut state = OnboardingState::new(config_path.to_path_buf());

    let result = onboarding_loop(&mut terminal, &mut state);

    disable_raw_mode().map_err(AppError::Io)?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(AppError::Io)?;
    terminal.show_cursor().map_err(AppError::Io)?;

    result
}

fn onboarding_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut OnboardingState,
) -> AppResult<()> {
    while state.running {
        terminal
            .draw(|frame| render_onboarding(frame, state))
            .map_err(AppError::Io)?;

        if event::poll(Duration::from_millis(100)).map_err(AppError::Io)? {
            if let Event::Key(key) = event::read().map_err(AppError::Io)? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                handle_key(state, key.code);
            }
        }
    }

    Ok(())
}

fn handle_key(state: &mut OnboardingState, code: KeyCode) {
    match code {
        KeyCode::Char('q') => state.running = false,
        KeyCode::Esc => state.go_back(),
        KeyCode::Tab | KeyCode::Down => state.move_field(1),
        KeyCode::BackTab | KeyCode::Up => state.move_field(-1),
        KeyCode::Enter => {
            if enter_moves_to_next_field(state) {
                state.move_field(1);
            } else {
                state.try_advance();
            }
        }
        KeyCode::Backspace if supports_text_input(state.current_step) => {
            active_buffer_mut(state).pop();
        }
        KeyCode::Char(character)
            if !character.is_control() && supports_text_input(state.current_step) =>
        {
            active_buffer_mut(state).push(character)
        }
        _ => {}
    }
}

fn enter_moves_to_next_field(state: &OnboardingState) -> bool {
    supports_text_input(state.current_step)
        && field_count_for(state.current_step) > 1
        && state.selected_field + 1 < field_count_for(state.current_step)
}

fn supports_text_input(step: OnboardingStep) -> bool {
    matches!(
        step,
        OnboardingStep::AccountBasics
            | OnboardingStep::MaildirPath
            | OnboardingStep::SyncCommand
            | OnboardingStep::AiProviders
            | OnboardingStep::SmtpIdentity
    )
}

fn field_count_for(step: OnboardingStep) -> usize {
    match step {
        OnboardingStep::Welcome => 0,
        OnboardingStep::AccountBasics => 2,
        OnboardingStep::MaildirPath => 1,
        OnboardingStep::SetupMode => 2,
        OnboardingStep::SyncCommand => 1,
        OnboardingStep::AiProviders => 4,
        OnboardingStep::SmtpIdentity => 7,
        OnboardingStep::Review => 0,
        OnboardingStep::Complete => 0,
    }
}

fn starting_field_for(step: OnboardingStep) -> usize {
    match step {
        OnboardingStep::AccountBasics => 0,
        OnboardingStep::SmtpIdentity => 2,
        _ => 0,
    }
}

fn active_buffer_mut(state: &mut OnboardingState) -> &mut String {
    match state.current_step {
        OnboardingStep::AccountBasics => match state.selected_field {
            0 => &mut state.field_buffers.account_name,
            _ => &mut state.field_buffers.email_address,
        },
        OnboardingStep::MaildirPath => &mut state.field_buffers.maildir_root,
        OnboardingStep::SetupMode => &mut state.field_buffers.account_name,
        OnboardingStep::SyncCommand => &mut state.field_buffers.sync_command,
        OnboardingStep::AiProviders => match state.selected_field {
            0 => &mut state.field_buffers.ai_provider,
            1 => &mut state.field_buffers.ai_model,
            2 => &mut state.field_buffers.ai_api_url,
            _ => &mut state.field_buffers.ai_api_key_env,
        },
        OnboardingStep::SmtpIdentity => match state.selected_field {
            0 => &mut state.field_buffers.smtp_name,
            1 => &mut state.field_buffers.smtp_email_address,
            2 => &mut state.field_buffers.smtp_host,
            3 => &mut state.field_buffers.smtp_port,
            4 => &mut state.field_buffers.smtp_username,
            5 => &mut state.field_buffers.smtp_password_env,
            _ => &mut state.field_buffers.smtp_tls_mode,
        },
        OnboardingStep::Welcome | OnboardingStep::Review | OnboardingStep::Complete => {
            &mut state.field_buffers.account_name
        }
    }
}

fn render_onboarding(frame: &mut ratatui::Frame, state: &OnboardingState) {
    let theme = Theme::default();
    let area = frame.area();

    frame.render_widget(
        Block::default().style(ratatui::style::Style::default().bg(theme.bg)),
        area,
    );

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    render_header(frame, header, &theme, state);
    render_body(frame, body, &theme, state);
    render_footer(frame, footer, &theme, state);
}

fn render_header(frame: &mut ratatui::Frame, area: Rect, theme: &Theme, state: &OnboardingState) {
    let lines = vec![
        Line::from(vec![
            Span::styled(" cour setup ", theme.title),
            Span::styled("· local-first onboarding wizard", theme.text_muted),
        ]),
        Line::from(vec![
            Span::styled("Step: ", theme.text_muted),
            Span::styled(state.current_step.title(), theme.text_accent),
            Span::styled("  Config path: ", theme.text_muted),
            Span::styled(state.config_path.display().to_string(), theme.text),
        ]),
    ];

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme.border),
    );
    frame.render_widget(widget, area);
}

fn render_body(frame: &mut ratatui::Frame, area: Rect, theme: &Theme, state: &OnboardingState) {
    let body = Paragraph::new(step_lines(state, theme))
        .style(theme.text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border)
                .title(format!(" {} ", state.current_step.title())),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_footer(frame: &mut ratatui::Frame, area: Rect, theme: &Theme, state: &OnboardingState) {
    let footer_text = state
        .status_message
        .clone()
        .unwrap_or_else(|| footer_help(state.current_step).to_string());
    let widget = Paragraph::new(footer_text).style(theme.keyhint_desc).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.border),
    );
    frame.render_widget(widget, area);
}

fn footer_help(step: OnboardingStep) -> &'static str {
    match step {
        OnboardingStep::Welcome => "Enter continue  q quit",
        OnboardingStep::AccountBasics | OnboardingStep::AiProviders | OnboardingStep::SmtpIdentity => {
            "Enter next field or continue  Tab/Down next field  Shift+Tab/Up previous  Esc back  q quit"
        }
        OnboardingStep::MaildirPath => {
            "Type path  Enter continue — cour creates it if missing  Esc back  q quit"
        }
        OnboardingStep::SetupMode => {
            "Tab/Down choose setup style  Enter continue  Esc back  q quit"
        }
        OnboardingStep::SyncCommand => "Type to edit  Enter continue  Esc back  q quit",
        OnboardingStep::Review => "Enter write config  Esc back  q quit",
        OnboardingStep::Complete => "Enter finish  Esc back  q quit",
    }
}

fn step_lines(state: &OnboardingState, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = match state.current_step {
        OnboardingStep::Welcome => vec![
            Line::from(vec![Span::styled(
                "Welcome to cour. We'll start with the basics and keep advanced mail settings out of your way.",
                theme.text,
            )]),
            Line::from(""),
            Line::from("Recommended setup asks for just a few things:"),
            Line::from("• an account name you recognize"),
            Line::from("• your email address"),
            Line::from("• where cour should keep your local mail"),
            Line::from(""),
            Line::from("Advanced options for mail download, AI, and custom sending settings are available later."),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press Enter to continue.",
                theme.text_accent,
            )]),
        ],
        OnboardingStep::AccountBasics => vec![
            field_line(
                "Account name",
                &state.field_buffers.account_name,
                state.selected_field == 0,
                theme,
            ),
            field_line(
                "Email address",
                &state.field_buffers.email_address,
                state.selected_field == 1,
                theme,
            ),
            Line::from(""),
            Line::from("Account name is just a label inside cour, like Personal or Work."),
            Line::from("Press Enter to move to the next field, then Enter again to continue."),
        ],
        OnboardingStep::MaildirPath => vec![
            field_line(
                "Maildir root",
                &state.field_buffers.maildir_root,
                true,
                theme,
            ),
            Line::from(""),
            Line::from("Choose where cour should keep this account's local mail."),
            Line::from("We prefilled a suggested location based on this account."),
            Line::from("If this folder does not exist yet, cour will create it for you automatically."),
            Line::from("That includes the required cur/, new/, and tmp/ Maildir folders."),
        ],
        OnboardingStep::SetupMode => vec![
            choice_line(
                "Simple setup (recommended)",
                "Set up local mail storage now. Add advanced options later.",
                state.selected_field == 0,
                theme,
            ),
            choice_line(
                "Advanced setup",
                "Configure mail download, AI providers, and custom sending settings now.",
                state.selected_field == 1,
                theme,
            ),
        ],
        OnboardingStep::SyncCommand => vec![
            field_line(
                "Download command",
                &state.field_buffers.sync_command,
                true,
                theme,
            ),
            Line::from(""),
            Line::from("Optional advanced step."),
            Line::from("Only fill this in if you already use a tool like mbsync or offlineimap."),
        ],
        OnboardingStep::AiProviders => vec![
            field_line(
                "Provider",
                &state.field_buffers.ai_provider,
                state.selected_field == 0,
                theme,
            ),
            field_line(
                "Model",
                &state.field_buffers.ai_model,
                state.selected_field == 1,
                theme,
            ),
            field_line(
                "API URL",
                &state.field_buffers.ai_api_url,
                state.selected_field == 2,
                theme,
            ),
            field_line(
                "API key env",
                &state.field_buffers.ai_api_key_env,
                state.selected_field == 3,
                theme,
            ),
            Line::from(""),
            Line::from("Optional advanced step. Leave these blank if you do not want AI features yet."),
            Line::from("Enter moves to the next field before continuing."),
        ],
        OnboardingStep::SmtpIdentity => {
            let mut lines = vec![
                field_line("Sending name", &state.field_buffers.smtp_name, state.selected_field == 0, theme),
                field_line(
                    "Email address",
                    &state.field_buffers.smtp_email_address,
                    state.selected_field == 1,
                    theme,
                ),
                field_line("SMTP host", &state.field_buffers.smtp_host, state.selected_field == 2, theme),
                field_line("SMTP port", &state.field_buffers.smtp_port, state.selected_field == 3, theme),
                field_line(
                    "Username",
                    &state.field_buffers.smtp_username,
                    state.selected_field == 4,
                    theme,
                ),
                field_line(
                    "Password env",
                    &state.field_buffers.smtp_password_env,
                    state.selected_field == 5,
                    theme,
                ),
                field_line(
                    "TLS mode",
                    &state.field_buffers.smtp_tls_mode,
                    state.selected_field == 6,
                    theme,
                ),
                Line::from(""),
            ];
            if let Some(provider) = detect_mail_provider(&state.field_buffers.email_address) {
                lines.push(Line::from(format!(
                    "cour detected {} settings and prefilled the SMTP host, port, and TLS mode.",
                    provider.display_name
                )));
            }
            lines.push(Line::from(
                "Advanced sending setup. Leave blank if you want to configure sending later.",
            ));
            lines.push(Line::from("Enter moves to the next field before continuing."));
            lines
        },
        OnboardingStep::Review => review_lines(state),
        OnboardingStep::Complete => vec![
            Line::from("Setup complete."),
            Line::from(""),
            Line::from(format!("Config written to {}", state.config_path.display())),
            Line::from(""),
            Line::from("Next steps:"),
            Line::from("• run cour doctor"),
            Line::from("• run cour reindex"),
            Line::from("• rerun cour setup any time to add advanced sync, AI, or sending options"),
        ],
    };

    if !state.validation_errors.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Validation",
            theme.text_error,
        )]));
        for error in &state.validation_errors {
            lines.push(Line::from(vec![Span::styled(
                format!("• {error}"),
                theme.text_error,
            )]));
        }
    }

    lines
}

fn review_lines(state: &OnboardingState) -> Vec<Line<'static>> {
    let account = state.draft_config.accounts.first();
    let smtp = state.draft_config.smtp.first();
    let ai = &state.draft_config.ai.drafting;
    let sync_command = account
        .map(|entry| entry.sync_command.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("not configured");
    let smtp_summary = if let Some(smtp) = smtp {
        format!(
            "{}:{} user={} password_env={}",
            empty_as_placeholder(&smtp.host),
            empty_as_placeholder(&smtp.port),
            empty_as_placeholder(&smtp.username),
            empty_as_placeholder(&smtp.password_env),
        )
    } else {
        "not configured".to_string()
    };

    let mut lines = vec![
        Line::from("Review config to be written:"),
        Line::from(""),
        Line::from(format!(
            "Setup style: {}",
            if state.advanced_setup {
                "advanced"
            } else {
                "simple"
            }
        )),
        Line::from(format!(
            "Account: {} <{}>",
            account.map(|a| a.name.as_str()).unwrap_or(""),
            account.map(|a| a.email_address.as_str()).unwrap_or(""),
        )),
        Line::from(format!(
            "Maildir: {}",
            account.map(|a| a.maildir_root.as_str()).unwrap_or(""),
        )),
        Line::from(format!("Mail download: {sync_command}")),
        Line::from(format!(
            "AI features: provider={} model={} api_url={} key_env={}",
            empty_as_placeholder(&ai.provider),
            empty_as_placeholder(&ai.model),
            empty_as_placeholder(&ai.api_url),
            empty_as_placeholder(&ai.api_key_env),
        )),
        Line::from(format!("Sending: {smtp_summary}")),
        Line::from(""),
    ];

    if !state.advanced_setup {
        lines.push(Line::from(
            "You chose simple setup. You can add mail download, AI, or sending settings later with cour setup.",
        ));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(
        "Press Enter to write the config file and continue.",
    ));
    lines
}

fn field_line(label: &str, value: &str, active: bool, theme: &Theme) -> Line<'static> {
    let prefix = if active { "› " } else { "  " };
    let value_text = if active {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            "█".to_string()
        } else {
            format!("{trimmed}█")
        }
    } else {
        empty_as_placeholder(value).to_string()
    };

    Line::from(vec![
        Span::styled(
            prefix,
            if active {
                theme.text_accent
            } else {
                theme.text_dim
            },
        ),
        Span::styled(
            format!("{label:<14}"),
            if active {
                theme.text_accent
            } else {
                theme.text_muted
            },
        ),
        Span::styled(" ", theme.text_dim),
        Span::styled(value_text, theme.text),
    ])
}

fn choice_line(title: &str, detail: &str, active: bool, theme: &Theme) -> Line<'static> {
    let prefix = if active { "◉ " } else { "○ " };

    Line::from(vec![
        Span::styled(
            prefix,
            if active {
                theme.text_accent
            } else {
                theme.text_dim
            },
        ),
        Span::styled(
            title.to_string(),
            if active {
                theme.text_accent
            } else {
                theme.text
            },
        ),
        Span::styled(" — ", theme.text_dim),
        Span::styled(detail.to_string(), theme.text_muted),
    ])
}

fn empty_as_placeholder(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "(empty)"
    } else {
        trimmed
    }
}

fn ensure_maildir_root(maildir_root: &str) -> Result<(), String> {
    let trimmed = maildir_root.trim();
    if trimmed.is_empty() {
        return Err("Maildir root is required.".to_string());
    }

    let root = Path::new(trimmed);
    if root.exists() && !root.is_dir() {
        return Err("Maildir root must be a directory path.".to_string());
    }

    std::fs::create_dir_all(root)
        .map_err(|error| format!("Failed to create Maildir root: {error}"))?;

    for required_dir in ["cur", "new", "tmp"] {
        let path = root.join(required_dir);
        std::fs::create_dir_all(&path).map_err(|error| {
            format!("Failed to create Maildir {required_dir}/ directory: {error}")
        })?;
    }

    Ok(())
}

fn default_maildir_root(account_name: &str, email_address: &str) -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let label = if account_name.trim().is_empty() {
        email_address
            .split('@')
            .next()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("mail")
    } else {
        account_name
    };
    let folder_name = slugify_maildir_label(label);
    Some(home.join("Mail").join(folder_name).display().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectedMailProvider {
    display_name: &'static str,
    smtp_host: &'static str,
    smtp_port: &'static str,
    tls_mode: &'static str,
}

fn detect_mail_provider(email_address: &str) -> Option<DetectedMailProvider> {
    let domain = email_address.split('@').nth(1)?.trim().to_ascii_lowercase();

    match domain.as_str() {
        "gmail.com" | "googlemail.com" => Some(DetectedMailProvider {
            display_name: "Gmail",
            smtp_host: "smtp.gmail.com",
            smtp_port: "465",
            tls_mode: "tls",
        }),
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" | "office365.com" => {
            Some(DetectedMailProvider {
                display_name: "Outlook",
                smtp_host: "smtp.office365.com",
                smtp_port: "587",
                tls_mode: "starttls",
            })
        }
        "yahoo.com" | "yahoo.co.uk" | "ymail.com" => Some(DetectedMailProvider {
            display_name: "Yahoo",
            smtp_host: "smtp.mail.yahoo.com",
            smtp_port: "465",
            tls_mode: "tls",
        }),
        "icloud.com" | "me.com" | "mac.com" => Some(DetectedMailProvider {
            display_name: "iCloud",
            smtp_host: "smtp.mail.me.com",
            smtp_port: "587",
            tls_mode: "starttls",
        }),
        "fastmail.com" | "fastmail.fm" => Some(DetectedMailProvider {
            display_name: "Fastmail",
            smtp_host: "smtp.fastmail.com",
            smtp_port: "465",
            tls_mode: "tls",
        }),
        "purelymail.com" => Some(DetectedMailProvider {
            display_name: "Purelymail",
            smtp_host: "smtp.purelymail.com",
            smtp_port: "465",
            tls_mode: "tls",
        }),
        _ => None,
    }
}

fn prefill_smtp_settings(email_address: &str, field_buffers: &mut FieldBuffers) {
    let Some(provider) = detect_mail_provider(email_address) else {
        return;
    };

    if field_buffers.smtp_host.trim().is_empty() {
        field_buffers.smtp_host = provider.smtp_host.to_string();
    }
    if field_buffers.smtp_port.trim().is_empty() || field_buffers.smtp_port.trim() == "465" {
        field_buffers.smtp_port = provider.smtp_port.to_string();
    }
    if field_buffers.smtp_tls_mode.trim().is_empty() || field_buffers.smtp_tls_mode.trim() == "tls"
    {
        field_buffers.smtp_tls_mode = provider.tls_mode.to_string();
    }
    if field_buffers.smtp_username.trim().is_empty() {
        field_buffers.smtp_username = email_address.trim().to_string();
    }
}

fn slugify_maildir_label(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_separator = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "mail".to_string()
    } else {
        trimmed.to_string()
    }
}

fn provider_block(name: &str, provider: &DraftProviderConfig) -> Option<String> {
    let provider_name = provider.provider.trim();
    if provider_name.is_empty() || provider_name.eq_ignore_ascii_case("disabled") {
        return None;
    }

    let mut lines = vec![
        format!("[ai.{name}]"),
        format!("provider = {:?}", provider_name),
    ];
    if !provider.model.trim().is_empty() {
        lines.push(format!("model = {:?}", provider.model.trim()));
    }
    if !provider.api_url.trim().is_empty() {
        lines.push(format!("api_url = {:?}", provider.api_url.trim()));
    }
    if !provider.api_key_env.trim().is_empty() {
        lines.push(format!("api_key_env = {:?}", provider.api_key_env.trim()));
    }
    lines.push(format!("enabled = {}", provider.enabled));
    Some(lines.join("\n"))
}

fn write_config_from_onboarding(config_path: &Path, draft: &DraftConfigModel) -> AppResult<()> {
    let mut sections = Vec::new();

    let account_lines = draft
        .accounts
        .iter()
        .map(|account| {
            let mut fields = vec![
                format!("name = {:?}", account.name),
                format!("email_address = {:?}", account.email_address),
                format!("maildir_root = {:?}", account.maildir_root),
            ];
            if !account.sync_command.trim().is_empty() {
                fields.push(format!("sync_command = {:?}", account.sync_command.trim()));
            }
            if account.default {
                fields.push("default = true".to_string());
            }
            format!("  {{ {} }}", fields.join(", "))
        })
        .collect::<Vec<_>>();

    let accounts_toml = if account_lines.is_empty() {
        "accounts = []".to_string()
    } else {
        format!("accounts = [\n{}\n]", account_lines.join(",\n"))
    };
    sections.push(accounts_toml);

    for (name, provider) in [
        ("embedding", &draft.ai.embedding),
        ("extraction", &draft.ai.extraction),
        ("drafting", &draft.ai.drafting),
    ] {
        if let Some(block) = provider_block(name, provider) {
            sections.push(block);
        }
    }

    for smtp in &draft.smtp {
        if smtp.name.trim().is_empty() && smtp.host.trim().is_empty() {
            continue;
        }
        let mut lines = vec!["[[smtp]]".to_string()];
        lines.push(format!("name = {:?}", smtp.name.trim()));
        lines.push(format!("email_address = {:?}", smtp.email_address.trim()));
        lines.push(format!("host = {:?}", smtp.host.trim()));
        let port = smtp.port.trim().parse::<u16>().unwrap_or(465);
        lines.push(format!("port = {port}"));
        if !smtp.username.trim().is_empty() {
            lines.push(format!("username = {:?}", smtp.username.trim()));
        }
        if !smtp.password_env.trim().is_empty() {
            lines.push(format!("password_env = {:?}", smtp.password_env.trim()));
        }
        if !smtp.tls_mode.trim().is_empty() {
            lines.push(format!("tls_mode = {:?}", smtp.tls_mode.trim()));
        }
        lines.push(format!("default = {}", smtp.default));
        sections.push(lines.join("\n"));
    }

    let raw = sections.join("\n\n") + "\n";
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }
    std::fs::write(config_path, raw).map_err(AppError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::test_support::TestEnvGuard;
    use crossterm::event::KeyCode;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_maildir_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("cour-onboarding-{name}-{unique}"))
    }

    fn make_maildir(root: &Path) {
        fs::create_dir_all(root.join("cur")).unwrap();
        fs::create_dir_all(root.join("new")).unwrap();
        fs::create_dir_all(root.join("tmp")).unwrap();
    }

    #[test]
    fn creates_missing_maildir_root_with_cur_new_tmp() {
        let root = temp_maildir_path("missing");

        let result = ensure_maildir_root(root.to_string_lossy().as_ref());

        assert!(result.is_ok(), "unexpected creation error: {result:?}");
        assert!(root.join("cur").is_dir());
        assert!(root.join("new").is_dir());
        assert!(root.join("tmp").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_maildir_root_with_cur_new_tmp() {
        let root = temp_maildir_path("valid");
        make_maildir(&root);

        let result = ensure_maildir_root(root.to_string_lossy().as_ref());

        assert!(result.is_ok(), "unexpected validation error: {result:?}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_maildir_root_when_path_is_a_file() {
        let root = temp_maildir_path("file");
        fs::write(&root, "not a directory").unwrap();

        let error = ensure_maildir_root(root.to_string_lossy().as_ref()).unwrap_err();

        assert!(
            error.contains("directory path"),
            "unexpected error: {error}"
        );
        fs::remove_file(root).unwrap();
    }

    #[test]
    fn persists_account_and_maildir_values_when_steps_advance() {
        let root = temp_maildir_path("progression");

        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));
        state.try_advance();
        assert_eq!(state.current_step, OnboardingStep::AccountBasics);

        state.field_buffers.account_name = "personal".to_string();
        state.field_buffers.email_address = "me@example.com".to_string();
        state.try_advance();
        assert_eq!(state.current_step, OnboardingStep::MaildirPath);

        let account = state.draft_config.accounts.first().unwrap();
        assert_eq!(account.name, "personal");
        assert_eq!(account.email_address, "me@example.com");

        state.field_buffers.maildir_root = root.to_string_lossy().to_string();
        state.try_advance();
        assert_eq!(state.current_step, OnboardingStep::SetupMode);
        assert_eq!(
            state.draft_config.accounts.first().unwrap().maildir_root,
            root.to_string_lossy()
        );
        assert!(root.join("cur").is_dir());
        assert!(root.join("new").is_dir());
        assert!(root.join("tmp").is_dir());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn account_basics_starts_with_account_name_selected() {
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));

        state.try_advance();

        assert_eq!(state.current_step, OnboardingStep::AccountBasics);
        assert_eq!(state.selected_field, 0);
        assert_eq!(state.field_buffers.account_name, "");
    }

    #[test]
    fn tab_navigation_changes_which_field_receives_input() {
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));
        state.current_step = OnboardingStep::AccountBasics;
        state.selected_field = 0;

        handle_key(&mut state, KeyCode::Char('W'));
        handle_key(&mut state, KeyCode::Char('o'));
        handle_key(&mut state, KeyCode::Char('r'));
        handle_key(&mut state, KeyCode::Char('k'));
        assert_eq!(state.field_buffers.account_name, "Work");
        assert_eq!(state.field_buffers.email_address, "");

        handle_key(&mut state, KeyCode::Tab);
        handle_key(&mut state, KeyCode::Char('m'));
        handle_key(&mut state, KeyCode::Char('e'));
        handle_key(&mut state, KeyCode::Char('@'));

        assert_eq!(state.field_buffers.account_name, "Work");
        assert_eq!(state.field_buffers.email_address, "me@");
    }

    #[test]
    fn enter_moves_between_fields_before_advancing_step() {
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));
        state.current_step = OnboardingStep::AccountBasics;
        state.selected_field = 0;
        state.field_buffers.account_name = "Work".to_string();
        state.field_buffers.email_address = "me@example.com".to_string();

        handle_key(&mut state, KeyCode::Enter);
        assert_eq!(state.current_step, OnboardingStep::AccountBasics);
        assert_eq!(state.selected_field, 1);

        handle_key(&mut state, KeyCode::Enter);
        assert_eq!(state.current_step, OnboardingStep::MaildirPath);
    }

    #[test]
    fn welcome_step_ignores_text_input() {
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));

        handle_key(&mut state, KeyCode::Char('x'));

        assert_eq!(state.current_step, OnboardingStep::Welcome);
        assert_eq!(state.field_buffers.account_name, "");
    }

    #[test]
    fn simple_setup_skips_advanced_steps() {
        let root = temp_maildir_path("simple");
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));

        state.try_advance();
        state.field_buffers.account_name = "Personal".to_string();
        state.field_buffers.email_address = "me@example.com".to_string();
        state.try_advance();
        state.field_buffers.maildir_root = root.to_string_lossy().to_string();
        state.try_advance();

        assert_eq!(state.current_step, OnboardingStep::SetupMode);
        assert_eq!(state.selected_field, 0);

        state.try_advance();

        assert_eq!(state.current_step, OnboardingStep::Review);
        assert!(!state.advanced_setup);
        assert!(state.draft_config.smtp.is_empty());
        assert!(state.draft_config.accounts[0].sync_command.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn advanced_setup_includes_advanced_steps() {
        let root = temp_maildir_path("advanced");
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));

        state.try_advance();
        state.field_buffers.account_name = "Personal".to_string();
        state.field_buffers.email_address = "me@example.com".to_string();
        state.try_advance();
        state.field_buffers.maildir_root = root.to_string_lossy().to_string();
        state.try_advance();
        state.selected_field = 1;

        state.try_advance();

        assert_eq!(state.current_step, OnboardingStep::SyncCommand);
        assert!(state.advanced_setup);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn account_basics_prefills_maildir_and_detected_smtp_settings() {
        let home = temp_maildir_path("home");
        fs::create_dir_all(&home).unwrap();
        let mut env_guard = TestEnvGuard::acquire();
        env_guard.set_var("HOME", &home);

        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));
        state.current_step = OnboardingStep::AccountBasics;
        state.field_buffers.account_name = "Personal Inbox".to_string();
        state.field_buffers.email_address = "me@gmail.com".to_string();

        state.capture_current_step();

        assert!(state
            .field_buffers
            .maildir_root
            .contains("Mail/personal-inbox"));
        assert_eq!(state.field_buffers.smtp_host, "smtp.gmail.com");
        assert_eq!(state.field_buffers.smtp_port, "465");
        assert_eq!(state.field_buffers.smtp_tls_mode, "tls");
        assert_eq!(state.field_buffers.smtp_username, "me@gmail.com");

        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn account_basics_detects_purelymail_defaults() {
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));
        state.current_step = OnboardingStep::AccountBasics;
        state.field_buffers.account_name = "Work".to_string();
        state.field_buffers.email_address = "me@purelymail.com".to_string();

        state.capture_current_step();

        assert_eq!(state.field_buffers.smtp_host, "smtp.purelymail.com");
        assert_eq!(state.field_buffers.smtp_port, "465");
        assert_eq!(state.field_buffers.smtp_tls_mode, "tls");
        assert_eq!(state.field_buffers.smtp_username, "me@purelymail.com");
    }

    #[test]
    fn blank_smtp_host_keeps_sending_unconfigured() {
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));
        state.current_step = OnboardingStep::SmtpIdentity;
        state.field_buffers.smtp_name = "Personal".to_string();
        state.field_buffers.smtp_email_address = "me@example.com".to_string();
        state.field_buffers.smtp_host = String::new();

        state.capture_current_step();

        assert!(state.draft_config.smtp.is_empty());
    }

    #[test]
    fn builds_provider_config_without_storing_secret_value() {
        let mut state = OnboardingState::new(PathBuf::from("/tmp/cour.toml"));
        state.current_step = OnboardingStep::AiProviders;
        state.field_buffers.ai_provider = "openai-compatible".to_string();
        state.field_buffers.ai_model = "gpt-4o-mini".to_string();
        state.field_buffers.ai_api_url = "https://api.openai.com/v1".to_string();
        state.field_buffers.ai_api_key_env = "OPENAI_API_KEY".to_string();

        state.capture_current_step();

        assert_eq!(
            state.draft_config.ai.embedding.provider,
            "openai-compatible"
        );
        assert_eq!(state.draft_config.ai.embedding.model, "gpt-4o-mini");
        assert_eq!(
            state.draft_config.ai.embedding.api_key_env,
            "OPENAI_API_KEY"
        );
        assert_ne!(
            state.draft_config.ai.embedding.api_key_env,
            "sk-secret-value"
        );
    }

    #[test]
    fn writes_valid_config_toml_from_completed_onboarding() {
        let root = temp_maildir_path("write-config");
        let maildir = root.join("Maildir");
        make_maildir(&maildir);
        let config_path = root.join("config.toml");

        let draft = DraftConfigModel {
            accounts: vec![DraftAccountConfig {
                name: "personal".to_string(),
                email_address: "me@example.com".to_string(),
                maildir_root: maildir.to_string_lossy().to_string(),
                sync_command: "mbsync personal".to_string(),
                default: true,
            }],
            ai: DraftAiConfig {
                embedding: DraftProviderConfig {
                    provider: "ollama".to_string(),
                    model: "nomic-embed-text".to_string(),
                    api_url: "http://localhost:11434".to_string(),
                    api_key_env: String::new(),
                    enabled: true,
                },
                extraction: DraftProviderConfig {
                    provider: "openai-compatible".to_string(),
                    model: "gpt-4o-mini".to_string(),
                    api_url: "https://api.openai.com/v1".to_string(),
                    api_key_env: "OPENAI_API_KEY".to_string(),
                    enabled: true,
                },
                drafting: DraftProviderConfig {
                    provider: "openai-compatible".to_string(),
                    model: "gpt-4o-mini".to_string(),
                    api_url: "https://api.openai.com/v1".to_string(),
                    api_key_env: "OPENAI_API_KEY".to_string(),
                    enabled: true,
                },
            },
            smtp: vec![DraftSmtpIdentity {
                name: "default".to_string(),
                email_address: "me@example.com".to_string(),
                host: "smtp.example.com".to_string(),
                port: "465".to_string(),
                username: "me@example.com".to_string(),
                password_env: "COUR_SMTP_PASSWORD".to_string(),
                tls_mode: "tls".to_string(),
                default: true,
            }],
        };

        write_config_from_onboarding(&config_path, &draft).expect("write config");
        let loaded = AppConfig::load(Some(&config_path)).expect("load written config");

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].name, "personal");
        assert_eq!(loaded.ai.embedding.as_ref().unwrap().provider, "ollama");
        assert_eq!(
            loaded
                .ai
                .extraction
                .as_ref()
                .unwrap()
                .api_key_env
                .as_deref(),
            Some("OPENAI_API_KEY")
        );
        assert_eq!(
            loaded.smtp[0].password_env.as_deref(),
            Some("COUR_SMTP_PASSWORD")
        );

        fs::remove_dir_all(root).unwrap();
    }
}
