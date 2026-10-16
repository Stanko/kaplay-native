use std::sync::Arc;
use std::time::{Duration, Instant};

use kaplay_native_video::{RgbaFrame, Video};

fn open() -> Video {
    Video::open(Arc::from(
        include_bytes!("fixtures/sample.mp4").as_slice().to_vec(),
    ))
    .unwrap()
}

fn wait_for(video: &mut Video, time: f64) -> RgbaFrame {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if video.frame_ready(time) {
            return video.frame_at(time).clone();
        }
        assert!(
            Instant::now() < deadline,
            "frame at {time} did not decode in time"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
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
fn open_returns_before_the_first_frame_decodes() {
    let mut video = open();
    assert!(!video.frame_ready(0.0));
    let first = wait_for(&mut video, 0.0);
    assert_eq!((first.width, first.height), (160, 120));
    assert_eq!(first.pixels.len(), 160 * 120 * 4);
    assert!(first.pixels.chunks(4).all(|pixel| pixel[3] == 255));
    assert!(first
        .pixels
        .chunks(4)
        .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0));
}

#[test]
fn decodes_frames_at_times() {
    let mut video = open();
    let first = wait_for(&mut video, 0.0);
    let later = wait_for(&mut video, 0.8);
    assert_ne!(first.pixels, later.pixels);
}

#[test]
fn replays_the_same_frame_after_seeking_back() {
    let mut video = open();
    let first = wait_for(&mut video, 0.0);
    let _ = wait_for(&mut video, 0.8);
    let again = wait_for(&mut video, 0.0);
    assert_eq!(first.pixels, again.pixels);
}

#[test]
fn plays_forward_frame_by_frame() {
    let mut video = open();
    let mut previous = wait_for(&mut video, 0.0);
    let mut time = 0.1;
    while time <= 1.0 {
        let frame = wait_for(&mut video, time);
        assert_eq!(frame.pixels.len(), previous.pixels.len());
        time += 0.1;
        previous = frame;
    }
}

#[test]
fn dropping_videos_joins_their_workers() {
    for _ in 0..20 {
        let mut video = open();
        let _ = video.frame_at(0.0);
        drop(video);
    }
    let mut video = open();
    let frame = wait_for(&mut video, 0.5);
    assert_eq!(frame.pixels.len(), 160 * 120 * 4);
}

#[test]
fn rejects_non_video_bytes() {
    assert!(Video::open(Arc::from(b"not a video".as_slice().to_vec())).is_err());
}
