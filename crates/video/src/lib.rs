mod frame;

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use rusty_h264_common::YuvFrame;
use rusty_h264_decoder::Decoder;
use symphonia::core::codecs::video::well_known::extra_data::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG;
use symphonia::core::codecs::video::well_known::CODEC_ID_H264;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase};

pub use frame::{yuv_to_rgba, RgbaFrame};

const LOOKAHEAD_SECONDS: f64 = 0.5;
const BACKWARD_RETAIN_SECONDS: f64 = 0.5;
const FORWARD_JUMP_SECONDS: f64 = 1.0;
const MAX_WINDOW_FRAMES: usize = 32;
const MAX_WINDOW_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("probe failed: {0}")]
    Probe(#[from] SymphoniaError),
    #[error("no video track")]
    NoVideoTrack,
    #[error("unsupported video: {0}")]
    Unsupported(String),
}

pub struct Metadata {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
}

struct DecodedFrame {
    id: u64,
    time: f64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

struct State {
    playhead: f64,
    pending_seek: Option<f64>,
    seeking: bool,
    started: bool,
    shutdown: bool,
    eof: bool,
    error: Option<String>,
    next_frame: u64,
    window_bytes: usize,
    window: VecDeque<DecodedFrame>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

enum Job {
    Decode,
    Seek(f64),
}

struct Stream {
    reader: Box<dyn FormatReader>,
    decoder: Decoder,
    track_id: u32,
    time_base: TimeBase,
    length_size: usize,
    headers: Vec<u8>,
    width: u32,
    height: u32,
    eof: bool,
    positioned: bool,
}

impl Stream {
    fn open(bytes: &Arc<[u8]>) -> Result<(Self, Metadata), VideoError> {
        let (track_id, time_base, duration, width, height, length_size, headers, reader) = {
            let reader = probe(bytes)?;
            let track = reader
                .first_track(TrackType::Video)
                .ok_or(VideoError::NoVideoTrack)?;
            let track_id = track.id;
            let time_base = track.time_base.ok_or_else(|| {
                VideoError::Unsupported("video track has no timebase".to_string())
            })?;
            let duration = track
                .duration
                .map(|duration| seconds(time_base.calc_duration_saturating(duration)))
                .unwrap_or(0.0);
            let params = track
                .codec_params
                .as_ref()
                .and_then(|params| params.video())
                .ok_or_else(|| VideoError::Unsupported("missing codec parameters".to_string()))?;
            if params.codec != CODEC_ID_H264 {
                return Err(VideoError::Unsupported(
                    "only H.264 video is supported".to_string(),
                ));
            }
            let extra = params
                .extra_data
                .iter()
                .find(|extra| extra.id == VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG)
                .ok_or_else(|| VideoError::Unsupported("missing avcC configuration".to_string()))?;
            let length_size = nal_length_size(extra.data.as_ref()).ok_or_else(|| {
                VideoError::Unsupported("malformed avcC configuration".to_string())
            })?;
            let headers = avc_headers(extra.data.as_ref()).ok_or_else(|| {
                VideoError::Unsupported("malformed avcC configuration".to_string())
            })?;
            (
                track_id,
                time_base,
                duration,
                params.width.unwrap_or(0) as u32,
                params.height.unwrap_or(0) as u32,
                length_size,
                headers,
                reader,
            )
        };
        let mut decoder = Decoder::new();
        decode_headers(&mut decoder, &headers)?;
        let metadata = Metadata {
            width,
            height,
            duration,
        };
        let stream = Self {
            reader,
            decoder,
            track_id,
            time_base,
            length_size,
            headers,
            width,
            height,
            eof: false,
            positioned: false,
        };
        Ok((stream, metadata))
    }

    fn seek(&mut self, bytes: &Arc<[u8]>, time: f64) -> Result<(), VideoError> {
        if !self.positioned && time <= 0.0 {
            self.positioned = true;
            return Ok(());
        }
        self.reader = probe(bytes)?;
        if time > 0.0 {
            if let Some(position) = Time::try_from_secs_f64(time) {
                self.reader.seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time: position,
                        track_id: Some(self.track_id),
                    },
                )?;
            }
        }
        self.decoder = Decoder::new();
        decode_headers(&mut self.decoder, &self.headers.clone())?;
        self.eof = false;
        self.positioned = true;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<(f64, YuvFrame)>, VideoError> {
        loop {
            if self.eof {
                return Ok(None);
            }
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    self.eof = true;
                    return Ok(None);
                }
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.eof = true;
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let time = seconds(self.time_base.calc_time_saturating(packet.pts));
            self.positioned = true;
            let annex_b = sample_to_annex_b(packet.data.as_ref(), self.length_size);
            if let Ok(Some(frame)) = self.decoder.decode(&annex_b) {
                if self.width == 0 || self.height == 0 {
                    self.width = frame.width as u32;
                    self.height = frame.height as u32;
                }
                return Ok(Some((time, frame)));
            }
        }
    }
}

pub struct Video {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    width: u32,
    height: u32,
    duration: f64,
    current_frame: Option<u64>,
    current: RgbaFrame,
}

impl Video {
    pub fn open(bytes: Arc<[u8]>) -> Result<Self, VideoError> {
        let (stream, metadata) = Stream::open(&bytes)?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                playhead: 0.0,
                pending_seek: None,
                seeking: false,
                started: false,
                shutdown: false,
                eof: false,
                error: None,
                next_frame: 0,
                window_bytes: 0,
                window: VecDeque::new(),
            }),
            wake: Condvar::new(),
        });
        let worker = std::thread::Builder::new()
            .name("kaplay-video-decode".to_string())
            .spawn({
                let shared = Arc::clone(&shared);
                move || decode_worker(stream, &bytes, &shared)
            })
            .ok();
        Ok(Self {
            shared,
            worker,
            width: metadata.width,
            height: metadata.height,
            duration: metadata.duration,
            current_frame: None,
            current: RgbaFrame::new(metadata.width.max(1), metadata.height.max(1)),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn frame_ready(&mut self, seconds: f64) -> bool {
        let mut time = seconds.max(0.0);
        if self.duration > 0.0 {
            time = time.min(self.duration);
        }
        let mut state = lock(&self.shared);
        state.playhead = time;
        let outside = match (state.window.front(), state.window.back()) {
            (Some(first), Some(last)) => {
                time < first.time || time > last.time + FORWARD_JUMP_SECONDS
            }
            _ => !state.started,
        };
        if outside && state.pending_seek.is_none() {
            state.pending_seek = Some(time);
            state.seeking = true;
        }
        self.shared.wake.notify_all();
        if state.seeking {
            return false;
        }
        state
            .window
            .back()
            .is_some_and(|back| back.time >= time - 1e-6 || state.eof)
    }

    pub fn frame_at(&mut self, seconds: f64) -> &RgbaFrame {
        let mut time = seconds.max(0.0);
        if self.duration > 0.0 {
            time = time.min(self.duration);
        }
        {
            let mut state = lock(&self.shared);
            state.playhead = time;
            while state
                .window
                .front()
                .is_some_and(|frame| frame.time < time - BACKWARD_RETAIN_SECONDS)
            {
                if let Some(frame) = state.window.pop_front() {
                    state.window_bytes = state.window_bytes.saturating_sub(frame.pixels.len());
                }
            }
            let outside = match (state.window.front(), state.window.back()) {
                (Some(first), Some(last)) => {
                    time < first.time || time > last.time + FORWARD_JUMP_SECONDS
                }
                _ => !state.started,
            };
            if outside && state.pending_seek.is_none() {
                state.pending_seek = Some(time);
                state.seeking = true;
            }
            self.shared.wake.notify_all();
            let picked = (!state.seeking)
                .then(|| {
                    state
                        .window
                        .iter()
                        .rev()
                        .find(|frame| frame.time <= time + 1e-6)
                        .or_else(|| state.window.front())
                })
                .flatten();
            if let Some(frame) = picked {
                if self.current_frame != Some(frame.id) {
                    self.current.width = frame.width;
                    self.current.height = frame.height;
                    self.current.pixels.clear();
                    self.current.pixels.extend_from_slice(&frame.pixels);
                    self.current_frame = Some(frame.id);
                }
            }
        }
        &self.current
    }

    pub fn take_error(&mut self) -> Option<String> {
        lock(&self.shared).error.take()
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.shared);
            state.shutdown = true;
        }
        self.shared.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn decode_worker(mut stream: Stream, bytes: &Arc<[u8]>, shared: &Arc<Shared>) {
    loop {
        let job = {
            let mut state = lock(shared);
            loop {
                if state.shutdown {
                    return;
                }
                if let Some(target) = state.pending_seek.take() {
                    break Job::Seek(target);
                }
                let behind = state
                    .window
                    .back()
                    .map(|frame| frame.time)
                    .unwrap_or(f64::MIN);
                let room =
                    state.window.len() < MAX_WINDOW_FRAMES && state.window_bytes < MAX_WINDOW_BYTES;
                if !stream.eof && room && behind < state.playhead + LOOKAHEAD_SECONDS {
                    break Job::Decode;
                }
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        match job {
            Job::Seek(target) => {
                let result = stream.seek(bytes, target);
                let mut state = lock(shared);
                state.window.clear();
                state.window_bytes = 0;
                state.started = true;
                state.seeking = false;
                state.eof = false;
                if let Err(error) = result {
                    state.error = Some(error.to_string());
                    state.eof = true;
                }
            }
            Job::Decode => match stream.next() {
                Ok(Some((time, frame))) => {
                    let pixels = yuv_to_rgba(&frame).pixels;
                    let mut state = lock(shared);
                    let id = state.next_frame;
                    state.next_frame += 1;
                    state.window_bytes += pixels.len();
                    state.window.push_back(DecodedFrame {
                        id,
                        time,
                        width: frame.width as u32,
                        height: frame.height as u32,
                        pixels,
                    });
                    state.started = true;
                }
                Ok(None) => {
                    let mut state = lock(shared);
                    state.eof = true;
                    state.started = true;
                }
                Err(error) => {
                    let mut state = lock(shared);
                    state.error = Some(error.to_string());
                    state.eof = true;
                    state.started = true;
                }
            },
        }
    }
}

fn probe(bytes: &Arc<[u8]>) -> Result<Box<dyn FormatReader>, VideoError> {
    let source = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(Arc::clone(bytes))),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("mp4");
    Ok(symphonia::default::get_probe().probe(
        &hint,
        source,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?)
}

fn seconds(time: Time) -> f64 {
    time.as_nanos() as f64 / 1e9
}

fn decode_headers(decoder: &mut Decoder, headers: &[u8]) -> Result<(), VideoError> {
    decoder
        .decode(headers)
        .map(|_| ())
        .map_err(|error| VideoError::Unsupported(format!("invalid H.264 parameter sets: {error}")))
}

fn nal_length_size(avcc: &[u8]) -> Option<usize> {
    (avcc.len() >= 5).then(|| (avcc[4] & 3) as usize + 1)
}

fn avc_headers(avcc: &[u8]) -> Option<Vec<u8>> {
    if avcc.len() < 6 {
        return None;
    }
    let mut out = Vec::new();
    let mut index = 5;
    let sps = (avcc[index] & 0x1f) as usize;
    index += 1;
    for _ in 0..sps {
        let length = read_u16(avcc, index)?;
        index += 2;
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(avcc.get(index..index + length)?);
        index += length;
    }
    let pps = *avcc.get(index)? as usize;
    index += 1;
    for _ in 0..pps {
        let length = read_u16(avcc, index)?;
        index += 2;
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(avcc.get(index..index + length)?);
        index += length;
    }
    Some(out)
}

fn read_u16(bytes: &[u8], index: usize) -> Option<usize> {
    let first = *bytes.get(index)? as usize;
    let second = *bytes.get(index + 1)? as usize;
    Some((first << 8) | second)
}

fn sample_to_annex_b(sample: &[u8], length_size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(sample.len());
    let mut index = 0;
    while index + length_size <= sample.len() {
        let mut length = 0usize;
        for byte in &sample[index..index + length_size] {
            length = (length << 8) | *byte as usize;
        }
        index += length_size;
        if length == 0 || index + length > sample.len() {
            break;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&sample[index..index + length]);
        index += length;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_length_prefixed_samples() {
        let sample = [0, 0, 0, 2, 0x65, 0x88, 0, 0, 0, 1, 0x41];
        let annex_b = sample_to_annex_b(&sample, 4);
        assert_eq!(annex_b, [0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x41]);
    }

    #[test]
    fn ignores_truncated_samples() {
        let sample = [0, 0, 0, 8, 0x65, 0x88];
        assert!(sample_to_annex_b(&sample, 4).is_empty());
    }

    #[test]
    fn reads_avcc_headers() {
        let avcc = [
            1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 0x67, 1, 2, 3, 1, 0, 3, 0x68, 4, 5, 6,
        ];
        let headers = avc_headers(&avcc).unwrap();
        assert_eq!(headers, [0, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 0, 1, 0x68, 4, 5]);
        assert_eq!(nal_length_size(&avcc), Some(4));
    }

    #[test]
    fn rejects_short_avcc() {
        assert_eq!(nal_length_size(&[1, 2, 3]), None);
        assert!(avc_headers(&[1, 2, 3]).is_none());
    }
}
