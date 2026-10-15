use rusty_h264_common::YuvFrame;

#[derive(Clone)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaFrame {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }
}

pub fn yuv_to_rgba(frame: &YuvFrame) -> RgbaFrame {
    let width = frame.width as u32;
    let height = frame.height as u32;
    let mut out = RgbaFrame::new(width, height);
    let cw = (width as usize).div_ceil(2);
    let ch = (height as usize).div_ceil(2);
    let has_chroma = frame.u.len() >= cw * ch && frame.v.len() >= cw * ch;
    for row in 0..height as usize {
        for column in 0..width as usize {
            let luma = frame
                .y
                .get(row * width as usize + column)
                .copied()
                .unwrap_or(16);
            let (u, v) = if has_chroma {
                let index = (row / 2) * cw + column / 2;
                (frame.u[index], frame.v[index])
            } else {
                (128, 128)
            };
            let c = i32::from(luma) - 16;
            let d = i32::from(u) - 128;
            let e = i32::from(v) - 128;
            let r = (298 * c + 409 * e + 128) >> 8;
            let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
            let b = (298 * c + 516 * d + 128) >> 8;
            let index = (row * width as usize + column) * 4;
            out.pixels[index] = r.clamp(0, 255) as u8;
            out.pixels[index + 1] = g.clamp(0, 255) as u8;
            out.pixels[index + 2] = b.clamp(0, 255) as u8;
            out.pixels[index + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize, y: u8, u: u8, v: u8) -> YuvFrame {
        YuvFrame {
            width,
            height,
            y: vec![y; width * height],
            u: vec![u; width.div_ceil(2) * height.div_ceil(2)],
            v: vec![v; width.div_ceil(2) * height.div_ceil(2)],
        }
    }

    #[test]
    fn converts_white_and_black() {
        let white = yuv_to_rgba(&frame(4, 2, 235, 128, 128));
        assert_eq!(&white.pixels[..4], &[255, 255, 255, 255]);
        let black = yuv_to_rgba(&frame(4, 2, 16, 128, 128));
        assert_eq!(&black.pixels[..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn converts_red() {
        let red = yuv_to_rgba(&frame(2, 2, 82, 90, 240));
        assert!(red.pixels[0] > 250);
        assert!(red.pixels[1] < 5);
        assert!(red.pixels[2] < 5);
        assert_eq!(red.pixels[3], 255);
    }

    #[test]
    fn handles_missing_chroma() {
        let mut empty = frame(4, 2, 128, 128, 128);
        empty.u.clear();
        empty.v.clear();
        let gray = yuv_to_rgba(&empty);
        assert_eq!(gray.pixels[0], gray.pixels[1]);
        assert_eq!(gray.pixels[1], gray.pixels[2]);
        assert_eq!(gray.pixels[3], 255);
        assert_eq!(gray.pixels.len(), 4 * 2 * 4);
    }
}
