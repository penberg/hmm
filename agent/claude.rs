//! Claude Code.

use std::process::Command;

use super::Agent;

/// Claude Code.
pub struct Claude;

impl Agent for Claude {
    /// Skips Claude Code's "do you trust this folder?" dialog. It would ask
    /// on every draft, as each is a new directory, and accepting it would
    /// write to `~/.claude.json`, which the command may not. Claude Code
    /// trusts the folder it runs in when told it is sandboxed, as it is.
    fn configure(&self, command: &mut Command) {
        command.env("CLAUDE_CODE_SANDBOXED", "1");
    }
}
