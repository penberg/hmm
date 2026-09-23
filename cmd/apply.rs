//! `hmm apply`: applies what changed in a workspace to the directory it was
//! made from.

use std::{
    env,
    io::{self, Write},
    path::Path,
    process::{ExitCode, Stdio},
};

use argh::FromArgs;

use crate::{
    Workspace,
    git::{Changes, Repo},
    root,
};

/// Apply what changed in a workspace to the directory it was made from.
#[derive(FromArgs)]
#[argh(subcommand, name = "apply")]
pub struct Apply {
    /// only check whether the changes apply, changing nothing
    #[argh(switch)]
    check: bool,

    /// the workspace, by name, ID, or the start of an ID; the working
    /// directory's latest if none
    #[argh(positional)]
    workspace: Option<String>,
}

impl Apply {
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let workspace = Workspace::pick(&root, &cwd, self.workspace.as_deref())?;
        // A command still running could be halfway through a change.
        let _lock = workspace.lock()?;
        let origin = workspace.origin()?;
        let changes = Changes::of(&workspace)?;
        if changes.before == changes.after {
            eprintln!("hmm: workspace {} changed nothing", workspace.label());
            return Ok(ExitCode::SUCCESS);
        }
        let patch = changes.patch()?;
        if apply(
            &changes.repo,
            &origin,
            &patch,
            &["--reverse", "--check"],
            false,
        )? {
            eprintln!(
                "hmm: workspace {} is already applied to {}",
                workspace.label(),
                origin.display()
            );
            return Ok(ExitCode::SUCCESS);
        }
        let options: &[&str] = if self.check { &["--check"] } else { &[] };
        if !apply(&changes.repo, &origin, &patch, options, true)? {
            return Err(io::Error::other(format!(
                "workspace {} does not apply to {}, which is unchanged",
                workspace.label(),
                origin.display()
            )));
        }
        if self.check {
            eprintln!(
                "hmm: workspace {} applies to {}",
                workspace.label(),
                origin.display()
            );
        } else {
            eprintln!(
                "hmm: applied workspace {} to {}",
                workspace.label(),
                origin.display()
            );
        }
        Ok(ExitCode::SUCCESS)
    }
}

/// Applies `patch` to the files in `origin` with `git apply` and `options`,
/// which does all of it or none, and refuses to touch anything outside
/// `origin`. Returns whether it applied, and shows git's reasons if it did not
/// and `verbose` is set.
fn apply(
    repo: &Repo,
    origin: &Path,
    patch: &[u8],
    options: &[&str],
    verbose: bool,
) -> io::Result<bool> {
    let mut child = repo
        .git()
        .arg("--work-tree")
        .arg(origin)
        .args(["apply", "--whitespace=nowarn"])
        .args(options)
        .current_dir(origin)
        .stdin(Stdio::piped())
        .stderr(if verbose {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .spawn()?;
    child.stdin.take().unwrap().write_all(patch)?;
    Ok(child.wait()?.success())
}
