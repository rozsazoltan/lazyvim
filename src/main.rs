use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};

const APP_NAME: &str = "lazyvim";
const DEFAULT_HOME_DIR: &str = ".lazyvim";

const STARTER_REPOSITORY: &str = "https://github.com/LazyVim/starter.git";
const ZIG_VERSION: &str = "0.14.0";
const TREE_SITTER_VERSION: &str = "0.26.10";
const RIPGREP_VERSION: &str = "15.1.0";
const FD_VERSION: &str = "10.4.2";
const LAZYGIT_VERSION: &str = "0.62.2";
const PORTABLE_TOOLCHAIN_STAMP: &str = "2026-07-04-portable-cc-v2";

#[derive(Debug)]
struct Cli {
    home: Option<PathBuf>,
    portable_home: bool,
    command: CliCommand,
}

#[derive(Debug)]
enum CliCommand {
    Launch(Vec<String>),
    Doctor,
    Where,
    Sync,
    Restore,
    Update,
    Clean,
    InstallNvim,
    InstallTools,
    InstallDeps,
    Reset { yes: bool },
    Help,
    Version,
}

#[derive(Debug)]
struct Runtime {
    home: PathBuf,
    config_home: PathBuf,
    data_home: PathBuf,
    state_home: PathBuf,
    cache_home: PathBuf,
    config_dir: PathBuf,
    exe_dir: Option<PathBuf>,
    nvim: PathBuf,
    path_value: OsString,
}

fn main() {
    if is_compiler_wrapper_invocation() {
        match run_compiler_wrapper() {
            Ok(code) => exit(code),
            Err(error) => {
                eprintln!("lazyvim compiler wrapper: {error}");
                exit(1);
            }
        }
    }

    if let Err(error) = run() {
        eprintln!("lazyvim: {error}");
        exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = parse_cli(env::args().skip(1).collect());

    match cli.command {
        CliCommand::Help => {
            print_help();
            Ok(())
        }
        CliCommand::Version => {
            println!("lazyvim {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliCommand::Reset { yes } => {
            let runtime = prepare_runtime(cli.home, cli.portable_home, false, false)?;
            if !yes {
                println!("This will delete: {}", runtime.home.display());
                println!("Run `lazyvim reset --yes` to confirm.");
                return Ok(());
            }
            if runtime.home.exists() {
                fs::remove_dir_all(&runtime.home)?;
            }
            println!("Removed {}", runtime.home.display());
            Ok(())
        }
        command => {
            let bootstrap = !matches!(command, CliCommand::Doctor | CliCommand::Where);
            let runtime = prepare_runtime(cli.home, cli.portable_home, bootstrap, true)?;
            match command {
                CliCommand::Launch(args) => launch_nvim(&runtime, &args),
                CliCommand::Doctor => doctor(&runtime),
                CliCommand::Where => print_locations(&runtime),
                CliCommand::Sync => run_lazy_command(&runtime, "sync"),
                CliCommand::Restore => run_lazy_command(&runtime, "restore"),
                CliCommand::Update => run_lazy_command(&runtime, "update"),
                CliCommand::Clean => run_lazy_command(&runtime, "clean"),
                CliCommand::InstallNvim => install_neovim_command(&runtime),
                CliCommand::InstallTools => install_tools_command(&runtime),
                CliCommand::InstallDeps => install_deps_command(&runtime),
                CliCommand::Help | CliCommand::Version | CliCommand::Reset { .. } => unreachable!(),
            }
        }
    }
}

fn parse_cli(mut args: Vec<String>) -> Cli {
    let mut home = None;
    let mut portable_home = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if arg == "--home" {
            if index + 1 < args.len() {
                home = Some(PathBuf::from(args.remove(index + 1)));
                args.remove(index);
                continue;
            }
            break;
        }

        if let Some(value) = arg.strip_prefix("--home=") {
            home = Some(PathBuf::from(value));
            args.remove(index);
            continue;
        }

        if arg == "--portable" || arg == "--portable-home" {
            portable_home = true;
            args.remove(index);
            continue;
        }

        index += 1;
    }

    let command = match args.first().map(String::as_str) {
        Some("doctor") => CliCommand::Doctor,
        Some("where") => CliCommand::Where,
        Some("sync") => CliCommand::Sync,
        Some("restore") => CliCommand::Restore,
        Some("update") => CliCommand::Update,
        Some("clean") => CliCommand::Clean,
        Some("install-nvim") => CliCommand::InstallNvim,
        Some("install-tools") => CliCommand::InstallTools,
        Some("install-deps") => CliCommand::InstallDeps,
        Some("reset") => CliCommand::Reset {
            yes: args.iter().any(|arg| arg == "--yes" || arg == "-y"),
        },
        Some("help") | Some("--help") | Some("-h") => CliCommand::Help,
        Some("--version") | Some("-V") => CliCommand::Version,
        _ => CliCommand::Launch(args),
    };

    Cli {
        home,
        portable_home,
        command,
    }
}

fn prepare_runtime(
    home_override: Option<PathBuf>,
    portable_home: bool,
    bootstrap: bool,
    migrate_custom_home: bool,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    let uses_custom_home = portable_home || home_override.is_some() || env::var_os("LAZYVIM_HOME").is_some();
    let home = resolve_home(home_override, portable_home, exe_dir.as_deref())?;

    if migrate_custom_home && uses_custom_home {
        migrate_default_home_if_needed(&home)?;
    }

    let config_home = home.join("config");
    let data_home = home.join("data");
    let state_home = home.join("state");
    let cache_home = home.join("cache");
    let config_dir = config_home.join(APP_NAME);

    fs::create_dir_all(home.join("bin"))?;
    fs::create_dir_all(&config_home)?;
    fs::create_dir_all(&data_home)?;
    fs::create_dir_all(&state_home)?;
    fs::create_dir_all(&cache_home)?;

    let path_value = build_path(&home, exe_dir.as_deref())?;
    let mut nvim = resolve_nvim(&home, exe_dir.as_deref());

    if bootstrap {
        ensure_system_dependencies(&path_value)?;

        if !command_runs(&nvim, &["--version"], Some(&path_value)) {
            install_neovim(&home)?;
            nvim = resolve_nvim(&home, exe_dir.as_deref());
        }

        if !command_runs(&nvim, &["--version"], Some(&path_value)) {
            return Err(format!(
                "Neovim is not available. Install it manually, set LAZYVIM_NVIM, or run `lazyvim install-nvim`. Tried: {}",
                nvim.display()
            )
            .into());
        }

        ensure_managed_tools(&home, &path_value)?;
        ensure_treesitter_cache_for_current_toolchain(&data_home, &state_home)?;
        ensure_starter_config(&config_dir)?;
        ensure_portable_lazyvim_config(&config_dir)?;
    }

    Ok(Runtime {
        home,
        config_home,
        data_home,
        state_home,
        cache_home,
        config_dir,
        exe_dir,
        nvim,
        path_value,
    })
}

fn resolve_home(
    home_override: Option<PathBuf>,
    portable_home: bool,
    exe_dir: Option<&Path>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if portable_home {
        return home_next_to_executable(exe_dir);
    }

    if let Some(path) = home_override {
        return expand_home(path, exe_dir);
    }

    if let Some(value) = env::var_os("LAZYVIM_HOME") {
        return expand_home(PathBuf::from(value), exe_dir);
    }

    Ok(user_home_dir()?.join(DEFAULT_HOME_DIR))
}

fn expand_home(path: PathBuf, exe_dir: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let text = path.to_string_lossy();

    if is_portable_home_alias(&text) {
        return home_next_to_executable(exe_dir);
    }

    let expanded = if text == "~" {
        user_home_dir()?
    } else if let Some(rest) = text.strip_prefix("~/") {
        user_home_dir()?.join(rest)
    } else if cfg!(windows) {
        if let Some(rest) = text.strip_prefix("~\\") {
            user_home_dir()?.join(rest)
        } else {
            PathBuf::from(text.as_ref())
        }
    } else {
        PathBuf::from(text.as_ref())
    };

    absolute_path(expanded)
}

fn is_portable_home_alias(value: &str) -> bool {
    matches!(value, "portable" | "self" | "exe" | "launcher")
}

fn home_next_to_executable(exe_dir: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let exe_dir = exe_dir.ok_or("could not resolve launcher executable directory")?;
    Ok(exe_dir.join(DEFAULT_HOME_DIR))
}

fn migrate_default_home_if_needed(destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = user_home_dir()?.join(DEFAULT_HOME_DIR);
    let source = absolute_path(source)?;
    let destination = absolute_path(destination.to_path_buf())?;

    if source == destination || !source.exists() || destination.exists() {
        return Ok(());
    }

    if destination.starts_with(&source) {
        return Err(format!(
            "cannot move {} into itself at {}",
            source.display(),
            destination.display()
        )
        .into());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    println!(
        "Moving portable home from {} to {}",
        source.display(),
        destination.display()
    );

    move_directory(&source, &destination)?;
    Ok(())
}

fn move_directory(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            copy_directory(source, destination).map_err(|copy_error| {
                format!(
                    "failed to move {} to {}: rename failed with {}; copy fallback failed with {}",
                    source.display(),
                    destination.display(),
                    rename_error,
                    copy_error
                )
            })?;
            fs::remove_dir_all(source)?;
            Ok(())
        }
    }
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            copy_symlink(&source_path, &destination_path)?;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let target = fs::read_link(source)?;
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    symlink(target, destination)
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let target = fs::read_link(source)?;
    if destination.exists() {
        if destination.is_dir() {
            fs::remove_dir(destination)?;
        } else {
            fs::remove_file(destination)?;
        }
    }

    if source.is_dir() {
        symlink_dir(target, destination)
    } else {
        symlink_file(target, destination)
    }
}

fn absolute_path(path: PathBuf) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn user_home_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home));
    }

    if let Some(profile) = env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(profile));
    }

    let drive = env::var_os("HOMEDRIVE");
    let path = env::var_os("HOMEPATH");

    match (drive, path) {
        (Some(drive), Some(path)) => {
            let mut combined = drive;
            combined.push(path);
            Ok(PathBuf::from(combined))
        }
        _ => Err("could not resolve user home directory".into()),
    }
}


fn ensure_system_dependencies(path_value: &OsString) -> Result<(), Box<dyn std::error::Error>> {
    let mut missing = Vec::new();

    for command in required_system_commands() {
        if !command_available(command, path_value) {
            missing.push(command.to_string());
        }
    }

    if missing.is_empty() {
        return Ok(());
    }

    println!("Missing system dependencies: {}", missing.join(", "));
    install_system_dependencies(&missing)?;

    let still_missing: Vec<_> = required_system_commands()
        .iter()
        .filter(|command| !command_available(command, path_value))
        .map(|command| command.to_string())
        .collect();

    if still_missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "missing required system dependencies after install attempt: {}",
            still_missing.join(", ")
        )
        .into())
    }
}

fn required_system_commands() -> &'static [&'static str] {
    if cfg!(windows) {
        &["git", "powershell"]
    } else {
        &["git", "curl", "tar", "unzip"]
    }
}

fn command_available(command_name: &str, path_value: &OsString) -> bool {
    let mut command = Command::new(command_name);
    command
        .args(command_probe_args(command_name))
        .env("PATH", path_value)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.status().is_ok()
}

fn install_system_dependencies(missing: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(windows) {
        return install_windows_system_dependencies(missing);
    }

    if cfg!(target_os = "macos") {
        return install_macos_system_dependencies(missing);
    }

    if cfg!(target_os = "linux") {
        return install_linux_system_dependencies();
    }

    Err(format!("automatic dependency installation is not supported on this platform; missing: {}", missing.join(", ")).into())
}

fn install_windows_system_dependencies(missing: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if !missing.iter().any(|name| name == "git") {
        return Ok(());
    }

    if !command_available_without_path("winget") {
        return Err("Git is required, but it was not found and winget is not available. Install Git for Windows and run lazyvim again.".into());
    }

    println!("Installing Git for Windows with winget");
    let status = Command::new("winget")
        .args([
            "install",
            "--id",
            "Git.Git",
            "--exact",
            "--source",
            "winget",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ])
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("winget failed to install Git for Windows: {status}").into())
    }
}

fn install_macos_system_dependencies(missing: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if command_available_without_path("brew") {
        let mut packages = Vec::new();
        if missing.iter().any(|name| name == "git") {
            packages.push("git");
        }
        if missing.iter().any(|name| name == "curl") {
            packages.push("curl");
        }
        if packages.is_empty() {
            return Ok(());
        }

        let status = Command::new("brew").arg("install").args(packages).status()?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("brew failed to install missing dependencies: {status}").into());
    }

    if missing.iter().any(|name| name == "git") {
        let _ = Command::new("xcode-select").arg("--install").status();
    }

    Err(format!(
        "missing macOS dependencies: {}. Install Xcode Command Line Tools or Homebrew, then run lazyvim again.",
        missing.join(", ")
    )
    .into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxDistro {
    Alpine,
    Debian,
    Arch,
    Fedora,
    AmazonLinux,
    Rhel,
    Unknown,
}

fn install_linux_system_dependencies() -> Result<(), Box<dyn std::error::Error>> {
    let distro = detect_linux_distro();
    let command = match distro {
        LinuxDistro::Alpine => "apk add --no-cache git curl ca-certificates tar unzip xz tree-sitter-cli",
        LinuxDistro::Debian => "apt-get update && apt-get install -y git curl ca-certificates tar unzip xz-utils",
        LinuxDistro::Arch => "pacman -Sy --noconfirm --needed git curl ca-certificates tar unzip xz tree-sitter-cli",
        LinuxDistro::Fedora => "dnf install -y git curl ca-certificates tar unzip xz tree-sitter-cli",
        LinuxDistro::AmazonLinux | LinuxDistro::Rhel => {
            if command_available_without_path("dnf") {
                "dnf install -y git curl ca-certificates tar unzip xz tree-sitter-cli"
            } else {
                "yum install -y git curl ca-certificates tar unzip xz"
            }
        }
        LinuxDistro::Unknown => {
            return Err("could not detect a supported Linux distribution for dependency installation".into());
        }
    };

    run_privileged_shell(command)
}

fn install_tree_sitter_from_system_package(path_value: &OsString) -> Result<(), Box<dyn std::error::Error>> {
    if command_available("tree-sitter", path_value) {
        return Ok(());
    }

    match detect_linux_distro() {
        LinuxDistro::Alpine => run_privileged_shell("apk add --no-cache tree-sitter-cli"),
        LinuxDistro::Arch => run_privileged_shell("pacman -Sy --noconfirm --needed tree-sitter-cli"),
        LinuxDistro::Fedora => run_privileged_shell("dnf install -y tree-sitter-cli"),
        LinuxDistro::Rhel | LinuxDistro::AmazonLinux => {
            if command_available_without_path("dnf") {
                run_privileged_shell("dnf install -y tree-sitter-cli")
            } else {
                Err("tree-sitter-cli package is not available through yum on this platform".into())
            }
        }
        LinuxDistro::Debian | LinuxDistro::Unknown => Err("tree-sitter-cli system package is not available for this distro".into()),
    }
}

fn detect_linux_distro() -> LinuxDistro {
    if Path::new("/etc/alpine-release").exists() {
        return LinuxDistro::Alpine;
    }
    if Path::new("/etc/debian_version").exists() {
        return LinuxDistro::Debian;
    }
    if Path::new("/etc/arch-release").exists() {
        return LinuxDistro::Arch;
    }

    if let Ok(os_release) = fs::read_to_string("/etc/os-release") {
        let id = os_release_value(&os_release, "ID").unwrap_or_default();
        let id_like = os_release_value(&os_release, "ID_LIKE").unwrap_or_default();
        let id_like_words = format!(" {id_like} ");

        return match id.as_str() {
            "alpine" => LinuxDistro::Alpine,
            "debian" | "ubuntu" | "linuxmint" | "pop" => LinuxDistro::Debian,
            "arch" | "manjaro" | "endeavouros" => LinuxDistro::Arch,
            "fedora" => LinuxDistro::Fedora,
            "amzn" | "amazonlinux" => LinuxDistro::AmazonLinux,
            "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "olinux" => LinuxDistro::Rhel,
            _ if id_like_words.contains(" rhel ") || id_like_words.contains(" fedora ") => LinuxDistro::Rhel,
            _ if id_like_words.contains(" debian ") => LinuxDistro::Debian,
            _ if id_like_words.contains(" arch ") => LinuxDistro::Arch,
            _ => LinuxDistro::Unknown,
        };
    }

    if Path::new("/etc/redhat-release").exists() {
        LinuxDistro::Rhel
    } else {
        LinuxDistro::Unknown
    }
}

fn os_release_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (line_key, value) = line.split_once('=')?;
        if line_key != key {
            return None;
        }
        Some(value.trim_matches('"').to_string())
    })
}

fn run_privileged_shell(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Installing system dependencies with: {command}");

    let status = if is_unix_root() {
        Command::new("sh").arg("-c").arg(command).status()?
    } else if command_available_without_path("sudo") {
        Command::new("sudo").arg("sh").arg("-c").arg(command).status()?
    } else {
        return Err(format!("missing system dependencies and sudo is not available. Run as root: {command}").into());
    };

    if status.success() {
        Ok(())
    } else {
        Err(format!("system dependency installation failed: {status}").into())
    }
}

fn is_unix_root() -> bool {
    let output = Command::new("id").arg("-u").output();
    matches!(output, Ok(output) if String::from_utf8_lossy(&output.stdout).trim() == "0")
}

fn command_available_without_path(command_name: &str) -> bool {
    let mut command = Command::new(command_name);
    command
        .args(command_probe_args(command_name))
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.status().is_ok()
}

fn command_probe_args(command_name: &str) -> &'static [&'static str] {
    match command_name {
        "powershell" => &["-NoProfile", "-Command", "exit 0"],
        "sh" => &["-c", "exit 0"],
        _ => &["--version"],
    }
}

fn ensure_starter_config(config_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if config_dir.join("init.lua").exists() {
        return Ok(());
    }

    if config_dir.exists() && fs::read_dir(config_dir)?.next().transpose()?.is_some() {
        return Err(format!(
            "{} exists but does not contain init.lua; move it away or run `lazyvim reset --yes`",
            config_dir.display()
        )
        .into());
    }

    if config_dir.exists() {
        fs::remove_dir_all(config_dir)?;
    }

    let starter_repository =
        env::var("LAZYVIM_STARTER_REPOSITORY").unwrap_or_else(|_| STARTER_REPOSITORY.to_string());

    let output = Command::new("git")
        .arg("clone")
        .arg("--depth=1")
        .arg(&starter_repository)
        .arg(config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "failed to clone LazyVim starter from {starter_repository}: {}{}",
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }

    let git_dir = config_dir.join(".git");
    if git_dir.exists() {
        fs::remove_dir_all(git_dir)?;
    }

    Ok(())
}

fn resolve_nvim(home: &Path, exe_dir: Option<&Path>) -> PathBuf {
    if let Some(value) = env::var_os("LAZYVIM_NVIM") {
        return PathBuf::from(value);
    }

    let executable_name = nvim_executable_name();
    let mut candidates = Vec::new();

    if let Some(exe_dir) = exe_dir {
        candidates.push(exe_dir.join("nvim").join("bin").join(executable_name));
        candidates.push(exe_dir.join("bin").join(executable_name));
    }

    candidates.push(home.join("nvim").join("bin").join(executable_name));
    candidates.push(home.join("bin").join(executable_name));

    for candidate in candidates {
        if candidate.exists() {
            return candidate;
        }
    }

    PathBuf::from(executable_name)
}

fn nvim_executable_name() -> &'static str {
    if cfg!(windows) {
        "nvim.exe"
    } else {
        "nvim"
    }
}

fn build_path(home: &Path, exe_dir: Option<&Path>) -> io::Result<OsString> {
    let mut paths = Vec::new();

    paths.push(home.join("nvim").join("bin"));
    paths.push(home.join("bin"));
    paths.push(home.join("tools").join("zig"));

    if cfg!(windows) {
        paths.push(PathBuf::from(r"C:\Program Files\Git\cmd"));
        paths.push(PathBuf::from(r"C:\Program Files\Git\bin"));
        paths.push(PathBuf::from(r"C:\Program Files\Git\usr\bin"));
    }

    if cfg!(target_os = "macos") {
        paths.push(PathBuf::from("/opt/homebrew/bin"));
        paths.push(PathBuf::from("/usr/local/bin"));
    }

    if let Some(exe_dir) = exe_dir {
        paths.push(exe_dir.join("nvim").join("bin"));
        paths.push(exe_dir.join("bin"));
        paths.push(exe_dir.join("tools").join("zig"));
    }

    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }

    env::join_paths(paths).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}


fn install_deps_command(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    ensure_system_dependencies(&runtime.path_value)?;
    ensure_managed_tools(&runtime.home, &runtime.path_value)?;
    println!("Installed LazyVim dependencies into {}", runtime.home.display());
    Ok(())
}

fn install_tools_command(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    ensure_managed_tools(&runtime.home, &runtime.path_value)?;
    println!("Installed portable LazyVim tools into {}", runtime.home.display());
    Ok(())
}

fn install_neovim_command(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    if command_runs(&runtime.nvim, &["--version"], Some(&runtime.path_value))
        && runtime.nvim.starts_with(runtime.home.join("nvim"))
    {
        println!("Neovim is already installed at {}", runtime.nvim.display());
        return Ok(());
    }

    install_neovim(&runtime.home)?;

    let installed = runtime.home.join("nvim").join("bin").join(nvim_executable_name());
    if !command_runs(&installed, &["--version"], Some(&runtime.path_value)) {
        return Err(format!("Neovim was installed but could not be started from {}", installed.display()).into());
    }

    println!("Installed Neovim into {}", runtime.home.join("nvim").display());
    Ok(())
}


fn ensure_managed_tools(home: &Path, path_value: &OsString) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(home.join("tools"))?;
    fs::create_dir_all(home.join("bin"))?;

    let zig = zig_executable_path(home);
    if !command_runs(&zig, &["version"], Some(path_value)) {
        install_zig(home)?;
    }
    install_c_compiler_wrappers(home)?;

    let tree_sitter = tree_sitter_executable_path(home);
    if !command_runs(&tree_sitter, &["--version"], Some(path_value)) {
        install_tree_sitter(home, path_value)?;
    }

    let rg = managed_tool_path(home, if cfg!(windows) { "rg.exe" } else { "rg" });
    if !command_runs(&rg, &["--version"], Some(path_value)) {
        install_ripgrep(home)?;
    }

    let fd = managed_tool_path(home, if cfg!(windows) { "fd.exe" } else { "fd" });
    if !command_runs(&fd, &["--version"], Some(path_value)) {
        install_fd(home)?;
    }

    let lazygit = managed_tool_path(home, if cfg!(windows) { "lazygit.exe" } else { "lazygit" });
    if !command_runs(&lazygit, &["--version"], Some(path_value)) {
        install_lazygit(home)?;
    }

    Ok(())
}

fn managed_tool_path(home: &Path, executable_name: &str) -> PathBuf {
    home.join("bin").join(executable_name)
}

fn install_c_compiler_wrappers(home: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bin_dir = home.join("bin");
    fs::create_dir_all(&bin_dir)?;

    let launcher = env::current_exe()?;
    let wrapper_names: &[&str] = if cfg!(windows) {
        &["cc.exe", "gcc.exe", "clang.exe", "c++.exe", "g++.exe", "clang++.exe"]
    } else {
        &["cc", "gcc", "clang", "c++", "g++", "clang++"]
    };

    for name in wrapper_names {
        install_launcher_wrapper(&launcher, &bin_dir.join(name))?;
    }

    Ok(())
}

fn install_launcher_wrapper(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if destination.exists() {
        fs::remove_file(destination)?;
    }

    if fs::hard_link(source, destination).is_err() {
        fs::copy(source, destination)?;
    }

    make_executable(destination)?;
    Ok(())
}

fn is_compiler_wrapper_invocation() -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };

    let Some(stem) = exe.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };

    matches!(stem, "cc" | "gcc" | "clang" | "c++" | "g++" | "clang++")
}

fn run_compiler_wrapper() -> Result<i32, Box<dyn std::error::Error>> {
    let exe = env::current_exe()?;
    let bin_dir = exe.parent().ok_or("compiler wrapper has no parent directory")?;
    let home = bin_dir.parent().ok_or("compiler wrapper is not inside a LazyVim home bin directory")?;
    let zig = zig_executable_path(home);

    if !zig.exists() {
        return Err(format!("Zig compiler was not found at {}", zig.display()).into());
    }

    let mode = compiler_wrapper_mode(&exe);
    let mut command = Command::new(zig);
    command.arg(mode);
    command.args(env::args_os().skip(1));
    command.env("PATH", build_path(home, None)?);

    let status = command.status()?;
    Ok(status.code().unwrap_or(1))
}

fn compiler_wrapper_mode(exe: &Path) -> &'static str {
    match exe.file_stem().and_then(|value| value.to_str()) {
        Some("c++" | "g++" | "clang++") => "c++",
        _ => "cc",
    }
}

fn zig_executable_path(home: &Path) -> PathBuf {
    home.join("tools").join("zig").join(if cfg!(windows) { "zig.exe" } else { "zig" })
}

fn tree_sitter_executable_path(home: &Path) -> PathBuf {
    home.join("bin").join(if cfg!(windows) { "tree-sitter.exe" } else { "tree-sitter" })
}


fn compiler_wrapper_path(home: &Path) -> PathBuf {
    home.join("bin").join(if cfg!(windows) { "cc.exe" } else { "cc" })
}

fn cxx_compiler_wrapper_path(home: &Path) -> PathBuf {
    home.join("bin").join(if cfg!(windows) { "c++.exe" } else { "c++" })
}

fn ensure_treesitter_cache_for_current_toolchain(
    data_home: &Path,
    state_home: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let state_dir = state_home.join(APP_NAME);
    let stamp_path = state_dir.join("portable-toolchain-stamp");

    if matches!(fs::read_to_string(&stamp_path), Ok(value) if value == PORTABLE_TOOLCHAIN_STAMP) {
        return Ok(());
    }

    let data_dir = data_home.join(APP_NAME);
    for path in [
        data_dir.join("site").join("parser"),
        data_dir.join("lazy").join("nvim-treesitter").join("parser"),
        data_dir.join("lazy").join("nvim-treesitter").join("parser-info"),
    ] {
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
    }

    fs::create_dir_all(&state_dir)?;
    fs::write(stamp_path, PORTABLE_TOOLCHAIN_STAMP)?;
    Ok(())
}

fn ensure_portable_lazyvim_config(config_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let plugins_dir = config_dir.join("lua").join("plugins");
    fs::create_dir_all(&plugins_dir)?;

    let portable_plugin = plugins_dir.join("portable.lua");
    let contents = r#"-- Generated by the portable LazyVim launcher.
-- This keeps Treesitter parser builds inside the managed toolchain.
return {
  {
    "nvim-treesitter/nvim-treesitter",
    init = function()
      local ok, install = pcall(require, "nvim-treesitter.install")
      if ok then
        install.compilers = { vim.env.CC or "cc", "zig", "clang", "gcc", "cc" }
      end
    end,
    opts = function(_, opts)
      opts.install = opts.install or {}
      opts.install.compilers = { vim.env.CC or "cc", "zig", "clang", "gcc", "cc" }
    end,
  },
}
"#;

    fs::write(portable_plugin, contents)?;
    Ok(())
}


fn install_ripgrep(home: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let asset = ripgrep_release_asset()?;
    let url = format!("https://github.com/BurntSushi/ripgrep/releases/download/{RIPGREP_VERSION}/{asset}");
    install_single_binary_from_archive(home, "ripgrep", &url, &asset, if cfg!(windows) { "rg.exe" } else { "rg" })
}

fn ripgrep_release_asset() -> Result<String, Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok(format!("ripgrep-{RIPGREP_VERSION}-x86_64-pc-windows-msvc.zip"));
    }
    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok(format!("ripgrep-{RIPGREP_VERSION}-x86_64-unknown-linux-musl.tar.gz"));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok(format!("ripgrep-{RIPGREP_VERSION}-x86_64-apple-darwin.tar.gz"));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok(format!("ripgrep-{RIPGREP_VERSION}-aarch64-apple-darwin.tar.gz"));
    }
    Err("automatic ripgrep installation is not supported on this platform".into())
}

fn install_fd(home: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let asset = fd_release_asset()?;
    let url = format!("https://github.com/sharkdp/fd/releases/download/v{FD_VERSION}/{asset}");
    install_single_binary_from_archive(home, "fd", &url, &asset, if cfg!(windows) { "fd.exe" } else { "fd" })
}

fn fd_release_asset() -> Result<String, Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok(format!("fd-v{FD_VERSION}-x86_64-pc-windows-msvc.zip"));
    }
    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok(format!("fd-v{FD_VERSION}-x86_64-unknown-linux-musl.tar.gz"));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok(format!("fd-v{FD_VERSION}-x86_64-apple-darwin.tar.gz"));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok(format!("fd-v{FD_VERSION}-aarch64-apple-darwin.tar.gz"));
    }
    Err("automatic fd installation is not supported on this platform".into())
}

fn install_lazygit(home: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let asset = lazygit_release_asset()?;
    let url = format!("https://github.com/jesseduffield/lazygit/releases/download/v{LAZYGIT_VERSION}/{asset}");
    install_single_binary_from_archive(home, "lazygit", &url, &asset, if cfg!(windows) { "lazygit.exe" } else { "lazygit" })
}

fn lazygit_release_asset() -> Result<String, Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok(format!("lazygit_{LAZYGIT_VERSION}_windows_x86_64.zip"));
    }
    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok(format!("lazygit_{LAZYGIT_VERSION}_linux_x86_64.tar.gz"));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok(format!("lazygit_{LAZYGIT_VERSION}_darwin_x86_64.tar.gz"));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok(format!("lazygit_{LAZYGIT_VERSION}_darwin_arm64.tar.gz"));
    }
    Err("automatic lazygit installation is not supported on this platform".into())
}

fn install_single_binary_from_archive(
    home: &Path,
    label: &str,
    url: &str,
    asset: &str,
    executable_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let downloads_dir = home.join("downloads");
    let archive_path = downloads_dir.join(asset);
    let temp_dir = home.join(format!(".{label}-install"));
    let destination = managed_tool_path(home, executable_name);

    fs::create_dir_all(&downloads_dir)?;
    println!("{label} was not found. Downloading {url}");
    download_file(url, &archive_path)?;
    extract_single_tool_archive(&archive_path, &temp_dir, &destination, executable_name)?;
    make_executable(&destination)?;
    Ok(())
}

fn extract_single_tool_archive(
    archive_path: &Path,
    temp_dir: &Path,
    destination: &Path,
    executable_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)?;
    }
    fs::create_dir_all(temp_dir)?;

    if archive_path.extension().and_then(|value| value.to_str()) == Some("zip") {
        extract_zip_archive(archive_path, temp_dir)?;
    } else {
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(archive_path)
            .arg("-C")
            .arg(temp_dir)
            .status()?;
        if !status.success() {
            return Err(format!("failed to extract {}: tar exited with {status}", archive_path.display()).into());
        }
    }

    let source = find_extracted_tool(temp_dir, executable_name)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source, destination)?;

    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)?;
    }

    Ok(())
}

fn extract_zip_archive(archive_path: &Path, temp_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(windows) {
        let script = r#"
$ErrorActionPreference = 'Stop'
$archive = $env:LAZYVIM_ZIP_ARCHIVE
$temp = $env:LAZYVIM_ZIP_TEMP
Expand-Archive -LiteralPath $archive -DestinationPath $temp -Force
"#;
        let status = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(script)
            .env("LAZYVIM_ZIP_ARCHIVE", archive_path)
            .env("LAZYVIM_ZIP_TEMP", temp_dir)
            .status()?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("failed to extract {}: PowerShell exited with {status}", archive_path.display()).into());
    }

    extract_zip_with_available_tool(archive_path, temp_dir).map(|_| ())
}

fn install_zig(home: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let asset = zig_release_asset()?;
    let url = format!("https://ziglang.org/download/{ZIG_VERSION}/{asset}");
    let downloads_dir = home.join("downloads");
    let archive_path = downloads_dir.join(asset);
    let install_dir = home.join("tools").join("zig");
    let temp_dir = home.join(".zig-install");

    fs::create_dir_all(&downloads_dir)?;
    println!("C compiler was not found. Downloading Zig {ZIG_VERSION} from {url}");
    download_file(&url, &archive_path)?;
    extract_archive_strip_first_directory(&archive_path, &temp_dir, &install_dir)?;
    make_executable(&install_dir.join(if cfg!(windows) { "zig.exe" } else { "zig" }))?;

    Ok(())
}

fn zig_release_asset() -> Result<String, Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok(format!("zig-windows-x86_64-{ZIG_VERSION}.zip"));
    }

    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok(format!("zig-linux-x86_64-{ZIG_VERSION}.tar.xz"));
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok(format!("zig-macos-x86_64-{ZIG_VERSION}.tar.xz"));
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok(format!("zig-macos-aarch64-{ZIG_VERSION}.tar.xz"));
    }

    Err("automatic Zig installation is not supported on this platform".into())
}

fn install_tree_sitter(home: &Path, path_value: &OsString) -> Result<(), Box<dyn std::error::Error>> {
    let asset = tree_sitter_release_asset()?;
    let url = format!("https://github.com/tree-sitter/tree-sitter/releases/download/v{TREE_SITTER_VERSION}/{asset}");
    let downloads_dir = home.join("downloads");
    let archive_path = downloads_dir.join(asset);
    let temp_dir = home.join(".tree-sitter-install");
    let destination = tree_sitter_executable_path(home);

    fs::create_dir_all(&downloads_dir)?;

    if cfg!(target_os = "linux")
        && install_tree_sitter_from_system_package(path_value).is_ok()
        && (command_runs(&destination, &["--version"], Some(path_value))
            || command_runs(Path::new("tree-sitter"), &["--version"], Some(path_value)))
    {
        return Ok(());
    }

    println!("tree-sitter CLI was not found. Downloading tree-sitter {TREE_SITTER_VERSION} from {url}");
    download_file(&url, &archive_path)?;
    extract_tree_sitter_archive(&archive_path, &temp_dir, &destination)?;
    make_executable(&destination)?;

    Ok(())
}

fn tree_sitter_release_asset() -> Result<&'static str, Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok("tree-sitter-cli-windows-x64.zip");
    }

    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok("tree-sitter-cli-linux-x64.zip");
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok("tree-sitter-cli-macos-x64.zip");
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok("tree-sitter-cli-macos-arm64.zip");
    }

    Err("automatic tree-sitter CLI installation is not supported on this platform".into())
}

fn extract_archive_strip_first_directory(
    archive_path: &Path,
    temp_dir: &Path,
    install_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)?;
    }
    fs::create_dir_all(temp_dir)?;

    if cfg!(windows) {
        extract_zip_with_powershell(archive_path, temp_dir, install_dir)?;
    } else {
        let status = Command::new("tar")
            .arg("-xf")
            .arg(archive_path)
            .arg("--strip-components=1")
            .arg("-C")
            .arg(temp_dir)
            .status()?;

        if !status.success() {
            return Err(format!("failed to extract {}: tar exited with {status}", archive_path.display()).into());
        }

        if install_dir.exists() {
            fs::remove_dir_all(install_dir)?;
        }
        fs::rename(temp_dir, install_dir)?;
    }

    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)?;
    }

    Ok(())
}

fn extract_tree_sitter_archive(
    archive_path: &Path,
    temp_dir: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)?;
    }
    fs::create_dir_all(temp_dir)?;

    if cfg!(windows) {
        extract_tree_sitter_with_powershell(archive_path, temp_dir, destination)?;
    } else {
        let extracted = extract_zip_with_available_tool(archive_path, temp_dir)?;
        let source = find_extracted_tool(&extracted, if cfg!(windows) { "tree-sitter.exe" } else { "tree-sitter" })?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, destination)?;
    }

    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)?;
    }

    Ok(())
}

fn extract_zip_with_available_tool(archive_path: &Path, temp_dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let python = Command::new("python3")
        .arg("-m")
        .arg("zipfile")
        .arg("-e")
        .arg(archive_path)
        .arg(temp_dir)
        .status();

    if matches!(python, Ok(status) if status.success()) {
        return Ok(temp_dir.to_path_buf());
    }

    let unzip = Command::new("unzip")
        .arg("-q")
        .arg(archive_path)
        .arg("-d")
        .arg(temp_dir)
        .status();

    if matches!(unzip, Ok(status) if status.success()) {
        return Ok(temp_dir.to_path_buf());
    }

    Err(format!(
        "failed to extract {}: install python3 or unzip, or place tree-sitter manually in ~/.lazyvim/bin",
        archive_path.display()
    )
    .into())
}

fn find_extracted_tool(root: &Path, executable_name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if entry_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == executable_name)
            {
                return Ok(entry_path);
            }
        }
    }

    Err(format!("could not find {executable_name} in extracted archive").into())
}

fn extract_tree_sitter_with_powershell(
    archive_path: &Path,
    temp_dir: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
$ErrorActionPreference = 'Stop'
$archive = $env:LAZYVIM_TREE_SITTER_ARCHIVE
$temp = $env:LAZYVIM_TREE_SITTER_TEMP
$dest = $env:LAZYVIM_TREE_SITTER_DEST

if (Test-Path -LiteralPath $temp) {
    Remove-Item -LiteralPath $temp -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $temp | Out-Null
Expand-Archive -LiteralPath $archive -DestinationPath $temp -Force

$tool = Get-ChildItem -LiteralPath $temp -Recurse -File | Where-Object { $_.Name -eq 'tree-sitter.exe' -or $_.Name -like 'tree-sitter*.exe' } | Select-Object -First 1
if ($null -eq $tool) {
    throw "Could not find tree-sitter.exe in extracted archive"
}

$parent = Split-Path -Parent $dest
New-Item -ItemType Directory -Force -Path $parent | Out-Null
Copy-Item -LiteralPath $tool.FullName -Destination $dest -Force
"#;

    let status = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .env("LAZYVIM_TREE_SITTER_ARCHIVE", archive_path)
        .env("LAZYVIM_TREE_SITTER_TEMP", temp_dir)
        .env("LAZYVIM_TREE_SITTER_DEST", destination)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to extract {}: PowerShell exited with {status}",
            archive_path.display()
        )
        .into())
    }
}

fn make_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            let mut permissions = fs::metadata(path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions)?;
        }
    }

    Ok(())
}

fn install_neovim(home: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let asset = neovim_release_asset()?;
    let url = format!("https://github.com/neovim/neovim/releases/latest/download/{asset}");
    let downloads_dir = home.join("downloads");
    let archive_path = downloads_dir.join(asset);

    fs::create_dir_all(&downloads_dir)?;
    println!("Neovim was not found. Downloading {url}");
    download_file(&url, &archive_path)?;
    extract_neovim_archive(home, &archive_path)?;

    Ok(())
}

fn neovim_release_asset() -> Result<&'static str, Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok("nvim-win64.zip");
    }

    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok("nvim-linux-x86_64.tar.gz");
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok("nvim-macos-x86_64.tar.gz");
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok("nvim-macos-arm64.tar.gz");
    }

    Err("automatic Neovim installation is not supported on this platform; install Neovim manually or set LAZYVIM_NVIM".into())
}

fn download_file(url: &str, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let curl = Command::new("curl")
        .arg("-fL")
        .arg("--retry")
        .arg("3")
        .arg("--output")
        .arg(destination)
        .arg(url)
        .status();

    if matches!(curl, Ok(status) if status.success()) {
        return Ok(());
    }

    if cfg!(windows) {
        return download_file_with_powershell(url, destination);
    }

    match curl {
        Ok(status) => Err(format!("failed to download {url}: curl exited with {status}").into()),
        Err(error) => Err(format!("failed to download {url}: curl is required but could not be started: {error}").into()),
    }
}

fn download_file_with_powershell(url: &str, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
$ErrorActionPreference = 'Stop'
$url = $env:LAZYVIM_DOWNLOAD_URL
$dest = $env:LAZYVIM_DOWNLOAD_DEST
$parent = Split-Path -Parent $dest
New-Item -ItemType Directory -Force -Path $parent | Out-Null
Invoke-WebRequest -Uri $url -OutFile $dest
"#;

    let status = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .env("LAZYVIM_DOWNLOAD_URL", url)
        .env("LAZYVIM_DOWNLOAD_DEST", destination)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("failed to download {url}: PowerShell exited with {status}").into())
    }
}

fn extract_neovim_archive(home: &Path, archive_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let install_dir = home.join("nvim");
    let temp_dir = home.join(".nvim-install");

    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }
    fs::create_dir_all(&temp_dir)?;

    if cfg!(windows) {
        extract_zip_with_powershell(archive_path, &temp_dir, &install_dir)?;
    } else {
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(archive_path)
            .arg("--strip-components=1")
            .arg("-C")
            .arg(&temp_dir)
            .status()?;

        if !status.success() {
            return Err(format!("failed to extract {}: tar exited with {status}", archive_path.display()).into());
        }

        if install_dir.exists() {
            fs::remove_dir_all(&install_dir)?;
        }
        fs::rename(&temp_dir, &install_dir)?;
        make_nvim_executable(&install_dir)?;
        clear_macos_quarantine(&install_dir);
    }

    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }

    Ok(())
}

fn extract_zip_with_powershell(
    archive_path: &Path,
    temp_dir: &Path,
    install_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
$ErrorActionPreference = 'Stop'
$archive = $env:LAZYVIM_NVIM_ARCHIVE
$temp = $env:LAZYVIM_NVIM_TEMP
$dest = $env:LAZYVIM_NVIM_DEST

if (Test-Path -LiteralPath $temp) {
    Remove-Item -LiteralPath $temp -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $temp | Out-Null
Expand-Archive -LiteralPath $archive -DestinationPath $temp -Force

$root = Get-ChildItem -LiteralPath $temp -Directory | Select-Object -First 1
if ($null -eq $root) {
    throw "Could not find extracted Neovim directory"
}

if (Test-Path -LiteralPath $dest) {
    Remove-Item -LiteralPath $dest -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Get-ChildItem -LiteralPath $root.FullName | Copy-Item -Destination $dest -Recurse -Force
"#;

    let status = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .env("LAZYVIM_NVIM_ARCHIVE", archive_path)
        .env("LAZYVIM_NVIM_TEMP", temp_dir)
        .env("LAZYVIM_NVIM_DEST", install_dir)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to extract {}: PowerShell exited with {status}",
            archive_path.display()
        )
        .into())
    }
}

#[cfg(unix)]
fn make_nvim_executable(install_dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let nvim = install_dir.join("bin").join("nvim");
    if nvim.exists() {
        let mut permissions = fs::metadata(&nvim)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(nvim, permissions)?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn make_nvim_executable(_install_dir: &Path) -> io::Result<()> {
    Ok(())
}

fn clear_macos_quarantine(install_dir: &Path) {
    if !cfg!(target_os = "macos") {
        return;
    }

    let _ = Command::new("xattr")
        .arg("-cr")
        .arg(install_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn command_runs(command_path: &Path, args: &[&str], path_value: Option<&OsString>) -> bool {
    let mut command = Command::new(command_path);
    command.args(args).stdout(Stdio::null()).stderr(Stdio::null());

    if let Some(path_value) = path_value {
        command.env("PATH", path_value);
    }

    matches!(command.status(), Ok(status) if status.success())
}

fn base_command(runtime: &Runtime) -> Command {
    let mut command = Command::new(&runtime.nvim);

    command
        .env("LAZYVIM_HOME", &runtime.home)
        .env("NVIM_APPNAME", APP_NAME)
        .env("XDG_CONFIG_HOME", &runtime.config_home)
        .env("XDG_DATA_HOME", &runtime.data_home)
        .env("XDG_STATE_HOME", &runtime.state_home)
        .env("XDG_CACHE_HOME", &runtime.cache_home)
        .env("PATH", &runtime.path_value)
        .env("CC", compiler_wrapper_path(&runtime.home))
        .env("CXX", cxx_compiler_wrapper_path(&runtime.home))
        .env("TREE_SITTER_CLI", tree_sitter_executable_path(&runtime.home));

    command
}

fn launch_nvim(runtime: &Runtime, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let status = base_command(runtime)
        .args(args)
        .status()
        .map_err(|error| format!("failed to start Neovim at {}: {error}", runtime.nvim.display()))?;
    exit(status.code().unwrap_or(1));
}

fn run_lazy_command(runtime: &Runtime, command_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let lazy_command = format!("+Lazy! {command_name}");
    let status = base_command(runtime)
        .arg("--headless")
        .arg(lazy_command)
        .arg("+qa")
        .status()?;

    exit(status.code().unwrap_or(1));
}

fn doctor(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    println!("lazyvim {}", env!("CARGO_PKG_VERSION"));
    println!();
    print_locations(runtime)?;
    println!();

    check_command("nvim", &runtime.nvim, &["--version"], true, Some(&runtime.path_value))?;
    check_command("zig", Path::new("zig"), &["version"], true, Some(&runtime.path_value))?;
    check_command("cc", &compiler_wrapper_path(&runtime.home), &["--version"], true, Some(&runtime.path_value))?;
    check_command("tree-sitter", Path::new("tree-sitter"), &["--version"], true, Some(&runtime.path_value))?;
    check_command("git", Path::new("git"), &["--version"], true, Some(&runtime.path_value))?;
    check_command("curl", Path::new("curl"), &["--version"], true, Some(&runtime.path_value))?;
    check_command("rg", Path::new("rg"), &["--version"], true, Some(&runtime.path_value))?;
    check_command("fd", Path::new("fd"), &["--version"], true, Some(&runtime.path_value))?;
    check_command("lazygit", Path::new("lazygit"), &["--version"], true, Some(&runtime.path_value))?;

    Ok(())
}

fn print_locations(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    println!("home:        {}", runtime.home.display());
    println!("config:      {}", runtime.config_dir.display());
    println!("data:        {}", runtime.data_home.join(APP_NAME).display());
    println!("state:       {}", runtime.state_home.join(APP_NAME).display());
    println!("cache:       {}", runtime.cache_home.join(APP_NAME).display());
    println!("nvim:        {}", runtime.nvim.display());

    if let Some(exe_dir) = &runtime.exe_dir {
        println!("launcher:    {}", exe_dir.display());
    }

    Ok(())
}

fn check_command(label: &str, command_path: &Path, args: &[&str], required: bool, path_value: Option<&OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::new(command_path);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

    if let Some(path_value) = path_value {
        command.env("PATH", path_value);
    }

    let output = command.output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let first_line = stdout
                .lines()
                .chain(stderr.lines())
                .find(|line| !line.trim().is_empty())
                .unwrap_or("ok");

            println!("[ok]   {label}: {first_line}");
        }
        Ok(output) => {
            let message = String::from_utf8_lossy(&output.stderr);
            let level = if required { "fail" } else { "warn" };
            println!("[{level}] {label}: exited with {} {}", output.status, message.trim());
        }
        Err(error) => {
            let level = if required { "fail" } else { "warn" };
            println!("[{level}] {label}: {error}");
        }
    }

    Ok(())
}

fn print_help() {
    println!("lazyvim {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage:");
    println!("  lazyvim [--home <path>] [--portable-home] [nvim args...]");
    println!("  lazyvim [--home <path>] [--portable-home] <command>");
    println!();
    println!("Options:");
    println!("  --home <path>      Use a custom portable home for this run");
    println!("  --portable-home    Store .lazyvim next to the launcher executable");
    println!();
    println!("Commands:");
    println!("  where      Print resolved portable directories");
    println!("  doctor     Check Neovim and common LazyVim dependencies");
    println!("  sync       Run Lazy sync in headless mode");
    println!("  restore    Run Lazy restore in headless mode");
    println!("  update     Run Lazy update in headless mode");
    println!("  clean      Run Lazy clean in headless mode");
    println!("  install-nvim  Install Neovim into the portable home");
    println!("  install-tools Install managed portable tools into the portable home");
    println!("  install-deps  Install system and managed LazyVim dependencies");
    println!("  reset      Delete the portable home directory; requires --yes");
    println!("  help       Print this help");
    println!();
    println!("Environment:");
    println!("  LAZYVIM_HOME                Override ~/.lazyvim; use 'portable' for executable-local home");
    println!("  LAZYVIM_NVIM                Use a specific nvim executable");
    println!("  LAZYVIM_STARTER_REPOSITORY  Override the LazyVim starter repository");
}
