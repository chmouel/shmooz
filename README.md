# `shmooz`

A zoom / magnifier utility for Wayland compositors.

`shmooz` is a zoom / magnifier utility for Wayland compositors written in Rust.

## Usage

```sh
shmooz [options...]
```

### Options

* `-h, --help` - Show help message and quit
* `--map-close KEY` - Set key to close (e.g. `Esc`, `q`, `x`)
* `--mouse-track` - Enable mouse tracking
* `--output NAME` - Run on a specific output (e.g. `DP-1`)
* `--zoom-in PERCENT` - Start with an initial zoom (for example `10%` or `0.5`)
* `--invert-scroll` - Invert scroll direction
* `--spotlight` - Dim the screen outside a circle when zoomed
* `--no-indicator` - Hide the zoom badge
* `--live` - Periodically refresh the captured content
* `--live-fps N` - Set the live refresh rate in frames per second

### Controls

**Mouse:**

* Scroll wheel - Zoom in/out at mouse position
* Left click + drag - Pan the view
* Right click - Exit
* Double click - Restore the original view

**Keyboard:**

* `+` / `-` - Zoom in/out at screen center
* Arrow keys - Pan the view
* `0` - Restore the original view
* `s` - Toggle spotlight
* `[` / `]` - Decrease / increase spotlight radius
* `Esc` - Exit by default

### Examples

```sh
# Start with a small zoom
shmooz --zoom-in 10%

# Run on one output only
shmooz --output DP-1

# Follow the mouse while zoomed
shmooz --mouse-track

# Enable spotlight from startup
shmooz --spotlight --zoom-in 25%

# Refresh the captured content a few times per second
shmooz --live --live-fps 4
```

## Building

Build it with Cargo:

```sh
cargo build
```

Run it directly:

```sh
cargo run -- --help
```

From the repository root you can also use:

```sh
make -C build shmooz
```

## Acknowledgement

Inspired by [wooz](https://github.com/negrel/wooz/) it follows the similar keybindings and features and add some of its own.

## License

Apache-2.0.
