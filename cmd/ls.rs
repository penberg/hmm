//! `hmm ls`, and `hmm` alone: lists workspaces.

use std::{env, io, path::Path, process::ExitCode, time::SystemTime};

use argh::FromArgs;

use crate::{Workspace, root};

/// List the workspaces of the working directory.
#[derive(FromArgs)]
#[argh(subcommand, name = "ls")]
pub struct Ls {
    /// list the workspaces of every directory
    #[argh(switch, short = 'a')]
    all: bool,
}

impl Ls {
    pub fn run(self) -> io::Result<ExitCode> {
        list(self.all)
    }
}

/// Lists the workspaces of the working directory, or of every directory if
/// `all` is set.
pub fn list(all: bool) -> io::Result<ExitCode> {
    let cwd = env::current_dir()?.canonicalize()?;
    let home = dirs::home_dir();
    let mut rows = Vec::new();
    for workspace in Workspace::all(&root()?)? {
        // A workspace without an origin is still being created.
        let Ok(origin) = workspace.origin() else {
            continue;
        };
        if !all && origin != cwd {
            continue;
        }
        let state = if workspace.running() {
            "running"
        } else {
            "done"
        };
        let age = workspace
            .created()
            .ok()
            .and_then(|created| SystemTime::now().duration_since(created).ok())
            .map(|age| short(age.as_secs()))
            .unwrap_or_default();
        rows.push([
            workspace.id.clone(),
            workspace.name().unwrap_or_else(|| "-".to_string()),
            state.to_string(),
            age,
            tilde(&origin, home.as_deref()),
        ]);
    }
    if rows.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    let header = ["ID", "NAME", "STATE", "AGE", "ORIGIN"].map(String::from);
    let mut widths = [0; 5];
    for row in [&header].into_iter().chain(&rows) {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    for row in [&header].into_iter().chain(&rows) {
        let [id, name, state, age, origin] = row;
        println!(
            "{id:<w0$}  {name:<w1$}  {state:<w2$}  {age:>w3$}  {origin}",
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `secs` as a count of its largest unit: `42s`, `5m`, `3h`, `2d`.
fn short(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// `path` with the home directory written as `~`.
fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}
