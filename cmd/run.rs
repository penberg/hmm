//! `hmm run`: clones the working directory into a new workspace and runs a
//! command in it.

use std::{
    env, fs, io,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
};

use argh::FromArgs;

use crate::{Workspace, agent, cmd::rm, darwin, git, root};

/// Directories under the home directory that commands may write to, so that
/// builds and package managers keep working.
const CACHES: &[&str] = &[".cargo", ".rustup", ".cache", ".npm", "Library/Caches"];

/// Directories under the home directory that commands may not read.
const SECRETS: &[&str] = &[".ssh", ".aws", ".gnupg", ".config/gh"];

/// Run a command in a new workspace of the working directory.
#[derive(FromArgs)]
#[argh(subcommand, name = "run")]
pub struct Run {
    /// a name for the workspace, unique among the working directory's
    #[argh(option, short = 'n')]
    name: Option<String>,

    /// another path the command may write to; can be repeated
    #[argh(option, short = 'w')]
    write: Vec<PathBuf>,

    /// remove the workspace when the command exits
    #[argh(switch)]
    rm: bool,

    /// the command to run and its arguments; the shell if none
    #[argh(positional, greedy)]
    command: Vec<String>,
}

impl Run {
    /// Runs the command (the shell if none is given) in a new workspace of the
    /// working directory, removing the workspace afterwards with `--rm`, and
    /// returns its exit code.
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let workspace = Workspace::create(&root, &cwd, self.name.as_deref())?;
        let lock = workspace.lock()?;
        if let Err(e) = git::make(&workspace, &cwd) {
            let _ = fs::remove_dir_all(&workspace.dir);
            return Err(io::Error::new(
                e.kind(),
                format!("copying {}: {e}", cwd.display()),
            ));
        }
        let result = self.confined(&root, &workspace);
        if self.rm {
            drop(lock);
            if let Err(e) = rm::remove(&workspace) {
                eprintln!("hmm: removing workspace {}: {e}", workspace.label());
            }
        }
        let status = result?;
        let code = status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
        Ok(ExitCode::from(code as u8))
    }

    /// Runs the command in `workspace`, confined to it, and waits for it to
    /// exit.
    fn confined(&self, root: &Path, workspace: &Workspace) -> io::Result<ExitStatus> {
        let tree = workspace.tree()?;
        eprintln!("hmm: workspace {}: {}", workspace.label(), tree.display());
        let command = if self.command.is_empty() {
            vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
        } else {
            self.command.clone()
        };
        let mut confined = confine(root, &tree, &self.write, &command)?;
        if let Some(agent) = agent::detect_agent(&command) {
            agent.configure(&mut confined);
        }
        let mut child = confined.spawn()?;
        // The terminal's signals reach the command too: `hmm` outlives it, as
        // a shell does, to say where the workspace is or remove it.
        for signal in [libc::SIGINT, libc::SIGQUIT, libc::SIGHUP] {
            unsafe { libc::signal(signal, libc::SIG_IGN) };
        }
        let status = child.wait()?;
        if !self.rm {
            eprintln!("hmm: workspace {}: {}", workspace.label(), tree.display());
        }
        Ok(status)
    }
}

/// A command that runs `command` in the workspace whose tree is `tree`,
/// confined to it: it may write to the tree, the temporary directories, caches,
/// and `write`, and may not read secrets or the other workspaces under `root`.
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
    // The other workspaces are hidden too, as they hold other commands' work.
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
