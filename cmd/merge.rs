//! `hmm merge`: merges the commits made in a workspace into the branch the
//! directory it was made from is on.

use std::{env, fs, io, path::Path, process::ExitCode};

use argh::FromArgs;

use crate::{
    Workspace,
    cmd::run::confine,
    git::{self, Changes},
    root,
};

/// Merge the commits made in a workspace into the branch the directory it was
/// made from is on.
#[derive(FromArgs)]
#[argh(subcommand, name = "merge")]
pub struct Merge {
    /// the workspace, by name, ID, or the start of an ID; the working
    /// directory's latest if none
    #[argh(positional)]
    workspace: Option<String>,
}

impl Merge {
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let workspace = Workspace::pick(&root, &cwd, self.workspace.as_deref())?;
        // A command still running could be halfway through a commit.
        let _lock = workspace.lock()?;
        let origin = workspace.origin()?;
        if git::toplevel(&origin).is_none() || !workspace.dir.join("fork").exists() {
            return Err(io::Error::other(format!(
                "workspace {} was not made from the top of a git repository: hmm apply \
                 applies its changes",
                workspace.label()
            )));
        }
        let tree = workspace.tree()?;
        let head = git::head(&workspace);

        // The workspace's repository is read by git in the sandbox, as the
        // command in the workspace may have changed its configuration: git
        // there packs the commits made since the workspace was made into a
        // bundle, which is only data, and git here fetches them from it.
        let tip = confine(
            &root,
            &tree,
            &[],
            &sh(&["git", "rev-parse", "--verify", "HEAD"]),
        )?
        .output()?;
        let tip = String::from_utf8_lossy(&tip.stdout).trim().to_string();
        if !is_commit(&tip) || head.as_deref() == Some(tip.as_str()) {
            eprintln!(
                "hmm: workspace {} has no commits to merge: hmm apply applies its changes",
                workspace.label()
            );
            return Ok(ExitCode::FAILURE);
        }
        let bundle = tree.join(".git").join("hmm.bundle");
        let mut create = sh(&[
            "git",
            "bundle",
            "create",
            "--quiet",
            ".git/hmm.bundle",
            "HEAD",
        ]);
        if let Some(head) = &head {
            create.push(format!("^{head}"));
        }
        let created = confine(&root, &tree, &[], &create)?.status()?;
        let fetched = created.success() && fetch(&origin, &bundle, &workspace.id)?;
        let _ = fs::remove_file(&bundle);
        if !fetched {
            return Err(io::Error::other(format!(
                "could not take the commits from workspace {}",
                workspace.label()
            )));
        }

        let reference = format!("refs/hmm/{}", workspace.id);
        let committed = git::git()
            .arg("-C")
            .arg(&origin)
            .args(["rev-parse", &format!("{reference}^{{tree}}")])
            .output()?;
        let committed = String::from_utf8_lossy(&committed.stdout)
            .trim()
            .to_string();
        if Changes::of(&workspace)?.after != committed {
            eprintln!(
                "hmm: workspace {} has changes it did not commit, which are not merged",
                workspace.label()
            );
        }
        let merged = git::git()
            .arg("-C")
            .arg(&origin)
            .args(["merge", "--no-edit", "-m"])
            .arg(format!("Merge workspace {}", workspace.label()))
            .arg(&reference)
            .status()?;
        Ok(if merged.success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        })
    }
}

/// Fetches the commits in `bundle` into `origin`'s repository as
/// `refs/hmm/<id>`, checking every object, and returns whether it could.
fn fetch(origin: &Path, bundle: &Path, id: &str) -> io::Result<bool> {
    // The bundle is in the workspace, where the command could have left
    // something else under its name.
    if !fs::symlink_metadata(bundle).is_ok_and(|m| m.file_type().is_file()) {
        return Ok(false);
    }
    let fetched = git::git()
        .arg("-C")
        .arg(origin)
        .args([
            "-c",
            "fetch.fsckObjects=true",
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-write-fetch-head",
        ])
        .arg(bundle)
        .arg(format!("+HEAD:refs/hmm/{id}"))
        .status()?;
    Ok(fetched.success())
}

/// `args` as the arguments of a command.
fn sh(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

/// Whether `id` looks like a commit's ID: 40 or 64 hexadecimal digits.
fn is_commit(id: &str) -> bool {
    matches!(id.len(), 40 | 64) && id.bytes().all(|b| b.is_ascii_hexdigit())
}
