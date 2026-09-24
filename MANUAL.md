# hmm manual

`hmm` runs a command in a workspace: a copy-on-write clone of the working
directory that the command may change as it likes, while the working
directory and the rest of the system stay as they are.

This manual explains how to use `hmm`. For how the sandbox works on each
platform, see [README.md](README.md#sandbox); for the man page, see
[`man/hmm.1`](man/hmm.1) (`man ./man/hmm.1`).

- [Getting started](#getting-started)
- [Workspaces](#workspaces)
- [Commands](#commands)
  - [`hmm`](#hmm)
  - [`hmm run`](#hmm-run)
  - [`hmm ls`](#hmm-ls)
  - [`hmm diff`](#hmm-diff)
  - [`hmm merge`](#hmm-merge)
  - [`hmm apply`](#hmm-apply)
  - [`hmm rm`](#hmm-rm)
- [What the command can do](#what-the-command-can-do)
- [Files](#files)
- [Exit status](#exit-status)
- [Caveats](#caveats)

## Getting started

Go to the directory you want to work on, usually the top of a git
repository, and run a command in a new workspace of it. Here, Claude Code,
in a workspace named `parser`, letting it save its sessions and settings in
`~/.claude`:

```sh
cd ~/src/dwim
hmm run -n parser -w ~/.claude claude
```

The command runs in the workspace, not in `~/src/dwim`. It can edit, build,
and commit there as usual. When it exits, `hmm` prints where the workspace
is, and keeps it.

Then look at what it did, and take it or leave it:

```sh
hmm diff parser     # see what changed
hmm merge parser    # merge the commits it made into your branch
hmm rm parser       # remove the workspace
```

If the command changed files without committing them, use `hmm apply`
rather than `hmm merge` to bring the changes into the working directory.

You can keep working in the meantime, and run more workspaces side by side.

## Workspaces

A workspace is a clone of the directory it was made from, its *origin*,
taken when `hmm run` starts. It includes everything in the directory: the
repository, uncommitted changes, and build output, so builds start warm. On
a filesystem that supports copy-on-write, making a workspace is fast and
takes no space until the command writes to it.

Every workspace has an ID of six letters and digits chosen at random, such
as `k3xq9a`, and a name if it was given one with `hmm run -n`. Wherever a
command takes a workspace, you can give:

- its name, if it is a workspace of the working directory,
- its ID, or
- the start of its ID, if no other workspace's ID starts the same way:
  `hmm rm k3` removes `k3xq9a` if it is the only ID starting with `k3`.

`hmm diff`, `hmm merge`, and `hmm apply` use the working directory's latest
workspace when none is given.

A workspace stays until you remove it with `hmm rm`, or `hmm run --rm`
removes it when the command exits.

## Commands

### `hmm`

```
hmm
```

With no command, `hmm` lists the workspaces of the working directory, as
[`hmm ls`](#hmm-ls) does.

`hmm --help` prints a summary of the commands, and `hmm <command> --help`
the options of a command.

### `hmm run`

```
hmm run [--rm] [-n name] [-w path]... [command [argument...]]
```

Makes a new workspace of the working directory and runs `command` in it,
confined to it (see [What the command can do](#what-the-command-can-do)).
With no command, runs your shell (`$SHELL`, or `/bin/sh` if it is not set).

Everything from `command` on belongs to the command, its own options
included, so `hmm run claude --resume` passes `--resume` to `claude`.

`hmm` prints where the workspace is when the command starts and again when
it exits, and exits with the command's exit status. Signals from the
terminal, such as Ctrl-C, go to the command; `hmm` waits for it to exit.

**Options**

| Option | Description |
| --- | --- |
| `-n`, `--name name` | Name the workspace, so it can be referred to by name. A name is letters, digits, `-`, `_`, and `.`, does not start with `-`, and is unique among the workspaces of the working directory. |
| `-w`, `--write path` | Let the command write to `path` too, and everything under it. Can be repeated. `path` must exist. |
| `--rm` | Remove the workspace when the command exits, as `hmm rm` would. |

**Coding agents**

Commands that keep state in the home directory need `-w` to write it. For
Claude Code, that is `~/.claude`:

```sh
hmm run -w ~/.claude claude
```

`hmm` also tells Claude Code it runs in a sandbox, so it does not ask
whether to trust each new workspace.

**Examples**

Open a shell in a throwaway workspace, removed when you exit it:

```sh
hmm run --rm
```

Try a build in a workspace, without touching the working directory:

```sh
hmm run --rm cargo build --release
```

### `hmm ls`

```
hmm ls [-a]
```

Lists the workspaces of the working directory, oldest first:

```console
$ hmm ls
ID      NAME    STATE    AGE  ORIGIN
k3xq9a  parser  done      2h  ~/src/dwim
p7mw2d  -       running   5m  ~/src/dwim
```

| Column | Description |
| --- | --- |
| `ID` | The workspace's ID |
| `NAME` | The workspace's name, or `-` if it has none |
| `STATE` | `running` if a command is running in it, `done` otherwise |
| `AGE` | How long ago it was made |
| `ORIGIN` | The directory it is a workspace of |

Nothing is printed if there are no workspaces.

**Options**

| Option | Description |
| --- | --- |
| `-a`, `--all` | List the workspaces of every directory, not just the working directory's. |

### `hmm diff`

```
hmm diff [--stat] [workspace]
```

Shows what changed in `workspace` since it was made, as a patch, committed
or not. With no workspace, uses the working directory's latest.

If the origin is a git repository, what git ignores, such as build output,
is left out. The output is git's, so it is paged and colored as your git is
configured to.

**Options**

| Option | Description |
| --- | --- |
| `--stat` | Show only which files changed, and how much. |

### `hmm merge`

```
hmm merge [workspace]
```

Merges the commits made in `workspace` into the branch its origin is on,
as `git merge` does:

- a fast-forward if the branch has not moved since the workspace was made,
- a merge commit, `Merge workspace <id> (<name>)`, if it has,
- conflicts to resolve and commit as for any merge, if both changed the
  same lines.

With no workspace, uses the working directory's latest.

Only commits are merged. If the command left changes it did not commit,
`hmm merge` says so and leaves them in the workspace; use
[`hmm apply`](#hmm-apply) for those.

The workspace must have been made from the top of a git repository, and no
command may be running in it. The merged commits stay in the repository as
`refs/hmm/<id>` until the workspace is removed.

`hmm merge` never runs git in the workspace's repository outside the
sandbox, as the command could have changed its configuration: the commits
are packed into a bundle inside the sandbox, and fetched from it with every
object checked.

### `hmm apply`

```
hmm apply [--check] [workspace]
```

Makes the changes in `workspace`, the ones `hmm diff` shows, committed or
not, in its origin. Only files change: nothing is staged or committed. With
no workspace, uses the working directory's latest.

It applies all of the changes or, if any does not apply because the origin
has changed in the same places since, none, and shows why.

A workspace that changed nothing, or is already applied, is left alone, and
so is one in which a command is running.

Works whether or not the origin is a git repository.

**Options**

| Option | Description |
| --- | --- |
| `--check` | Only say whether the changes apply, changing nothing. |

### `hmm rm`

```
hmm rm workspace...
```

Removes the workspaces given, everything in them, and the `refs/hmm/<id>`
references that `hmm merge` made to their commits. At least one workspace
must be given.

A workspace in which a command is running is not removed. If a workspace
cannot be found or removed, `hmm rm` says so, goes on to the rest, and exits
with 1.

## What the command can do

The command may write only to:

- the workspace,
- the temporary directories (`/tmp` and `$TMPDIR`),
- devices,
- build and package manager caches (`~/.cargo`, `~/.rustup`, `~/.cache`,
  `~/.npm`, `~/Library/Caches`),
- the paths given with `hmm run -w`.

It cannot read `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`, or other
workspaces.

Everything else, including the network, is allowed. See
[README.md](README.md#sandbox) for how this is enforced, and for the
differences between macOS and Linux.

## Files

Workspaces are kept in `~/.local/share/hmm/` on Linux and
`~/Library/Application Support/hmm/` on macOS, one directory each, named by
ID. In each:

| File | Description |
| --- | --- |
| `tree/<name>/` | The clone the command runs in, named as its origin. |
| `origin` | The path of the origin. A running command holds a lock on it. |
| `name` | The workspace's name, if it has one. |
| `head`, `fork` | If the origin is the top of a git repository: the commit it was at when the workspace was made, and the tree of its files then. |
| `base/` | Otherwise: a clone of the origin as it was when the workspace was made. |
| `objects/`, `*.index`, `git/` | What `hmm` keeps to compare the workspace with, out of the origin's repository. |

## Exit status

`hmm run` exits with the command's exit status, or 128 plus the number of
the signal that ended it.

The other commands exit with 0 if every workspace given was found and dealt
with, and 1 otherwise.

## Caveats

- The command runs in the workspace's path rather than the working
  directory's, so tools that remember absolute paths, such as build caches,
  may rebuild.
- The network is not confined: a command can send anything it can read to
  anywhere, and can push to remotes whose credentials it can reach, such as
  through the macOS keychain or an SSH agent.
- Commits made in a workspace are not signed if signing needs `~/.gnupg` or
  `~/.ssh`, which the command cannot read; the merge commit `hmm merge`
  makes is.
- A workspace of a linked worktree cannot be committed to, as its
  repository lies outside the workspace.
- On Linux, the command cannot run set-user-ID programs such as `sudo`.
