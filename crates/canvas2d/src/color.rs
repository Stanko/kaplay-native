#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);
    pub const BLACK: Self = Self::new(0, 0, 0, 255);
    pub const WHITE: Self = Self::new(255, 255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_color(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }

    pub fn from_css(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        if let Some(hex) = value.strip_prefix('#') {
            return Self::from_hex(hex);
        }
        let lower = value.to_ascii_lowercase();
        if let Some(inner) = lower
            .strip_prefix("rgba(")
            .or_else(|| lower.strip_prefix("rgb("))
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
            if parts.len() < 3 {
                return None;
            }
            let r = parse_channel(parts[0])?;
            let g = parse_channel(parts[1])?;
            let b = parse_channel(parts[2])?;
            let a = match parts.get(3) {
                Some(part) => parse_alpha(part)?,
                None => 255,
            };
            return Some(Self::new(r, g, b, a));
        }
        named_color(&lower)
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    fn from_hex(hex: &str) -> Option<Self> {
        let digits: Vec<u8> = hex
            .chars()
            .map(|character| character.to_digit(16).map(|value| value as u8))
            .collect::<Option<Vec<u8>>>()?;
        match digits.len() {
            3 | 4 => {
                let r = digits[0] * 17;
                let g = digits[1] * 17;
                let b = digits[2] * 17;
                let a = digits.get(3).map_or(255, |value| value * 17);
                Some(Self::new(r, g, b, a))
            }
            6 | 8 => {
                let r = digits[0] * 16 + digits[1];
                let g = digits[2] * 16 + digits[3];
                let b = digits[4] * 16 + digits[5];
                let a = if digits.len() == 8 {
                    digits[6] * 16 + digits[7]
                } else {
                    255
                };
                Some(Self::new(r, g, b, a))
            }
            _ => None,
        }
    }
}

fn parse_channel(value: &str) -> Option<u8> {
    if let Some(percent) = value.strip_suffix('%') {
        let percent: f32 = percent.trim().parse().ok()?;
        return Some((percent / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    let number: f32 = value.parse().ok()?;
    Some(number.round().clamp(0.0, 255.0) as u8)
}

fn parse_alpha(value: &str) -> Option<u8> {
    if let Some(percent) = value.strip_suffix('%') {
        let percent: f32 = percent.trim().parse().ok()?;
        return Some((percent / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    let number: f32 = value.parse().ok()?;
    Some((number * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn named_color(name: &str) -> Option<Rgba> {
    let value = match name {
        "black" => (0, 0, 0, 255),
        "white" => (255, 255, 255, 255),
        "red" => (255, 0, 0, 255),
        "green" => (0, 128, 0, 255),
        "blue" => (0, 0, 255, 255),
        "yellow" => (255, 255, 0, 255),
        "magenta" | "fuchsia" => (255, 0, 255, 255),
        "cyan" | "aqua" => (0, 255, 255, 255),
        "gray" | "grey" => (128, 128, 128, 255),
        "orange" => (255, 165, 0, 255),
        "purple" => (128, 0, 128, 255),
        "transparent" => (0, 0, 0, 0),
        _ => return None,
    };
    Some(Rgba::new(value.0, value.1, value.2, value.3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(Rgba::from_css("#fff"), Some(Rgba::WHITE));
        assert_eq!(Rgba::from_css("#ff0000"), Some(Rgba::new(255, 0, 0, 255)));
        assert_eq!(Rgba::from_css("#00ff0080"), Some(Rgba::new(0, 255, 0, 128)));
        assert_eq!(Rgba::from_css("#0f08"), Some(Rgba::new(0, 255, 0, 136)));
    }

    #[test]
    fn parses_functions_and_names() {
        assert_eq!(
            Rgba::from_css("rgb(255, 0, 0)"),
            Some(Rgba::new(255, 0, 0, 255))
        );
        assert_eq!(
            Rgba::from_css("rgba(0,0,0,0.5)"),
            Some(Rgba::new(0, 0, 0, 128))
        );
        assert_eq!(Rgba::from_css("white"), Some(Rgba::WHITE));
        assert_eq!(Rgba::from_css("nope"), None);
    }
}
