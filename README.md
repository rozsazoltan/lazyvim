# rozsazoltan/lazyvim

Portable LazyVim launcher for the truly lazy.

`rozsazoltan/lazyvim` gives you a portable LazyVim entry point that can be installed as a single executable and started from any project directory. It keeps the LazyVim configuration, plugins, Mason packages, state, and cache in one dedicated home directory instead of using your normal Neovim setup.

LazyVim made even lazier: install one executable and start creating.

- [How it works](#how-it-works)
- [Get started](#get-started)
  - [Install](#install)
  - [First run](#first-run)
  - [Upgrade](#upgrade)
- [Usage](#usage)
  - [Open projects and files](#open-projects-and-files)
  - [Lazy commands](#lazy-commands)
  - [Portable home](#portable-home)
  - [Neovim resolution](#neovim-resolution)
  - [Environment variables](#environment-variables)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)

Read on to learn how the launcher isolates LazyVim, or jump straight to [Get started](#get-started) if you only want the install commands.

## How it works

This project is not a fork of LazyVim or Neovim. It is a small Rust launcher that starts Neovim with an isolated LazyVim profile.

By default, everything is stored under:

```text
~/.lazyvim
```

The launcher prepares the portable directory on first run, clones the official [LazyVim starter](https://github.com/LazyVim/starter) config if it does not exist yet, removes its `.git` directory, then starts Neovim with dedicated XDG paths:

```text
~/.lazyvim/config/lazyvim   # LazyVim config
~/.lazyvim/data/lazyvim     # plugins, lazy.nvim, Mason packages
~/.lazyvim/state/lazyvim    # Neovim state
~/.lazyvim/cache/lazyvim    # cache
~/.lazyvim/bin              # optional local tools
```

That means you can use `lazyvim` without touching `~/.config/nvim` or your existing Neovim profile.

## Get started

### Install

Download the executable for your platform from the latest GitHub Release and place it somewhere in your `PATH`.

Linux x86_64:

```sh
mkdir -p ~/.local/bin
curl -fL https://github.com/rozsazoltan/lazyvim/releases/latest/download/lazyvim-linux-x86_64 -o ~/.local/bin/lazyvim
chmod +x ~/.local/bin/lazyvim
```

macOS Apple Silicon:

```sh
mkdir -p ~/.local/bin
curl -fL https://github.com/rozsazoltan/lazyvim/releases/latest/download/lazyvim-macos-arm64 -o ~/.local/bin/lazyvim
chmod +x ~/.local/bin/lazyvim
```

macOS Intel:

```sh
mkdir -p ~/.local/bin
curl -fL https://github.com/rozsazoltan/lazyvim/releases/latest/download/lazyvim-macos-x86_64 -o ~/.local/bin/lazyvim
chmod +x ~/.local/bin/lazyvim
```

Windows PowerShell:

```powershell
$bin = "$env:LOCALAPPDATA\Programs\lazyvim\bin"
New-Item -ItemType Directory -Force $bin | Out-Null
Invoke-WebRequest https://github.com/rozsazoltan/lazyvim/releases/latest/download/lazyvim-windows-x86_64.exe -OutFile "$bin\lazyvim.exe"
```

Then add `%LOCALAPPDATA%\Programs\lazyvim\bin` to your user `PATH` if it is not already there.

> [!IMPORTANT]
> The release asset is the `lazyvim` launcher executable. It manages the portable LazyVim home, but it still starts Neovim. Install Neovim normally, put `nvim` in `~/.lazyvim/bin`, or set `LAZYVIM_NVIM` if the launcher cannot find it. The first run also needs Git so the launcher can fetch the official LazyVim starter config.

Release checksums are published as `SHA256SUMS` next to the executables.

### First run

Open any project directory and run:

```sh
lazyvim .
```

The first run creates the portable home, fetches the official LazyVim starter config, and lets LazyVim/lazy.nvim install plugins into `~/.lazyvim`.

### Upgrade

Install a newer release by replacing the `lazyvim` executable. Your LazyVim config, plugins, state, and cache stay in `~/.lazyvim` unless you remove or change that directory.

To update LazyVim plugins after upgrading the launcher:

```sh
lazyvim update
```

To restore plugins from the lockfile:

```sh
lazyvim restore
```

## Usage

### Open projects and files

```sh
lazyvim
lazyvim .
lazyvim src/main.rs
lazyvim -- README.md
```

The current working directory is inherited, so `lazyvim` opens the directory you are already in, similar to terminal tools such as `lazygit` or `lazydocker`.

### Lazy commands

```sh
lazyvim sync      # install and sync plugins
lazyvim restore   # restore plugins from the lockfile
lazyvim update    # update plugins
lazyvim clean     # remove unused plugins
```

These commands run lazy.nvim in headless mode and use the same portable home as normal editor sessions.

### Portable home

The default portable home is:

```text
~/.lazyvim
```

Use a different location for one command:

```sh
lazyvim --home ~/.work-lazyvim .
```

Or persist it with an environment variable:

```sh
LAZYVIM_HOME=~/.work-lazyvim lazyvim .
```

Print the resolved locations:

```sh
lazyvim where
```

Reset the portable home:

```sh
lazyvim reset --yes
```

> [!WARNING]
> `reset --yes` deletes the selected portable home directory, including config, plugins, cache, state, Mason packages, and lock files stored there.

### Neovim resolution

The launcher looks for Neovim in this order:

1. `LAZYVIM_NVIM`
2. `nvim/bin/nvim` next to the launcher executable
3. `bin/nvim` next to the launcher executable
4. `~/.lazyvim/bin/nvim`
5. `nvim` from `PATH`

This keeps the release itself as a single executable while still allowing custom or manually bundled Neovim layouts.

### Environment variables

| Variable | Description |
|---|---|
| `LAZYVIM_HOME` | Overrides the default `~/.lazyvim` portable home. |
| `LAZYVIM_NVIM` | Uses a specific Neovim executable. |
| `LAZYVIM_STARTER_REPOSITORY` | Overrides the LazyVim starter repository used for first-run bootstrap. |

The launcher sets these variables automatically before starting Neovim:

| Variable | Value |
|---|---|
| `NVIM_APPNAME` | `lazyvim` |
| `XDG_CONFIG_HOME` | `$LAZYVIM_HOME/config` |
| `XDG_DATA_HOME` | `$LAZYVIM_HOME/data` |
| `XDG_STATE_HOME` | `$LAZYVIM_HOME/state` |
| `XDG_CACHE_HOME` | `$LAZYVIM_HOME/cache` |

## Troubleshooting

Run the built-in doctor command first:

```sh
lazyvim doctor
```

The first run needs Git to clone the LazyVim starter config. LazyVim plugins may also need external developer tools depending on the enabled extras and the project you open. Common examples are curl, ripgrep, fd, a C compiler, language runtimes, package managers, formatters, linters, and LSP servers.

If Neovim cannot be found, either install Neovim normally or point the launcher to a binary:

```sh
LAZYVIM_NVIM=/path/to/nvim lazyvim .
```

If you want a completely fresh LazyVim profile:

```sh
lazyvim reset --yes
lazyvim sync
```

## Contributing

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --version
cargo run -- where
```

Keep changes small and focused. User-facing behavior should stay portable by default and must not write into the user's normal Neovim config directories.

## License & Acknowledgments

This project would not exist without [Neovim](https://github.com/neovim/neovim), [LazyVim](https://github.com/LazyVim/LazyVim), the [LazyVim starter](https://github.com/LazyVim/starter), [lazy.nvim](https://github.com/folke/lazy.nvim), and their creators and contributors.

It is open source and released under the [GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later)](https://www.gnu.org/licenses/agpl-3.0.html).

Copyright (C) 2020–present [Zoltán Rózsa](https://github.com/rozsazoltan)
