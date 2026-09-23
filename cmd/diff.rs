//! `hmm diff`: shows what changed in a draft since it was made.

use std::{
    env, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use argh::FromArgs;

use crate::{Draft, root};

/// Show what changed in a draft since it was made.
#[derive(FromArgs)]
#[argh(subcommand, name = "diff")]
pub struct Diff {
    /// show only which files changed, and how much
    #[argh(switch)]
    stat: bool,

    /// the draft, by name, ID, or the start of an ID; the working
    /// directory's latest if none
    #[argh(positional)]
    draft: Option<String>,
}

impl Diff {
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let draft = match &self.draft {
            Some(key) => Draft::find(&root, &cwd, key)?,
            None => Draft::latest(&root, &cwd)?,
        };
        let base = draft.base();
        if !base.is_dir() {
            return Err(io::Error::other(format!(
                "draft {} has no base to compare it with",
                draft.label()
            )));
        }
        let tree = draft.tree()?;
        let repo = repository(&draft)?;
        let before = snapshot(&repo, &base, &draft.dir.join("base.index"))?;
        let after = snapshot(&repo, &tree, &draft.dir.join("tree.index"))?;
        let stat = if self.stat { "--stat" } else { "--patch" };
        let status = git(&repo).args(["diff", stat, &before, &after]).status()?;
        Ok(if status.success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        })
    }
}

/// The repository to compare `draft`'s trees in: the base's, if the directory
/// was a repository, so that git ignores what it ignores, such as build
/// output; otherwise one of the draft's own, made the first time it is
/// needed.
///
/// Git runs outside the sandbox, so it never uses the repository in the
/// draft's tree: its configuration can make git run any command, and the
/// command in the draft may have changed it. The base's is the working
/// directory's as it was when the draft was made, which the command could not
/// see.
fn repository(draft: &Draft) -> io::Result<PathBuf> {
    let repo = draft.base().join(".git");
    if repo.exists() {
        return Ok(repo);
    }
    let repo = draft.dir.join("git");
    if !repo.exists() {
        let made = Command::new("git")
            .args(["init", "--quiet", "--bare"])
            .arg(&repo)
            .status()?;
        if !made.success() {
            return Err(io::Error::other("git could not make a repository"));
        }
    }
    Ok(repo)
}

/// A command that runs git on the repository `repo`.
fn git(repo: &Path) -> Command {
    let mut git = Command::new("git");
    git.arg("--git-dir").arg(repo);
    for var in ["GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY"] {
        git.env_remove(var);
    }
    git
}

/// Records the files in `tree` that git does not ignore as a tree in the
/// repository `repo`, keeping the index in `index`, and returns the tree's
/// ID.
fn snapshot(repo: &Path, tree: &Path, index: &Path) -> io::Result<String> {
    let added = git(repo)
        .arg("--work-tree")
        .arg(tree)
        .args(["add", "--all", "--", "."])
        .current_dir(tree)
        .env("GIT_INDEX_FILE", index)
        .status()?;
    if !added.success() {
        return Err(io::Error::other(format!(
            "git could not read {}",
            tree.display()
        )));
    }
    let written = git(repo)
        .arg("write-tree")
        .env("GIT_INDEX_FILE", index)
        .output()?;
    if !written.status.success() {
        return Err(io::Error::other(format!(
            "git could not record {}",
            tree.display()
        )));
    }
    Ok(String::from_utf8_lossy(&written.stdout).trim().to_string())
}
