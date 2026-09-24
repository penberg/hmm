//! Git, as `hmm` uses it: to record what a directory was when a workspace was
//! made from it, and to compare the workspace with that.
//!
//! Git always runs outside the sandbox here, so it never uses the repository in
//! a workspace's tree: its configuration can make git run any command, and the
//! command in the workspace may have changed it. It uses the repository of the
//! directory the workspace was made from, or, if that is not one, the
//! workspace's own.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{Workspace, os};

/// The variables that would point git at another repository than the one it
/// is given.
const ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
];

/// A command that runs git, free of the variables that would point it at
/// another repository.
pub fn git() -> Command {
    let mut git = Command::new("git");
    for var in ENV {
        git.env_remove(var);
    }
    git
}

/// The repository `dir` is the top of, if it is one: its git directory and
/// its object directory.
pub fn toplevel(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let out = git()
        .arg("-C")
        .arg(dir)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-path",
            "objects",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let out = String::from_utf8(out.stdout).ok()?;
    let mut lines = out.lines().map(PathBuf::from);
    let top = lines.next()?.canonicalize().ok()?;
    let (git_dir, objects) = (lines.next()?, lines.next()?);
    (top == dir).then_some((git_dir, objects))
}

/// A repository to record trees in.
pub struct Repo {
    /// Its git directory.
    dir: PathBuf,
    /// Where the objects git makes go, and the repository's own objects,
    /// when the two are kept apart, so that the directory's repository gets
    /// none of the objects made for a workspace.
    objects: Option<(PathBuf, PathBuf)>,
}

impl Repo {
    /// A command that runs git on the repository.
    pub fn git(&self) -> Command {
        let mut git = git();
        git.arg("--git-dir").arg(&self.dir);
        if let Some((new, own)) = &self.objects {
            git.env("GIT_OBJECT_DIRECTORY", new)
                .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", own);
        }
        git
    }

    /// Records the files in `tree` that git does not ignore as a tree,
    /// keeping the index in `index`, and returns the tree's ID.
    pub fn snapshot(&self, tree: &Path, index: &Path) -> io::Result<String> {
        let added = self
            .git()
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
        let written = self
            .git()
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
}

/// Makes `workspace` of `origin`: records what `origin` is, then clones it into
/// the workspace's tree.
///
/// If `origin` is the top of a repository, what it is is the commit it is
/// at, in `head`, and the tree of its files, uncommitted changes included, in
/// `fork`, made from a copy of its index so that only the files changed since
/// it was written are read. Otherwise, it is a clone, `base`, which the tree
/// is cloned from, so that both are the directory at the same moment.
pub fn make(workspace: &Workspace, origin: &Path) -> io::Result<()> {
    let tree = workspace.tree()?;
    fs::create_dir(workspace.dir.join("tree"))?;
    let Some((dir, objects)) = toplevel(origin) else {
        let base = workspace.base();
        os::clone(origin, &base)?;
        return os::clone(&base, &tree);
    };
    let head = git()
        .arg("-C")
        .arg(origin)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()?;
    if head.status.success() {
        fs::write(workspace.dir.join("head"), &head.stdout)?;
    }
    let index = workspace.dir.join("fork.index");
    if let Err(e) = fs::copy(dir.join("index"), &index)
        && e.kind() != io::ErrorKind::NotFound
    {
        return Err(e);
    }
    let new = workspace.dir.join("objects");
    fs::create_dir(&new)?;
    let repo = Repo {
        dir,
        objects: Some((new, objects)),
    };
    let fork = repo.snapshot(origin, &index)?;
    fs::write(workspace.dir.join("fork"), fork)?;
    os::clone(origin, &tree)
}

/// The commit the directory was at when `workspace` was made from it, if it was
/// a repository with a commit.
pub fn head(workspace: &Workspace) -> Option<String> {
    let head = fs::read_to_string(workspace.dir.join("head")).ok()?;
    Some(head.trim().to_string())
}

/// What changed in a workspace: what the directory was when the workspace was
/// made and what the workspace's tree is now, recorded as trees in a
/// repository.
pub struct Changes {
    /// The repository the trees are in.
    pub repo: Repo,
    /// The ID of the tree the workspace was made from.
    pub before: String,
    /// The ID of the workspace's tree.
    pub after: String,
}

impl Changes {
    /// Records what changed in `workspace`.
    pub fn of(workspace: &Workspace) -> io::Result<Changes> {
        let tree = workspace.tree()?;
        let after = workspace.dir.join("tree.index");
        if let Ok(fork) = fs::read_to_string(workspace.dir.join("fork")) {
            let origin = workspace.origin()?;
            let (dir, objects) = toplevel(&origin).ok_or_else(|| {
                io::Error::other(format!(
                    "{} is no longer a git repository",
                    origin.display()
                ))
            })?;
            let repo = Repo {
                dir,
                objects: Some((workspace.dir.join("objects"), objects)),
            };
            let after = repo.snapshot(&tree, &after)?;
            let before = fork.trim().to_string();
            return Ok(Changes {
                repo,
                before,
                after,
            });
        }
        let base = workspace.base();
        if !base.is_dir() {
            return Err(io::Error::other(format!(
                "workspace {} has nothing to compare it with",
                workspace.label()
            )));
        }
        let dir = workspace.dir.join("git");
        if !dir.exists() {
            let made = git()
                .args(["init", "--quiet", "--bare"])
                .arg(&dir)
                .status()?;
            if !made.success() {
                return Err(io::Error::other("git could not make a repository"));
            }
        }
        let repo = Repo { dir, objects: None };
        let before = repo.snapshot(&base, &workspace.dir.join("base.index"))?;
        let after = repo.snapshot(&tree, &after)?;
        Ok(Changes {
            repo,
            before,
            after,
        })
    }

    /// The changes as a patch that `git apply` applies to the directory the
    /// workspace was made from, whatever the configuration says about how to
    /// show a diff.
    pub fn patch(&self) -> io::Result<Vec<u8>> {
        let diff = self
            .repo
            .git()
            .args([
                "diff",
                "--binary",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-relative",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                &self.before,
                &self.after,
            ])
            .output()?;
        if !diff.status.success() {
            return Err(io::Error::other("git could not make a patch"));
        }
        Ok(diff.stdout)
    }
}
