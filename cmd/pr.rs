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
    /// open the pull request as a draft
    #[argh(switch)]
    draft: bool,

    /// the workspace, by name, ID, or the start of an ID; the working
    /// directory's latest if none
    #[argh(positional)]
    workspace: Option<String>,
}

impl Pr {
    pub fn run(self) -> io::Result<ExitCode> {
        let root = root()?;
        let cwd = env::current_dir()?.canonicalize()?;
        let workspace = Workspace::pick(&root, &cwd, self.workspace.as_deref())?;
        // A command still running could be halfway through a commit.
        let _lock = workspace.lock()?;
        let origin = workspace.origin()?;
        let target = Target::of(&origin)?;
        let Some(reference) = take(&root, &workspace, &origin)? else {
            eprintln!(
                "hmm: workspace {} has no commits to push",
                workspace.label()
            );
            return Ok(ExitCode::FAILURE);
        };
        warn_uncommitted(&workspace, &origin, &reference)?;

        let branch = format!(
            "hmm/{}",
            workspace.name().unwrap_or_else(|| workspace.id.clone())
        );
        let tip = rev_parse(&origin, &reference)?;
        if !push(&workspace, &origin, &target.remote, &reference, &branch)? {
            return Err(io::Error::other(format!(
                "could not push workspace {} to {} as {branch}",
                workspace.label(),
                target.remote
            )));
        }
        fs::write(workspace.dir.join("pushed"), &tip)?;
        eprintln!(
            "hmm: pushed workspace {} to {} as {branch}",
            workspace.label(),
            target.remote
        );

        let Some(repo) = target.github else {
            eprintln!(
                "hmm: {} is not on GitHub: no pull request opened",
                target.remote
            );
            return Ok(ExitCode::SUCCESS);
        };
        match open(
            &origin,
            &repo,
            &target.base,
            &branch,
            &workspace,
            &reference,
            self.draft,
        ) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                eprintln!("hmm: gh is not installed: open the pull request at");
                println!(
                    "https://github.com/{repo}/compare/{}...{branch}?expand=1",
                    target.base
                );
                Ok(ExitCode::SUCCESS)
            }
            result => result,
        }
    }
}

/// Where a workspace's commits go: the remote to push them to, the branch on
/// it to ask to merge them into, and the remote's GitHub repository, as
/// `owner/name`, if it is on GitHub.
struct Target {
    remote: String,
    base: String,
    github: Option<String>,
}

impl Target {
    /// The target of `origin`'s workspaces: the remote and branch that the
    /// branch `origin` is on tracks, or, if it tracks none, `origin` and the
    /// branch of the same name.
    fn of(origin: &Path) -> io::Result<Target> {
        let branch = git::git()
            .arg("-C")
            .arg(origin)
            .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
            .output()?;
        let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
        if branch.is_empty() {
            return Err(io::Error::other(format!(
                "{} is not on a branch",
                origin.display()
            )));
        }
        let upstream = git::git()
            .arg("-C")
            .arg(origin)
            .args([
                "for-each-ref",
                "--format=%(upstream:remotename)%00%(upstream:remoteref)",
            ])
            .arg(format!("refs/heads/{branch}"))
            .output()?;
        let upstream = String::from_utf8_lossy(&upstream.stdout).trim().to_string();
        let (remote, base) = match upstream.split_once('\0') {
            Some((remote, base))
                if !remote.is_empty() && remote != "." && base.starts_with("refs/heads/") =>
            {
                (remote.to_string(), base["refs/heads/".len()..].to_string())
            }
            _ => ("origin".to_string(), branch),
        };
        // The URL as configured: `git remote get-url` would rewrite it with
        // `url.<base>.insteadOf`, which can hide where the remote is on GitHub.
        let url = git::git()
            .arg("-C")
            .arg(origin)
            .args(["config", "--get", &format!("remote.{remote}.url")])
            .output()?;
        if !url.status.success() {
            return Err(io::Error::other(format!(
                "{} has no remote {remote} to push to",
                origin.display()
            )));
        }
        let github = github(String::from_utf8_lossy(&url.stdout).trim());
        Ok(Target {
            remote,
            base,
            github,
        })
    }
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

/// Pushes `reference` to `remote` as `branch`, and returns whether it could.
///
/// The branch is overwritten only if it is what `hmm pr` last pushed from the
/// workspace, or, if it has pushed nothing, only if it does not exist, so that
/// a branch someone else pushed, or added to, is never lost.
fn push(
    workspace: &Workspace,
    origin: &Path,
    remote: &str,
    reference: &str,
    branch: &str,
) -> io::Result<bool> {
    let pushed = fs::read_to_string(workspace.dir.join("pushed")).unwrap_or_default();
    let pushed = git::git()
        .arg("-C")
        .arg(origin)
        .args(["push", "--quiet"])
        .arg(format!(
            "--force-with-lease=refs/heads/{branch}:{}",
            pushed.trim()
        ))
        .arg(remote)
        .arg(format!("{reference}:refs/heads/{branch}"))
        .status()?;
    Ok(pushed.success())
}

/// Opens a pull request on `repo` to merge `branch` into `base` with `gh`, or,
/// if one is already open, says where it is. The title and description are
/// those of the commit, if there is one, or else the first commit's subject
/// and a list of the commits' subjects.
fn open(
    origin: &Path,
    repo: &str,
    base: &str,
    branch: &str,
    workspace: &Workspace,
    reference: &str,
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

    let range = match git::head(workspace) {
        Some(head) => format!("{head}..{reference}"),
        None => reference.to_string(),
    };
    let log = |format: &str| -> io::Result<String> {
        let out = git::git()
            .arg("-C")
            .arg(origin)
            .args(["log", "--reverse", "--no-color", "-z"])
            .arg(format!("--format={format}"))
            .arg(&range)
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other("git could not read the commits"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let subjects = log("%s")?;
    let subjects: Vec<&str> = subjects.split('\0').filter(|s| !s.is_empty()).collect();
    let title = subjects.first().copied().unwrap_or(branch).to_string();
    let body = if subjects.len() == 1 {
        log("%b")?.trim_matches(['\0', '\n']).to_string()
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
