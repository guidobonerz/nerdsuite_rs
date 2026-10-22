//! The bytes of a file as rows of the Hex view: offsets, hex and the characters of a char set.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CharSet {
    #[default]
    Ascii,
    /// PETSCII, upper case and graphics (the C64 after power on).
    Petscii,
    /// The screen codes of the Commodore machines (what `!scr` writes).
    ScreenCodes,
}

impl CharSet {
    pub const ALL: [CharSet; 3] = [CharSet::Ascii, CharSet::Petscii, CharSet::ScreenCodes];

    pub fn label(self) -> &'static str {
        match self {
            CharSet::Ascii => "ASCII",
            CharSet::Petscii => "PETSCII",
            CharSet::ScreenCodes => "Screen codes",
        }
    }

    /// The character of a byte, `·` for the ones without a printable one.
    pub fn char(self, byte: u8) -> char {
        const COMMODORE_40_5F: [char; 32] = [
            '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P',
            'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '£', ']', '↑', '←',
        ];
        match self {
            CharSet::Ascii => match byte {
                0x20..=0x7e => byte as char,
                _ => '·',
            },
            CharSet::Petscii => match byte {
                0x20..=0x3f => byte as char,
                0x40..=0x5f => COMMODORE_40_5F[(byte - 0x40) as usize],
                _ => '·',
            },
            // reverse chars ($80-$ff) look as their plain ones
            CharSet::ScreenCodes => match byte & 0x7f {
                code @ 0x00..=0x1f => COMMODORE_40_5F[code as usize],
                code @ 0x20..=0x3f => code as char,
                _ => '·',
            },
        }
    }
}

pub const BYTES_PER_ROW: usize = 16;

pub fn row_count(len: usize) -> usize {
    len.div_ceil(BYTES_PER_ROW).max(1)
}

/// A load address for the offsets: the first two bytes of a .prg.
pub fn prg_load_address(path: &str, bytes: &[u8]) -> Option<u16> {
    (path.to_ascii_lowercase().ends_with(".prg") && bytes.len() >= 2)
        .then(|| u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// `hex` of a row: `a9 00 8d 20 d0 …`, padded to the full width.
pub fn hex_cells(row: &[u8]) -> String {
    let mut out = String::with_capacity(BYTES_PER_ROW * 3 + 1);
    for ix in 0..BYTES_PER_ROW {
        if ix == 8 {
            out.push(' ');
        }
        match row.get(ix) {
            Some(byte) => out.push_str(&format!("{byte:02x} ")),
            None => out.push_str("   "),
        }
    }
    out.truncate(out.trim_end().len());
    out
}

pub fn char_cells(row: &[u8], charset: CharSet) -> String {
    row.iter().map(|&byte| charset.char(byte)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charsets() {
        assert_eq!(CharSet::Ascii.char(b'a'), 'a');
        assert_eq!(CharSet::Ascii.char(0), '·');
        assert_eq!(CharSet::Petscii.char(0x41), 'A');
        assert_eq!(CharSet::Petscii.char(0x5c), '£');
        assert_eq!(CharSet::ScreenCodes.char(0x08), 'H');
        assert_eq!(CharSet::ScreenCodes.char(0x88), 'H');
        assert_eq!(CharSet::ScreenCodes.char(0x20), ' ');
    }

    #[test]
    fn rows() {
        assert_eq!(row_count(0), 1);
        assert_eq!(row_count(16), 1);
        assert_eq!(row_count(17), 2);
        assert_eq!(hex_cells(&[0xa9, 0x00]), "a9 00");
        let full: Vec<u8> = (0..16).collect();
        assert_eq!(
            hex_cells(&full),
            "00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f"
        );
        assert_eq!(prg_load_address("a.PRG", &[0x01, 0x08, 0]), Some(0x0801));
        assert_eq!(prg_load_address("a.bin", &[0x01, 0x08]), None);
    }
}
