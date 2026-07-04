# lazyvim

Portable LazyVim launcher for Linux, Windows, and macOS.

This repository does not fork LazyVim or Neovim. It provides a small Rust launcher that starts Neovim with an isolated LazyVim profile. The launcher keeps config, plugins, Mason packages, state, and cache under a dedicated portable home directory instead of touching the user's normal Neovim setup.

Default portable home:

```text
~/.lazyvim
```

The launcher sets these environment variables before starting Neovim:

```text
NVIM_APPNAME=lazyvim
XDG_CONFIG_HOME=<home>/config
XDG_DATA_HOME=<home>/data
XDG_STATE_HOME=<home>/state
XDG_CACHE_HOME=<home>/cache
PATH=<home>/bin:<package>/nvim/bin:<package>/bin:$PATH
```

That means LazyVim is resolved from:

```text
~/.lazyvim/config/lazyvim/init.lua
```

and plugins are installed under:

```text
~/.lazyvim/data/lazyvim/lazy
```

## Usage

```bash
lazyvim
lazyvim .
lazyvim src/main.rs
lazyvim --home ~/.my-lazyvim
lazyvim where
lazyvim doctor
lazyvim sync
lazyvim restore
lazyvim update
lazyvim clean
```

The current working directory is inherited, so running `lazyvim` from a project opens that project in the same way as terminal tools like `lazygit` or `lazydocker`.

## Portable packages

Release packages are intended to contain:

```text
lazyvim                 # or lazyvim.exe
nvim/                   # optional bundled Neovim runtime
README-PORTABLE.md
```

The launcher resolves Neovim in this order:

1. `LAZYVIM_NVIM`
2. `<package>/nvim/bin/nvim` or `<package>/nvim/bin/nvim.exe`
3. `<package>/bin/nvim` or `<package>/bin/nvim.exe`
4. `~/.lazyvim/bin/nvim` or `~/.lazyvim/bin/nvim.exe`
5. `nvim` from `PATH`

## Environment variables

| Variable | Description |
| --- | --- |
| `LAZYVIM_HOME` | Overrides the default portable home directory. |
| `LAZYVIM_NVIM` | Points to a specific Neovim executable. |
| `NVIM_APPNAME` | Set by the launcher to `lazyvim` unless already overwritten in code. |

## Release workflow

The manual release workflow accepts a version input and a target branch input. It updates repository version files on `chore/release-{version}`, creates a pull request, squash-merges it into the selected target branch when there are changes, builds platform executables, and creates the GitHub Release only after all build jobs succeed.

Required repository settings:

- GitHub Actions workflow permissions: read/write
- Squash merge enabled
- `GITHUB_TOKEN` must be allowed to create pull requests and write contents

Run it from GitHub Actions:

```text
Actions -> Release -> Run workflow
```

Inputs:

| Input | Example | Description |
| --- | --- | --- |
| `version` | `0.1.1` | SemVer version without leading `v`. |
| `target_branch` | `master` | Branch to release from and squash-merge version updates into. |
| `neovim_version` | `stable` | Neovim release tag to bundle, for example `stable`, `nightly`, or `v0.12.2`. |
| `bundle_neovim` | `true` | Whether to download and include Neovim in the release package. |
| `draft` | `false` | Whether to create the GitHub Release as a draft. |
| `prerelease` | `false` | Whether to mark the GitHub Release as a prerelease. |

## Local development

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --version
cargo run -- where
```

## Notes

This launcher isolates Neovim paths, but LazyVim plugins may still require external developer tools such as Git, curl, ripgrep, fd, compilers, language runtimes, or package managers depending on the enabled extras and project type.
