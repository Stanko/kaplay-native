use kaplay_native_audio::Audio;

#[test]
#[ignore]
fn plays_a_tone_through_the_device() {
    let audio = Audio::new();
    assert!(audio.has_device(), "no audio device available");
    let frames = 48000;
    let samples = (0..frames)
        .flat_map(|frame| {
            let value = (frame as f32 / 48000.0 * 440.0 * std::f32::consts::TAU).sin() * 0.2;
            [value, value]
        })
        .collect();
    let id = audio.upload_buffer(48000, 2, samples);
    let voice = audio
        .play_voice(id, 1.0, 0.0, 1.0, false, 0.0, None)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(audio.current_time() > 0.1);
    let position = audio.voice_position(voice).unwrap();
    assert!(position > 0.05, "{position}");
    audio.stop_voice(voice);
}
