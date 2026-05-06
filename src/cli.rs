use clap::Parser;

const HELP_EPILOG: &str = "\
Controls:
  Mouse scroll            Zoom in/out at mouse position
  Left click + drag       Pan the view
  Right click             Exit
  Double click            Restore/unzoom
  +/-                     Zoom in/out at center
  Arrow keys              Pan the view
  0                       Restore/unzoom
  s                       Toggle spotlight dim overlay
  [ / ]                   Decrease/increase spotlight radius
  Esc                     Exit (default)";

#[derive(Debug, Clone, Parser)]
#[command(
    name = "shmooz",
    disable_help_subcommand = true,
    about = "A zoom / magnifier utility for Wayland compositors",
    override_usage = "shmooz [options...]",
    after_help = HELP_EPILOG
)]
pub struct Cli {
    #[arg(
        long = "map-close",
        value_name = "KEY",
        help = "Set key to close (e.g., 'Esc', 'q', 'x')"
    )]
    pub map_close: Option<String>,

    #[arg(
        long = "mouse-track",
        help = "Enable mouse tracking (follow mouse without clicking)"
    )]
    pub mouse_track: bool,

    #[arg(
        long = "output",
        value_name = "NAME",
        help = "Run on a specific output (e.g., 'DP-1')"
    )]
    pub output: Option<String>,

    #[arg(
        long = "zoom-in",
        value_name = "PERCENT",
        help = "Set initial zoom percentage (e.g., '10%', '0.5')"
    )]
    pub zoom_in: Option<String>,

    #[arg(
        long = "invert-scroll",
        help = "Invert scroll direction (scroll up zooms in)"
    )]
    pub invert_scroll: bool,

    #[arg(
        long = "spotlight",
        help = "Dim screen outside a spotlight circle when zoomed"
    )]
    pub spotlight: bool,

    #[arg(long = "no-indicator", help = "Hide the zoom mode indicator badge")]
    pub no_indicator: bool,
}
