//! `hmm run`.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use crate::{World, assert_fails, need_sandbox, read, stderr, stdout, write};

#[test]
fn exits_with_the_commands_status() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let out = world.hmm(&dir, &["run", "sh", "-c", "exit 7"]);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn exits_with_128_plus_the_signal_that_ended_the_command() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let out = world.hmm(&dir, &["run", "sh", "-c", "kill -TERM $$"]);
    assert_eq!(out.status.code(), Some(128 + 15));
}

#[test]
fn prints_where_the_workspace_is_before_and_after() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let out = world.hmm(&dir, &["run", "true"]);
    assert!(out.status.success());
    let lines: Vec<String> = stderr(&out).lines().map(String::from).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0], lines[1]);
    let [id] = &world.workspaces()[..] else {
        panic!("not one workspace: {:?}", world.workspaces());
    };
    assert_eq!(id.len(), 6);
    assert!(
        id.bytes()
            .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b))
    );
    let tree = world
        .root()
        .canonicalize()
        .unwrap()
        .join(id)
        .join("tree/project");
    assert_eq!(lines[0], format!("hmm: workspace {id}: {}", tree.display()));
}

#[test]
fn passes_everything_from_the_command_on_to_it() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let out = world.hmm(&dir, &["run", "echo", "--resume", "-n", "x"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "--resume -n x\n");
}

#[test]
fn runs_the_shell_without_a_command() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let mut child = world
        .hmm_command(&dir, &["run"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"echo shell > f; exit 3\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let workspace = world.workspace_of(&dir, &stderr(&out));
    assert_eq!(read(&workspace.tree, "f"), "shell\n");
}

#[test]
fn runs_the_command_in_a_copy_of_everything_in_the_directory() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    write(&dir, ".gitignore", "target/\n");
    write(&dir, "a", "uncommitted\n");
    write(&dir, "untracked", "untracked\n");
    write(&dir, "target/out", "built\n");
    let out = world.hmm(
        &dir,
        &[
            "run",
            "sh",
            "-c",
            "pwd && cat a untracked target/out && git rev-parse --show-toplevel",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let workspace = world.workspace_of(&dir, &stderr(&out));
    let tree = workspace.tree.display().to_string();
    assert_eq!(
        stdout(&out),
        format!("{tree}\nuncommitted\nuntracked\nbuilt\n{tree}\n")
    );
}

#[test]
fn changes_stay_in_the_workspace() {
    need_sandbox!();
    let world = World::new();
    let dir = world.repo("project");
    write(&dir, "b", "b\n");
    let workspace = world.sh(&dir, "echo two > a && echo new > new && rm b");
    assert_eq!(read(&dir, "a"), "one\n");
    assert_eq!(read(&dir, "b"), "b\n");
    assert!(!dir.join("new").exists());
    assert_eq!(read(&workspace.tree, "a"), "two\n");
    assert_eq!(read(&workspace.tree, "new"), "new\n");
    assert!(!workspace.tree.join("b").exists());
    assert_eq!(read(&workspace.dir, "origin"), dir.display().to_string());
    assert!(!workspace.dir.join("name").exists());
}

#[test]
fn names_a_workspace() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let workspace = world.run(&dir, &["-n", "parser", "true"]);
    assert_eq!(read(&workspace.dir, "name"), "parser");
    let out = world.hmm(&dir, &["ls"]);
    let row = stdout(&out)
        .lines()
        .find(|line| line.starts_with(&workspace.id))
        .map(String::from)
        .unwrap();
    assert_eq!(row.split_whitespace().nth(1), Some("parser"));
    let workspace = world.run(&dir, &["--name", "lexer", "true"]);
    assert_eq!(read(&workspace.dir, "name"), "lexer");
}

#[test]
fn a_name_is_unique_among_the_directorys_workspaces() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let other = world.dir("other");
    world.run(&dir, &["-n", "parser", "true"]);
    let out = world.hmm(&dir, &["run", "-n", "parser", "true"]);
    assert_fails(&out, "already has a workspace named parser");
    assert_eq!(world.workspaces().len(), 1);
    world.run(&other, &["-n", "parser", "true"]);
    assert_eq!(world.workspaces().len(), 2);
}

#[test]
fn a_name_is_letters_digits_dashes_underscores_and_dots() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    for name in ["a/b", "a b", "", "é", "a:b"] {
        let out = world.hmm(&dir, &["run", "-n", name, "true"]);
        assert_fails(&out, "a workspace's name is");
    }
    assert!(world.workspaces().is_empty());
    world.run(&dir, &["-n", "Aa0-_.", "true"]);
}

#[test]
fn removes_the_workspace_with_rm() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let out = world.hmm(&dir, &["run", "--rm", "sh", "-c", "echo x > f; exit 3"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let err = stderr(&out);
    assert_eq!(err.lines().count(), 1, "{err}");
    let workspace = world.workspace_of(&dir, &err);
    assert!(!workspace.dir.exists());
    assert!(world.workspaces().is_empty(), "{:?}", world.workspaces());
    assert!(!dir.join("f").exists());
}

#[test]
fn removes_the_workspace_with_rm_when_the_command_cannot_run() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let missing = world.home().join("missing");
    let out = world.hmm(
        &dir,
        &["run", "--rm", "-w", missing.to_str().unwrap(), "true"],
    );
    assert_fails(&out, "missing");
    assert!(world.workspaces().is_empty(), "{:?}", world.workspaces());
}

#[test]
fn removes_the_workspace_with_rm_when_the_command_is_interrupted() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    // As the terminal does, the interrupt goes to hmm and the command both.
    let mut child = world
        .hmm_command(
            &dir,
            &["run", "--rm", "sh", "-c", "touch started; exec sleep 60"],
        )
        .process_group(0)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let workspace = world.workspace_of(&dir, &line);
    while !workspace.tree.join("started").exists() {
        thread::sleep(Duration::from_millis(10));
    }
    let group = format!("-{}", child.id());
    assert!(
        Command::new("kill")
            .args(["-INT", "--", &group])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(child.wait().unwrap().code(), Some(128 + 2));
    assert!(world.workspaces().is_empty(), "{:?}", world.workspaces());
}

#[test]
fn the_command_writes_only_to_the_workspace() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let home = world.home();
    for path in [
        dir.join("escape"),
        home.join("escape"),
        home.join(".claude/x"),
    ] {
        let out = world.hmm(
            &dir,
            &[
                "run",
                "sh",
                "-c",
                "echo x > \"$1\"",
                "sh",
                path.to_str().unwrap(),
            ],
        );
        assert!(!out.status.success(), "wrote {}", path.display());
        assert!(!path.exists());
    }
}

#[test]
fn the_command_writes_to_paths_given_with_w() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let claude = world.home().join(".claude");
    let other = world.dir("other");
    fs::create_dir(&claude).unwrap();
    world.run(
        &dir,
        &[
            "-w",
            claude.to_str().unwrap(),
            "--write",
            other.to_str().unwrap(),
            "sh",
            "-c",
            "mkdir \"$1/sessions\" && echo x > \"$1/sessions/s\" && echo y > \"$2/y\"",
            "sh",
            claude.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    assert_eq!(read(&claude, "sessions/s"), "x\n");
    assert_eq!(read(&other, "y"), "y\n");
}

#[test]
fn a_path_given_with_w_must_exist() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let missing = world.home().join("missing");
    let out = world.hmm(&dir, &["run", "-w", missing.to_str().unwrap(), "true"]);
    assert_fails(&out, "missing");
}

#[test]
fn the_command_writes_to_temporary_directories_and_caches() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    for cache in [".cargo", ".rustup", ".cache", ".npm", "Library/Caches"] {
        fs::create_dir_all(world.home().join(cache)).unwrap();
    }
    world.sh(
        &dir,
        "for d in /tmp \"${TMPDIR:-/tmp}\" ~/.cargo ~/.rustup ~/.cache ~/.npm ~/Library/Caches; do \
           echo x > \"$d/hmm-test-$$\" && rm \"$d/hmm-test-$$\" || exit 1; \
         done",
    );
}

#[test]
fn the_command_cannot_read_secrets() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let home = world.home();
    for secret in [
        ".ssh/id_ed25519",
        ".aws/credentials",
        ".gnupg/key",
        ".config/gh/hosts.yml",
    ] {
        write(&home, secret, "secret\n");
        let out = world.hmm(&dir, &["run", "cat", home.join(secret).to_str().unwrap()]);
        assert!(!out.status.success(), "read {secret}");
        assert!(!stdout(&out).contains("secret"));
    }
    write(&home, ".config/other", "readable\n");
    let out = world.hmm(
        &dir,
        &["run", "cat", home.join(".config/other").to_str().unwrap()],
    );
    assert_eq!(stdout(&out), "readable\n");
}

#[test]
fn the_command_cannot_read_or_write_other_workspaces() {
    need_sandbox!();
    let world = World::new();
    let dir = world.dir("project");
    let first = world.sh(&dir, "echo secret > s");
    let s = first.tree.join("s");
    let out = world.hmm(&dir, &["run", "cat", s.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!stdout(&out).contains("secret"));
    let out = world.hmm(
        &dir,
        &[
            "run",
            "sh",
            "-c",
            "echo x > \"$1\"",
            "sh",
            s.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    assert_eq!(read(&first.tree, "s"), "secret\n");
}
