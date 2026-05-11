# `shmooz`

Zoom, annotate, and capture the current Wayland view.

`shmooz` is a Wayland presentation and screenshot tool. It lets you zoom into an output, add simple annotations, dim the rest of the screen with a spotlight, and save or copy exactly what is visible.

### Screenshots

* Flow

https://github.com/user-attachments/assets/7f517044-b52f-477b-8721-d22397a73a75

* Zoom

<img width="3840" height="2160" alt="image" src="https://github.com/user-attachments/assets/a1a96fac-578f-4baf-bdee-f01ee9299244" />

* Spotlight

<img width="3831" height="2160" alt="image" src="https://github.com/user-attachments/assets/289c925d-b771-4a31-9f84-4bfe24d947cb" />

* Annotation

<img width="3322" height="1851" alt="CopyQ qCHygC" src="https://github.com/user-attachments/assets/ee604e72-f1cc-4403-9f1c-350b7d1c8043" />

## Features

* Zoom and pan the current output.
* Toggle a spotlight around the pointer.
* Annotate with pen, highlighter, line, rectangle, ellipse, text, and move tools.
* Pick from a shared annotation color palette and adjust text size while annotating.
* Save the visible view as a screenshot or copy it to the Wayland clipboard.

## Usage

```sh
shmooz [options...]
```

### Options

* `-h`, `--help` - Show help and exit
* `--map-close KEY` - Set the close key
* `--output NAME` - Use a specific output
* `--zoom-in PERCENT` - Start zoomed in
* `--invert-scroll` - Invert scroll direction
* `--spotlight` - Start with spotlight enabled
* `--screenshot-dir DIR` - Set the screenshot directory
* `--no-indicator` - Hide the on-screen indicator

### Controls

**Navigation**

* Scroll - Zoom at the pointer
* Left drag - Pan
* `+` / `-` - Zoom at screen center
* Arrow keys - Pan
* `0` - Reset view
* `f` - Toggle spotlight
* Right click - Exit
* Double click - Reset view

**Annotation**

* `d` - Draw on the current zoomed view
* `w` - Draw on the full view
* `p` / `h` / `l` / `r` / `e` - Select pen, highlighter, line, rectangle, or ellipse
* `t` - Toggle text mode
* `m` - Toggle move mode
* Left drag - Draw or move the selected annotation
* Left click - Place text
* `1`-`0`, `-`, `=` - Select one of 12 annotation colors
* `[` / `]` - Change text size while annotating, or spotlight radius while navigating
* `Enter` - Commit text
* `Backspace` - Delete the last text character
* `u` - Undo
* `c` - Clear annotations on the focused output
* `Esc` - Leave text or move mode, then leave annotation mode

## Examples

```sh
# Start with a small zoom
shmooz --zoom-in 10%

# Run on one output
shmooz --output DP-1

# Start with spotlight enabled
shmooz --spotlight --zoom-in 25%

# Save screenshots elsewhere
shmooz --screenshot-dir /tmp/shmooz-shots
```

## Install

Build from source with Cargo, or install from the AUR on Arch.

## Configure

Example sway binding:

```conf
bindsym $super+Control+Backspace exec pgrep shmooz && killall shmooz || shmooz --zoom-in 10% --output "$(swaymsg -t get_outputs | jq -r 'map(select(.focused == true)) | .[].name')"
```

## Build

```sh
cargo build
```

Run directly:

```sh
cargo run -- --help
```

Or use the `Makefile` target from the repository root:

```sh
make -C build shmooz
```

## Acknowledgement

Inspired by [wooz](https://github.com/negrel/wooz/).

## License

Apache-2.0.
