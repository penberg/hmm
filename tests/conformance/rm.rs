//! `hmm rm`.

use std::collections::HashMap;

use crate::{World, assert_fails, need_sandbox, read, stderr, stdout};

#[test]
fn removes_drafts_by_name_id_and_the_start_of_an_id() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    world.run(&dir, &["-n", "parser", "true"]);
    let by_id = world.run(&dir, &["true"]);
    let by_prefix = world.run(&dir, &["true"]);
    let prefix = unique_prefix(&world.drafts(), &by_prefix.id);
    let out = world.hmm(&dir, &["rm", "parser", &by_id.id, prefix]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(world.drafts().is_empty(), "{:?}", world.drafts());
    assert_eq!(stdout(&world.hmm(&dir, &["ls"])), "");
}

#[test]
fn keeps_the_directory() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let draft = world.sh(&dir, "echo two > a && git commit --quiet -am two");
    assert!(world.hmm(&dir, &["rm", &draft.id]).status.success());
    assert_eq!(read(&dir, "a"), "one\n");
    assert_eq!(world.git(&dir, &["log", "--format=%s"]), "one\n");
    assert_eq!(world.git(&dir, &["status", "--porcelain"]), "");
}

#[test]
fn removes_the_reference_that_merge_made() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    let draft = world.sh(&dir, "echo two > a && git commit --quiet -am two");
    assert!(world.hmm(&dir, &["merge"]).status.success());
    let reference = format!("refs/hmm/{}", draft.id);
    world.git(&dir, &["rev-parse", "--verify", &reference]);
    assert!(world.hmm(&dir, &["rm", &draft.id]).status.success());
    assert_eq!(world.git(&dir, &["for-each-ref", "refs/hmm"]), "");
    assert_eq!(read(&dir, "a"), "two\n");
}

#[test]
fn removes_the_others_when_one_is_not_found() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let draft = world.run(&dir, &["true"]);
    let out = world.hmm(&dir, &["rm", "nope", &draft.id]);
    assert_fails(&out, "no draft nope");
    assert!(world.drafts().is_empty());
}

#[test]
fn names_are_the_directorys_own_and_ids_are_not() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let other = world.dir("other");
    let draft = world.run(&dir, &["-n", "parser", "true"]);
    assert_fails(&world.hmm(&other, &["rm", "parser"]), "no draft parser");
    assert_eq!(world.drafts(), [draft.id.as_str()]);
    assert!(world.hmm(&other, &["rm", &draft.id]).status.success());
    assert!(world.drafts().is_empty());
}

#[test]
fn refuses_the_start_of_more_than_one_drafts_id() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    // Of 33 IDs, two start with the same of the 32 letters.
    let mut seen = HashMap::new();
    let (first, second) = loop {
        let draft = world.run(&dir, &["true"]);
        let letter = draft.id[..1].to_string();
        if let Some(other) = seen.insert(letter, draft.id.clone()) {
            break (other, draft.id);
        }
    };
    let out = world.hmm(&dir, &["rm", &first[..1]]);
    assert_fails(&out, "is the start of more than one draft's ID");
    assert!(world.drafts().contains(&first));
    assert!(world.drafts().contains(&second));
}

#[test]
fn leaves_a_draft_in_which_a_command_is_running_alone() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let running = world.start(&dir, &["-n", "busy"]);
    let id = running.draft.id.clone();
    let ls = stdout(&world.hmm(&dir, &["ls"]));
    assert!(
        ls.lines()
            .any(|l| l.starts_with(&id) && l.contains("running")),
        "{ls}"
    );
    assert_fails(&world.hmm(&dir, &["rm", "busy"]), "is running");
    assert_eq!(world.drafts(), [id.as_str()]);
    running.stop();
    let ls = stdout(&world.hmm(&dir, &["ls"]));
    assert!(
        ls.lines().any(|l| l.starts_with(&id) && l.contains("done")),
        "{ls}"
    );
    assert!(world.hmm(&dir, &["rm", "busy"]).status.success());
    assert!(world.drafts().is_empty());
}

#[test]
fn needs_the_drafts_to_remove() {
    let world = World::new();
    let dir = world.dir("project");
    assert_fails(&world.hmm(&dir, &["rm"]), "rm needs the drafts to remove");
}

/// The shortest start of `id` that no other of `ids` starts with.
fn unique_prefix<'a>(ids: &[String], id: &'a str) -> &'a str {
    (1..=id.len())
        .map(|n| &id[..n])
        .find(|prefix| ids.iter().filter(|other| other.starts_with(prefix)).count() == 1)
        .unwrap()
}
