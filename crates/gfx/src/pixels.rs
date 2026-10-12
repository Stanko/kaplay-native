use std::borrow::Cow;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UploadOptions {
    pub flip_y: bool,
    pub premultiply: bool,
}

pub fn convert<'a>(
    source: &'a [u8],
    width: usize,
    height: usize,
    options: UploadOptions,
) -> Cow<'a, [u8]> {
    if !options.flip_y && !options.premultiply {
        return Cow::Borrowed(source);
    }

    let mut data = source.to_vec();
    if options.flip_y {
        flip_rows(&mut data, width, height);
    }
    if options.premultiply {
        premultiply_alpha(&mut data);
    }
    Cow::Owned(data)
}

pub fn flip_rows(data: &mut [u8], width: usize, height: usize) {
    let row_size = width * 4;
    if row_size == 0 || data.len() < row_size * height {
        return;
    }
    let mut top = 0;
    let mut bottom = (height - 1) * row_size;
    while top < bottom {
        let (head, tail) = data.split_at_mut(bottom);
        head[top..top + row_size].swap_with_slice(&mut tail[..row_size]);
        top += row_size;
        bottom -= row_size;
    }
}

pub fn premultiply_alpha(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha == 255 {
            continue;
        }
        pixel[0] = ((pixel[0] as u32 * alpha + 127) / 255) as u8;
        pixel[1] = ((pixel[1] as u32 * alpha + 127) / 255) as u8;
        pixel[2] = ((pixel[2] as u32 * alpha + 127) / 255) as u8;
    }
}

pub fn unpremultiply_alpha(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        pixel[0] = ((pixel[0] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        pixel[1] = ((pixel[1] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        pixel[2] = ((pixel[2] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrows_when_no_options() {
        let data = [1u8, 2, 3, 4];
        assert!(matches!(
            convert(&data, 1, 1, UploadOptions::default()),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn flips_rows() {
        let data = [
            1, 2, 3, 4, 5, 6, 7, 8, //
            9, 10, 11, 12, 13, 14, 15, 16,
        ];
        let converted = convert(
            &data,
            2,
            2,
            UploadOptions {
                flip_y: true,
                premultiply: false,
            },
        );
        assert_eq!(&converted[..8], &[9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(&converted[8..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn premultiplies() {
        let data = [100u8, 50, 25, 128];
        let converted = convert(
            &data,
            1,
            1,
            UploadOptions {
                flip_y: false,
                premultiply: true,
            },
        );
        assert_eq!(&converted[..], &[50, 25, 13, 128]);
    }

    #[test]
    fn round_trips_premultiply() {
        let original = [100u8, 50, 25, 128];
        let mut data = original;
        premultiply_alpha(&mut data);
        unpremultiply_alpha(&mut data);
        for (before, after) in original.iter().zip(data.iter()) {
            assert!(before.abs_diff(*after) <= 1, "{before} vs {after}");
        }
    }
}
