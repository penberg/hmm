# hmm

`hmm` runs a command in a draft: a copy-on-write clone of the working
directory that the command may change as it likes, while the working
directory and the rest of the system stay as they are. It is meant for
running coding agents, and anything else you would rather not trust with
your files.

Making a draft is quick and takes no space until the command writes to it.
The draft includes everything in the directory (the repository, uncommitted
changes, and build output), so builds start warm.

## Install

```sh
cargo install --path .
```

`hmm` runs only on macOS for now, and needs the working directory to be on
an APFS volume, the same one as the home directory.

## Usage

Run Claude Code in a draft named `parser`, letting it save its sessions and
settings in `~/.claude`:

```sh
hmm run -n parser -w ~/.claude claude
```

List the drafts of the working directory:

```console
$ hmm
ID      NAME    STATE    AGE  ORIGIN
k3xq9a  parser  done      2h  ~/src/penberg/dwim
p7mw2d  -       running   5m  ~/src/penberg/dwim
```

See what changed in a draft, and remove it:

```sh
hmm diff parser
hmm rm parser
```

| Command | Description |
| --- | --- |
| `hmm run [-n name] [-w path]... [command...]` | Run a command (or your shell) in a new draft |
| `hmm ls [-a]` | List drafts of the working directory, or of every directory with `-a` |
| `hmm diff [--stat] [draft]` | Show what changed in a draft (the latest one by default) |
| `hmm rm draft...` | Remove drafts |

A draft can be referred to by its name, its ID, or a unique prefix of its
ID. See [`man/hmm.1`](man/hmm.1) (`man ./man/hmm.1`) for the details.

## Sandbox

The command runs under the macOS sandbox. It may write only to the draft,
temporary directories, build and package manager caches (`~/.cargo`,
`~/.rustup`, `~/.cache`, `~/.npm`, `~/Library/Caches`), and paths given
with `-w`. It cannot read `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`,
or other drafts.

The network is not confined: a command can send anything it can read to
anywhere.

## License

MIT
