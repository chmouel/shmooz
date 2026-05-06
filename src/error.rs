use std::fmt;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to connect to the Wayland display: {message}")]
    WaylandConnect { message: String },

    #[error("failed to dispatch Wayland events during {context}: {message}")]
    WaylandDispatch {
        context: &'static str,
        message: String,
    },

    #[error("failed to manage the runtime during {context}: {message}")]
    EventLoop {
        context: &'static str,
        message: String,
    },

    #[error("{protocol} is missing")]
    MissingProtocol { protocol: &'static str },

    #[error("no wl_output globals are available")]
    NoOutputs,

    #[error("no outputs found")]
    NoSelectedOutputs,

    #[error("no output found matching '{name}'")]
    OutputNotFound { name: String },

    #[error("failed to allocate a shared-memory buffer: {message}")]
    BufferAllocation { message: String },

    #[error("unsupported wl_shm buffer format: {raw}")]
    UnsupportedBufferFormat { raw: u32 },

    #[error("failed to capture {output}")]
    CaptureFailed { output: String },

    #[error("{message}")]
    RuntimeState { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("invalid key name: {value} (supported: Esc, q, x)")]
    InvalidCloseKey { value: String },

    #[error("invalid zoom percentage: {value} (must be 0-99%)")]
    InvalidZoom { value: String },

    #[error("invalid fps value: {value} (must be > 0)")]
    InvalidFps { value: String },
}

impl AppError {
    pub fn connect(err: impl fmt::Display) -> Self {
        Self::WaylandConnect {
            message: err.to_string(),
        }
    }

    pub fn dispatch(context: &'static str, err: impl fmt::Display) -> Self {
        Self::WaylandDispatch {
            context,
            message: err.to_string(),
        }
    }

    pub fn missing_protocol(protocol: &'static str) -> Self {
        Self::MissingProtocol { protocol }
    }

    pub fn event_loop(context: &'static str, err: impl fmt::Display) -> Self {
        Self::EventLoop {
            context,
            message: err.to_string(),
        }
    }

    pub fn buffer_allocation(err: impl fmt::Display) -> Self {
        Self::BufferAllocation {
            message: err.to_string(),
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::RuntimeState {
            message: message.into(),
        }
    }
}
