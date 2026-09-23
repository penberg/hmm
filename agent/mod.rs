//! Coding agents: commands `hmm` knows, and prepares to run in a workspace.

mod claude;

use std::{path::Path, process::Command};

/// A coding agent, which may need more than the sandbox to run well in a
/// workspace.
pub trait Agent {
    /// Prepares `command`, which runs the agent in a workspace.
    fn configure(&self, command: &mut Command);
}

/// The agent `argv` runs, if it runs one `hmm` knows.
pub fn detect_agent(argv: &[String]) -> Option<Box<dyn Agent>> {
    let program = Path::new(argv.first()?).file_name()?;
    match program.to_str()? {
        "claude" => Some(Box::new(claude::Claude)),
        _ => None,
    }
}
