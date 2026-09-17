//! Crow is an ultra light desktop folder organizer for Windows. It uses no std,
//! no CRT and no UI framework, only raw Win32 and a hand written rasterizer.
#![no_std]
#![allow(clippy::missing_safety_doc, clippy::too_many_arguments)]

extern crate alloc;

#[macro_use]
pub mod util;
pub mod actions;
pub mod app;
pub mod art;
pub mod blur;
pub mod config;
pub mod desk;
pub mod drop;
pub mod edit;
pub mod ffi;
pub mod folder;
pub mod gfx;
pub mod glyph;
pub mod i18n;
pub mod image;
pub mod input;
pub mod popup;
pub mod prefs;
pub mod raster;
pub mod rt;
pub mod settings;
pub mod snapshot;
pub mod store;
pub mod svg;
pub mod text;
pub mod ui;
pub mod uikit;

pub use app::Fonts;
