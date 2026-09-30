mod capture;
mod cli;
mod daemon;
mod database;
mod event;
mod integrations;
mod paths;
mod service;
mod tray;

use anyhow::Result;
use clap::Parser;

fn main() {
    if let Err(error) = run() {
        eprintln!("monitor: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let command = cli::Cli::parse();
    cli::run(command)
}
