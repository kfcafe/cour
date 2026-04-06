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
            Self::MaildirPath => "Maildir path",
            Self::SyncCommand => "Sync command",
            Self::AiProviders => "AI providers",
            Self::SmtpIdentity => "SMTP identity",
            Self::Review => "Review",
            Self::Complete => "Complete",
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Welcome => Some(Self::AccountBasics),
            Self::AccountBasics => Some(Self::MaildirPath),
            Self::MaildirPath => Some(Self::SyncCommand),
            Self::SyncCommand => Some(Self::AiProviders),
            Self::AiProviders => Some(Self::SmtpIdentity),
            Self::SmtpIdentity => Some(Self::Review),
            Self::Review => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    fn previous(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::AccountBasics => Some(Self::Welcome),
            Self::MaildirPath => Some(Self::AccountBasics),
            Self::SyncCommand => Some(Self::MaildirPath),
            Self::AiProviders => Some(Self::SyncCommand),
            Self::SmtpIdentity => Some(Self::AiProviders),
            Self::Review => Some(Self::SmtpIdentity),
            Self::Complete => Some(Self::Review),
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
            account_name: "personal".to_string(),
            email_address: String::new(),
            maildir_root: String::new(),
            sync_command: String::new(),
            ai_provider: "local".to_string(),
            ai_model: String::new(),
            ai_api_url: String::new(),
            ai_api_key_env: String::new(),
            smtp_name: "default".to_string(),
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
        }
    }

    fn go_back(&mut self) {
        self.validation_errors.clear();
        self.status_message = None;
        if let Some(previous) = self.current_step.previous() {
            self.current_step = previous;
        }
    }

    fn try_advance(&mut self) {
        self.validation_errors.clear();
        self.status_message = None;

        if !self.validate_current_step() {
            return;
        }

        self.capture_current_step();

        match self.current_step.next() {
            Some(next) => self.current_step = next,
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
                self.validate_maildir_step();
                self.validation_errors.is_empty()
            }
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

    fn validate_maildir_step(&mut self) {
        match validate_maildir_root(self.field_buffers.maildir_root.trim()) {
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
                account.name = account_name;
                account.email_address = email.clone();
                account.default = true;

                if self.field_buffers.smtp_email_address.trim().is_empty() {
                    self.field_buffers.smtp_email_address = email.clone();
                }
                if self.field_buffers.smtp_username.trim().is_empty() {
                    self.field_buffers.smtp_username = email;
                }
            }
            OnboardingStep::MaildirPath => {
                let maildir_root = self.field_buffers.maildir_root.trim().to_string();
                let account = self.ensure_account();
                account.maildir_root = maildir_root;
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
                let smtp = self.ensure_smtp();
                smtp.name = smtp_name;
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
        KeyCode::Enter => state.try_advance(),
        KeyCode::Backspace => {
            active_buffer_mut(state).pop();
        }
        KeyCode::Char(character) => active_buffer_mut(state).push(character),
        _ => {}
    }
}

fn active_buffer_mut(state: &mut OnboardingState) -> &mut String {
    match state.current_step {
        OnboardingStep::AccountBasics => {
            if state.field_buffers.email_address.trim().is_empty() {
                &mut state.field_buffers.email_address
            } else {
                &mut state.field_buffers.account_name
            }
        }
        OnboardingStep::MaildirPath => &mut state.field_buffers.maildir_root,
        OnboardingStep::SyncCommand => &mut state.field_buffers.sync_command,
        OnboardingStep::AiProviders => &mut state.field_buffers.ai_model,
        OnboardingStep::SmtpIdentity => &mut state.field_buffers.smtp_host,
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
        .as_deref()
        .unwrap_or("Enter next when valid  Esc back  q quit");
    let widget = Paragraph::new(footer_text).style(theme.keyhint_desc).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.border),
    );
    frame.render_widget(widget, area);
}

fn step_lines(state: &OnboardingState, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = match state.current_step {
        OnboardingStep::Welcome => vec![
            Line::from(vec![Span::styled(
                "Welcome to cour. This setup flow will draft a local config before anything is written.",
                theme.text,
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Planned wizard scope:",
                theme.section_header,
            )]),
            Line::from("• one or more email accounts"),
            Line::from("• Maildir roots and optional sync commands"),
            Line::from("• local or remote AI providers, explicit about secrets"),
            Line::from("• SMTP identities for sending"),
        ],
        OnboardingStep::AccountBasics => vec![
            kv_line("Account name", &state.field_buffers.account_name),
            kv_line("Email address", &state.field_buffers.email_address),
            Line::from(""),
            Line::from("Enter advances only when required fields are present."),
        ],
        OnboardingStep::MaildirPath => vec![
            kv_line("Maildir root", &state.field_buffers.maildir_root),
            Line::from(""),
            Line::from("Use an existing Maildir path on disk. No files are modified in this step."),
        ],
        OnboardingStep::SyncCommand => vec![
            kv_line("Sync command", &state.field_buffers.sync_command),
            Line::from(""),
            Line::from("Optional: mbsync personal, offlineimap, or another local sync command."),
        ],
        OnboardingStep::AiProviders => vec![
            kv_line("Provider", &state.field_buffers.ai_provider),
            kv_line("Model", &state.field_buffers.ai_model),
            kv_line("API URL", &state.field_buffers.ai_api_url),
            kv_line("API key env", &state.field_buffers.ai_api_key_env),
            Line::from(""),
            Line::from("Secrets stay explicit: reference env vars instead of storing raw keys in config."),
        ],
        OnboardingStep::SmtpIdentity => vec![
            kv_line("SMTP name", &state.field_buffers.smtp_name),
            kv_line("Email address", &state.field_buffers.smtp_email_address),
            kv_line("Host", &state.field_buffers.smtp_host),
            kv_line("Port", &state.field_buffers.smtp_port),
            kv_line("Username", &state.field_buffers.smtp_username),
            kv_line("Password env", &state.field_buffers.smtp_password_env),
            kv_line("TLS mode", &state.field_buffers.smtp_tls_mode),
        ],
        OnboardingStep::Review => review_lines(state),
        OnboardingStep::Complete => vec![
            Line::from("Setup complete."),
            Line::from(""),
            Line::from(format!("Config written to {}", state.config_path.display())),
            Line::from(""),
            Line::from("Next steps:"),
            Line::from("• run cour doctor"),
            Line::from("• run cour reindex"),
            Line::from("• export the listed AI / SMTP env vars before using remote providers or send"),
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

    vec![
        Line::from("Review config to be written:"),
        Line::from(""),
        Line::from(format!(
            "Account: {} <{}>",
            account.map(|a| a.name.as_str()).unwrap_or("personal"),
            account.map(|a| a.email_address.as_str()).unwrap_or(""),
        )),
        Line::from(format!(
            "Maildir: {}",
            account.map(|a| a.maildir_root.as_str()).unwrap_or(""),
        )),
        Line::from(format!(
            "Sync: {}",
            account
                .map(|a| a.sync_command.as_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("(none)"),
        )),
        Line::from(format!(
            "AI: provider={} model={} api_url={} key_env={}",
            empty_as_placeholder(&ai.provider),
            empty_as_placeholder(&ai.model),
            empty_as_placeholder(&ai.api_url),
            empty_as_placeholder(&ai.api_key_env),
        )),
        Line::from(format!(
            "SMTP: {} {}:{} user={} password_env={}",
            smtp.map(|s| s.host.as_str()).unwrap_or(""),
            smtp.map(|s| s.port.as_str()).unwrap_or(""),
            smtp.map(|s| s.tls_mode.as_str()).unwrap_or(""),
            smtp.map(|s| s.username.as_str()).unwrap_or(""),
            smtp.map(|s| s.password_env.as_str()).unwrap_or(""),
        )),
        Line::from(""),
        Line::from("Press Enter to write the config file and continue."),
    ]
}

fn kv_line(label: &str, value: &str) -> Line<'static> {
    Line::from(format!("{label}: {}", empty_as_placeholder(value)))
}

fn empty_as_placeholder(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "(empty)"
    } else {
        trimmed
    }
}

fn validate_maildir_root(maildir_root: &str) -> Result<(), String> {
    let trimmed = maildir_root.trim();
    if trimmed.is_empty() {
        return Err("Maildir root is required.".to_string());
    }

    let root = Path::new(trimmed);
    if !root.exists() {
        return Err("Maildir root does not exist.".to_string());
    }

    for required_dir in ["cur", "new", "tmp"] {
        let path = root.join(required_dir);
        if !path.is_dir() {
            return Err(format!(
                "Maildir root must contain {required_dir}/ as a directory."
            ));
        }
    }

    Ok(())
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
    fn rejects_missing_maildir_root() {
        let missing = temp_maildir_path("missing");
        let error = validate_maildir_root(missing.to_string_lossy().as_ref()).unwrap_err();
        assert!(
            error.contains("does not exist"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn accepts_maildir_root_with_cur_new_tmp() {
        let root = temp_maildir_path("valid");
        make_maildir(&root);

        let result = validate_maildir_root(root.to_string_lossy().as_ref());

        assert!(result.is_ok(), "unexpected validation error: {result:?}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_account_and_maildir_values_when_steps_advance() {
        let root = temp_maildir_path("progression");
        make_maildir(&root);

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
        assert_eq!(state.current_step, OnboardingStep::SyncCommand);
        assert_eq!(
            state.draft_config.accounts.first().unwrap().maildir_root,
            root.to_string_lossy()
        );

        fs::remove_dir_all(root).unwrap();
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
