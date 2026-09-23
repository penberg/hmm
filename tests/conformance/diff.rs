//! `hmm diff`.

use crate::{World, assert_fails, need_sandbox, stderr, stdout, write};

#[test]
fn shows_what_changed_as_a_patch() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo two > a && echo new > b");
    let out = world.hmm(&dir, &["diff"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let patch = stdout(&out);
    for line in [
        "+++ b/a",
        "-one",
        "+two",
        "new file mode",
        "+++ b/b",
        "+new",
    ] {
        assert!(
            patch.lines().any(|l| l.starts_with(line)),
            "{line}: {patch}"
        );
    }
}

#[test]
fn stat_shows_only_which_files_changed_and_how_much() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo two > a && echo new > b");
    let out = world.hmm(&dir, &["diff", "--stat"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let stat = stdout(&out);
    assert!(stat.contains(" a | 2 +-"), "{stat}");
    assert!(stat.contains(" b | 1 +"), "{stat}");
    assert!(stat.contains("2 files changed"), "{stat}");
    assert!(!stat.contains("+two"), "{stat}");
}

#[test]
fn shows_the_latest_workspace_or_the_one_given() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let first = world.run(&dir, &["-n", "first", "sh", "-c", "echo first > a"]);
    let second = world.sh(&dir, "echo second > a");
    let latest = stdout(&world.hmm(&dir, &["diff"]));
    assert!(latest.contains("+second"), "{latest}");
    for key in ["first", &first.id, &first.id[..5]] {
        let out = world.hmm(&dir, &["diff", key]);
        assert!(out.status.success(), "{key}: {}", stderr(&out));
        assert!(stdout(&out).contains("+first"), "{key}: {}", stdout(&out));
    }
    let out = world.hmm(&dir, &["diff", &second.id]);
    assert!(stdout(&out).contains("+second"));
}

#[test]
fn shows_a_workspace_of_another_directory_by_id() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let other = world.dir("other");
    let workspace = world.run(&dir, &["-n", "feature", "sh", "-c", "echo two > a"]);
    let out = world.hmm(&other, &["diff", &workspace.id]);
    assert!(stdout(&out).contains("+two"), "{}", stderr(&out));
    // Names are the directory's own.
    assert_fails(
        &world.hmm(&other, &["diff", "feature"]),
        "no workspace feature",
    );
}

#[test]
fn leaves_out_what_git_ignores() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    write(&dir, ".gitignore", "target/\n");
    world.git(&dir, &["add", ".gitignore"]);
    world.git(&dir, &["commit", "--quiet", "-m", "ignore"]);
    world.sh(
        &dir,
        "mkdir target && echo built > target/out && echo two > a",
    );
    let patch = stdout(&world.hmm(&dir, &["diff"]));
    assert!(patch.contains("+two"), "{patch}");
    assert!(!patch.contains("target"), "{patch}");
}

#[test]
fn leaves_out_what_changed_before_the_workspace_was_made() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    write(&dir, "a", "uncommitted\n");
    write(&dir, "untracked", "untracked\n");
    world.run(&dir, &["true"]);
    let out = world.hmm(&dir, &["diff"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn leaves_out_what_changed_in_the_directory_since() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo new > b");
    write(&dir, "a", "changed since\n");
    world.git(&dir, &["commit", "--quiet", "-am", "since"]);
    let patch = stdout(&world.hmm(&dir, &["diff"]));
    assert!(patch.contains("+new"), "{patch}");
    assert!(!patch.contains("since"), "{patch}");
}

#[test]
fn shows_what_was_committed_in_the_workspace() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    world.sh(&dir, "echo two > a && git commit --quiet -am two");
    let patch = stdout(&world.hmm(&dir, &["diff"]));
    assert!(patch.contains("-one"), "{patch}");
    assert!(patch.contains("+two"), "{patch}");
}

#[test]
fn works_in_a_directory_that_is_not_a_repository() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    write(&dir, "a", "one\n");
    write(&dir, "c", "c\n");
    world.sh(&dir, "echo two > a && echo new > b && rm c");
    let patch = stdout(&world.hmm(&dir, &["diff"]));
    for line in [
        "-one",
        "+two",
        "+++ b/b",
        "+new",
        "--- a/c",
        "deleted file mode",
    ] {
        assert!(
            patch.lines().any(|l| l.starts_with(line)),
            "{line}: {patch}"
        );
    }
}

#[test]
fn fails_for_a_workspace_that_does_not_exist() {
    let world = World::new();
    let dir = world.dir("project");
    assert_fails(&world.hmm(&dir, &["diff", "nope"]), "no workspace nope");
}

#[test]
fn fails_without_workspaces() {
    let world = World::new();
    let dir = world.dir("project");
    assert_fails(&world.hmm(&dir, &["diff"]), "has no workspaces");
}
