use crate::{cli::Cli, error::ConfigError};

pub const APP_ID: &str = "com.chmouel.shmooz";
pub const DEFAULT_LIVE_FPS: u32 = 4;
pub const DEFAULT_LIVE_REFRESH_MS: u32 = 1000 / DEFAULT_LIVE_FPS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseKey {
    Escape,
    Q,
    X,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Config {
    pub app_id: &'static str,
    pub close_key: Option<CloseKey>,
    pub mouse_track: bool,
    pub initial_zoom: f64,
    pub output_filter: Option<String>,
    pub invert_scroll: bool,
    pub spotlight: bool,
    pub show_indicator: bool,
    pub live_zoom: bool,
    pub live_refresh_ms: u32,
}

impl TryFrom<Cli> for Config {
    type Error = ConfigError;

    fn try_from(cli: Cli) -> Result<Self, Self::Error> {
        let close_key = cli.map_close.as_deref().map(CloseKey::parse).transpose()?;

        let initial_zoom = cli
            .zoom_in
            .as_deref()
            .map(parse_zoom)
            .transpose()?
            .unwrap_or(0.0);

        let live_refresh_ms = cli
            .live_fps
            .as_deref()
            .map(parse_fps)
            .transpose()?
            .map(|fps| 1000 / fps)
            .unwrap_or(DEFAULT_LIVE_REFRESH_MS);

        Ok(Self {
            app_id: APP_ID,
            close_key,
            mouse_track: cli.mouse_track,
            initial_zoom,
            output_filter: cli.output,
            invert_scroll: cli.invert_scroll,
            spotlight: cli.spotlight,
            show_indicator: !cli.no_indicator,
            live_zoom: cli.live,
            live_refresh_ms,
        })
    }
}

impl CloseKey {
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        match raw {
            "Esc" | "Escape" => Ok(Self::Escape),
            "q" | "Q" => Ok(Self::Q),
            "x" | "X" => Ok(Self::X),
            _ => Err(ConfigError::CloseKey {
                value: raw.to_owned(),
            }),
        }
    }

    pub fn key_code(self) -> u32 {
        match self {
            Self::Escape => 1,
            Self::Q => 16,
            Self::X => 45,
        }
    }
}

fn parse_zoom(raw: &str) -> Result<f64, ConfigError> {
    let trimmed = raw.trim();
    let (value, is_percent) = match trimmed.strip_suffix('%') {
        Some(value) => (value, true),
        None => (trimmed, false),
    };

    let parsed = value.parse::<f64>().map_err(|_| ConfigError::Zoom {
        value: raw.to_owned(),
    })?;

    let normalized = if is_percent { parsed / 100.0 } else { parsed };

    if !(0.0..1.0).contains(&normalized) {
        return Err(ConfigError::Zoom {
            value: raw.to_owned(),
        });
    }

    Ok(normalized)
}

fn parse_fps(raw: &str) -> Result<u32, ConfigError> {
    let fps = raw.parse::<i64>().map_err(|_| ConfigError::Fps {
        value: raw.to_owned(),
    })?;

    if fps <= 0 {
        return Err(ConfigError::Fps {
            value: raw.to_owned(),
        });
    }

    Ok(fps as u32)
}

#[cfg(test)]
mod tests {
    use super::{CloseKey, parse_fps, parse_zoom};

    #[test]
    fn parse_zoom_percent() {
        assert_eq!(parse_zoom("10%").unwrap(), 0.1);
    }

    #[test]
    fn parse_zoom_fraction() {
        assert_eq!(parse_zoom("0.5").unwrap(), 0.5);
    }

    #[test]
    fn parse_zoom_rejects_out_of_range_values() {
        assert!(parse_zoom("1.0").is_err());
        assert!(parse_zoom("100%").is_err());
    }

    #[test]
    fn parse_close_key_names() {
        assert_eq!(CloseKey::parse("Esc").unwrap(), CloseKey::Escape);
        assert_eq!(CloseKey::parse("q").unwrap(), CloseKey::Q);
        assert_eq!(CloseKey::parse("X").unwrap(), CloseKey::X);
    }

    #[test]
    fn parse_fps_requires_positive_value() {
        assert_eq!(parse_fps("4").unwrap(), 4);
        assert!(parse_fps("0").is_err());
    }
}
