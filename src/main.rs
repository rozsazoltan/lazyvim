use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};

const APP_NAME: &str = "lazyvim";
const DEFAULT_HOME_DIR: &str = ".lazyvim";

const STARTER_REPOSITORY: &str = "https://github.com/LazyVim/starter.git";

#[derive(Debug)]
struct Cli {
    home: Option<PathBuf>,
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
            let runtime = prepare_runtime(cli.home, false)?;
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
            let runtime = prepare_runtime(cli.home, bootstrap)?;
            match command {
                CliCommand::Launch(args) => launch_nvim(&runtime, &args),
                CliCommand::Doctor => doctor(&runtime),
                CliCommand::Where => print_locations(&runtime),
                CliCommand::Sync => run_lazy_command(&runtime, "sync"),
                CliCommand::Restore => run_lazy_command(&runtime, "restore"),
                CliCommand::Update => run_lazy_command(&runtime, "update"),
                CliCommand::Clean => run_lazy_command(&runtime, "clean"),
                CliCommand::InstallNvim => install_neovim_command(&runtime),
                CliCommand::Help | CliCommand::Version | CliCommand::Reset { .. } => unreachable!(),
            }
        }
    }
}

fn parse_cli(mut args: Vec<String>) -> Cli {
    let mut home = None;
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
        Some("reset") => CliCommand::Reset {
            yes: args.iter().any(|arg| arg == "--yes" || arg == "-y"),
        },
        Some("help") | Some("--help") | Some("-h") => CliCommand::Help,
        Some("--version") | Some("-V") => CliCommand::Version,
        _ => CliCommand::Launch(args),
    };

    Cli { home, command }
}

fn prepare_runtime(home_override: Option<PathBuf>, bootstrap: bool) -> Result<Runtime, Box<dyn std::error::Error>> {
    let home = resolve_home(home_override)?;
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

    let exe_dir = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    let path_value = build_path(&home, exe_dir.as_deref())?;
    let mut nvim = resolve_nvim(&home, exe_dir.as_deref());

    if bootstrap {
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

        ensure_starter_config(&config_dir)?;
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

fn resolve_home(home_override: Option<PathBuf>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = home_override {
        return expand_home(path);
    }

    if let Some(value) = env::var_os("LAZYVIM_HOME") {
        return expand_home(PathBuf::from(value));
    }

    Ok(user_home_dir()?.join(DEFAULT_HOME_DIR))
}

fn expand_home(path: PathBuf) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let text = path.to_string_lossy();

    if text == "~" {
        return user_home_dir();
    }

    if let Some(rest) = text.strip_prefix("~/") {
        return Ok(user_home_dir()?.join(rest));
    }

    if cfg!(windows) {
        if let Some(rest) = text.strip_prefix("~\\") {
            return Ok(user_home_dir()?.join(rest));
        }
    }

    Ok(PathBuf::from(text.as_ref()))
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

    if let Some(exe_dir) = exe_dir {
        paths.push(exe_dir.join("nvim").join("bin"));
        paths.push(exe_dir.join("bin"));
    }

    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }

    env::join_paths(paths).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
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
    let status = Command::new("curl")
        .arg("-fL")
        .arg("--retry")
        .arg("3")
        .arg("--output")
        .arg(destination)
        .arg(url)
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("failed to download {url}: curl exited with {status}").into()),
        Err(error) => Err(format!("failed to download {url}: curl is required but could not be started: {error}").into()),
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
        .env("PATH", &runtime.path_value);

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

    check_command("nvim", &runtime.nvim, &["--version"], true)?;
    check_path_tool("git", &["--version"])?;
    check_path_tool("curl", &["--version"])?;
    check_path_tool("rg", &["--version"])?;
    check_path_tool("fd", &["--version"])?;
    check_path_tool("lazygit", &["--version"])?;

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

fn check_path_tool(name: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    check_command(name, Path::new(name), args, false)
}

fn check_command(label: &str, command_path: &Path, args: &[&str], required: bool) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(command_path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

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
    println!("  lazyvim [--home <path>] [nvim args...]");
    println!("  lazyvim [--home <path>] <command>");
    println!();
    println!("Commands:");
    println!("  where      Print resolved portable directories");
    println!("  doctor     Check Neovim and common LazyVim dependencies");
    println!("  sync       Run Lazy sync in headless mode");
    println!("  restore    Run Lazy restore in headless mode");
    println!("  update     Run Lazy update in headless mode");
    println!("  clean      Run Lazy clean in headless mode");
    println!("  install-nvim Install Neovim into the portable home");
    println!("  reset      Delete the portable home directory; requires --yes");
    println!("  help       Print this help");
    println!();
    println!("Environment:");
    println!("  LAZYVIM_HOME                Override ~/.lazyvim");
    println!("  LAZYVIM_NVIM                Use a specific nvim executable");
    println!("  LAZYVIM_STARTER_REPOSITORY  Override the LazyVim starter repository");
}
