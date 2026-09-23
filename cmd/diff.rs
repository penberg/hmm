//! `hmm diff`: shows what changed in a draft since it was made.

use std::{env, io, process::ExitCode};

use argh::FromArgs;

use crate::{Draft, git::Changes, root};

/// Show what changed in a draft since it was made.
#[derive(FromArgs)]
#[argh(subcommand, name = "diff")]
pub struct Diff {
    /// show only which files changed, and how much
    #[argh(switch)]
    stat: bool,

    /// the draft, by name, ID, or the start of an ID; the working
    /// directory's latest if none
    #[argh(positional)]
    draft: Option<String>,
}

impl Diff {
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let draft = Draft::pick(&root, &cwd, self.draft.as_deref())?;
        let changes = Changes::of(&draft)?;
        let stat = if self.stat { "--stat" } else { "--patch" };
        let status = changes
            .repo
            .git()
            .args(["diff", stat, &changes.before, &changes.after])
            .status()?;
        Ok(if status.success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        })
    }
}
