use crate::{cli::Cli, error::ConfigError};

pub const APP_ID: &str = "com.chmouel.shmooz";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseKey {
    Escape,
    Q,
    X,
}

#[derive(Debug, Clone)]
pub struct Config {
    #[allow(dead_code)]
    pub app_id: &'static str,
    pub close_key: Option<CloseKey>,
    pub initial_zoom: f64,
    pub output_filter: Option<String>,
    pub invert_scroll: bool,
    pub spotlight: bool,
    pub show_indicator: bool,
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

        Ok(Self {
            app_id: APP_ID,
            close_key,
            initial_zoom,
            output_filter: cli.output,
            invert_scroll: cli.invert_scroll,
            spotlight: cli.spotlight,
            show_indicator: !cli.no_indicator,
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

#[cfg(test)]
mod tests {
    use super::{CloseKey, parse_zoom};

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
}
