//! `hmm pr`: pushes the commits made in a workspace to a branch, and opens a
//! GitHub pull request for them.

use std::{
    env, fs, io,
    path::Path,
    process::{Command, ExitCode, Stdio},
};

use argh::FromArgs;

use crate::{
    Workspace,
    cmd::merge::{take, warn_uncommitted},
    git, root,
};

/// Push the commits made in a workspace to a branch, and open a GitHub pull
/// request for them.
#[derive(FromArgs)]
#[argh(subcommand, name = "pr")]
pub struct Pr {
    /// the branch to push to; the one pushed to before, or else the
    /// workspace's name, or else one made from the first commit's subject
    #[argh(option, short = 'b')]
    branch: Option<String>,

    /// the branch to ask to merge into; the remote's default branch if none
    #[argh(option)]
    base: Option<String>,

    /// open the pull request as a draft
    #[argh(switch)]
    draft: bool,

    /// the workspace, by name, ID, or the start of an ID; the working
    /// directory's latest if none
    #[argh(positional)]
    workspace: Option<String>,
}

/// The remote that `hmm pr` pushes to.
const REMOTE: &str = "origin";

impl Pr {
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let workspace = Workspace::pick(&root, &cwd, self.workspace.as_deref())?;
        // A command still running could be halfway through a commit.
        let _lock = workspace.lock()?;
        let origin = workspace.origin()?;
        let github = remote_github(&origin)?;
        let base = match self.base {
            Some(base) => base,
            None => default_branch(&origin)?,
        };
        let Some(reference) = take(&root, &workspace, &origin)? else {
            eprintln!(
                "hmm: workspace {} has no commits to push",
                workspace.label()
            );
            return Ok(ExitCode::FAILURE);
        };
        warn_uncommitted(&workspace, &origin, &reference)?;

        let range = match git::head(&workspace) {
            Some(head) => format!("{head}..{reference}"),
            None => reference.clone(),
        };
        let subjects = log(&origin, &range, "%s")?;
        let pushed = Pushed::of(&workspace);
        let branch = match (self.branch, &pushed, workspace.name()) {
            (Some(branch), _, _) => branch,
            (None, Some(pushed), _) => pushed.branch.clone(),
            (None, None, Some(name)) => name,
            (None, None, None) => {
                slug(subjects.first().map_or("", String::as_str)).unwrap_or(workspace.id.clone())
            }
        };
        // What the branch is expected to be: as it was last pushed, or else
        // not there at all.
        let expected = pushed
            .filter(|pushed| pushed.branch == branch)
            .map(|pushed| pushed.commit)
            .unwrap_or_default();
        let tip = rev_parse(&origin, &reference)?;
        if !push(&origin, &reference, &branch, &expected)? {
            return Err(io::Error::other(format!(
                "could not push workspace {} to {REMOTE} as {branch}: if a branch of \
                 that name is there already, give another with --branch",
                workspace.label(),
            )));
        }
        fs::write(workspace.dir.join("pushed"), format!("{branch} {tip}\n"))?;
        eprintln!(
            "hmm: pushed workspace {} to {REMOTE} as {branch}",
            workspace.label(),
        );

        let Some(repo) = github else {
            eprintln!("hmm: {REMOTE} is not on GitHub: no pull request opened");
            return Ok(ExitCode::SUCCESS);
        };
        match open(
            &origin, &repo, &base, &branch, &range, &subjects, self.draft,
        ) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                eprintln!("hmm: gh is not installed: open the pull request at");
                println!("https://github.com/{repo}/compare/{base}...{branch}?expand=1");
                Ok(ExitCode::SUCCESS)
            }
            result => result,
        }
    }
}

/// What `hmm pr` last pushed from a workspace, as it records it in the
/// workspace's `pushed` file: the branch, and the commit.
struct Pushed {
    branch: String,
    commit: String,
}

impl Pushed {
    fn of(workspace: &Workspace) -> Option<Pushed> {
        let pushed = fs::read_to_string(workspace.dir.join("pushed")).ok()?;
        let (branch, commit) = pushed.trim().split_once(' ')?;
        Some(Pushed {
            branch: branch.to_string(),
            commit: commit.to_string(),
        })
    }
}

/// The GitHub repository of `origin`'s remote, as `owner/name`, if it is on
/// GitHub.
fn remote_github(origin: &Path) -> io::Result<Option<String>> {
    // The URL as configured: `git remote get-url` would rewrite it with
    // `url.<base>.insteadOf`, which can hide where the remote is on GitHub.
    let url = git::git()
        .arg("-C")
        .arg(origin)
        .args(["config", "--get", &format!("remote.{REMOTE}.url")])
        .output()?;
    if !url.status.success() {
        return Err(io::Error::other(format!(
            "{} has no remote {REMOTE} to push to",
            origin.display()
        )));
    }
    Ok(github(String::from_utf8_lossy(&url.stdout).trim()))
}

/// The remote's default branch: as `origin`'s repository last saw it, or else
/// as the remote says.
fn default_branch(origin: &Path) -> io::Result<String> {
    let known = git::git()
        .arg("-C")
        .arg(origin)
        .args(["symbolic-ref", "--quiet", "--short"])
        .arg(format!("refs/remotes/{REMOTE}/HEAD"))
        .output()?;
    let known = String::from_utf8_lossy(&known.stdout);
    if let Some(branch) = known.trim().strip_prefix(&format!("{REMOTE}/")) {
        return Ok(branch.to_string());
    }
    let asked = git::git()
        .arg("-C")
        .arg(origin)
        .args(["ls-remote", "--symref", REMOTE, "HEAD"])
        .output()?;
    String::from_utf8_lossy(&asked.stdout)
        .lines()
        .find_map(|line| {
            let (target, name) = line.strip_prefix("ref: refs/heads/")?.split_once('\t')?;
            (name == "HEAD").then(|| target.to_string())
        })
        .ok_or_else(|| {
            io::Error::other(format!(
                "could not tell {REMOTE}'s default branch: give the branch to merge into with \
                 --base"
            ))
        })
}

/// A branch name made from a commit's `subject`: its first few words, after
/// any `area:` prefix, lowercase and joined with `-`.
fn slug(subject: &str) -> Option<String> {
    let subject = match subject.split_once(": ") {
        Some((area, rest)) if !area.contains(' ') => rest,
        _ => subject,
    };
    let words: Vec<String> = subject
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .take(5)
        .collect();
    (!words.is_empty()).then(|| words.join("-"))
}

/// The GitHub repository at `url`, as `owner/name`, if it is one.
fn github(url: &str) -> Option<String> {
    let path = [
        "https://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(prefix))?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (valid(owner) && valid(name)).then(|| format!("{owner}/{name}"))
}

/// Pushes `reference` to the remote as `branch`, and returns whether it could.
///
/// The branch is overwritten only if it is at `expected`, or, if that is
/// empty, only if it is not there, so that a branch someone else pushed, or
/// added to, is never lost.
fn push(origin: &Path, reference: &str, branch: &str, expected: &str) -> io::Result<bool> {
    let pushed = git::git()
        .arg("-C")
        .arg(origin)
        .args(["push", "--quiet"])
        .arg(format!("--force-with-lease=refs/heads/{branch}:{expected}"))
        .arg(REMOTE)
        .arg(format!("{reference}:refs/heads/{branch}"))
        .status()?;
    Ok(pushed.success())
}

/// Opens a pull request on `repo` to merge `branch` into `base` with `gh`, or,
/// if one is already open, says where it is. The title and description are
/// those of the commit in `range`, if there is one, or else the first commit's
/// subject and a list of the commits' `subjects`.
fn open(
    origin: &Path,
    repo: &str,
    base: &str,
    branch: &str,
    range: &str,
    subjects: &[String],
    draft: bool,
) -> io::Result<ExitCode> {
    let existing = gh(origin)
        .args(["pr", "view", branch, "--repo", repo, "--json", "url,state"])
        .args(["--jq", r#"select(.state == "OPEN") | .url"#])
        .stderr(Stdio::null())
        .output()?;
    let existing = String::from_utf8_lossy(&existing.stdout).trim().to_string();
    if !existing.is_empty() {
        eprintln!("hmm: pull request updated:");
        println!("{existing}");
        return Ok(ExitCode::SUCCESS);
    }

    let title = subjects.first().map_or(branch, String::as_str);
    let body = if subjects.len() == 1 {
        log(origin, range, "%b")?.concat().trim().to_string()
    } else {
        subjects
            .iter()
            .map(|subject| format!("- {subject}\n"))
            .collect()
    };
    let mut create = gh(origin);
    create
        .args([
            "pr", "create", "--repo", repo, "--base", base, "--head", branch,
        ])
        .arg(format!("--title={title}"))
        .arg(format!("--body={body}"));
    if draft {
        create.arg("--draft");
    }
    Ok(if create.status()?.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// What `format` shows of each commit in `range` in `origin`'s repository,
/// oldest first.
fn log(origin: &Path, range: &str, format: &str) -> io::Result<Vec<String>> {
    let out = git::git()
        .arg("-C")
        .arg(origin)
        .args(["log", "--reverse", "--no-color", "-z"])
        .arg(format!("--format={format}"))
        .arg(range)
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other("git could not read the commits"));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect())
}

/// A command that runs `gh` in `origin`.
fn gh(origin: &Path) -> Command {
    let mut gh = Command::new("gh");
    gh.current_dir(origin);
    gh
}

/// The commit `reference` is in `origin`'s repository.
fn rev_parse(origin: &Path, reference: &str) -> io::Result<String> {
    let out = git::git()
        .arg("-C")
        .arg(origin)
        .args(["rev-parse", "--verify", reference])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!("no commit {reference}")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
