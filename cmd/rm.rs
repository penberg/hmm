//! `hmm rm`: removes workspaces.

use std::{
    env, fs, io,
    process::{ExitCode, Stdio},
};

use argh::FromArgs;

use crate::{Workspace, git, root};

/// Remove workspaces, and everything in them.
#[derive(FromArgs)]
#[argh(subcommand, name = "rm")]
pub struct Rm {
    /// the workspaces to remove, by name, ID, or the start of an ID
    #[argh(positional)]
    workspaces: Vec<String>,
}

impl Rm {
    pub fn run(self) -> io::Result<ExitCode> {
        if self.workspaces.is_empty() {
            return Err(io::Error::other("rm needs the workspaces to remove"));
        }
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let mut code = ExitCode::SUCCESS;
        for key in &self.workspaces {
            let result = Workspace::find(&root, &cwd, key).and_then(|workspace| remove(&workspace));
            if let Err(e) = result {
                eprintln!("hmm: {e}");
                code = ExitCode::FAILURE;
            }
        }
        Ok(code)
    }
}

/// Removes `workspace`, unless a command is running in it, and the reference
/// `hmm merge` made to its commits.
pub fn remove(workspace: &Workspace) -> io::Result<()> {
    let _lock = workspace.lock()?;
    let origin = workspace.origin();
    fs::remove_dir_all(&workspace.dir)?;
    if let Ok(origin) = origin
        && git::toplevel(&origin).is_some()
    {
        let _ = git::git()
            .arg("-C")
            .arg(&origin)
            .args(["update-ref", "-d", &format!("refs/hmm/{}", workspace.id)])
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}
