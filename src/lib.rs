mod commands;
mod with_tui;

use crate::commands::pomo::Pomo;
use clap::{Parser, Subcommand};
use std::error::Error;

#[derive(Parser, Debug)]
#[command(name = "kit", arg_required_else_help(true))]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(name = "pomo")]
    Pomo(Pomo),
}

impl Command {
    async fn run(&self) -> Result<(), Box<dyn Error>> {
        match self {
            Command::Pomo(pomo) => pomo.run().await,
        }
    }
}

#[tokio::main]
pub async fn kit_main() -> Result<(), Box<dyn Error>> {
    match Cli::parse().command {
        Some(command) => command.run().await,
        None => Ok(()),
    }
}
