//! `hmm apply`.

use std::fs;

use crate::{World, assert_fails, need_sandbox, read, stderr, write};

#[test]
fn makes_the_changes_in_the_directory_without_staging_or_committing() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    write(&dir, "c", "c\n");
    world.git(&dir, &["add", "c"]);
    world.git(&dir, &["commit", "--quiet", "-m", "c"]);
    let head = world.git(&dir, &["rev-parse", "HEAD"]);
    let workspace = world.sh(&dir, "echo two > a && echo new > b && rm c");
    let out = world.hmm(&dir, &["apply"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!("applied workspace {}", workspace.id)),
        "{}",
        stderr(&out)
    );
    assert_eq!(read(&dir, "a"), "two\n");
    assert_eq!(read(&dir, "b"), "new\n");
    assert!(!dir.join("c").exists());
    assert_eq!(world.git(&dir, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        world.git(&dir, &["status", "--porcelain"]),
        " M a\n D c\n?? b\n"
    );
}

#[test]
fn applies_uncommitted_and_committed_changes_alike() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(
        &dir,
        "echo two > a && git commit --quiet -am two && echo new > b",
    );
    assert!(world.hmm(&dir, &["apply"]).status.success());
    assert_eq!(read(&dir, "a"), "two\n");
    assert_eq!(read(&dir, "b"), "new\n");
    assert_eq!(world.git(&dir, &["log", "--format=%s"]), "one\n");
}

#[test]
fn applies_binary_files() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "printf 'x\\000y\\377' > bin");
    assert!(world.hmm(&dir, &["apply"]).status.success());
    assert_eq!(fs::read(dir.join("bin")).unwrap(), b"x\0y\xff");
}

#[test]
fn works_in_a_directory_that_is_not_a_repository() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    write(&dir, "a", "one\n");
    write(&dir, "c", "c\n");
    world.sh(&dir, "echo two > a && echo new > b && rm c");
    let out = world.hmm(&dir, &["apply"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&dir, "a"), "two\n");
    assert_eq!(read(&dir, "b"), "new\n");
    assert!(!dir.join("c").exists());
}

#[test]
fn applies_the_workspace_given() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.run(&dir, &["-n", "first", "sh", "-c", "echo first > a"]);
    world.sh(&dir, "echo second > a");
    assert!(world.hmm(&dir, &["apply", "first"]).status.success());
    assert_eq!(read(&dir, "a"), "first\n");
}

#[test]
fn applies_over_changes_elsewhere_in_the_directory() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    write(&dir, "a", "1\n2\n3\n4\n5\n6\n7\n8\n");
    world.git(&dir, &["commit", "--quiet", "-am", "lines"]);
    world.sh(&dir, "sed -i '' 's/^8$/eight/' a");
    write(&dir, "a", "one\n2\n3\n4\n5\n6\n7\n8\n");
    write(&dir, "other", "other\n");
    let out = world.hmm(&dir, &["apply"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&dir, "a"), "one\n2\n3\n4\n5\n6\n7\neight\n");
    assert_eq!(read(&dir, "other"), "other\n");
}

#[test]
fn applies_all_of_the_changes_or_none() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    write(&dir, "b", "b\n");
    world.git(&dir, &["add", "b"]);
    world.git(&dir, &["commit", "--quiet", "-m", "b"]);
    world.sh(&dir, "echo two > a && echo bee > b && echo new > c");
    write(&dir, "a", "changed since\n");
    let out = world.hmm(&dir, &["apply"]);
    assert_fails(&out, "does not apply");
    assert_eq!(read(&dir, "a"), "changed since\n");
    assert_eq!(read(&dir, "b"), "b\n");
    assert!(!dir.join("c").exists());
}

#[test]
fn check_says_whether_the_changes_apply_and_changes_nothing() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let workspace = world.sh(&dir, "echo two > a && echo new > b");
    let out = world.hmm(&dir, &["apply", "--check"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!("workspace {} applies to", workspace.id)),
        "{}",
        stderr(&out)
    );
    assert_eq!(read(&dir, "a"), "one\n");
    assert!(!dir.join("b").exists());
    write(&dir, "a", "changed since\n");
    assert_fails(&world.hmm(&dir, &["apply", "--check"]), "does not apply");
    assert_eq!(read(&dir, "a"), "changed since\n");
}

#[test]
fn leaves_an_applied_workspace_alone() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo two > a && echo new > b");
    assert!(world.hmm(&dir, &["apply"]).status.success());
    let out = world.hmm(&dir, &["apply"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("already applied"), "{}", stderr(&out));
    assert_eq!(read(&dir, "a"), "two\n");
    assert_eq!(read(&dir, "b"), "new\n");
}

#[test]
fn says_so_when_the_workspace_changed_nothing() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.run(&dir, &["true"]);
    let out = world.hmm(&dir, &["apply"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("changed nothing"), "{}", stderr(&out));
}

#[test]
fn keeps_the_workspace() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let workspace = world.sh(&dir, "echo two > a");
    assert!(world.hmm(&dir, &["apply"]).status.success());
    assert!(workspace.tree.is_dir());
    assert_eq!(world.workspaces(), [workspace.id]);
}

#[test]
fn leaves_a_workspace_in_which_a_command_is_running_alone() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let running = world.start(&dir, &[]);
    write(&running.workspace.tree, "a", "two\n");
    assert_fails(&world.hmm(&dir, &["apply"]), "is running");
    assert_eq!(read(&dir, "a"), "one\n");
    running.stop();
    assert!(world.hmm(&dir, &["apply"]).status.success());
    assert_eq!(read(&dir, "a"), "two\n");
}

#[test]
fn fails_for_a_workspace_that_does_not_exist() {
    let world = World::new();
    let dir = world.dir("project");
    assert_fails(&world.hmm(&dir, &["apply", "nope"]), "no workspace nope");
    assert_fails(&world.hmm(&dir, &["apply"]), "has no workspaces");
}
