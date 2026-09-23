//! `hmm apply`: applies what changed in a draft to the directory it was made
//! from.

use std::{
    env,
    io::{self, Write},
    path::Path,
    process::{ExitCode, Stdio},
};

use argh::FromArgs;

use crate::{
    Draft,
    cmd::diff::{Changes, git},
    root,
};

/// Apply what changed in a draft to the directory it was made from.
#[derive(FromArgs)]
#[argh(subcommand, name = "apply")]
pub struct Apply {
    /// only check whether the changes apply, changing nothing
    #[argh(switch)]
    check: bool,

    /// the draft, by name, ID, or the start of an ID; the working
    /// directory's latest if none
    #[argh(positional)]
    draft: Option<String>,
}

impl Apply {
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let draft = Draft::pick(&root, &cwd, self.draft.as_deref())?;
        // A command still running could be halfway through a change.
        let _lock = draft.lock()?;
        let origin = draft.origin()?;
        let changes = Changes::of(&draft)?;
        if changes.before == changes.after {
            eprintln!("hmm: draft {} changed nothing", draft.label());
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
                "hmm: draft {} is already applied to {}",
                draft.label(),
                origin.display()
            );
            return Ok(ExitCode::SUCCESS);
        }
        let options: &[&str] = if self.check { &["--check"] } else { &[] };
        if !apply(&changes.repo, &origin, &patch, options, true)? {
            return Err(io::Error::other(format!(
                "draft {} does not apply to {}, which is unchanged",
                draft.label(),
                origin.display()
            )));
        }
        if self.check {
            eprintln!(
                "hmm: draft {} applies to {}",
                draft.label(),
                origin.display()
            );
        } else {
            eprintln!(
                "hmm: applied draft {} to {}",
                draft.label(),
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
    repo: &Path,
    origin: &Path,
    patch: &[u8],
    options: &[&str],
    verbose: bool,
) -> io::Result<bool> {
    let mut child = git(repo)
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
