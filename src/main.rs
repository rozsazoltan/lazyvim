use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};

const APP_NAME: &str = "lazyvim";
const DEFAULT_HOME_DIR: &str = ".lazyvim";

const STARTER_REPOSITORY: &str = "https://github.com/LazyVim/starter.git";
const ZIG_VERSION: &str = "0.14.0";
const TREE_SITTER_VERSION: &str = "0.26.10";
const RIPGREP_VERSION: &str = "15.1.0";
const FD_VERSION: &str = "10.4.2";
const FD_MACOS_X86_64_VERSION: &str = "10.3.0";
const LAZYGIT_VERSION: &str = "0.62.2";
const PORTABLE_TOOLCHAIN_STAMP: &str = "2026-07-04-portable-cc-v4";
const EMBEDDED_TREE_SITTER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded-tree-sitter"));

#[derive(Debug)]
struct Cli {
    home: Option<PathBuf>,
    portable_home: bool,
    user_home: bool,
    home_action: HomeSwitchAction,
    command: CliCommand,
}

#[derive(Debug)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeSwitchAction {
    Prompt,
    Move,
    StartNew,
    DeleteOld,
    KeepRemembered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeSource {
    CliHome,
    PortableFlag,
    UserHomeFlag,
    Environment,
    Remembered,
    Default,
}

#[derive(Debug)]
struct HomeSelection {
    path: PathBuf,
    source: HomeSource,
    remembered: Option<PathBuf>,
}

enum HomeConflictResolution {
    Move,
    StartNew,
    DeleteOld,
    KeepRemembered,
}

enum HomeRegistryWrite {
    Written,
    Failed(String),
}

enum HomeComparison {
    Same,
    Different,
    Unknown,
}

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
    let Cli {
        home,
        portable_home,
        user_home,
        home_action,
        command,
    } = parse_cli(env::args().skip(1).collect());

    match command {
        CliCommand::Help => {
            print_help();
            Ok(())
        }
        CliCommand::Version => {
            println!("lazyvim {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliCommand::Reset { yes } => {
            let runtime = prepare_runtime(home, portable_home, user_home, home_action, false, false)?;
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
            let bootstrap = matches!(
                command,
                CliCommand::Launch(_) | CliCommand::Sync | CliCommand::Restore | CliCommand::Update | CliCommand::Clean
            );
            let runtime = prepare_runtime(home, portable_home, user_home, home_action, bootstrap, true)?;
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
    let mut user_home = false;
    let mut home_action = HomeSwitchAction::Prompt;
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

        if arg == "--user-home" {
            user_home = true;
            args.remove(index);
            continue;
        }

        if arg == "--move-home" {
            home_action = HomeSwitchAction::Move;
            args.remove(index);
            continue;
        }

        if arg == "--new-home" || arg == "--start-new-home" {
            home_action = HomeSwitchAction::StartNew;
            args.remove(index);
            continue;
        }

        if arg == "--delete-old-home" {
            home_action = HomeSwitchAction::DeleteOld;
            args.remove(index);
            continue;
        }

        if arg == "--keep-home" {
            home_action = HomeSwitchAction::KeepRemembered;
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
        user_home,
        home_action,
        command,
    }
}

fn prepare_runtime(
    home_override: Option<PathBuf>,
    portable_home: bool,
    user_home: bool,
    home_action: HomeSwitchAction,
    bootstrap: bool,
    remember_home: bool,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    let selection = resolve_home_selection(home_override, portable_home, user_home, exe_dir.as_deref())?;
    let home = resolve_home_switch(selection, home_action, exe_dir.as_deref(), remember_home)?;

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

fn resolve_home_selection(
    home_override: Option<PathBuf>,
    portable_home: bool,
    user_home: bool,
    exe_dir: Option<&Path>,
) -> Result<HomeSelection, Box<dyn std::error::Error>> {
    let remembered = read_remembered_home(exe_dir)?;

    if portable_home {
        return Ok(HomeSelection {
            path: home_next_to_executable(exe_dir)?,
            source: HomeSource::PortableFlag,
            remembered,
        });
    }

    if user_home {
        return Ok(HomeSelection {
            path: default_home()?,
            source: HomeSource::UserHomeFlag,
            remembered,
        });
    }

    if let Some(path) = home_override {
        return Ok(HomeSelection {
            path: expand_home(path, exe_dir)?,
            source: HomeSource::CliHome,
            remembered,
        });
    }

    if let Some(value) = env::var_os("LAZYVIM_HOME") {
        let value = PathBuf::from(value);
        let text = value.to_string_lossy();
        let path = if is_user_home_alias(&text) {
            default_home()?
        } else {
            expand_home(value, exe_dir)?
        };

        return Ok(HomeSelection {
            path,
            source: HomeSource::Environment,
            remembered,
        });
    }

    if let Some(remembered) = remembered.clone() {
        return Ok(HomeSelection {
            path: remembered,
            source: HomeSource::Remembered,
            remembered,
        });
    }

    Ok(HomeSelection {
        path: default_home()?,
        source: HomeSource::Default,
        remembered,
    })
}

fn default_home() -> Result<PathBuf, Box<dyn std::error::Error>> {
    absolute_path(user_home_dir()?.join(DEFAULT_HOME_DIR))
}

fn resolve_home_switch(
    selection: HomeSelection,
    action: HomeSwitchAction,
    exe_dir: Option<&Path>,
    remember_home: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let selected = absolute_path(selection.path.clone())?;
    let previous = previous_home_for_selection(&selection, &selected)?;

    if let Some(previous) = previous {
        if homes_are_same(&previous, &selected) != HomeComparison::Same {
            let resolution = resolve_home_conflict(&previous, &selected, action)?;

            match resolution {
                HomeConflictResolution::Move => {
                    move_remembered_home(&previous, &selected)?;
                    remember_home_if_requested(&selected, exe_dir, remember_home)?;
                    return Ok(selected);
                }
                HomeConflictResolution::StartNew => {
                    remember_home_if_requested(&selected, exe_dir, remember_home)?;
                    return Ok(selected);
                }
                HomeConflictResolution::DeleteOld => {
                    delete_previous_home(&previous, &selected)?;
                    remember_home_if_requested(&selected, exe_dir, remember_home)?;
                    return Ok(selected);
                }
                HomeConflictResolution::KeepRemembered => {
                    remember_home_if_requested(&previous, exe_dir, remember_home)?;
                    return Ok(previous);
                }
            }
        }
    }

    remember_home_if_requested(&selected, exe_dir, remember_home)?;
    Ok(selected)
}

fn previous_home_for_selection(
    selection: &HomeSelection,
    selected: &Path,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    if matches!(selection.source, HomeSource::Remembered | HomeSource::Default) {
        return Ok(None);
    }

    if let Some(remembered) = &selection.remembered {
        return Ok(Some(absolute_path(remembered.clone())?));
    }

    let default = default_home()?;
    if default.exists() && homes_are_same(&default, selected) != HomeComparison::Same {
        return Ok(Some(default));
    }

    Ok(None)
}

fn resolve_home_conflict(
    previous: &Path,
    selected: &Path,
    action: HomeSwitchAction,
) -> Result<HomeConflictResolution, Box<dyn std::error::Error>> {
    match action {
        HomeSwitchAction::Move => return Ok(HomeConflictResolution::Move),
        HomeSwitchAction::StartNew => return Ok(HomeConflictResolution::StartNew),
        HomeSwitchAction::DeleteOld => return Ok(HomeConflictResolution::DeleteOld),
        HomeSwitchAction::KeepRemembered => return Ok(HomeConflictResolution::KeepRemembered),
        HomeSwitchAction::Prompt => {}
    }

    if !should_prompt_for_home_switch() {
        return Err(format!(
            "portable home is already remembered at {}; requested {}; rerun with --move-home, --new-home, --delete-old-home, or --keep-home",
            previous.display(),
            selected.display()
        )
        .into());
    }

    prompt_home_conflict(previous, selected)
}

fn should_prompt_for_home_switch() -> bool {
    env::var_os("CI").is_none() && env::var_os("GITHUB_ACTIONS").is_none()
}

fn prompt_home_conflict(
    previous: &Path,
    selected: &Path,
) -> Result<HomeConflictResolution, Box<dyn std::error::Error>> {
    println!("A different LazyVim home is already remembered.");
    println!("current: {}", previous.display());
    println!("new:     {}", selected.display());
    println!();
    println!("Choose what to do:");
    println!("  m  move the current home to the new path");
    println!("  n  start a new home at the new path and keep the old one");
    println!("  d  delete the old home and start at the new path");
    println!("  k  keep using the remembered home");
    println!("  a  abort");
    print!("Selection [a]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    match input.trim().to_ascii_lowercase().as_str() {
        "m" | "move" => Ok(HomeConflictResolution::Move),
        "n" | "new" | "start" => Ok(HomeConflictResolution::StartNew),
        "d" | "delete" => Ok(HomeConflictResolution::DeleteOld),
        "k" | "keep" => Ok(HomeConflictResolution::KeepRemembered),
        "a" | "abort" | "" => Err("aborted home switch".into()),
        value => Err(format!("unknown home switch selection: {value}").into()),
    }
}

fn move_remembered_home(previous: &Path, selected: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !previous.exists() {
        return Err(format!("cannot move {}; it does not exist", previous.display()).into());
    }

    if selected.exists() {
        return Err(format!("cannot move to {}; destination already exists", selected.display()).into());
    }

    if selected.starts_with(previous) {
        return Err(format!(
            "cannot move {} into itself at {}",
            previous.display(),
            selected.display()
        )
        .into());
    }

    if let Some(parent) = selected.parent() {
        fs::create_dir_all(parent)?;
    }

    println!(
        "Moving portable home from {} to {}",
        previous.display(),
        selected.display()
    );

    move_directory(previous, selected)
}

fn delete_previous_home(previous: &Path, selected: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !previous.exists() {
        return Ok(());
    }

    if selected.starts_with(previous) {
        return Err(format!(
            "cannot delete {}; selected home {} is inside it",
            previous.display(),
            selected.display()
        )
        .into());
    }

    println!("Deleting previous portable home at {}", previous.display());
    fs::remove_dir_all(previous)?;
    Ok(())
}

fn remember_home_if_requested(
    home: &Path,
    exe_dir: Option<&Path>,
    remember_home: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !remember_home {
        return Ok(());
    }

    match write_remembered_home(home, exe_dir) {
        HomeRegistryWrite::Written => Ok(()),
        HomeRegistryWrite::Failed(error) => Err(error.into()),
    }
}

fn read_remembered_home(exe_dir: Option<&Path>) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    for path in home_registry_candidates(exe_dir)? {
        match fs::read_to_string(&path) {
            Ok(content) => {
                let value = content
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && !line.starts_with('#'));

                if let Some(value) = value {
                    return Ok(Some(expand_home(PathBuf::from(value), exe_dir)?));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }

    Ok(None)
}

fn write_remembered_home(home: &Path, exe_dir: Option<&Path>) -> HomeRegistryWrite {
    let content = format!("# Managed by lazyvim.\n{}\n", home.display());
    let mut last_error = None;

    for path in match home_registry_candidates(exe_dir) {
        Ok(paths) => paths,
        Err(error) => return HomeRegistryWrite::Failed(error.to_string()),
    } {
        match path.parent().map(fs::create_dir_all) {
            Some(Ok(())) | None => {}
            Some(Err(error)) => {
                last_error = Some(format!("{}: {}", path.display(), error));
                continue;
            }
        }

        match fs::write(&path, &content) {
            Ok(()) => return HomeRegistryWrite::Written,
            Err(error) => last_error = Some(format!("{}: {}", path.display(), error)),
        }
    }

    HomeRegistryWrite::Failed(format!(
        "failed to remember portable home: {}",
        last_error.unwrap_or_else(|| String::from("no registry location is available"))
    ))
}

fn home_registry_candidates(exe_dir: Option<&Path>) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut paths = Vec::new();

    if let Some(exe_dir) = exe_dir {
        paths.push(exe_dir.join(".lazyvim-home"));
    }

    paths.push(user_home_dir()?.join(".lazyvim-home"));
    Ok(paths)
}

fn homes_are_same(left: &Path, right: &Path) -> HomeComparison {
    let left_abs = absolute_path(left.to_path_buf());
    let right_abs = absolute_path(right.to_path_buf());

    match (left_abs, right_abs) {
        (Ok(left), Ok(right)) if left == right => HomeComparison::Same,
        (Ok(left), Ok(right)) => {
            let left_canon = fs::canonicalize(&left);
            let right_canon = fs::canonicalize(&right);
            match (left_canon, right_canon) {
                (Ok(left), Ok(right)) if left == right => HomeComparison::Same,
                _ => HomeComparison::Different,
            }
        }
        _ => HomeComparison::Unknown,
    }
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

fn is_user_home_alias(value: &str) -> bool {
    matches!(value, "user" | "user-home" | "home" | "default")
}

fn home_next_to_executable(exe_dir: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let exe_dir = exe_dir.ok_or("could not resolve launcher executable directory")?;
    Ok(exe_dir.join(DEFAULT_HOME_DIR))
}

fn move_directory(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let temp_destination = temporary_move_destination(destination)?;
            if temp_destination.exists() {
                fs::remove_dir_all(&temp_destination)?;
            }

            let copy_result = copy_directory_for_move(source, &temp_destination);
            if let Err(copy_error) = copy_result {
                let _ = fs::remove_dir_all(&temp_destination);
                return Err(format!(
                    "failed to move {} to {}: rename failed with {}; copy fallback failed with {}",
                    source.display(),
                    destination.display(),
                    rename_error,
                    copy_error
                )
                .into());
            }

            if destination.exists() {
                let _ = fs::remove_dir_all(&temp_destination);
                return Err(format!(
                    "failed to move {} to {}: rename failed with {}; destination already exists",
                    source.display(),
                    destination.display(),
                    rename_error
                )
                .into());
            }

            fs::rename(&temp_destination, destination).map_err(|move_error| {
                let _ = fs::remove_dir_all(&temp_destination);
                format!(
                    "failed to move copied home from {} to {}: {}",
                    temp_destination.display(),
                    destination.display(),
                    move_error
                )
            })?;

            fs::remove_dir_all(source)?;
            Ok(())
        }
    }
}

fn temporary_move_destination(destination: &Path) -> io::Result<PathBuf> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", destination.display()),
        )
    })?;

    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("lazyvim-home");

    Ok(parent.join(format!(".{name}.moving-{}", std::process::id())))
}

fn copy_directory_for_move(
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        if let Err(robocopy_error) = copy_directory_with_robocopy(source, destination) {
            if destination.exists() {
                let _ = fs::remove_dir_all(destination);
            }

            copy_directory(source, destination).map_err(|copy_error| {
                format!("robocopy failed with {robocopy_error}; rust copy failed with {copy_error}")
            })?;
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        copy_directory(source, destination)?;
        Ok(())
    }
}

#[cfg(windows)]
fn copy_directory_with_robocopy(
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(destination)?;

    let output = Command::new("robocopy")
        .arg(source)
        .arg(destination)
        .args([
            "/E",
            "/COPY:DAT",
            "/DCOPY:DAT",
            "/R:2",
            "/W:1",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
            "/NP",
        ])
        .output()?;

    let code = output.status.code().unwrap_or(16);
    if code <= 7 {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    Err(format!("exit code {code}; stdout: {stdout}; stderr: {stderr}").into())
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
        return install_linux_system_dependencies(missing);
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

fn install_linux_system_dependencies(missing: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let distro = detect_linux_distro();
    if distro == LinuxDistro::Unknown {
        return Err("could not detect a supported Linux distribution for dependency installation".into());
    }

    let packages = linux_system_packages(distro, missing);
    if packages.is_empty() {
        return Ok(());
    }

    let package_list = packages.join(" ");
    let command = match distro {
        LinuxDistro::Alpine => format!("apk add --no-cache {package_list}"),
        LinuxDistro::Debian => format!("apt-get update && apt-get install -y {package_list}"),
        LinuxDistro::Arch => format!("pacman -Sy --noconfirm --needed {package_list}"),
        LinuxDistro::Fedora | LinuxDistro::AmazonLinux | LinuxDistro::Rhel => {
            if command_available_without_path("dnf") {
                format!("dnf install -y {package_list}")
            } else {
                let yum_packages = linux_system_packages_for_yum(distro, missing).join(" ");
                format!("yum install -y {yum_packages}")
            }
        }
        LinuxDistro::Unknown => unreachable!("unknown distro is handled above"),
    };

    run_privileged_shell(&command)
}

fn linux_system_packages(distro: LinuxDistro, missing: &[String]) -> Vec<&'static str> {
    let mut packages = Vec::new();

    push_unique(&mut packages, "ca-certificates");
    push_unique(&mut packages, "gzip");
    push_unique(
        &mut packages,
        match distro {
            LinuxDistro::Debian => "xz-utils",
            _ => "xz",
        },
    );

    for command in missing {
        match command.as_str() {
            "git" => push_unique(&mut packages, "git"),
            "curl" => push_unique(
                &mut packages,
                match distro {
                    LinuxDistro::AmazonLinux | LinuxDistro::Rhel => "curl-minimal",
                    _ => "curl",
                },
            ),
            "tar" => push_unique(&mut packages, "tar"),
            "unzip" => push_unique(&mut packages, "unzip"),
            _ => {}
        }
    }

    packages
}

fn linux_system_packages_for_yum(distro: LinuxDistro, missing: &[String]) -> Vec<&'static str> {
    let mut packages = linux_system_packages(distro, missing);

    for package in &mut packages {
        if *package == "curl-minimal" {
            *package = "curl";
        }
    }

    packages
}

fn push_unique(packages: &mut Vec<&'static str>, package: &'static str) {
    if !packages.contains(&package) {
        packages.push(package);
    }
}

fn install_tree_sitter_from_system_package(path_value: &OsString) -> Result<(), Box<dyn std::error::Error>> {
    if command_available("tree-sitter", path_value) {
        return Ok(());
    }

    match detect_linux_distro() {
        LinuxDistro::Alpine => run_privileged_shell("apk add --no-cache tree-sitter-cli"),
        LinuxDistro::Arch => run_privileged_shell("pacman -Sy --noconfirm --needed tree-sitter-cli"),
        LinuxDistro::Fedora => run_privileged_shell("dnf install -y tree-sitter-cli"),
        LinuxDistro::Debian => run_privileged_shell("apt-get update && apt-get install -y tree-sitter-cli"),
        LinuxDistro::Rhel | LinuxDistro::AmazonLinux | LinuxDistro::Unknown => {
            Err("tree-sitter-cli system package is not available for this distro".into())
        }
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

    let nvim = resolve_nvim(&runtime.home, runtime.exe_dir.as_deref());
    if !command_runs(&nvim, &["--version"], Some(&runtime.path_value)) {
        install_neovim(&runtime.home)?;
    }

    ensure_managed_tools(&runtime.home, &runtime.path_value)?;
    ensure_treesitter_cache_for_current_toolchain(&runtime.data_home, &runtime.state_home)?;
    ensure_starter_config(&runtime.config_dir)?;
    ensure_portable_lazyvim_config(&runtime.config_dir)?;

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
    command.args(sanitize_compiler_args(env::args_os().skip(1)));
    command.env("PATH", build_path(home, None)?);
    clear_cargo_target_env(&mut command);

    let status = command.status()?;
    Ok(status.code().unwrap_or(1))
}

fn sanitize_compiler_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut sanitized = Vec::new();
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }

        let arg_text = arg.to_string_lossy();
        if arg_text == "-target" || arg_text == "--target" {
            skip_next = true;
            continue;
        }
        if arg_text.starts_with("--target=") {
            continue;
        }

        sanitized.push(arg);
    }

    sanitized
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
    let (version, asset) = fd_release_asset()?;
    let url = format!("https://github.com/sharkdp/fd/releases/download/v{version}/{asset}");
    install_single_binary_from_archive(home, "fd", &url, &asset, if cfg!(windows) { "fd.exe" } else { "fd" })
}

fn fd_release_asset() -> Result<(&'static str, String), Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok((FD_VERSION, format!("fd-v{FD_VERSION}-x86_64-pc-windows-msvc.zip")));
    }
    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok((FD_VERSION, format!("fd-v{FD_VERSION}-x86_64-unknown-linux-musl.tar.gz")));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok((
            FD_MACOS_X86_64_VERSION,
            format!("fd-v{FD_MACOS_X86_64_VERSION}-x86_64-apple-darwin.tar.gz"),
        ));
    }
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok((FD_VERSION, format!("fd-v{FD_VERSION}-aarch64-apple-darwin.tar.gz")));
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

fn install_embedded_tree_sitter(home: &Path, path_value: &OsString) -> Result<bool, Box<dyn std::error::Error>> {
    if EMBEDDED_TREE_SITTER.is_empty() {
        return Ok(false);
    }

    let destination = tree_sitter_executable_path(home);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&destination, EMBEDDED_TREE_SITTER)?;
    make_executable(&destination)?;

    if command_runs(&destination, &["--version"], Some(path_value)) {
        return Ok(true);
    }

    let _ = fs::remove_file(&destination);
    Ok(false)
}

fn install_system_tree_sitter_wrapper(home: &Path, path_value: &OsString) -> Result<bool, Box<dyn std::error::Error>> {
    if !cfg!(target_os = "linux") {
        return Ok(false);
    }

    if install_tree_sitter_from_system_package(path_value).is_err() {
        return Ok(false);
    }

    let Some(system_tree_sitter) = resolve_system_command("tree-sitter", path_value) else {
        return Ok(false);
    };

    let destination = tree_sitter_executable_path(home);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(&system_tree_sitter, &destination)?;
    make_executable(&destination)?;

    Ok(command_runs(&destination, &["--version"], Some(path_value)))
}

fn resolve_system_command(command_name: &str, path_value: &OsString) -> Option<PathBuf> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command_name}"))
        .env("PATH", path_value)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn install_tree_sitter(home: &Path, path_value: &OsString) -> Result<(), Box<dyn std::error::Error>> {
    let destination = tree_sitter_executable_path(home);

    if install_embedded_tree_sitter(home, path_value)? {
        return Ok(());
    }

    if install_system_tree_sitter_wrapper(home, path_value)? {
        return Ok(());
    }

    let downloads_dir = home.join("downloads");
    let temp_dir = home.join(".tree-sitter-install");
    fs::create_dir_all(&downloads_dir)?;

    let mut errors = Vec::new();
    for (version, asset) in tree_sitter_release_assets()? {
        let url = format!("https://github.com/tree-sitter/tree-sitter/releases/download/v{version}/{asset}");
        let archive_path = downloads_dir.join(&asset);

        println!("tree-sitter CLI was not found. Downloading tree-sitter {version} from {url}");
        match download_file(&url, &archive_path)
            .and_then(|_| extract_tree_sitter_archive(&archive_path, &temp_dir, &destination))
            .and_then(|_| make_executable(&destination).map_err(Into::into))
        {
            Ok(()) if command_runs(&destination, &["--version"], Some(path_value)) => return Ok(()),
            Ok(()) => {
                let _ = fs::remove_file(&destination);
                errors.push(format!("{asset} was installed but could not be executed"));
            }
            Err(error) => {
                let _ = fs::remove_file(&destination);
                errors.push(format!("{asset}: {error}"));
            }
        }
    }

    Err(format!("failed to install a working tree-sitter CLI: {}", errors.join("; ")).into())
}

fn tree_sitter_release_assets() -> Result<Vec<(&'static str, String)>, Box<dyn std::error::Error>> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        return Ok(vec![(TREE_SITTER_VERSION, "tree-sitter-cli-windows-x64.zip".to_string())]);
    }

    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok(vec![
            (TREE_SITTER_VERSION, "tree-sitter-cli-linux-x64.zip".to_string()),
            ("0.26.7", "tree-sitter-linux-x64.gz".to_string()),
            ("0.25.10", "tree-sitter-linux-x64.gz".to_string()),
            ("0.24.7", "tree-sitter-linux-x64.gz".to_string()),
        ]);
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        return Ok(vec![(TREE_SITTER_VERSION, "tree-sitter-cli-macos-x64.zip".to_string())]);
    }

    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return Ok(vec![(TREE_SITTER_VERSION, "tree-sitter-cli-macos-arm64.zip".to_string())]);
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

    if archive_path.extension().and_then(|value| value.to_str()) == Some("gz") {
        extract_gzip_executable(archive_path, destination)?;
    } else if cfg!(windows) {
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

fn extract_gzip_executable(archive_path: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = Command::new("gzip")
        .arg("-dc")
        .arg(archive_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("failed to extract {} with gzip: {}", archive_path.display(), stderr.trim()).into());
    }

    fs::write(destination, output.stdout)?;
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

fn install_neovim_from_system_package(path_value: &OsString) -> Result<bool, Box<dyn std::error::Error>> {
    if !cfg!(target_os = "linux") || detect_linux_distro() != LinuxDistro::Alpine {
        return Ok(false);
    }

    if !command_available("nvim", path_value) {
        run_privileged_shell("apk add --no-cache neovim")?;
    }

    Ok(command_runs(Path::new("nvim"), &["--version"], Some(path_value)))
}

fn install_neovim(home: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path_value = build_path(home, None)?;
    if install_neovim_from_system_package(&path_value)? {
        return Ok(());
    }

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

fn clear_cargo_target_env(command: &mut Command) {
    // tree-sitter's parser builder can pass Rust target triples such as
    // x86_64-unknown-linux-musl or x86_64-pc-windows-msvc to CC. Zig uses its
    // own target query format, so the portable compiler wrapper strips target
    // flags and this removes the environment variables that commonly introduce
    // those Rust triples in CI and cross-compiled binaries.
    for name in [
        "TARGET",
        "HOST",
        "BUILD_TARGET",
        "BUILD_HOST",
        "CARGO_BUILD_TARGET",
        "CARGO_CFG_TARGET_ARCH",
        "CARGO_CFG_TARGET_ENV",
        "CARGO_CFG_TARGET_FAMILY",
        "CARGO_CFG_TARGET_OS",
        "CARGO_CFG_TARGET_VENDOR",
    ] {
        command.env_remove(name);
    }
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
        .env("CFLAGS", "-O2")
        .env("CXXFLAGS", "-O2")
        .env("CC_KNOWN_WRAPPER_CUSTOM", "cc")
        .env("TREE_SITTER_CLI", tree_sitter_executable_path(&runtime.home));

    clear_cargo_target_env(&mut command);
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
    let output = base_command(runtime)
        .arg("--headless")
        .arg(lazy_command)
        .arg("+qa")
        .output()?;

    print_process_output(&output.stdout, &output.stderr);

    if lazy_output_has_errors(&output.stdout) || lazy_output_has_errors(&output.stderr) {
        return Err(format!("Lazy {command_name} reported plugin install errors").into());
    }

    exit(output.status.code().unwrap_or(1));
}

fn print_process_output(stdout: &[u8], stderr: &[u8]) {
    print!("{}", String::from_utf8_lossy(stdout));
    eprint!("{}", String::from_utf8_lossy(stderr));
}

fn lazy_output_has_errors(output: &[u8]) -> bool {
    let text = String::from_utf8_lossy(output).to_lowercase();
    text.contains("nvim-treesitter/install/") && (text.contains(" error:") || text.contains("error during") || text.contains("failed to compile parser"))
}

fn doctor(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    println!("lazyvim {}", env!("CARGO_PKG_VERSION"));
    println!();
    print_locations(runtime)?;
    println!();

    let mut failures = 0;

    for check in [
        check_command("nvim", &runtime.nvim, &["--version"], true, Some(&runtime.path_value))?,
        check_command("zig", Path::new("zig"), &["version"], true, Some(&runtime.path_value))?,
        check_command("cc", &compiler_wrapper_path(&runtime.home), &["--version"], true, Some(&runtime.path_value))?,
        check_command("tree-sitter", Path::new("tree-sitter"), &["--version"], true, Some(&runtime.path_value))?,
        check_command("git", Path::new("git"), &["--version"], true, Some(&runtime.path_value))?,
        check_command("curl", Path::new("curl"), &["--version"], true, Some(&runtime.path_value))?,
        check_command("rg", Path::new("rg"), &["--version"], true, Some(&runtime.path_value))?,
        check_command("fd", Path::new("fd"), &["--version"], true, Some(&runtime.path_value))?,
        check_command("lazygit", Path::new("lazygit"), &["--version"], true, Some(&runtime.path_value))?,
    ] {
        if !check {
            failures += 1;
        }
    }

    if failures > 0 {
        return Err(format!("doctor found {failures} failing required check(s)").into());
    }

    Ok(())
}

fn print_locations(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    println!("home:        {}", runtime.home.display());
    println!("config:      {}", runtime.config_dir.display());
    println!("data:        {}", runtime.data_home.join(APP_NAME).display());
    println!("state:       {}", runtime.state_home.join(APP_NAME).display());
    println!("cache:       {}", runtime.cache_home.join(APP_NAME).display());
    println!("nvim:        {}", runtime.nvim.display());

    if let Ok(Some(remembered)) = read_remembered_home(runtime.exe_dir.as_deref()) {
        println!("remembered:  {}", remembered.display());
    }

    if let Some(exe_dir) = &runtime.exe_dir {
        println!("launcher:    {}", exe_dir.display());
    }

    Ok(())
}

fn check_command(
    label: &str,
    command_path: &Path,
    args: &[&str],
    required: bool,
    path_value: Option<&OsString>,
) -> Result<bool, Box<dyn std::error::Error>> {
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
            Ok(true)
        }
        Ok(output) => {
            let message = String::from_utf8_lossy(&output.stderr);
            let level = if required { "fail" } else { "warn" };
            println!("[{level}] {label}: exited with {} {}", output.status, message.trim());
            Ok(!required)
        }
        Err(error) => {
            let level = if required { "fail" } else { "warn" };
            println!("[{level}] {label}: {error}");
            Ok(!required)
        }
    }
}

fn print_help() {
    println!("lazyvim {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage:");
    println!("  lazyvim [--home <path>|--portable-home|--user-home] [nvim args...]");
    println!("  lazyvim [--home <path>|--portable-home|--user-home] <command>");
    println!();
    println!("Options:");
    println!("  --home <path>       Use and remember a custom portable home");
    println!("  --portable-home     Store and remember .lazyvim next to the launcher executable");
    println!("  --user-home         Use and remember ~/.lazyvim");
    println!("  --move-home         Move the remembered home to the selected home without prompting");
    println!("  --new-home          Start a new selected home and keep the old one without prompting");
    println!("  --delete-old-home   Delete the old remembered home and use the selected home without prompting");
    println!("  --keep-home         Keep using the remembered home without prompting");
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
    println!("  LAZYVIM_HOME                Select a home; use 'portable' or 'user'");
    println!("  LAZYVIM_NVIM                Use a specific nvim executable");
    println!("  LAZYVIM_STARTER_REPOSITORY  Override the LazyVim starter repository");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os_args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(|arg| OsString::from(*arg)).collect()
    }

    fn string_args(args: Vec<OsString>) -> Vec<String> {
        args.into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn sanitize_compiler_args_removes_joined_target_flag() {
        let args = sanitize_compiler_args(os_args(&[
            "-O2",
            "--target=x86_64-unknown-linux-musl",
            "parser.c",
        ]));

        assert_eq!(string_args(args), vec![String::from("-O2"), String::from("parser.c")]);
    }

    #[test]
    fn sanitize_compiler_args_removes_split_target_flag() {
        let args = sanitize_compiler_args(os_args(&[
            "-O2",
            "-target",
            "x86_64-pc-windows-msvc",
            "parser.c",
        ]));

        assert_eq!(string_args(args), vec![String::from("-O2"), String::from("parser.c")]);
    }

    #[test]
    fn sanitize_compiler_args_removes_split_double_dash_target_flag() {
        let args = sanitize_compiler_args(os_args(&[
            "-O2",
            "--target",
            "x86_64-unknown-linux-gnu",
            "parser.c",
        ]));

        assert_eq!(string_args(args), vec![String::from("-O2"), String::from("parser.c")]);
    }

    #[test]
    fn linux_system_packages_does_not_install_curl_when_curl_is_not_missing_on_rhel() {
        let missing = vec![String::from("git"), String::from("unzip")];
        let packages = linux_system_packages(LinuxDistro::Rhel, &missing);

        assert!(packages.contains(&"git"));
        assert!(packages.contains(&"unzip"));
        assert!(!packages.contains(&"curl"));
        assert!(!packages.contains(&"curl-minimal"));
    }

    #[test]
    fn linux_system_packages_uses_curl_minimal_on_amazon_linux_when_curl_is_missing() {
        let missing = vec![String::from("curl")];
        let packages = linux_system_packages(LinuxDistro::AmazonLinux, &missing);

        assert!(packages.contains(&"curl-minimal"));
        assert!(!packages.contains(&"curl"));
    }

}
