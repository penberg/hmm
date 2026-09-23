# hmm

`hmm` runs a command in a workspace: a copy-on-write clone of the working
directory that the command may change as it likes, while the working
directory and the rest of the system stay as they are. It is meant for
running coding agents, and anything else you would rather not trust with
your files.

![Claude Code commits in a workspace, and hmm merges the commit](.github/assets/demo.gif)

Making a workspace is quick and takes no space until the command writes to
it. The workspace includes everything in the directory (the repository,
uncommitted changes, and build output), so builds start warm.

## Install

```sh
cargo install --path .
```

`hmm` runs only on macOS for now, and needs the working directory to be on
an APFS volume, the same one as the home directory.

## Usage

Run Claude Code in a workspace named `parser`, letting it save its sessions
and settings in `~/.claude`, and have it commit its work there as usual:

```sh
hmm run -n parser -w ~/.claude claude
```

When it is done, see what it changed, merge its commits into the branch you
are on, and remove the workspace:

```sh
hmm diff parser
hmm merge parser
hmm rm parser
```

`hmm merge` works like `git merge`: a fast-forward if your branch has not
moved since the workspace was made, a merge commit if it has, and conflicts
to resolve as for any merge if both changed the same lines. Only commits are
merged; if the command left changes it did not commit, `hmm merge` says so.

Meanwhile, you can keep working, and run more workspaces side by side. List
the workspaces of the working directory:

```console
$ hmm
ID      NAME    STATE    AGE  ORIGIN
k3xq9a  parser  done      2h  ~/src/penberg/dwim
p7mw2d  -       running   5m  ~/src/penberg/dwim
```

A workspace can be referred to by its name, its ID, or a unique prefix of
its ID; with none, commands use the working directory's latest workspace.

## Git

`hmm` knows about git, but works without it. When the working directory is
the top of a git repository, making a workspace records the commit it is at
and the tree of its files, uncommitted changes included. This takes little
time and puts nothing in your repository, and gives `hmm` something to
compare the workspace with:

- `hmm diff` shows what changed in the workspace, leaving out what git
  ignores, such as build output.
- `hmm merge` brings the commits made in the workspace into your branch.
- `hmm apply` makes the changes in the workspace, committed or not, in the
  working directory, without staging or committing anything. It applies all
  of them or, if any conflicts with what you have changed since, none.

Git runs outside the sandbox and never reads the repository in the
workspace, whose configuration the command could have changed to make git
run anything: commits are taken from the workspace as a bundle, and every
object is checked as it is fetched.

In a directory that is not a git repository, `hmm` keeps a second clone to
compare the workspace with, and `hmm diff` and `hmm apply` work the same.

## Sandbox

The command runs under the macOS sandbox. It may write only to the
workspace, temporary directories, build and package manager caches
(`~/.cargo`, `~/.rustup`, `~/.cache`, `~/.npm`, `~/Library/Caches`), and
paths given with `-w`. It cannot read `~/.ssh`, `~/.aws`, `~/.gnupg`,
`~/.config/gh`, or other workspaces.

The network is not confined: a command can send anything it can read to
anywhere, and can push to remotes whose credentials it can reach, such as
through the macOS keychain.

Commits made in a workspace are not signed if signing needs `~/.gnupg` or
`~/.ssh`, which the command cannot read; the merge commit `hmm merge` makes
is. A workspace of a linked worktree cannot be committed to, as its
repository lies outside the workspace.

## License

MIT

## Command Line Reference

| Command | Description |
| --- | --- |
| `hmm` | List the workspaces of the working directory, as `hmm ls` does |
| `hmm run [--rm] [-n name] [-w path]... [command...]` | Run a command (or your shell) in a new workspace, removing it afterwards with `--rm` |
| `hmm ls [-a]` | List workspaces of the working directory, or of every directory with `-a` |
| `hmm diff [--stat] [workspace]` | Show what changed in a workspace |
| `hmm merge [workspace]` | Merge the commits made in a workspace into the working directory's branch |
| `hmm apply [--check] [workspace]` | Make a workspace's changes in the working directory, or with `--check` only say whether they apply |
| `hmm rm workspace...` | Remove workspaces, and the references to their commits that `hmm merge` made |

| Option | Description |
| --- | --- |
| `--rm` | Remove the workspace when the command exits |
| `-n`, `--name name` | Name the workspace, so it can be referred to by name |
| `-w`, `--write path` | Let the command write to `path` too; can be repeated |
| `-a`, `--all` | List the workspaces of every directory |
| `--stat` | Show only which files changed, and how much |
| `--check` | Only say whether the changes apply |

See [`man/hmm.1`](man/hmm.1) (`man ./man/hmm.1`) for the details.
