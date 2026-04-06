use clap::Parser;
use cour::commands::{dispatch, Cli};

#[tokio::main]
async fn main() -> Result<(), cour::error::AppError> {
    let cli = Cli::parse();
    dispatch(cli).await
}
