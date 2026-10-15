//! Local, silent video playback decoded with FFmpeg. Frames stay in native memory.
use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

pub struct Clip {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    timestamps: Vec<f64>,
    pixels: Vec<u8>,
}

impl Clip {
    pub fn frame(&self, time: f64) -> (usize, &[u8]) {
        let index = self
            .timestamps
            .partition_point(|timestamp| *timestamp <= time.max(0.0))
            .saturating_sub(1)
            .min(self.timestamps.len() - 1);
        let size = self.width as usize * self.height as usize * 4;
        (index, &self.pixels[index * size..(index + 1) * size])
    }
}

pub struct Instance {
    pub clip: Rc<Clip>,
    pub frame: usize,
}

#[derive(Default)]
pub struct Videos {
    cache: HashMap<String, Rc<Clip>>,
    pub instances: HashMap<u32, Instance>,
}

impl Videos {
    pub fn load(&mut self, path: &str, bytes: &[u8]) -> Result<Rc<Clip>, String> {
        if let Some(clip) = self.cache.get(path) {
            return Ok(clip.clone());
        }
        let clip = Rc::new(decode(bytes)?);
        if self
            .cache
            .values()
            .map(|clip| clip.pixels.len())
            .sum::<usize>()
            + clip.pixels.len()
            > MAX_DECODED_BYTES
        {
            self.cache.clear();
        }
        self.cache.insert(path.to_string(), clip.clone());
        Ok(clip)
    }
}

#[derive(Serialize)]
pub struct VideoInfo {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    frames: Vec<Frame>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    width: u32,
    height: u32,
    avg_frame_rate: String,
}
#[derive(Deserialize)]
struct Frame {
    best_effort_timestamp_time: Option<String>,
}
#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

struct TempVideo(std::path::PathBuf);
impl Drop for TempVideo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn available() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|binary| {
        Command::new(binary)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

fn decode(bytes: &[u8]) -> Result<Clip, String> {
    let path = std::env::temp_dir().join(format!(
        "kaplay-video-{}-{}.media",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("cannot create temporary video: {error}"))?;
    let temporary = TempVideo(path);
    std::io::Write::write_all(&mut file, bytes)
        .map_err(|error| format!("cannot write temporary video: {error}"))?;
    drop(file);
    // Only read the supplied local asset. Playlists and embedded network URLs are disabled.
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file",
            "-select_streams",
            "v:0",
            "-show_frames",
            "-show_entries",
            "stream=width,height,avg_frame_rate:frame=best_effort_timestamp_time:format=duration",
            "-of",
            "json",
        ])
        .arg(&temporary.0)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("video playback requires ffmpeg and ffprobe on PATH: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot read video metadata: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let probe: Probe = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid video metadata: {error}"))?;
    let stream = probe.streams.first().ok_or("asset has no video stream")?;
    let frame_size = (stream.width as usize)
        .checked_mul(stream.height as usize)
        .and_then(|size| size.checked_mul(4))
        .filter(|size| *size > 0 && *size <= MAX_DECODED_BYTES)
        .ok_or("video dimensions are too large")?;
    if probe.frames.is_empty() || probe.frames.len() > MAX_DECODED_BYTES / frame_size {
        return Err("video exceeds the 256 MiB decoded clip limit".into());
    }
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file",
            "-i",
        ])
        .arg(&temporary.0)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("video playback requires ffmpeg on PATH: {error}"))?;
    let mut pixels = Vec::new();
    let result = child
        .stdout
        .take()
        .ok_or("cannot read decoded video")?
        .take(MAX_DECODED_BYTES as u64 + 1)
        .read_to_end(&mut pixels);
    if result.is_err() || pixels.len() > MAX_DECODED_BYTES {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for video decoder: {error}"))?;
    result.map_err(|error| format!("cannot read decoded video: {error}"))?;
    if !status.success()
        || pixels.is_empty()
        || pixels.len() > MAX_DECODED_BYTES
        || !pixels.len().is_multiple_of(frame_size)
    {
        return Err("FFmpeg could not decode this video within the 256 MiB clip limit".into());
    }
    let fps = stream
        .avg_frame_rate
        .split_once('/')
        .and_then(|(num, den)| Some(num.parse::<f64>().ok()? / den.parse::<f64>().ok()?))
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .unwrap_or(30.0);
    let count = pixels.len() / frame_size;
    let mut timestamps: Vec<f64> = (0..count)
        .map(|index| {
            probe
                .frames
                .get(index)
                .and_then(|frame| frame.best_effort_timestamp_time.as_ref())
                .and_then(|time| time.parse().ok())
                .filter(|time: &f64| time.is_finite())
                .unwrap_or(index as f64 / fps)
        })
        .collect();
    let start = timestamps[0];
    for time in &mut timestamps {
        *time = (*time - start).max(0.0);
    }
    let duration = probe
        .format
        .and_then(|format| format.duration)
        .and_then(|duration| duration.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration > timestamps[count - 1])
        .unwrap_or(timestamps[count - 1] + 1.0 / fps);
    Ok(Clip {
        width: stream.width,
        height: stream.height,
        duration,
        timestamps,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    fn decodes_video_frames_and_timestamps() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(2, 2, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(500, 1),
                    ))
                    .unwrap();
            }
        }
        let clip = decode(&bytes).unwrap();
        assert_eq!((clip.width, clip.height), (2, 2));
        assert_eq!(clip.timestamps.len(), 2);
        assert_eq!(&clip.frame(0.0).1[..4], &[255, 0, 0, 255]);
        assert_eq!(&clip.frame(0.75).1[..4], &[0, 0, 255, 255]);
        assert!((clip.duration - 1.0).abs() < 0.01);
        assert!(decode(b"not a video").is_err());
    }

    #[test]
    fn selects_variable_rate_frames_and_clamps_seeks() {
        let clip = Clip {
            width: 1,
            height: 1,
            duration: 1.0,
            timestamps: vec![0.0, 0.1, 0.7],
            pixels: vec![1; 12],
        };
        assert_eq!(clip.frame(-1.0).0, 0);
        assert_eq!(clip.frame(0.5).0, 1);
        assert_eq!(clip.frame(0.7).0, 2);
        assert_eq!(clip.frame(99.0).0, 2);
    }
}
