use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub script: Option<PathBuf>,
    pub preload: Vec<PathBuf>,
    pub app_id: Option<u32>,
    pub window: (u32, u32),
    pub fullscreen: bool,
    pub allow_network: bool,
    pub no_audio: bool,
    pub steam: bool,
    pub headless: bool,
    pub frames: Option<u32>,
    pub screenshot: Option<PathBuf>,
    pub screenshot_raw: Option<PathBuf>,
    pub save_dir: PathBuf,
    pub game_name: String,
    pub url: Option<String>,
}
