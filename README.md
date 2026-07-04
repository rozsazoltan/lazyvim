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
  - [Managed tools](#managed-tools)
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

The launcher prepares the portable directory on first run, checks required system tools, installs Neovim into the portable home if no usable `nvim` is available, installs the managed tools LazyVim needs for the default setup, clones the official [LazyVim starter](https://github.com/LazyVim/starter) config if it does not exist yet, removes its `.git` directory, then starts Neovim with dedicated XDG paths:

```text
~/.lazyvim/nvim             # launcher-managed Neovim installation
~/.lazyvim/config/lazyvim   # LazyVim config
~/.lazyvim/data/lazyvim     # plugins, lazy.nvim, Mason packages
~/.lazyvim/state/lazyvim    # Neovim state
~/.lazyvim/cache/lazyvim    # cache
~/.lazyvim/bin              # launcher-managed CLI tools and native compiler wrappers
~/.lazyvim/tools/zig        # launcher-managed Zig compiler
```

That means you can use `lazyvim` without touching `~/.config/nvim` or your existing Neovim profile.

## Get started

### Install

The recommended install method is [`bin`](https://github.com/marcosnils/bin), because this project publishes direct executable release assets. `bin` selects the asset that matches your platform and installs it into its binary directory.

```sh
bin install github.com/rozsazoltan/lazyvim
```

If `bin` cannot confidently select the right asset, show all matching release assets and choose one manually:

```sh
bin install -a github.com/rozsazoltan/lazyvim
```

For a prerelease or a specific version, install the release tag URL:

```sh
bin install github.com/rozsazoltan/lazyvim/releases/tag/v0.1.0
```

Manual installation works too. Download the executable for your platform from the latest GitHub Release and place it somewhere in your `PATH`.

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

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$bin*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$bin", "User")
}
$env:Path = "$env:Path;$bin"

lazyvim --version
```

The last `$env:Path` line makes `lazyvim` available in the current terminal session. New terminals will pick it up from the user `PATH`.

> [!IMPORTANT]
> The release asset is the `lazyvim` launcher executable. On first run, it prepares the missing dependencies it can manage automatically. It downloads the official Neovim release, installs Zig, adds native `cc`/`gcc`/`clang` wrappers backed by `zig cc`, installs `tree-sitter`, `rg`, `fd`, and `lazygit`, then fetches the official LazyVim starter config. If Git or curl are missing, the launcher tries to install them with the native package manager first.

Release checksums are published as `SHA256SUMS` next to the executables. The Linux x86_64 executable is built with the musl target to avoid depending on the glibc version installed by a specific distribution.

### First run

Open any project directory and run:

```sh
lazyvim .
```

The first run creates the selected portable home, installs missing system dependencies when possible, installs Neovim if needed, installs the managed portable tools, fetches the official LazyVim starter config, and lets LazyVim/lazy.nvim install plugins there.

### Upgrade

Install a newer release by replacing the `lazyvim` executable. Your LazyVim config, plugins, state, and cache stay in the selected portable home unless you remove or change that directory.

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
lazyvim install-nvim   # install Neovim into the selected portable home
lazyvim install-tools  # install managed portable tools into the selected portable home
lazyvim install-deps   # install system and managed dependencies
```

These commands run lazy.nvim in headless mode and use the same portable home as normal editor sessions.

### Portable home

The default portable home is:

```text
~/.lazyvim
```

The launcher remembers the last selected home. After you run one of these once, later `lazyvim` commands without a home flag use the remembered location:

```sh
lazyvim --user-home .          # remember ~/.lazyvim
lazyvim --portable-home .      # remember <launcher-directory>/.lazyvim
lazyvim --home ~/.work-lazyvim .
```

Windows PowerShell:

```powershell
lazyvim --home D:\Tools\lazyvim-home .
```

The aliases `portable`, `self`, `exe`, and `launcher` all resolve to:

```text
<launcher-directory>/.lazyvim
```

You can also select a home with `LAZYVIM_HOME`:

```sh
LAZYVIM_HOME=portable lazyvim .
LAZYVIM_HOME=~/.work-lazyvim lazyvim .
LAZYVIM_HOME=user lazyvim .
```

When you select a different home than the remembered one, the launcher asks what to do:

```text
m  move the current home to the new path
n  start a new home at the new path and keep the old one
d  delete the old home and start at the new path
k  keep using the remembered home
a  abort
```

For non-interactive scripts or CI, use an explicit action flag:

```sh
lazyvim --portable-home --move-home .
lazyvim --home ~/.work-lazyvim --new-home .
lazyvim --user-home --delete-old-home .
lazyvim --home ~/.work-lazyvim --keep-home .
```

On Windows, moving between drives uses a copy-then-swap fallback so the target home is only created after the copy succeeds. If an older failed migration left a partial target directory behind, remove that partial directory first and run the command again.

Print the resolved locations and the remembered home:

```sh
lazyvim where
lazyvim --portable-home where
```

Reset the selected portable home:

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
4. `$LAZYVIM_HOME/nvim/bin/nvim`
5. `$LAZYVIM_HOME/bin/nvim`
6. `nvim` from `PATH`

If none of these works during a normal launch, the launcher downloads the official Neovim release for the current platform into the selected portable home and then starts it from there. You can also install it explicitly:

```sh
lazyvim install-nvim
```

This keeps the release itself as a single executable while still giving users a working portable Neovim runtime when the system does not provide one.


### Managed tools

LazyVim needs external tooling during the first plugin install. The launcher keeps the portable pieces under the selected home:

```text
$LAZYVIM_HOME/tools/zig        # portable Zig compiler
$LAZYVIM_HOME/bin/cc           # native launcher wrapper around zig cc
$LAZYVIM_HOME/bin/gcc          # native launcher wrapper around zig cc
$LAZYVIM_HOME/bin/clang        # native launcher wrapper around zig cc
$LAZYVIM_HOME/bin/c++          # native launcher wrapper around zig c++
$LAZYVIM_HOME/bin/tree-sitter  # tree-sitter CLI
$LAZYVIM_HOME/bin/rg           # ripgrep
$LAZYVIM_HOME/bin/fd           # fd
$LAZYVIM_HOME/bin/lazygit      # lazygit
```

System tools that LazyVim/lazy.nvim still expect, such as Git and curl, are checked on first run. When they are missing, the launcher attempts to install them with the platform package manager:

```text
Windows: winget
macOS:   Homebrew when available, otherwise Xcode Command Line Tools prompt
Linux:   apk, apt, pacman, dnf, or yum depending on the distro
```

The Linux detection covers Alpine, Debian/Ubuntu-like, Arch-like, Fedora, Amazon Linux, RHEL, CentOS, Rocky, AlmaLinux, Oracle Linux, and ID_LIKE-based RHEL/Fedora variants.

Install or repair everything explicitly:

```sh
lazyvim install-deps
```

Install or repair only the managed portable tools:

```sh
lazyvim install-tools
```

The managed tool directories are prepended to `PATH` when Neovim is started, so LazyVim sees them before broken or incompatible system tools. The launcher also sets `CC`, `CXX`, and `TREE_SITTER_CLI` for Neovim so nvim-treesitter builds parsers with the portable toolchain instead of falling back to a missing or incompatible system compiler.

When the portable compiler wrapper changes, the launcher clears previously compiled Treesitter parser output once so broken `.so`/`.dll` files are rebuilt with the managed compiler.

### Environment variables

| Variable | Description |
|---|---|
| `LAZYVIM_HOME` | Selects the portable home. Use `portable`, `self`, `exe`, or `launcher` for executable-local storage, or `user` for `~/.lazyvim`. |
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
| `CC` | `$LAZYVIM_HOME/bin/cc` |
| `CXX` | `$LAZYVIM_HOME/bin/c++` |
| `TREE_SITTER_CLI` | `$LAZYVIM_HOME/bin/tree-sitter` |

## Troubleshooting

If Linux prints a glibc error such as `GLIBC_2.xx not found`, install a newer LazyVim release. Linux builds are published from the `x86_64-unknown-linux-musl` Rust target, so the launcher should not require the glibc version from the GitHub Actions runner.

If Windows prints `lazyvim: program not found`, the executable is not in your `PATH` under the name `lazyvim.exe`. Download `lazyvim-windows-x86_64.exe` as `lazyvim.exe`, place it in a directory included in `PATH`, and open a new terminal.

If you installed with `bin` and the command is still not found, make sure `bin`'s binary directory is part of your `PATH`. By default, `bin` uses `~/.local/bin` on Linux/macOS and `%LOCALAPPDATA%\bin` on Windows.

Run the built-in doctor command first:

```sh
lazyvim doctor
```

The first run needs Git for lazy.nvim plugin installation and curl for parts of the LazyVim stack. The launcher checks them and attempts native installation when they are missing. LazyVim extras may still need project-specific tools such as language runtimes, package managers, formatters, linters, and LSP servers.

If nvim-treesitter reports `program not found`, `gcc program not found`, or a parser dynamic library error, install or repair the managed tools. The launcher will rebuild the parser output on the next run when the portable toolchain stamp changes.

If Neovim or managed dependencies cannot be found, run the built-in installers or point the launcher to a binary:

```sh
lazyvim install-deps
lazyvim install-nvim
lazyvim install-tools
LAZYVIM_NVIM=/path/to/nvim lazyvim .
```

The automatic installers download the official Neovim, Zig, ripgrep, fd, and lazygit release assets for the current platform, then extract them into the selected portable home. Linux release builds embed a launcher-tested tree-sitter CLI so older glibc-based distributions and musl-based distributions do not depend on the upstream tree-sitter Linux binary. Other platforms download the official tree-sitter CLI asset.

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

Changes that affect bootstrap, platform support, release assets, or managed dependencies should also pass the `Portable Smoke` workflow. It builds the executable on Linux, Windows, macOS Intel, and macOS Apple Silicon, runs `install-deps`, checks `doctor`, and optionally runs `sync` to catch LazyVim/Treesitter bootstrap failures before release.

The release workflow runs the same smoke checks against the built release artifacts before creating the GitHub Release.

Keep changes small and focused. User-facing behavior should stay portable by default and must not write into the user's normal Neovim config directories.

## License & Acknowledgments

This project would not exist without [Neovim](https://github.com/neovim/neovim), [LazyVim](https://github.com/LazyVim/LazyVim), the [LazyVim starter](https://github.com/LazyVim/starter), [lazy.nvim](https://github.com/folke/lazy.nvim), and their creators and contributors.

It is open source and released under the [GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later)](https://www.gnu.org/licenses/agpl-3.0.html).

Copyright (C) 2020–present [Zoltán Rózsa](https://github.com/rozsazoltan)
