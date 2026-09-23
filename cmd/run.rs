//! `hmm run`: clones the working directory into a new draft and runs a
//! command in it.

use std::{env, fs, io, os::unix::process::ExitStatusExt, path::PathBuf, process::ExitCode};

use argh::FromArgs;

use crate::{Draft, darwin, root};

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
        let home = dirs::home_dir().ok_or_else(|| io::Error::other("no home directory"))?;
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let draft = Draft::create(&root, &cwd, self.name.as_deref())?;
        let _lock = draft.lock()?;
        // The tree is cloned from the base, so that both are the directory
        // at the same moment.
        let tree = draft.tree()?;
        let base = draft.base();
        let cloned = darwin::clone(&cwd, &base)
            .and_then(|()| fs::create_dir(draft.dir.join("tree")))
            .and_then(|()| darwin::clone(&base, &tree));
        if let Err(e) = cloned {
            let _ = fs::remove_dir_all(&draft.dir);
            return Err(io::Error::new(
                e.kind(),
                format!("cloning {}: {e}", cwd.display()),
            ));
        }
        eprintln!("hmm: draft {}: {}", draft.label(), tree.display());

        let mut writable = vec![PathBuf::from("/tmp"), env::temp_dir()];
        writable.extend(CACHES.iter().map(|dir| home.join(dir)));
        let mut writable: Vec<PathBuf> = writable
            .iter()
            .filter_map(|path| path.canonicalize().ok())
            .collect();
        for path in &self.write {
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
            .chain([root])
            .collect();

        let command = if self.command.is_empty() {
            vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
        } else {
            self.command
        };
        let status = darwin::confine(&command, &tree, &writable, &hidden)
            .current_dir(&tree)
            .status()?;
        eprintln!("hmm: draft {}: {}", draft.label(), tree.display());
        let code = status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
        Ok(ExitCode::from(code as u8))
    }
}
