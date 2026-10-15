mod frame;

use std::io::Cursor;
use std::sync::Arc;

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

const LOOKAHEAD_SECONDS: f64 = 0.25;
const BACKWARD_RETAIN_SECONDS: f64 = 0.1;
const FORWARD_JUMP_SECONDS: f64 = 2.0;

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("probe failed: {0}")]
    Probe(#[from] SymphoniaError),
    #[error("no video track")]
    NoVideoTrack,
    #[error("unsupported video: {0}")]
    Unsupported(String),
}

struct DecodedFrame {
    time: f64,
    frame: YuvFrame,
}

pub struct Video {
    bytes: Arc<[u8]>,
    reader: Box<dyn FormatReader>,
    decoder: Decoder,
    track_id: u32,
    time_base: TimeBase,
    length_size: usize,
    headers: Vec<u8>,
    width: u32,
    height: u32,
    duration: f64,
    window: Vec<DecodedFrame>,
    eof: bool,
    current: RgbaFrame,
}

impl Video {
    pub fn open(bytes: Vec<u8>) -> Result<Self, VideoError> {
        let bytes = Arc::<[u8]>::from(bytes);
        let reader = probe(bytes.clone())?;
        let (track_id, time_base, duration, width, height, length_size, headers) = {
            let track = reader
                .first_track(TrackType::Video)
                .ok_or(VideoError::NoVideoTrack)?;
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
            let avcc = extra.data.as_ref();
            let length_size = nal_length_size(avcc).ok_or_else(|| {
                VideoError::Unsupported("malformed avcC configuration".to_string())
            })?;
            let headers = avc_headers(avcc).ok_or_else(|| {
                VideoError::Unsupported("malformed avcC configuration".to_string())
            })?;
            (
                track.id,
                time_base,
                duration,
                params.width.unwrap_or(0) as u32,
                params.height.unwrap_or(0) as u32,
                length_size,
                headers,
            )
        };
        let mut video = Self {
            bytes,
            reader,
            decoder: Decoder::new(),
            track_id,
            time_base,
            length_size,
            headers,
            width,
            height,
            duration,
            window: Vec::new(),
            eof: false,
            current: RgbaFrame::new(width.max(1), height.max(1)),
        };
        let headers = video.headers.clone();
        decode_headers(&mut video.decoder, &headers)?;
        Ok(video)
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

    pub fn frame_at(&mut self, seconds: f64) -> Result<&RgbaFrame, VideoError> {
        let mut time = seconds.max(0.0);
        if self.duration > 0.0 {
            time = time.min(self.duration);
        }
        let newest = self.window.last().map(|frame| frame.time);
        let needs_seek = match (self.window.first().map(|frame| frame.time), newest) {
            (Some(first), Some(newest)) => time < first || time > newest + FORWARD_JUMP_SECONDS,
            _ => true,
        };
        if needs_seek {
            self.seek(time)?;
        }
        self.decode_until(time + LOOKAHEAD_SECONDS)?;
        let picked = self
            .window
            .iter()
            .rposition(|frame| frame.time <= time + 1e-6)
            .or(if self.window.is_empty() {
                None
            } else {
                Some(0)
            });
        if let Some(index) = picked {
            self.current = yuv_to_rgba(&self.window[index].frame);
        }
        self.window
            .retain(|frame| frame.time >= time - BACKWARD_RETAIN_SECONDS);
        Ok(&self.current)
    }

    fn seek(&mut self, time: f64) -> Result<(), VideoError> {
        self.reader = probe(self.bytes.clone())?;
        if let Some(position) = Time::try_from_secs_f64(time) {
            self.reader.seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: position,
                    track_id: Some(self.track_id),
                },
            )?;
        }
        self.decoder = Decoder::new();
        self.window.clear();
        self.eof = false;
        let headers = self.headers.clone();
        decode_headers(&mut self.decoder, &headers)?;
        Ok(())
    }

    fn decode_until(&mut self, horizon: f64) -> Result<(), VideoError> {
        while !self.eof
            && self
                .window
                .last()
                .map(|frame| frame.time)
                .unwrap_or(f64::MIN)
                < horizon
        {
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    self.eof = true;
                    break;
                }
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.eof = true;
                    break;
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let time = seconds(self.time_base.calc_time_saturating(packet.pts));
            let annex_b = sample_to_annex_b(packet.data.as_ref(), self.length_size);
            if let Ok(Some(frame)) = self.decoder.decode(&annex_b) {
                if self.width == 0 || self.height == 0 {
                    self.width = frame.width as u32;
                    self.height = frame.height as u32;
                }
                self.window.push(DecodedFrame { time, frame });
            }
        }
        Ok(())
    }
}

fn probe(bytes: Arc<[u8]>) -> Result<Box<dyn FormatReader>, VideoError> {
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
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
