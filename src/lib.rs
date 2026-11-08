//! Shared application entry point. Runtime modules remain private.
mod args;
mod chat;
mod checkpoints;
mod cli;
mod client;
mod engine;
mod extensions;
mod paths;
mod process;
mod tui;
mod ui;
mod update;

#[cfg(feature = "fuzzing")]
pub mod fuzzing;

use std::env;
use std::error::Error;
use std::io::{self, IsTerminal};

use args::{parse_args, Start};
use chat::{saved_sessions as sessions, start_chat};
use client::bridge::{Client, SessionStart};
use ui::dialog::Dialogs;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub fn run() -> Result<()> {
    let start = parse_args(env::args().skip(1))?;
    match start {
        Start::Update => update::update(),
        Start::Version => {
            println!("hibiscus {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Start::Help => {
            println!("{}", args::USAGE);
            Ok(())
        }
        Start::Prompt(prompt) => {
            if prompt.trim().is_empty() {
                return Err("prompt cannot be empty".into());
            }
            let mut rpc = Client::start(SessionStart::New)?;
            let mut dialogs = Dialogs;
            let outcome = rpc.prompt(
                &prompt,
                &mut io::stdout(),
                &mut io::stdin().lock(),
                &mut dialogs,
                None,
                &mut None,
                false,
            );
            rpc.finish(outcome)
        }
        Start::Chat { resume } => start_chat(if resume {
            SessionStart::Latest
        } else {
            SessionStart::New
        }),
        Start::Cli { resume } => chat::start_chat_with_mode(
            if resume {
                SessionStart::Latest
            } else {
                SessionStart::New
            },
            ui::mode::Mode::Cli,
        ),
        Start::Sessions => {
            let stdin = io::stdin();
            let cwd = env::current_dir()?;
            let list = sessions::list(&cwd)?;
            if let Some(path) = sessions::pick(
                &list,
                &mut stdin.lock(),
                &mut io::stdout(),
                stdin.is_terminal(),
            )? {
                start_chat(SessionStart::Selected(path))?;
            }
            Ok(())
        }
    }
}
