use kaplay_native_audio::{Audio, AudioEvent};

#[test]
fn silent_backend_advances_the_clock() {
    let audio = Audio::silent();
    assert!(!audio.has_device());
    std::thread::sleep(std::time::Duration::from_millis(120));
    let time = audio.current_time();
    assert!(time >= 0.05, "{time}");
}

#[test]
fn silent_backend_finishes_voices() {
    let audio = Audio::silent();
    let samples = vec![0.25f32; 480];
    let id = audio.upload_buffer(48000, 1, samples);
    let voice = audio
        .play_voice(id, 1.0, 0.0, 1.0, false, 0.0, None)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(120));
    assert!(audio
        .take_events()
        .iter()
        .any(|event| matches!(event, AudioEvent::VoiceEnded { voice: ended } if *ended == voice)));
}
