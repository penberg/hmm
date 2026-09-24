//! Conformance tests: `hmm` run as a user runs it, checked against what
//! `man/hmm.1` says it does.
//!
//! Every test has a world of its own: a home directory, so that its workspaces
//! go in a data directory no other test sees, and directories to make
//! workspaces of. Worlds are kept under Cargo's temporary directory for tests
//! rather than the system's, as the command in a workspace may write to the
//! latter.
//!
//! `hmm run` needs a sandbox: on macOS `sandbox-exec`, which cannot run
//! inside another sandbox, such as that of a command already running in a
//! workspace, and on Linux Landlock, which may not be enabled. Tests that make
//! workspaces are skipped where there is none, and say so.

mod apply;
mod diff;
mod merge;
mod rm;
mod run;

use std::{
    env, fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::OnceLock,
    thread,
};

/// Returns from the test, saying so, if the sandbox cannot run here.
macro_rules! need_sandbox {
    () => {
        if !$crate::sandbox() {
            eprintln!("skipped: the sandbox cannot run here");
            return;
        }
    };
}
pub(crate) use need_sandbox;

/// Whether `sandbox-exec` can run here.
#[cfg(target_os = "macos")]
pub fn sandbox() -> bool {
    static SANDBOX: OnceLock<bool> = OnceLock::new();
    *SANDBOX.get_or_init(|| {
        Command::new("/usr/bin/sandbox-exec")
            .args(["-p", "(version 1)(allow default)", "--", "/usr/bin/true"])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Whether Landlock is enabled here, in a version `hmm` can use.
#[cfg(target_os = "linux")]
pub fn sandbox() -> bool {
    static SANDBOX: OnceLock<bool> = OnceLock::new();
    *SANDBOX.get_or_init(|| {
        // `landlock_create_ruleset` with `LANDLOCK_CREATE_RULESET_VERSION`
        // returns the version of Landlock's ABI.
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<u64>(),
                0,
                1,
            )
        };
        abi >= 2
    })
}

/// A test's world: a home directory, and directories to make workspaces of.
pub struct World {
    dir: PathBuf,
}

impl World {
    /// A new, empty world, named after the test.
    #[allow(clippy::new_without_default)]
    pub fn new() -> World {
        let name = thread::current()
            .name()
            .unwrap_or("test")
            .replace("::", "-");
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("conformance")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("home")).unwrap();
        World {
            dir: dir.canonicalize().unwrap(),
        }
    }

    /// The home directory.
    pub fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    /// The directory workspaces are kept in.
    pub fn root(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home()
                .join("Library")
                .join("Application Support")
                .join("hmm")
        } else {
            self.home().join(".local").join("share").join("hmm")
        }
    }

    /// The IDs of every workspace.
    pub fn workspaces(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.root()) else {
            return Vec::new();
        };
        let mut ids: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        ids.sort();
        ids
    }

    /// A new, empty directory, `name`.
    pub fn dir(&self, name: &str) -> PathBuf {
        let dir = self.dir.join(name);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// A new git repository, `name`, on branch `main`, with one commit of a
    /// file `a` holding `one`.
    pub fn repo(&self, name: &str) -> PathBuf {
        let dir = self.dir(name);
        self.git(&dir, &["init", "--quiet", "--initial-branch=main"]);
        write(&dir, "a", "one\n");
        self.git(&dir, &["add", "a"]);
        self.git(&dir, &["commit", "--quiet", "-m", "one"]);
        dir
    }

    /// A command that runs `program` in `cwd` in the world: with its home
    /// directory, and git configured only by the repository.
    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>, cwd: &Path) -> Command {
        let mut command = Command::new(program);
        command.current_dir(cwd);
        for (key, _) in env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env("HOME", self.home())
            .env("SHELL", "/bin/sh")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_PAGER", "cat")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        command
    }

    /// A command that runs `hmm` with `args` in `cwd`.
    pub fn hmm_command(&self, cwd: &Path, args: &[&str]) -> Command {
        let mut command = self.command(env!("CARGO_BIN_EXE_hmm"), cwd);
        command.args(args);
        command
    }

    /// Runs `hmm` with `args` in `cwd`.
    pub fn hmm(&self, cwd: &Path, args: &[&str]) -> Output {
        self.hmm_command(cwd, args).output().unwrap()
    }

    /// Runs `hmm run` with `args` in `cwd`, which must succeed, and returns
    /// the workspace it made.
    pub fn run(&self, cwd: &Path, args: &[&str]) -> Workspace {
        let out = self.hmm(cwd, &[&["run"], args].concat());
        assert!(out.status.success(), "hmm run {args:?}: {}", stderr(&out));
        self.workspace_of(cwd, &stderr(&out))
    }

    /// Runs `cmd` with `sh -c` in a new workspace of `cwd`, which must succeed.
    pub fn sh(&self, cwd: &Path, cmd: &str) -> Workspace {
        self.run(cwd, &["sh", "-c", cmd])
    }

    /// Starts `hmm run` with `args` in `cwd` to run until it is stopped, and
    /// returns once the command is running.
    pub fn start(&self, cwd: &Path, args: &[&str]) -> Running {
        let wait = "while [ ! -e stop ]; do sleep 0.05; done";
        let mut child = self
            .hmm_command(cwd, &[&["run"], args, &["sh", "-c", wait]].concat())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stderr.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        let workspace = self.workspace_of(cwd, &line);
        Running { child, workspace }
    }

    /// The workspace `hmm run` in `cwd` says it made in `stderr`.
    pub fn workspace_of(&self, cwd: &Path, stderr: &str) -> Workspace {
        let line = stderr
            .lines()
            .find_map(|line| line.strip_prefix("hmm: workspace "))
            .unwrap_or_else(|| panic!("hmm run did not say where the workspace is: {stderr}"));
        let id = line[..line.find([' ', ':']).unwrap()].to_string();
        let dir = self.root().canonicalize().unwrap().join(&id);
        let tree = dir.join("tree").join(cwd.file_name().unwrap());
        assert!(
            line.ends_with(&format!(": {}", tree.display())),
            "workspace {id} is not at {}: {line}",
            tree.display()
        );
        Workspace { id, dir, tree }
    }

    /// Runs git with `args` in `cwd`, which must succeed, and returns what it
    /// printed.
    pub fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.command("git", cwd).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
        stdout(&out)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // A failed test's world is kept, to look into.
        if !thread::panicking() {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

/// A workspace, as `hmm run` made it.
pub struct Workspace {
    pub id: String,
    pub dir: PathBuf,
    pub tree: PathBuf,
}

/// A workspace with a command running in it.
pub struct Running {
    child: Child,
    pub workspace: Workspace,
}

impl Running {
    /// Stops the command, and waits for `hmm run` to exit.
    pub fn stop(mut self) {
        write(&self.workspace.tree, "stop", "");
        assert!(self.child.wait().unwrap().success());
    }
}

/// Writes `contents` to `name` in `dir`.
pub fn write(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// What `name` in `dir` holds.
pub fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap()
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Asserts that `out` is of a command that failed with status 1, saying
/// something with `message` in it.
#[track_caller]
pub fn assert_fails(out: &Output, message: &str) {
    assert_eq!(out.status.code(), Some(1), "{}", stderr(out));
    assert!(
        stderr(out).contains(message),
        "expected {message:?} in: {}",
        stderr(out)
    );
}
