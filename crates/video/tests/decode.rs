use kaplay_native_video::Video;

fn open() -> Video {
    Video::open(include_bytes!("fixtures/sample.mp4").to_vec()).unwrap()
}

#[test]
fn opens_an_mp4() {
    let video = open();
    assert_eq!(video.width(), 160);
    assert_eq!(video.height(), 120);
    assert!(
        (video.duration() - 1.0).abs() < 0.15,
        "duration {}",
        video.duration()
    );
}

#[test]
fn decodes_frames_at_times() {
    let mut video = open();
    let first = video.frame_at(0.0).unwrap().clone();
    assert_eq!((first.width, first.height), (160, 120));
    assert_eq!(first.pixels.len(), 160 * 120 * 4);
    assert!(first.pixels.chunks(4).all(|pixel| pixel[3] == 255));
    assert!(first
        .pixels
        .chunks(4)
        .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0));

    let later = video.frame_at(0.8).unwrap().clone();
    assert_ne!(first.pixels, later.pixels);
}

#[test]
fn replays_the_same_frame_after_seeking_back() {
    let mut video = open();
    let first = video.frame_at(0.0).unwrap().clone();
    let _ = video.frame_at(0.8).unwrap();
    let again = video.frame_at(0.0).unwrap().clone();
    assert_eq!(first.pixels, again.pixels);
}

#[test]
fn plays_forward_frame_by_frame() {
    let mut video = open();
    let mut previous = video.frame_at(0.0).unwrap().clone();
    let mut time = 0.1;
    while time <= 1.0 {
        let frame = video.frame_at(time).unwrap().clone();
        assert_eq!(frame.pixels.len(), previous.pixels.len());
        time += 0.1;
        previous = frame;
    }
    let end = video.frame_at(1.0).unwrap().clone();
    assert_eq!(end.pixels.len(), 160 * 120 * 4);
}

#[test]
fn rejects_non_video_bytes() {
    assert!(Video::open(b"not a video".to_vec()).is_err());
}
