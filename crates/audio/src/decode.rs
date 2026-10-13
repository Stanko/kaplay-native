use opus_pure::{OggOpusReader, Trim, MAX_PACKET_SAMPLES};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::TrackType;
use symphonia::core::io::MediaSourceStream;

use crate::buffer::AudioBuffer;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("unsupported audio: {0}")]
    Unsupported(String),
    #[error("opus decode failed: {0}")]
    Opus(String),
    #[error("audio decode failed: {0}")]
    Symphonia(String),
}

pub fn decode(bytes: &[u8]) -> Result<AudioBuffer, DecodeError> {
    if is_ogg(bytes) && contains(bytes, b"OpusHead") {
        return decode_opus(bytes);
    }
    decode_symphonia(bytes)
}

pub fn output_sample_rate() -> u32 {
    48000
}

fn is_ogg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[..4] == b"OggS"
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.len() >= needle.len() && bytes.windows(needle.len()).any(|window| window == needle)
}

fn decode_opus(bytes: &[u8]) -> Result<AudioBuffer, DecodeError> {
    let mut reader = OggOpusReader::new(std::io::Cursor::new(bytes)).map_err(opus_error)?;
    let channels = reader.head().channel_count as usize;
    let rate = output_sample_rate();
    let mut decoder = reader.head().decoder(rate as i32).map_err(opus_error)?;
    let mut trim = Trim::new(reader.head(), rate as i32, channels).map_err(opus_error)?;
    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
    let mut samples = Vec::new();
    for packet in reader.packets() {
        let packet = packet.map_err(opus_error)?;
        let decoded = decoder
            .decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)
            .map_err(opus_error)?;
        samples.extend_from_slice(trim.keep(&packet, &block[..decoded * channels]));
    }
    if samples.is_empty() {
        return Err(DecodeError::Opus("stream contained no audio".to_string()));
    }
    Ok(AudioBuffer::new(rate, channels as u16, samples))
}

fn opus_error(error: opus_pure::Error) -> DecodeError {
    DecodeError::Opus(error.to_string())
}

fn decode_symphonia(bytes: &[u8]) -> Result<AudioBuffer, DecodeError> {
    let source = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut format = symphonia::default::get_probe()
        .probe(&Hint::new(), source, Default::default(), Default::default())
        .map_err(symphonia_error)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| DecodeError::Unsupported("no audio track".to_string()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| DecodeError::Unsupported("missing codec parameters".to_string()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(symphonia_error)?;
    let mut samples = Vec::new();
    let mut sample_rate = params.sample_rate.unwrap_or(output_sample_rate());
    let mut channels = params
        .channels
        .as_ref()
        .map(|channels| channels.count())
        .unwrap_or(1)
        .max(1);
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(symphonia_error(error)),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                let spec = audio.spec();
                sample_rate = spec.rate();
                channels = spec.channels().count().max(1);
                let mut interleaved = vec![0.0f32; audio.samples_interleaved()];
                audio.copy_to_slice_interleaved(&mut interleaved);
                samples.extend_from_slice(&interleaved);
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(symphonia_error(error)),
        }
    }
    if samples.is_empty() {
        return Err(DecodeError::Unsupported(
            "stream contained no audio".to_string(),
        ));
    }
    Ok(AudioBuffer::new(sample_rate, channels as u16, samples))
}

fn symphonia_error(error: SymphoniaError) -> DecodeError {
    DecodeError::Symphonia(error.to_string())
}
