mod tune;
mod update;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pc_core::Config;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "imlec",
    version,
    about = "Particle effects that follow the cursor while you type, on any application"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Use a config file other than the default location.
    #[arg(short, long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Print the config file path and exit.
    #[arg(long)]
    print_config_path: bool,

    /// Overwrite the config file with the commented defaults.
    #[arg(long)]
    reset_config: bool,

    /// Force a display backend instead of detecting one. Linux only.
    #[arg(long, value_name = "wayland|x11")]
    backend: Option<String>,

    /// Log more detail. Repeat for debug output.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Edit settings in a live terminal UI; the running overlay picks up every change.
    Tune,
    /// Install the newest release from GitHub.
    Update {
        /// Report whether an update exists without installing it.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level)).init();

    if let Some(Command::Update { check }) = cli.command {
        return update::run(check);
    }

    let path = match cli.config.clone() {
        Some(path) => path,
        None => Config::config_path().context("resolving the config path")?,
    };

    if cli.print_config_path {
        println!("{}", path.display());
        return Ok(());
    }

    if cli.reset_config {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, pc_core::config::DEFAULT_TOML)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("wrote defaults to {}", path.display());
        return Ok(());
    }

    if let Some(Command::Tune) = cli.command {
        return tune::run(path);
    }

    let config = if cli.config.is_some() {
        Config::load_from(&path)?
    } else {
        Config::load_or_init()?
    };
    log::info!("config: {}", path.display());
    update::spawn_background_check();

    run(config, Some(path), cli.backend.as_deref())
}

#[cfg(target_os = "linux")]
fn run(config: Config, path: Option<PathBuf>, backend: Option<&str>) -> Result<()> {
    let session = match backend {
        Some("wayland") => Some(pc_linux::Session::Wayland),
        Some("x11") => Some(pc_linux::Session::X11),
        Some(other) => anyhow::bail!("unknown backend {other:?}; expected wayland or x11"),
        None => None,
    };
    pc_linux::run(config, path, session)
}

#[cfg(target_os = "windows")]
fn run(config: Config, path: Option<PathBuf>, backend: Option<&str>) -> Result<()> {
    if backend.is_some() {
        anyhow::bail!("--backend only applies on Linux");
    }
    pc_windows::run(config, path)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn run(_config: Config, _path: Option<PathBuf>, _backend: Option<&str>) -> Result<()> {
    anyhow::bail!("no backend for this platform yet; Linux and Windows are supported")
}
