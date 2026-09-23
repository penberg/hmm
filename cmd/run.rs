//! `hmm run`: clones the working directory into a new draft and runs a
//! command in it.

use std::{
    env, fs, io,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use argh::FromArgs;

use crate::{Draft, darwin, git, root};

/// Directories under the home directory that commands may write to, so that
/// builds and package managers keep working.
const CACHES: &[&str] = &[".cargo", ".rustup", ".cache", ".npm", "Library/Caches"];

/// Directories under the home directory that commands may not read.
const SECRETS: &[&str] = &[".ssh", ".aws", ".gnupg", ".config/gh"];

/// Run a command in a new draft of the working directory.
#[derive(FromArgs)]
#[argh(subcommand, name = "run")]
pub struct Run {
    /// a name for the draft, unique among the working directory's
    #[argh(option, short = 'n')]
    name: Option<String>,

    /// another path the command may write to; can be repeated
    #[argh(option, short = 'w')]
    write: Vec<PathBuf>,

    /// the command to run and its arguments; the shell if none
    #[argh(positional, greedy)]
    command: Vec<String>,
}

impl Run {
    /// Runs the command (the shell if none is given) in a new draft of the
    /// working directory, and returns its exit code.
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let draft = Draft::create(&root, &cwd, self.name.as_deref())?;
        let _lock = draft.lock()?;
        if let Err(e) = git::make(&draft, &cwd) {
            let _ = fs::remove_dir_all(&draft.dir);
            return Err(io::Error::new(
                e.kind(),
                format!("copying {}: {e}", cwd.display()),
            ));
        }
        let tree = draft.tree()?;
        eprintln!("hmm: draft {}: {}", draft.label(), tree.display());
        let command = if self.command.is_empty() {
            vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
        } else {
            self.command
        };
        let status = confine(&root, &tree, &self.write, &command)?.status()?;
        eprintln!("hmm: draft {}: {}", draft.label(), tree.display());
        let code = status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
        Ok(ExitCode::from(code as u8))
    }
}

/// A command that runs `command` in the draft whose tree is `tree`, confined
/// to it: it may write to the tree, the temporary directories, caches, and
/// `write`, and may not read secrets or the other drafts under `root`.
pub fn confine(
    root: &Path,
    tree: &Path,
    write: &[PathBuf],
    command: &[String],
) -> io::Result<Command> {
    let home = dirs::home_dir().ok_or_else(|| io::Error::other("no home directory"))?;
    let mut writable = vec![PathBuf::from("/tmp"), env::temp_dir()];
    writable.extend(CACHES.iter().map(|dir| home.join(dir)));
    let mut writable: Vec<PathBuf> = writable
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    for path in write {
        let path = path
            .canonicalize()
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
        writable.push(path);
    }
    // The other drafts are hidden too, as they hold other commands' work.
    let hidden: Vec<PathBuf> = SECRETS
        .iter()
        .map(|dir| home.join(dir))
        .filter_map(|path| path.canonicalize().ok())
        .chain([root.to_path_buf()])
        .collect();
    let mut command = darwin::confine(command, tree, &writable, &hidden);
    command.current_dir(tree);
    Ok(command)
}
