mod app;
mod bridge;
mod config;
mod keys;
mod screenshots;
mod state;

use std::path::PathBuf;

use clap::Parser;
use kaplay_native_pak::AssetSource;

use crate::app::HostApp;
use crate::config::HostConfig;

#[derive(Debug, Parser)]
#[command(name = "kaplay-native-host", version, about = "KAPLAY native runtime")]
struct Args {
    #[arg(long, help = "Game script evaluated after the runtime shim")]
    script: Option<PathBuf>,
    #[arg(long, help = "Preload script evaluated before the game script", action = clap::ArgAction::Append)]
    preload: Vec<PathBuf>,
    #[arg(long, help = "Read assets from a .pak zip archive")]
    pak: Option<PathBuf>,
    #[arg(long, help = "Read assets from a directory")]
    dir: Option<PathBuf>,
    #[arg(long, help = "Steam application id")]
    appid: Option<u32>,
    #[arg(long, default_value = "960x540", help = "Initial window size")]
    window: String,
    #[arg(long)]
    fullscreen: bool,
    #[arg(long, help = "Run without showing the window")]
    headless: bool,
    #[arg(long, help = "Exit after this many frames")]
    frames: Option<u32>,
    #[arg(long, help = "Write a PNG of the last frame before exiting")]
    screenshot: Option<PathBuf>,
    #[arg(long, help = "Directory for the localStorage save file")]
    save_dir: Option<PathBuf>,
    #[arg(long, help = "Allow http(s) requests (disabled by default)")]
    allow_network: bool,
    #[arg(long, help = "Disable Steam integration")]
    no_steam: bool,
    #[arg(long, help = "Prefer the X11 backend on Linux")]
    x11: bool,
    #[arg(long, default_value = "KAPLAY", help = "Window title")]
    title: String,
}

fn parse_size(value: &str) -> (u32, u32) {
    let mut parts = value.split('x');
    let width = parts
        .next()
        .and_then(|part| part.trim().parse().ok())
        .unwrap_or(960);
    let height = parts
        .next()
        .and_then(|part| part.trim().parse().ok())
        .unwrap_or(540);
    (width, height)
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    if args.x11 && std::env::var_os("DISPLAY").is_some() {
        std::env::set_var("WINIT_UNIX_BACKEND", "x11");
    }

    let source = match (&args.pak, &args.dir) {
        (Some(pak), _) => AssetSource::from_pak(pak)?,
        (None, Some(dir)) => AssetSource::from_dir(dir),
        (None, None) => {
            let root = args
                .script
                .as_ref()
                .and_then(|path| path.parent())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            AssetSource::from_dir(&root)
        }
    };

    let save_dir = args.save_dir.unwrap_or_else(|| {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("kaplay-native")
    });

    let config = HostConfig {
        script: args.script,
        preload: args.preload,
        app_id: args.appid,
        window: parse_size(&args.window),
        fullscreen: args.fullscreen,
        allow_network: args.allow_network,
        steam: !args.no_steam,
        headless: args.headless,
        frames: args.frames,
        screenshot: args.screenshot,
        save_dir,
        game_name: args.title,
    };

    HostApp::new(config, std::rc::Rc::new(source))?.run()
}
