//! `hmm merge`.

use crate::{World, assert_fails, need_sandbox, read, stderr, write};

#[test]
fn fast_forwards_when_the_branch_has_not_moved() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let draft = world.sh(&dir, "echo two > a && git commit --quiet -am two");
    let tip = world.git(&draft.tree, &["rev-parse", "HEAD"]);
    let out = world.hmm(&dir, &["merge"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(world.git(&dir, &["rev-parse", "HEAD"]), tip);
    assert_eq!(world.git(&dir, &["branch", "--show-current"]), "main\n");
    assert_eq!(read(&dir, "a"), "two\n");
    let reference = format!("refs/hmm/{}", draft.id);
    assert_eq!(world.git(&dir, &["rev-parse", &reference]), tip);
}

#[test]
fn makes_a_merge_commit_when_the_branch_has_moved() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let draft = world.sh(&dir, "echo two > a && git commit --quiet -am two");
    let tip = world.git(&draft.tree, &["rev-parse", "HEAD"]);
    write(&dir, "b", "b\n");
    world.git(&dir, &["add", "b"]);
    world.git(&dir, &["commit", "--quiet", "-m", "b"]);
    let moved = world.git(&dir, &["rev-parse", "HEAD"]);
    let out = world.hmm(&dir, &["merge"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(world.git(&dir, &["rev-parse", "HEAD^1"]), moved);
    assert_eq!(world.git(&dir, &["rev-parse", "HEAD^2"]), tip);
    assert_eq!(
        world.git(&dir, &["log", "-1", "--format=%s"]),
        format!("Merge draft {}\n", draft.id)
    );
    assert_eq!(read(&dir, "a"), "two\n");
    assert_eq!(read(&dir, "b"), "b\n");
}

#[test]
fn merges_the_draft_given() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let draft = world.run(
        &dir,
        &[
            "-n",
            "feature",
            "sh",
            "-c",
            "echo feature > b && git add b && git commit --quiet -m b",
        ],
    );
    world.sh(
        &dir,
        "echo latest > c && git add c && git commit --quiet -m c",
    );
    write(&dir, "a", "moved\n");
    world.git(&dir, &["commit", "--quiet", "-am", "moved"]);
    let out = world.hmm(&dir, &["merge", "feature"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        world.git(&dir, &["log", "-1", "--format=%s"]),
        format!("Merge draft {} (feature)\n", draft.id)
    );
    assert_eq!(read(&dir, "b"), "feature\n");
    assert!(!dir.join("c").exists());
}

#[test]
fn leaves_conflicts_to_resolve() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo two > a && git commit --quiet -am two");
    write(&dir, "a", "changed since\n");
    world.git(&dir, &["commit", "--quiet", "-am", "since"]);
    let out = world.hmm(&dir, &["merge"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(read(&dir, "a").contains("<<<<<<<"));
    world.git(&dir, &["rev-parse", "--verify", "MERGE_HEAD"]);
}

#[test]
fn merges_only_commits_and_says_so() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(
        &dir,
        "echo committed > b && git add b && git commit --quiet -m b && echo uncommitted > a",
    );
    let out = world.hmm(&dir, &["merge"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("has changes it did not commit"),
        "{}",
        stderr(&out)
    );
    assert_eq!(read(&dir, "b"), "committed\n");
    assert_eq!(read(&dir, "a"), "one\n");
}

#[test]
fn fails_without_commits() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo uncommitted > a");
    let head = world.git(&dir, &["rev-parse", "HEAD"]);
    assert_fails(&world.hmm(&dir, &["merge"]), "has no commits to merge");
    assert_eq!(world.git(&dir, &["rev-parse", "HEAD"]), head);
    assert_eq!(read(&dir, "a"), "one\n");
}

#[test]
fn fails_for_a_directory_that_is_not_a_repository() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    world.sh(&dir, "echo new > b");
    assert_fails(
        &world.hmm(&dir, &["merge"]),
        "not made from the top of a git repository",
    );
}

#[test]
fn fails_for_a_directory_inside_a_repository() {
    need_sandbox!();
    let world = World::new();
    let repo = world.repo("project");
    let dir = world.dir("project/sub");
    world.sh(&dir, "echo new > b");
    assert_fails(
        &world.hmm(&dir, &["merge"]),
        "not made from the top of a git repository",
    );
    assert_eq!(world.git(&repo, &["log", "--format=%s"]), "one\n");
}

#[test]
fn leaves_a_draft_in_which_a_command_is_running_alone() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let head = world.git(&dir, &["rev-parse", "HEAD"]);
    let running = world.start(&dir, &[]);
    write(&running.draft.tree, "a", "two\n");
    world.git(&running.draft.tree, &["commit", "--quiet", "-am", "two"]);
    assert_fails(&world.hmm(&dir, &["merge"]), "is running");
    assert_eq!(world.git(&dir, &["rev-parse", "HEAD"]), head);
    running.stop();
    assert!(world.hmm(&dir, &["merge"]).status.success());
    assert_eq!(read(&dir, "a"), "two\n");
}
