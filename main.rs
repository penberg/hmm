//! `hmm` runs a command in a draft: a copy-on-write clone of the working
//! directory, where the command may write to the clone and to caches, but
//! nowhere else, and may not read secrets such as SSH keys.

#[cfg(target_os = "macos")]
mod darwin;

mod cmd {
    pub mod diff;
    pub mod ls;
    pub mod rm;
    pub mod run;
}

#[cfg(not(target_os = "macos"))]
compile_error!("hmm runs only on macOS for now");

use std::{
    ffi::OsString,
    fs::{self, File, TryLockError},
    io,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::ExitCode,
    time::SystemTime,
};

use argh::FromArgs;

/// Run commands in drafts: copies of the working directory that they may
/// write to, while the rest of the system stays as it is. With no command,
/// list the drafts of the working directory.
#[derive(FromArgs)]
struct Args {
    #[argh(subcommand)]
    command: Option<Command>,
}

#[derive(FromArgs)]
#[argh(subcommand)]
enum Command {
    Run(cmd::run::Run),
    Ls(cmd::ls::Ls),
    Diff(cmd::diff::Diff),
    Rm(cmd::rm::Rm),
}

fn main() -> ExitCode {
    let args: Args = argh::from_env();
    let result = match args.command {
        None => cmd::ls::list(false),
        Some(Command::Run(run)) => run.run(),
        Some(Command::Ls(ls)) => ls.run(),
        Some(Command::Diff(diff)) => diff.run(),
        Some(Command::Rm(rm)) => rm.run(),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hmm: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The directory drafts are kept in: `hmm` under the platform's local data
/// directory (`~/Library/Application Support/hmm` on macOS).
pub fn root() -> io::Result<PathBuf> {
    let root = dirs::data_local_dir()
        .ok_or_else(|| io::Error::other("no local data directory"))?
        .join("hmm");
    fs::create_dir_all(&root)?;
    root.canonicalize()
}

/// The letters of a draft's ID: digits and lowercase letters, without the
/// ones easily mistaken for others (`i`, `l`, `o`, `u`).
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// The number of letters in a draft's ID.
const ID_LEN: usize = 6;

/// A draft: a directory under the root named by a random ID, holding two
/// clones of a directory: `base`, as it was when the draft was made, and
/// `tree/<name>`, named as the directory, which the command runs in and
/// changes; an `origin` file with the directory's path; and, if the draft was
/// given one, a `name` file with its name. The command running in the draft holds a lock on the `origin`
/// file.
pub struct Draft {
    pub id: String,
    pub dir: PathBuf,
}

impl Draft {
    /// Creates a draft under `root` for a clone of `origin`, without the
    /// clone, named `name` if one is given.
    pub fn create(root: &Path, origin: &Path, name: Option<&str>) -> io::Result<Draft> {
        if let Some(name) = name {
            check(name)?;
            for draft in Draft::all(root)? {
                if draft.origin().is_ok_and(|o| o == origin)
                    && draft.name().as_deref() == Some(name)
                {
                    return Err(io::Error::other(format!(
                        "{} already has a draft named {name}",
                        origin.display()
                    )));
                }
            }
        }
        loop {
            let id = random_id()?;
            let dir = root.join(&id);
            match fs::create_dir(&dir) {
                Ok(()) => {
                    fs::write(dir.join("origin"), origin.as_os_str().as_bytes())?;
                    if let Some(name) = name {
                        fs::write(dir.join("name"), name)?;
                    }
                    return Ok(Draft { id, dir });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Finds the draft `key` refers to: one of `origin`'s drafts by name, or
    /// any draft by ID or the start of an ID that no other shares.
    pub fn find(root: &Path, origin: &Path, key: &str) -> io::Result<Draft> {
        let (mut named, rest): (Vec<Draft>, Vec<Draft>) =
            Draft::all(root)?.into_iter().partition(|draft| {
                draft.name().as_deref() == Some(key) && draft.origin().is_ok_and(|o| o == origin)
            });
        if let Some(draft) = named.pop() {
            return Ok(draft);
        }
        let mut matches: Vec<Draft> = rest
            .into_iter()
            .filter(|draft| draft.id.starts_with(key))
            .collect();
        match matches.len() {
            0 => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no draft {key}"),
            )),
            1 => Ok(matches.pop().unwrap()),
            _ => Err(io::Error::other(format!(
                "{key} is the start of more than one draft's ID"
            ))),
        }
    }

    /// The latest of `origin`'s drafts.
    pub fn latest(root: &Path, origin: &Path) -> io::Result<Draft> {
        Draft::all(root)?
            .into_iter()
            .rfind(|draft| draft.origin().is_ok_and(|o| o == origin))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{} has no drafts", origin.display()),
                )
            })
    }

    /// Every draft under `root`, oldest first.
    pub fn all(root: &Path) -> io::Result<Vec<Draft>> {
        let mut all = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && let Some(id) = entry.file_name().to_str()
            {
                all.push(Draft {
                    id: id.to_string(),
                    dir: entry.path(),
                });
            }
        }
        all.sort_by_cached_key(|draft| draft.created().ok());
        Ok(all)
    }

    /// The directory the draft was cloned from.
    pub fn origin(&self) -> io::Result<PathBuf> {
        let bytes = fs::read(self.dir.join("origin"))?;
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }

    /// The name the draft was given, if any.
    pub fn name(&self) -> Option<String> {
        fs::read_to_string(self.dir.join("name")).ok()
    }

    /// When the draft was created.
    pub fn created(&self) -> io::Result<SystemTime> {
        self.dir.metadata()?.created()
    }

    /// The clone the command runs in, named as the directory it was cloned
    /// from.
    pub fn tree(&self) -> io::Result<PathBuf> {
        let origin = self.origin()?;
        Ok(self
            .dir
            .join("tree")
            .join(origin.file_name().unwrap_or("root".as_ref())))
    }

    /// The clone of the directory as it was when the draft was made, which
    /// the command cannot see: what the draft is compared with.
    pub fn base(&self) -> PathBuf {
        self.dir.join("base")
    }

    /// The draft as the user refers to it: its ID, and its name if it has
    /// one.
    pub fn label(&self) -> String {
        match self.name() {
            Some(name) => format!("{} ({name})", self.id),
            None => self.id.clone(),
        }
    }

    /// Locks the draft for a command to run in, failing if one already is.
    pub fn lock(&self) -> io::Result<File> {
        let file = File::open(self.dir.join("origin"))?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(TryLockError::WouldBlock) => Err(io::Error::other(format!(
                "draft {} is running",
                self.label()
            ))),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    /// Whether a command is running in the draft.
    pub fn running(&self) -> bool {
        self.lock().is_err()
    }
}

/// Checks that `name` can name a draft: letters, digits, `-`, `_` and `.`,
/// not starting with `-`.
fn check(name: &str) -> io::Result<()> {
    let valid = !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "a draft's name is letters, digits, '-', '_' and '.': {name}"
        )))
    }
}

/// A random draft ID of `ID_LEN` letters from `ALPHABET`.
fn random_id() -> io::Result<String> {
    let mut bytes = [0u8; ID_LEN];
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(bytes
        .iter()
        .map(|b| ALPHABET[(b % 32) as usize] as char)
        .collect())
}
