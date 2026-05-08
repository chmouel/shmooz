# `shmooz`

A zoom / magnifier and screenshot utility for Wayland compositors.

`shmooz` is a zoom / magnifier utility for Wayland compositors written in Rust.

It can also be used as a screenshot tool for the current visible shmooz view, including zoom,
spotlight, and annotations.

It includes an annotation overlay with pen, highlighter, line, text,
rectangle, ellipse, and move tools, plus undo, clear and other necessary features to
make it a useful presentation tool.

### Screenshots

* Zoom with mouse

<img width="3840" height="2160" alt="image" src="https://github.com/user-attachments/assets/a1a96fac-578f-4baf-bdee-f01ee9299244" />

* Zoom with Spotlight

<img width="3831" height="2160" alt="image" src="https://github.com/user-attachments/assets/289c925d-b771-4a31-9f84-4bfe24d947cb" />

* Annotate image

<img width="3322" height="1851" alt="CopyQ qCHygC" src="https://github.com/user-attachments/assets/ee604e72-f1cc-4403-9f1c-350b7d1c8043" />

## Usage

```sh
shmooz [options...]
```

### Options

* `-h, --help` - Show help message and quit
* `--map-close KEY` - Set key to close (e.g. `Esc`, `q`, `x`)
* `--output NAME` - Run on a specific output (e.g. `DP-1`)
* `--zoom-in PERCENT` - Start with an initial zoom (for example `10%` or `0.5`)
* `--invert-scroll` - Invert scroll direction
* `--spotlight` - Dim the screen outside a spotlight circle around the pointer
* `--screenshot-dir DIR` - Directory for saved screenshots (default: `~/Desktop/Screenshots`)
* `--no-indicator` - Hide the zoom badge

### Controls

**Mouse:**

* Scroll wheel - Zoom in/out at mouse position
* Left click + drag - Pan the view
* Right click - Exit
* Double click - Restore the original view

**Annotation mode mouse:**

* Left click + drag - Draw with the active draw tool, or move an annotation while move mode is active
* Left click - Place a text annotation when text mode is active

**Keyboard:**

* `+` / `-` - Zoom in/out at screen center
* Arrow keys - Pan the view
* `0` - Restore the original view
* `d` - Toggle draw mode on the current zoom level
* `w` - Toggle draw mode without zoom
* `s` - Save a screenshot of the current visible output
* `Ctrl+C` - Copy a screenshot of the current visible output to the Wayland clipboard
* `f` - Toggle spotlight
* `[` / `]` - Decrease / increase spotlight radius
* `p` - Select pen
* `h` - Select highlighter
* `m` - Toggle move mode to drag an existing annotation
* `t` - Toggle text mode, then click to place text
* `l` - Select line
* `r` - Select rectangle
* `e` - Select ellipse
* `Enter` - Commit the current text annotation and stay in text mode
* `Backspace` - Delete the last typed text character
* `u` - Undo the last annotation on the focused output
* `c` - Clear annotations on the focused output
* `Esc` - Leave the active text/move modifier first, then leave annotation mode, or close by default when navigating

### Annotation behavior

* Annotation mode freezes zoom and pan until you return to navigation.
* `d` keeps the current zoomed view and lets you draw over it.
* `w` restores the full view first, then enters draw mode without zoom.
* Annotations stay attached to the underlying image content when you zoom or restore the view.
* Screenshots save the currently visible output, including zoom, spotlight, and annotations.
* `Ctrl+C` copies that same visible output to the regular Wayland clipboard as a PNG without saving a file, and also updates the primary selection when the compositor supports it.
* Screenshots are written to `~/Desktop/Screenshots` by default, or to the directory set with `--screenshot-dir`.
* Text annotations now use the Wayland keyboard keymap, so accented characters and layout punctuation should follow your active keyboard layout much more closely.
* The on-screen indicator badge shows the current mode, active tool or modifier, and the most relevant actions for the current context.

### Examples

```sh
# Start with a small zoom
shmooz --zoom-in 10%

# Run on one output only
shmooz --output DP-1

# Enable spotlight from startup
shmooz --spotlight --zoom-in 25%

# Save screenshots somewhere else
shmooz --screenshot-dir /tmp/shmooz-shots

# Start and use draw mode after launch with d / w
cargo run -- --zoom-in 25%

```

## Install

Install via rust or on Arch via the AUR (yay install shmooz for example)

## Configure

On sway for example:

```conf
bindsym $super+Control+Backspace exec pgrep shmooz && killall shmooz || shmooz --zoom-in 10% --output "$(swaymsg -t get_outputs | jq -r 'map(select(.focused == true)) | .[].name')"
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
