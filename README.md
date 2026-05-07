# `shmooz`

A zoom / magnifier utility for Wayland compositors.

`shmooz` is a zoom / magnifier utility for Wayland compositors written in Rust.

It includes an annotation overlay with pen, highlighter, line, text,
rectangle, ellipse, and move tools, plus undo, clear and other necessary features to
make it a useful presentation tool.

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

### Controls

**Mouse:**

* Scroll wheel - Zoom in/out at mouse position
* Left click + drag - Pan the view
* Right click - Exit
* Double click - Restore the original view

**Annotation mode mouse:**

* Left click + drag - Draw with the selected annotation tool
* Left click - Place a text annotation when the text tool is selected

**Keyboard:**

* `+` / `-` - Zoom in/out at screen center
* Arrow keys - Pan the view
* `0` - Restore the original view
* `d` - Toggle draw mode on the current zoom level
* `w` - Toggle draw mode without zoom
* `s` - Toggle spotlight
* `[` / `]` - Decrease / increase spotlight radius
* `p` - Select pen
* `h` - Select highlighter
* `m` - Select move and drag an existing annotation
* `t` - Select text
* `l` - Select line
* `r` - Select rectangle
* `e` - Select ellipse
* `Enter` - Commit the current text annotation
* `Backspace` - Delete the last typed text character
* `u` - Undo the last annotation on the focused output
* `c` - Clear annotations on the focused output
* `Esc` - Leave annotation mode, or exit by default when navigating

### Annotation behavior

* Annotation mode freezes zoom and pan until you return to navigation.
* `d` keeps the current zoomed view and lets you draw over it.
* `w` restores the full view first, then enters draw mode without zoom.
* Annotations stay attached to the underlying image content when you zoom or restore the view.
* Text annotations now use the Wayland keyboard keymap, so accented characters and layout punctuation should follow your active keyboard layout much more closely.
* The on-screen indicator badge shows the current mode and the most important draw shortcuts.

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

# Start and use draw mode after launch with d / w
cargo run -- --zoom-in 25%

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
