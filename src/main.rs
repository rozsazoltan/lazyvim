use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};

const APP_NAME: &str = "lazyvim";
const DEFAULT_HOME_DIR: &str = ".lazyvim";

struct StarterFile {
    path: &'static str,
    contents: &'static str,
}

const STARTER_FILES: &[StarterFile] = &[
    StarterFile {
        path: "init.lua",
        contents: include_str!("../assets/starter/init.lua"),
    },
    StarterFile {
        path: "lazyvim.json",
        contents: include_str!("../assets/starter/lazyvim.json"),
    },
    StarterFile {
        path: "lua/config/options.lua",
        contents: include_str!("../assets/starter/lua/config/options.lua"),
    },
    StarterFile {
        path: "lua/config/keymaps.lua",
        contents: include_str!("../assets/starter/lua/config/keymaps.lua"),
    },
    StarterFile {
        path: "lua/config/autocmds.lua",
        contents: include_str!("../assets/starter/lua/config/autocmds.lua"),
    },
    StarterFile {
        path: "lua/plugins/example.lua",
        contents: include_str!("../assets/starter/lua/plugins/example.lua"),
    },
];

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
            let runtime = prepare_runtime(cli.home, true)?;
            match command {
                CliCommand::Launch(args) => launch_nvim(&runtime, &args),
                CliCommand::Doctor => doctor(&runtime),
                CliCommand::Where => print_locations(&runtime),
                CliCommand::Sync => run_lazy_command(&runtime, "sync"),
                CliCommand::Restore => run_lazy_command(&runtime, "restore"),
                CliCommand::Update => run_lazy_command(&runtime, "update"),
                CliCommand::Clean => run_lazy_command(&runtime, "clean"),
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

    if bootstrap {
        ensure_starter_config(&config_dir)?;
    }

    let exe_dir = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    let nvim = resolve_nvim(&home, exe_dir.as_deref());
    let path_value = build_path(&home, exe_dir.as_deref())?;

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

fn ensure_starter_config(config_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;

    for starter in STARTER_FILES {
        let target = config_dir.join(starter.path);

        if target.exists() {
            continue;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(target, starter.contents)?;
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
    let status = base_command(runtime).args(args).status()?;
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
        println!("package:     {}", exe_dir.display());
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
    println!("  reset      Delete the portable home directory; requires --yes");
    println!("  help       Print this help");
    println!();
    println!("Environment:");
    println!("  LAZYVIM_HOME  Override ~/.lazyvim");
    println!("  LAZYVIM_NVIM  Use a specific nvim executable");
}
