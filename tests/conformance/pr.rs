//! `hmm pr`.
//!
//! The remote is a bare repository in the world. Where it has to be on GitHub,
//! its URL is a GitHub one that git rewrites to the bare repository, and `gh`
//! is a script that records what it was asked to do.

use std::{
    env, fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use crate::{World, assert_fails, need_sandbox, stderr, stdout, write};

/// The GitHub repository the remote stands in for.
const GITHUB: &str = "https://github.com/owner/name";

/// A fake `gh`: `gh pr view` prints the pull request's URL if one has been
/// opened, and `gh pr create` opens one. Every call's arguments are appended
/// to `gh.log`, one per line, ending in a line `--`.
const GH: &str = r#"#!/bin/sh
dir=$(dirname "$0")/..
printf '%s\n' "$@" -- >> "$dir/gh.log"
case "$2" in
view) [ -e "$dir/opened" ] && echo https://github.com/owner/name/pull/1 && exit 0
      exit 1 ;;
create) touch "$dir/opened"; echo https://github.com/owner/name/pull/1 ;;
esac
"#;

/// A repository, `project`, whose `main` tracks `main` on a remote, `origin`,
/// in a bare repository, `remote.git`, which is returned too.
fn project(world: &World) -> (PathBuf, PathBuf) {
    let dir = world.repo("project");
    let remote = world.dir("remote.git");
    world.git(
        &remote,
        &["init", "--quiet", "--bare", "--initial-branch=main"],
    );
    world.git(&dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
    world.git(&dir, &["push", "--quiet", "-u", "origin", "main"]);
    (dir, remote)
}

/// Makes `dir`'s remote look as if it were on GitHub.
fn on_github(world: &World, dir: &Path, remote: &Path) {
    on_github_as(world, dir, remote, "origin");
}

/// A directory with the fake `gh` in it.
fn gh(world: &World) -> PathBuf {
    let bin = world.dir("gh/bin");
    let gh = bin.join("gh");
    fs::write(&gh, GH).unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// The calls the fake `gh` got, each as its arguments.
fn gh_calls(world: &World) -> Vec<Vec<String>> {
    let log = fs::read_to_string(world.dir("gh").join("gh.log")).unwrap_or_default();
    log.split_terminator("--\n")
        .map(|call| call.lines().map(str::to_string).collect())
        .collect()
}

/// Runs `hmm pr` with `args` in `dir`, with the fake `gh` first on the path.
fn pr(world: &World, dir: &Path, args: &[&str]) -> Output {
    let path = env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![gh(world)];
    paths.extend(env::split_paths(&path));
    world
        .hmm_command(dir, &[&["pr"], args].concat())
        .env("PATH", env::join_paths(paths).unwrap())
        .output()
        .unwrap()
}

/// What `branch` is at in `remote`.
fn remote_branch(world: &World, remote: &Path, branch: &str) -> String {
    world.git(remote, &["rev-parse", &format!("refs/heads/{branch}")])
}

/// The branches in `remote` other than `main`.
fn pushed(world: &World, remote: &Path) -> Vec<String> {
    world
        .git(
            remote,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )
        .lines()
        .filter(|branch| *branch != "main")
        .map(str::to_string)
        .collect()
}

/// The value given for `option` in a call to `gh`.
fn arg<'a>(call: &'a [String], option: &str) -> &'a str {
    let at = call.iter().position(|arg| arg == option).unwrap();
    &call[at + 1]
}

/// Makes `dir`'s remote look as if it were on GitHub.
fn on_github_as(world: &World, dir: &Path, remote: &Path, name: &str) {
    world.git(dir, &["remote", "set-url", name, GITHUB]);
    world.git(
        dir,
        &[
            "config",
            &format!("url.{}.insteadOf", remote.display()),
            GITHUB,
        ],
    );
}

#[test]
fn pushes_the_commits_to_a_branch_named_after_the_workspace() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    let head = world.git(&dir, &["rev-parse", "HEAD"]);
    let workspace = world.run(
        &dir,
        &[
            "-n",
            "feature",
            "sh",
            "-c",
            "echo two > a && git commit --quiet -am two",
        ],
    );
    let tip = world.git(&workspace.tree, &["rev-parse", "HEAD"]);
    let out = pr(&world, &dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(pushed(&world, &remote), ["feature"]);
    assert_eq!(remote_branch(&world, &remote, "feature"), tip);
    assert!(stderr(&out).contains("not on GitHub"), "{}", stderr(&out));
    assert!(gh_calls(&world).is_empty());
    // The working directory is left as it was.
    assert_eq!(world.git(&dir, &["rev-parse", "HEAD"]), head);
}

#[test]
fn names_the_branch_after_the_first_commit_without_a_name() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    world.sh(
        &dir,
        "echo two > a && git commit --quiet -am \"gpu/vulkan: Count a subgroup's lanes when scoring keys\" \
         && echo three > a && git commit --quiet -am three",
    );
    let out = pr(&world, &dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(pushed(&world, &remote), ["count-a-subgroups-lanes-when"]);
}

#[test]
fn pushes_to_the_branch_given() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    world.run(
        &dir,
        &[
            "-n",
            "feature",
            "sh",
            "-c",
            "echo two > a && git commit --quiet -am two",
        ],
    );
    let out = pr(&world, &dir, &["--branch", "fix-a"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(pushed(&world, &remote), ["fix-a"]);
}

#[test]
fn opens_a_pull_request_titled_after_the_commit() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    on_github(&world, &dir, &remote);
    world.run(
        &dir,
        &[
            "-n",
            "feature",
            "sh",
            "-c",
            "echo two > a && git commit --quiet -am two -m 'Why two.'",
        ],
    );
    let out = pr(&world, &dir, &["--draft"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "https://github.com/owner/name/pull/1\n");
    let calls = gh_calls(&world);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0][..3], ["pr", "view", "feature"]);
    assert_eq!(
        calls[1],
        [
            "pr",
            "create",
            "--repo",
            "owner/name",
            "--base",
            "main",
            "--head",
            "feature",
            "--title=two",
            "--body=Why two.",
            "--draft",
        ]
    );
}

#[test]
fn asks_to_merge_into_the_default_branch_whatever_branch_is_checked_out() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    // A branch that tracks one since merged and deleted on the remote.
    world.git(&dir, &["switch", "--quiet", "-c", "topic"]);
    world.git(&dir, &["push", "--quiet", "-u", "origin", "topic"]);
    world.git(&remote, &["branch", "-D", "topic"]);
    on_github(&world, &dir, &remote);
    world.sh(&dir, "echo two > a && git commit --quiet -am two");
    let out = pr(&world, &dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(arg(gh_calls(&world).last().unwrap(), "--base"), "main");
}

#[test]
fn asks_to_merge_into_the_branch_given() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    world.git(&dir, &["push", "--quiet", "origin", "main:release"]);
    on_github(&world, &dir, &remote);
    world.sh(&dir, "echo two > a && git commit --quiet -am two");
    let out = pr(&world, &dir, &["--base", "release"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(arg(gh_calls(&world).last().unwrap(), "--base"), "release");
}

#[test]
fn lists_the_commits_when_there_are_several() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    on_github(&world, &dir, &remote);
    world.sh(
        &dir,
        "echo two > a && git commit --quiet -am two && echo three > a && git commit --quiet -am three",
    );
    let out = pr(&world, &dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let calls = gh_calls(&world);
    let create = calls.last().unwrap();
    assert!(create.contains(&"--title=two".to_string()), "{create:?}");
    let body = create.iter().position(|arg| arg == "--body=- two").unwrap();
    assert_eq!(create[body + 1], "- three");
}

#[test]
fn updates_the_branch_and_the_pull_request_when_run_again() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    on_github(&world, &dir, &remote);
    let workspace = world.sh(&dir, "echo two > a && git commit --quiet -am two");
    assert!(pr(&world, &dir, &[]).status.success());
    // Rewritten, as after an amend or a rebase, and with another subject,
    // which does not rename the branch.
    world.git(
        &workspace.tree,
        &["commit", "--quiet", "--amend", "-m", "two, amended"],
    );
    let tip = world.git(&workspace.tree, &["rev-parse", "HEAD"]);
    let out = pr(&world, &dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(pushed(&world, &remote), ["two"]);
    assert_eq!(remote_branch(&world, &remote, "two"), tip);
    assert_eq!(stdout(&out), "https://github.com/owner/name/pull/1\n");
    let calls = gh_calls(&world);
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert_eq!(calls[2][..2], ["pr", "view"]);
}

#[test]
fn does_not_overwrite_a_branch_it_did_not_push() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    world.run(
        &dir,
        &[
            "-n",
            "feature",
            "sh",
            "-c",
            "echo two > a && git commit --quiet -am two",
        ],
    );
    world.git(&dir, &["push", "--quiet", "origin", "main:feature"]);
    let theirs = remote_branch(&world, &remote, "feature");
    assert_fails(&pr(&world, &dir, &[]), "--branch");
    assert_eq!(remote_branch(&world, &remote, "feature"), theirs);
}

#[test]
fn prints_where_to_open_the_pull_request_without_gh() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    on_github(&world, &dir, &remote);
    world.sh(&dir, "echo two > a && git commit --quiet -am two");
    // A path with git on it, but not gh.
    let bin = world.dir("bin");
    let git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    symlink(stdout(&git).trim(), bin.join("git")).unwrap();
    let out = world
        .hmm_command(&dir, &["pr"])
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "https://github.com/owner/name/compare/main...two?expand=1\n"
    );
    assert_eq!(pushed(&world, &remote), ["two"]);
}

#[test]
fn pushes_only_commits_and_says_so() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    let workspace = world.sh(
        &dir,
        "echo committed > b && git add b && git commit --quiet -m b && echo uncommitted > a",
    );
    let tip = world.git(&workspace.tree, &["rev-parse", "HEAD"]);
    let out = pr(&world, &dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("has changes it did not commit"),
        "{}",
        stderr(&out)
    );
    assert_eq!(remote_branch(&world, &remote, "b"), tip);
}

#[test]
fn fails_without_commits() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    world.sh(&dir, "echo uncommitted > a");
    assert_fails(&pr(&world, &dir, &[]), "has no commits to push");
    assert!(pushed(&world, &remote).is_empty());
}

#[test]
fn fails_without_a_remote() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo two > a && git commit --quiet -am two");
    assert_fails(&pr(&world, &dir, &[]), "has no remote origin");
}

#[test]
fn leaves_a_workspace_in_which_a_command_is_running_alone() {
    need_sandbox!();
    let world = World::new();
    let (dir, remote) = project(&world);
    let running = world.start(&dir, &[]);
    write(&running.workspace.tree, "a", "two\n");
    world.git(
        &running.workspace.tree,
        &["commit", "--quiet", "-am", "two"],
    );
    assert_fails(&pr(&world, &dir, &[]), "is running");
    assert!(pushed(&world, &remote).is_empty());
    running.stop();
    assert!(pr(&world, &dir, &[]).status.success());
}
