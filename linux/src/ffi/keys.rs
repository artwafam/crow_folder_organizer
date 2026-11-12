//! X keysyms as Windows virtual keys and characters, with dead keys for
//! accented letters (á, ã, ç...).

/// The virtual key for a keysym (0 when there is none).
pub fn vk(sym: u32) -> usize {
    match sym {
        0xff08 => 0x08,                         // BackSpace
        0xff09 | 0xfe20 => 0x09,                // Tab, ISO_Left_Tab
        0xff0d | 0xff8d => 0x0D,                // Return, KP_Enter
        0xff1b => 0x1B,                         // Escape
        0xff13 => 0x13,                         // Pause
        0xff50 | 0xff95 => 0x24,                // Home
        0xff51 | 0xff96 => 0x25,                // Left
        0xff52 | 0xff97 => 0x26,                // Up
        0xff53 | 0xff98 => 0x27,                // Right
        0xff54 | 0xff99 => 0x28,                // Down
        0xff55 | 0xff9a => 0x21,                // Prior
        0xff56 | 0xff9b => 0x22,                // Next
        0xff57 | 0xff9c => 0x23,                // End
        0xff63 | 0xff9e => 0x2D,                // Insert
        0xffff | 0xff9f => 0x2E,                // Delete
        0xffe1 | 0xffe2 => 0x10,                // Shift
        0xffe3 | 0xffe4 => 0x11,                // Control
        0xffe9 | 0xffea | 0xfe03 => 0x12,       // Alt, AltGr
        0xffeb | 0xffec => 0x5B,                // Super
        0x20 => 0x20,
        0x30..=0x39 => sym as usize,
        0x41..=0x5a => sym as usize,
        0x61..=0x7a => (sym - 0x20) as usize,
        0xffb0..=0xffb9 => (0x60 + sym - 0xffb0) as usize, // keypad digits
        0xffbe..=0xffc9 => (0x70 + sym - 0xffbe) as usize, // F1..F12
        _ => 0,
    }
}

/// The character a keysym types, if any.
pub fn char_of(sym: u32) -> Option<char> {
    let c = match sym {
        0x20..=0x7e | 0xa0..=0xff => sym,
        0x0100_0000..=0x0110_ffff => sym - 0x0100_0000,
        0xffaa..=0xffb9 => match sym {
            0xffaa => '*' as u32,
            0xffab => '+' as u32,
            0xffac => ',' as u32,
            0xffad => '-' as u32,
            0xffae => '.' as u32,
            0xffaf => '/' as u32,
            _ => '0' as u32 + (sym - 0xffb0),
        },
        0x6a1..=0x6ff => cyrillic(sym)?,
        0x20ac => 0x20ac,
        _ => return None,
    };
    char::from_u32(c)
}

/// The legacy Cyrillic keysym block (what Russian layouts send).
fn cyrillic(sym: u32) -> Option<u32> {
    const LOW: [u16; 32] = [
        0x44E, 0x430, 0x431, 0x446, 0x434, 0x435, 0x444, 0x433, 0x445, 0x438, 0x439, 0x43A, 0x43B, 0x43C, 0x43D, 0x43E,
        0x43F, 0x44F, 0x440, 0x441, 0x442, 0x443, 0x436, 0x432, 0x44C, 0x44B, 0x437, 0x448, 0x44D, 0x449, 0x447, 0x44A,
    ];
    match sym {
        0x6c0..=0x6df => Some(LOW[(sym - 0x6c0) as usize] as u32),
        0x6e0..=0x6ff => {
            let l = LOW[(sym - 0x6e0) as usize] as u32;
            Some(if l >= 0x430 && l <= 0x44F { l - 0x20 } else { l })
        }
        0x6a3 => Some(0x451), // ё
        0x6b3 => Some(0x401), // Ё
        0x6a6 => Some(0x456),
        0x6b6 => Some(0x406),
        0x6a8 => Some(0x458),
        0x6a4 => Some(0x454),
        0x6b4 => Some(0x404),
        0x6a7 => Some(0x457),
        0x6b7 => Some(0x407),
        0x6ae => Some(0x45E),
        0x6be => Some(0x40E),
        _ => None,
    }
}

/// The dead key a keysym stands for, as the combining mark it adds.
pub fn dead(sym: u32) -> Option<char> {
    Some(match sym {
        0xfe50 => '`',
        0xfe51 => '\u{b4}',
        0xfe52 => '^',
        0xfe53 => '~',
        0xfe57 => '\u{a8}',
        0xfe5b => '\u{b8}',
        0xfe58 => '\u{b0}',
        _ => return None,
    })
}

/// Combines a dead key with the next character, like á from ´ then a.
pub fn compose(mark: char, c: char) -> Option<char> {
    const T: &[(char, &str, &str)] = &[
        ('`', "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
        ('\u{b4}', "aeiouyAEIOUYcC", "áéíóúýÁÉÍÓÚÝćĆ"),
        ('^', "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
        ('~', "aonAON", "ãõñÃÕÑ"),
        ('\u{a8}', "aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
        ('\u{b8}', "cC", "çÇ"),
        ('\u{b0}', "aA", "åÅ"),
    ];
    if c == ' ' {
        return Some(match mark {
            '\u{b4}' => '\'',
            '\u{a8}' => '"',
            m => m,
        });
    }
    // Portuguese keyboards type ç as ´ then c, like GTK does in that locale.
    if mark == '\u{b4}' && (c == 'c' || c == 'C') && locale_is_pt() {
        return Some(if c == 'c' { 'ç' } else { 'Ç' });
    }
    let (_, from, to) = T.iter().find(|t| t.0 == mark)?;
    let i = from.chars().position(|x| x == c)?;
    to.chars().nth(i)
}

fn locale_is_pt() -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .is_some_and(|v| v.starts_with("pt"))
}
