//! Crow is an ultra light desktop folder organizer for Windows. It uses no std,
//! no CRT and no UI framework, only raw Win32 and a hand written rasterizer.
#![no_std]
#![allow(clippy::missing_safety_doc, clippy::too_many_arguments)]

extern crate alloc;

#[macro_use]
pub mod util;
pub mod ffi;
pub mod gfx;
pub mod image;
pub mod raster;
pub mod rt;
pub mod svg;
pub mod text;

