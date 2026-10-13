use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::Engine;

pub fn preferred_sample_rate() -> Option<u32> {
    let device = cpal::default_host().default_output_device()?;
    device
        .default_output_config()
        .ok()
        .map(|config| config.sample_rate())
}

pub fn start(engine: Arc<Engine>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no output device".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    if supported.channels() != 2 {
        return Err(format!(
            "unsupported channel count {}",
            supported.channels()
        ));
    }
    if supported.sample_rate() != engine.sample_rate() {
        return Err(format!(
            "unsupported sample rate {}",
            supported.sample_rate()
        ));
    }
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, engine),
        cpal::SampleFormat::I16 => build::<i16>(&device, config, engine),
        cpal::SampleFormat::U16 => build::<u16>(&device, config, engine),
        format => Err(format!("unsupported sample format {format:?}")),
    }?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    engine: Arc<Engine>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut scratch = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0);
                engine.render(&mut scratch);
                for (out, sample) in data.iter_mut().zip(scratch.iter()) {
                    *out = T::from_sample(*sample);
                }
            },
            |error| log::error!("audio stream error: {error}"),
            None,
        )
        .map_err(|error| error.to_string())
}
