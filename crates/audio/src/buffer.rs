#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: usize,
    samples: Vec<f32>,
}

impl AudioBuffer {
    pub fn new(sample_rate: u32, channels: u16, samples: Vec<f32>) -> Self {
        let channels = channels.max(1);
        let frames = samples.len() / channels as usize;
        Self {
            sample_rate: sample_rate.max(1),
            channels,
            frames,
            samples,
        }
    }

    pub fn silence(sample_rate: u32, channels: u16, frames: usize) -> Self {
        let channels = channels.max(1);
        Self {
            sample_rate: sample_rate.max(1),
            channels,
            frames,
            samples: vec![0.0; frames * channels as usize],
        }
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn duration(&self) -> f64 {
        self.frames as f64 / self.sample_rate as f64
    }

    pub fn channel(&self, index: usize) -> Option<Vec<f32>> {
        if index >= self.channels as usize {
            return None;
        }
        let mut out = Vec::with_capacity(self.frames);
        for frame in 0..self.frames {
            out.push(self.samples[frame * self.channels as usize + index]);
        }
        Some(out)
    }

    pub fn read_frame(&self, channel: usize, frame: usize) -> f32 {
        let channel = channel.min(self.channels as usize - 1);
        self.samples
            .get(frame * self.channels as usize + channel)
            .copied()
            .unwrap_or(0.0)
    }
}
