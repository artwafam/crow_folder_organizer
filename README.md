# Crow

A tiny, highly customizable desktop folder organizer for Windows. Group your shortcuts into folders that open when you hover over them, or keep them in panels that stay open.

## Features

- Folders open as soon as the pointer settles on them, with a smooth animation. Any folder can also be a panel that stays open.
- Drag shortcuts from the desktop or Explorer onto a folder to move them in. Drag them out to put them back on the desktop.
- Use your own PNG, ICO, JPG or SVG icons for folders and single shortcuts. Single color SVG icons can be tinted.
- Starts with Windows.

## Building

You need [Rust](https://rustup.rs).

```sh
rustup target add x86_64-pc-windows-msvc
cargo build --release
```
