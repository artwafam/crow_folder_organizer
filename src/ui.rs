//! The one system dialog Crow still uses: picking an image file for an icon.

use crate::ffi::*;
use crate::i18n::tr;
use crate::util::*;

/// File-open dialog for icon images.
pub fn pick_image(owner: HWND) -> Option<WString> {
    unsafe {
        let mut buf = [0u16; 1024];
        // Pairs of name and pattern, each ending in a NUL. wz adds the final NUL.
        let mut f = alloc::string::String::from(tr("dialog.images"));
        f.push_str(" (svg, png, ico, jpg, bmp, gif, webp)\0*.svg;*.png;*.ico;*.jpg;*.jpeg;*.bmp;*.gif;*.webp;*.tif;*.tiff\0");
        f.push_str(tr("dialog.all_files"));
        f.push_str("\0*.*\0");
        let filter = wz(&to_w(&f));
        let title = wz(&to_w(tr("dialog.choose_icon")));
        let mut of: OPENFILENAMEW = core::mem::zeroed();
        of.lStructSize = core::mem::size_of::<OPENFILENAMEW>() as u32;
        of.hwndOwner = owner;
        of.lpstrFilter = filter.as_ptr();
        of.lpstrFile = buf.as_mut_ptr();
        of.nMaxFile = buf.len() as u32;
        of.lpstrTitle = title.as_ptr();
        of.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER;
        if GetOpenFileNameW(&mut of) == 0 {
            return None;
        }
        Some(trim_nul(&buf).to_vec())
    }
}
