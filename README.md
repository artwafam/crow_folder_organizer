# Crow FO

Crow FO (Folder Organizer) is a tiny, highly customizable desktop folder organizer for Windows. Group your shortcuts into folders that open when you hover over them, or keep them in panels that stay open.

![Crow FO in action](docs/demo.gif)

## Download

Get `CrowFOSetup.exe` from the [latest release](../../releases/latest) and run it. That's all you need.

It installs for your user only, so no admin rights are required. It can start Crow FO with Windows and add a "Crow FO" entry to the desktop right-click menu. Uninstall it any time from Windows Settings, under Apps.

Windows may show a SmartScreen warning the first time, because the installer is not signed yet. Click "More info" and then "Run anyway".

Prefer no installer? Download `CrowFO-portable.exe` and run it from anywhere. To install without any window, for example from a script, run `CrowFOSetup.exe /S`.

## Features

- Customizable folders and panels that can be used to store multiple shortcuts and organize your desktop
- Smoothly animated (also customizable)
- Customizable icons
- SUPER small footprint on your computer
- Did I say it is customizable?
- Available in:
  - English
  - Español
  - Português Brasileiro
  - Русский
  - 简体中文
  - 繁體中文

<p>
  <img src="docs/prefs.png" alt="Crow FO settings" height="400">
  <img src="docs/settings.png" alt="Folder settings" height="400">
  <img src="docs/menu.png" alt="Context menu" width="300">
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