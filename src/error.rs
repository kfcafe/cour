use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(String),
    #[error("sync command failed: {0}")]
    SyncCommand(String),
    #[error("parse error: {0}")]
    Parsing(String),
    #[error("AI error: {0}")]
    Ai(String),
    #[error("send error: {0}")]
    Send(String),
}
