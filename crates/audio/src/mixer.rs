use std::sync::Arc;

use crate::buffer::AudioBuffer;

pub struct Voice {
    pub id: u32,
    pub buffer: Arc<AudioBuffer>,
    pub position: f64,
    pub rate: f64,
    pub gain: f32,
    pub pan: f32,
    gain_left: f32,
    gain_right: f32,
    pub looping: bool,
    pub start_at: Option<u64>,
    pub stop_at: Option<u64>,
    pub playing: bool,
}

pub struct Mixer {
    sample_rate: u32,
    voices: Vec<Voice>,
    next_voice: u32,
    finished: Vec<u32>,
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate.max(1),
            voices: Vec::new(),
            next_voice: 1,
            finished: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_voice(
        &mut self,
        buffer: Arc<AudioBuffer>,
        gain: f32,
        pan: f32,
        playback_rate: f64,
        looping: bool,
        offset_seconds: f64,
        start_at: Option<u64>,
    ) -> u32 {
        let id = self.next_voice;
        self.next_voice = self.next_voice.wrapping_add(1).max(1);
        let offset = (offset_seconds * buffer.sample_rate as f64).max(0.0);
        let rate = buffer.sample_rate as f64 / self.sample_rate as f64 * playback_rate;
        let pan = pan.clamp(-1.0, 1.0);
        let (gain_left, gain_right) = pan_gains(pan);
        self.voices.push(Voice {
            id,
            position: offset,
            rate: if rate.is_finite() && rate > 0.0 {
                rate
            } else {
                1.0
            },
            buffer,
            gain: gain.max(0.0),
            pan,
            gain_left,
            gain_right,
            looping,
            start_at,
            stop_at: None,
            playing: true,
        });
        id
    }

    pub fn set_gain(&mut self, id: u32, gain: f32) {
        if let Some(voice) = self.voice(id) {
            voice.gain = gain.max(0.0);
        }
    }

    pub fn set_pan(&mut self, id: u32, pan: f32) {
        if let Some(voice) = self.voice(id) {
            let pan = pan.clamp(-1.0, 1.0);
            let (gain_left, gain_right) = pan_gains(pan);
            voice.pan = pan;
            voice.gain_left = gain_left;
            voice.gain_right = gain_right;
        }
    }

    pub fn set_rate(&mut self, id: u32, playback_rate: f64) {
        let mixer_rate = self.sample_rate as f64;
        if let Some(voice) = self.voice(id) {
            let rate = voice.buffer.sample_rate as f64 / mixer_rate * playback_rate;
            if rate.is_finite() && rate > 0.0 {
                voice.rate = rate;
            }
        }
    }

    pub fn set_loop(&mut self, id: u32, looping: bool) {
        if let Some(voice) = self.voice(id) {
            voice.looping = looping;
        }
    }

    pub fn position(&self, id: u32) -> Option<f64> {
        self.voices
            .iter()
            .find(|voice| voice.id == id)
            .map(|voice| voice.position / voice.buffer.sample_rate as f64)
    }

    pub fn duration(&self, id: u32) -> Option<f64> {
        self.voices
            .iter()
            .find(|voice| voice.id == id)
            .map(|voice| voice.buffer.duration())
    }

    pub fn stop(&mut self, id: u32) {
        if let Some(voice) = self.voice(id) {
            voice.playing = false;
            voice.stop_at = None;
        }
    }

    pub fn stop_at(&mut self, id: u32, frame: u64) {
        if let Some(voice) = self.voice(id) {
            voice.stop_at = Some(frame);
        }
    }

    fn voice(&mut self, id: u32) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|voice| voice.id == id)
    }

    pub fn take_finished(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.finished)
    }

    pub fn render(&mut self, out: &mut [f32], base_frame: u64) {
        out.fill(0.0);
        let frames = out.len() / 2;
        for frame in 0..frames {
            let absolute = base_frame + frame as u64;
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for voice in &mut self.voices {
                if !voice.playing {
                    continue;
                }
                if voice.stop_at.is_some_and(|limit| absolute >= limit) {
                    voice.playing = false;
                    continue;
                }
                if voice.start_at.is_some_and(|start| absolute < start) {
                    continue;
                }
                voice.start_at = None;

                let buffer = &voice.buffer;
                let channels = buffer.channels as usize;
                let frames_total = buffer.frames;
                if frames_total == 0 || voice.position >= frames_total as f64 {
                    if voice.looping && frames_total > 0 {
                        voice.position %= frames_total as f64;
                    } else {
                        voice.playing = false;
                        self.finished.push(voice.id);
                        continue;
                    }
                }

                let index = voice.position.floor() as usize;
                let fraction = (voice.position - index as f64) as f32;
                let next_index = if index + 1 < frames_total {
                    index + 1
                } else if voice.looping {
                    0
                } else {
                    index
                };
                let (sound_left, sound_right) = if channels == 1 {
                    let a = buffer.read_frame(0, index);
                    let b = buffer.read_frame(0, next_index);
                    let value = a + (b - a) * fraction;
                    (value, value)
                } else {
                    let a = buffer.read_frame(0, index);
                    let b = buffer.read_frame(0, next_index);
                    let c = buffer.read_frame(1, index);
                    let d = buffer.read_frame(1, next_index);
                    (a + (b - a) * fraction, c + (d - c) * fraction)
                };

                left += sound_left * voice.gain_left * voice.gain;
                right += sound_right * voice.gain_right * voice.gain;

                voice.position += voice.rate;
                if voice.position >= frames_total as f64 {
                    if voice.looping {
                        voice.position %= frames_total as f64;
                    } else {
                        voice.playing = false;
                        self.finished.push(voice.id);
                    }
                }
            }
            out[frame * 2] = left.clamp(-1.0, 1.0);
            out[frame * 2 + 1] = right.clamp(-1.0, 1.0);
        }
        self.voices.retain(|voice| voice.playing);
    }
}

fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(frames: usize) -> Arc<AudioBuffer> {
        let samples = (0..frames)
            .map(|index| (index + 1) as f32 / frames as f32)
            .collect();
        Arc::new(AudioBuffer::new(48000, 1, samples))
    }

    fn render_all(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        mixer.render(&mut out, 0);
        out
    }

    #[test]
    fn mixes_a_voice_into_both_channels() {
        let mut mixer = Mixer::new(48000);
        mixer.add_voice(buffer(100), 1.0, 0.0, 1.0, false, 0.0, None);
        let out = render_all(&mut mixer, 100);
        assert!(out.iter().all(|sample| *sample > 0.0));
        assert!(out[0] < out[out.len() - 2]);
    }

    #[test]
    fn reports_finished_voices_once() {
        let mut mixer = Mixer::new(48000);
        let id = mixer.add_voice(buffer(10), 1.0, 0.0, 1.0, false, 0.0, None);
        render_all(&mut mixer, 10);
        assert_eq!(mixer.take_finished(), vec![id]);
        assert_eq!(mixer.take_finished(), vec![]);
    }

    #[test]
    fn loops_until_stopped() {
        let mut mixer = Mixer::new(48000);
        let id = mixer.add_voice(buffer(10), 1.0, 0.0, 1.0, true, 0.0, None);
        let out = render_all(&mut mixer, 30);
        assert!(out.iter().all(|sample| sample.is_finite()));
        assert_eq!(mixer.take_finished(), vec![]);
        mixer.stop(id);
        render_all(&mut mixer, 10);
        assert_eq!(mixer.take_finished(), vec![]);
    }

    #[test]
    fn pan_attenuates_one_channel() {
        let mut mixer = Mixer::new(48000);
        mixer.add_voice(buffer(50), 1.0, -1.0, 1.0, false, 0.0, None);
        let out = render_all(&mut mixer, 50);
        for frame in out.as_chunks::<2>().0 {
            assert!(frame[0] > 0.0);
            assert!(frame[1].abs() < 1e-6);
        }
    }

    #[test]
    fn start_at_delays_playback() {
        let mut mixer = Mixer::new(48000);
        mixer.add_voice(buffer(100), 1.0, 0.0, 1.0, false, 0.0, Some(10));
        let out = render_all(&mut mixer, 20);
        assert!(out[..20].iter().all(|sample| *sample == 0.0));
        assert!(out[20..].iter().any(|sample| *sample > 0.0));
    }

    #[test]
    fn gain_scales_the_output() {
        let mut quiet = Mixer::new(48000);
        quiet.add_voice(buffer(64), 0.5, 0.0, 1.0, false, 0.0, None);
        let mut loud = Mixer::new(48000);
        loud.add_voice(buffer(64), 1.0, 0.0, 1.0, false, 0.0, None);
        let quiet_out = render_all(&mut quiet, 64);
        let loud_out = render_all(&mut loud, 64);
        for (quiet, loud) in quiet_out.iter().zip(loud_out.iter()) {
            assert!((quiet * 2.0 - loud).abs() < 1e-5);
        }
    }
}
