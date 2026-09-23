//! `hmm rm`: removes drafts.

use std::{
    env, fs, io,
    process::{ExitCode, Stdio},
};

use argh::FromArgs;

use crate::{Draft, git, root};

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

/// Removes `draft`, unless a command is running in it, and the reference
/// `hmm merge` made to its commits.
pub fn remove(draft: &Draft) -> io::Result<()> {
    let _lock = draft.lock()?;
    let origin = draft.origin();
    fs::remove_dir_all(&draft.dir)?;
    if let Ok(origin) = origin
        && git::toplevel(&origin).is_some()
    {
        let _ = git::git()
            .arg("-C")
            .arg(&origin)
            .args(["update-ref", "-d", &format!("refs/hmm/{}", draft.id)])
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}
