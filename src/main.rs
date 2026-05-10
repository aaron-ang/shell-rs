use anyhow::Result;

mod command;
mod completion;
mod history;
mod job;
mod pipeline;
mod shell;
mod state;
mod token;
mod variable;
use state::Terminal;

fn main() -> Result<()> {
    let mut term = Terminal::new()?;
    term.start()
}
