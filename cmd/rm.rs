//! `hmm rm`: removes drafts.

use std::{env, fs, io, process::ExitCode};

use argh::FromArgs;

use crate::{Draft, root};

/// Remove drafts, and everything in them.
#[derive(FromArgs)]
#[argh(subcommand, name = "rm")]
pub struct Rm {
    /// the drafts to remove, by name, ID, or the start of an ID
    #[argh(positional)]
    drafts: Vec<String>,
}

impl Rm {
    pub fn run(self) -> io::Result<ExitCode> {
        if self.drafts.is_empty() {
            return Err(io::Error::other("rm needs the drafts to remove"));
        }
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let mut code = ExitCode::SUCCESS;
        for key in &self.drafts {
            let result = Draft::find(&root, &cwd, key).and_then(|draft| remove(&draft));
            if let Err(e) = result {
                eprintln!("hmm: {e}");
                code = ExitCode::FAILURE;
            }
        }
        Ok(code)
    }
}

/// Removes `draft`, unless a command is running in it.
fn remove(draft: &Draft) -> io::Result<()> {
    let _lock = draft.lock()?;
    fs::remove_dir_all(&draft.dir)
}
