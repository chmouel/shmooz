use clap::Parser;

const HELP_EPILOG: &str = "\
Controls:
  Navigation:
    Mouse scroll          Zoom in/out at mouse position
    Left click + drag     Pan the view
    Double click          Restore/unzoom
    +/-                   Zoom in/out at center
    Arrow keys            Pan the view
    0                     Restore/unzoom
    d                     Toggle draw mode on current zoom
    w                     Toggle draw-without-zoom mode
    s                     Save screenshot
    f                     Toggle spotlight dim overlay
    [ / ]                 Decrease/increase spotlight radius

  Annotation:
    Left click + drag     Draw, or move annotations while move mode is active
    Left click            Place text when text mode is active
    p                     Pen
    h                     Highlighter
    m                     Toggle move mode for existing annotations
    t                     Toggle text mode and place text with click
    l                     Line
    r                     Rectangle
    e                     Ellipse
    Enter                 Commit active text and stay in text mode
    Backspace             Delete last text character
    u                     Undo last annotation
    c                     Clear annotations
    Esc                   Leave active modifier first, then leave annotation mode

  Global:
    Right click           Exit
    Esc                   Exit from navigation by default";

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
        help = "Dim screen outside a spotlight circle around the pointer"
    )]
    pub spotlight: bool,

    #[arg(
        long = "screenshot-dir",
        value_name = "DIR",
        help = "Directory for saved screenshots (default: ~/Desktop/Screenshots)"
    )]
    pub screenshot_dir: Option<String>,

    #[arg(long = "no-indicator", help = "Hide the zoom mode indicator badge")]
    pub no_indicator: bool,
}
