# Crow FO

Crow FO (Folder Organizer) is a tiny, highly customizable desktop folder organizer for Windows. Group your shortcuts into folders that open when you hover over them, or keep them in panels that stay open.

![A folder opened on hover](docs/open.png)

<p>
  <img src="docs/panel.png" alt="A panel" width="420">
  <img src="docs/menu.png" alt="Context menu" width="280">
</p>

## Download

Get `CrowFOSetup.exe` from the [latest release](../../releases/latest) and run it. That's all you need.

It installs for your user only, so no admin rights are required. It can start Crow FO with Windows and add a "Crow FO" entry to the desktop right-click menu. Uninstall it any time from Windows Settings, under Apps.

Windows may show a SmartScreen warning the first time, because the installer is not signed yet. Click "More info" and then "Run anyway".

Prefer no installer? Download `CrowFO-portable.exe` and run it from anywhere. To install without any window, for example from a script, run `CrowFOSetup.exe /S`.

## Features

- Folders open as soon as the pointer settles on them, with a smooth animation. Any folder can also be a panel that stays open, and panels collapse to their header with one click.
- Drag shortcuts from the desktop or Explorer onto a folder to move them in. Drag them out to put them back on the desktop.
- Click a folder name to rename it right there.
- A settings button appears in the corner of each folder, with colors, icons, sizes and layout. Changes show up live.
- Use your own PNG, ICO, JPG or SVG icons for folders and single shortcuts. Single color SVG icons can be tinted.
- A settings window for the language, appearance, animation speed and the defaults for new folders.
- Available in English, Español, Português (Brasil), Русский, 简体中文 and 繁體中文.

<p>
  <img src="docs/prefs.png" alt="Crow FO settings" width="440">
  <img src="docs/settings.png" alt="Folder settings" width="250">
</p>

## Footprint

| | |
|---|---|
| CrowFO.exe | about 270 KB |
| Memory when idle | about 100 KB |
| CPU when idle | 0% |

## Translating

Translations are plain text files in [lang](lang). See [lang/README.md](lang/README.md) to add a language or fix one.

## Building from source

Only needed if you want to work on Crow FO. You need [Rust](https://rustup.rs).

```sh
rustup target add x86_64-pc-windows-msvc
cargo build --release -p crow
cargo build --release -p crow-setup
```

Both files end up in `target\x86_64-pc-windows-msvc\release`. Pushing a tag like `v1.0.0` makes the GitHub workflow build them and publish a release.
