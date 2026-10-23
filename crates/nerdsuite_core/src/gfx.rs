//! The graphics of the Commodore machines: the VIC-II palette, and charsets - 256 characters of
//! 8 x 8 pixels, a byte a row (2048 bytes, a load address before them tolerated). Hires: a bit a
//! pixel. Multicolor: a bit pair a pixel of double width - 00 the background, 01 $d022, 10 $d023,
//! 11 the cell color.

/// The 16 colors of the VIC-II (nerdsuite2's `video.js`), as 0xRRGGBB.
pub const PALETTE: [u32; 16] = [
    0x000000, 0xffffff, 0x880000, 0xaaffee, 0xcc44cc, 0x00cc55, 0x0000aa, 0xeeee77, 0xdd8855, 0x664400, 0xff7777,
    0x333333, 0x777777, 0xaaff66, 0x0088ff, 0xbbbbbb,
];

pub const COLOR_NAMES: [&str; 16] = [
    "black",
    "white",
    "red",
    "cyan",
    "purple",
    "green",
    "blue",
    "yellow",
    "orange",
    "brown",
    "light red",
    "dark grey",
    "grey",
    "light green",
    "light blue",
    "light grey",
];

pub const CHARSET_SIZE: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charset {
    pub bytes: Vec<u8>,
    /// the two bytes of a load address the file had, kept when it is written again
    pub load_address: Option<[u8; 2]>,
}

impl Default for Charset {
    fn default() -> Self {
        Charset { bytes: vec![0; CHARSET_SIZE], load_address: None }
    }
}

/// How a character is drawn: a bit a pixel, or a bit pair a pixel of double width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hires,
    Multicolor,
}

impl Mode {
    /// The pixels of a row, and the bits of a pixel.
    pub fn width(self) -> usize {
        match self {
            Mode::Hires => 8,
            Mode::Multicolor => 4,
        }
    }

    fn bits(self) -> usize {
        8 / self.width()
    }
}

impl Charset {
    /// The charset of a file: 2048 bytes, or fewer (padded), a load address before them dropped.
    pub fn from_file(data: &[u8]) -> Charset {
        let (load_address, data) = if data.len() % CHARSET_SIZE == 2 || data.len() == 2 + 1024 {
            (Some([data[0], data[1]]), &data[2..])
        } else {
            (None, data)
        };
        let mut bytes = data[..data.len().min(CHARSET_SIZE)].to_vec();
        bytes.resize(CHARSET_SIZE, 0);
        Charset { bytes, load_address }
    }

    pub fn to_file(&self) -> Vec<u8> {
        let mut out = self.load_address.map(|a| a.to_vec()).unwrap_or_default();
        out.extend(&self.bytes);
        out
    }

    pub fn glyph(&self, code: u8) -> &[u8] {
        &self.bytes[code as usize * 8..code as usize * 8 + 8]
    }

    pub fn glyph_mut(&mut self, code: u8) -> &mut [u8] {
        &mut self.bytes[code as usize * 8..code as usize * 8 + 8]
    }

    /// The pen of a pixel: 0/1 in hires, 0-3 in multicolor.
    pub fn pixel(&self, code: u8, mode: Mode, x: usize, y: usize) -> u8 {
        let bits = mode.bits();
        let shift = 8 - bits * (x + 1);
        (self.glyph(code)[y] >> shift) & ((1 << bits) - 1)
    }

    pub fn set_pixel(&mut self, code: u8, mode: Mode, x: usize, y: usize, pen: u8) {
        let bits = mode.bits();
        let shift = 8 - bits * (x + 1);
        let mask = ((1u8 << bits) - 1) << shift;
        let row = &mut self.glyph_mut(code)[y];
        *row = (*row & !mask) | ((pen << shift) & mask);
    }

    pub fn clear(&mut self, code: u8) {
        self.glyph_mut(code).fill(0);
    }

    pub fn invert(&mut self, code: u8) {
        for row in self.glyph_mut(code) {
            *row = !*row;
        }
    }

    /// Mirrored left to right: the pixels, a bit pair in multicolor staying a pair.
    pub fn flip_x(&mut self, code: u8, mode: Mode) {
        let w = mode.width();
        for y in 0..8 {
            let pens: Vec<u8> = (0..w).map(|x| self.pixel(code, mode, x, y)).collect();
            for (x, pen) in pens.into_iter().rev().enumerate() {
                self.set_pixel(code, mode, x, y, pen);
            }
        }
    }

    pub fn flip_y(&mut self, code: u8) {
        self.glyph_mut(code).reverse();
    }

    /// Shifted by a pixel, what leaves on one side coming in on the other.
    pub fn shift(&mut self, code: u8, mode: Mode, dx: i32, dy: i32) {
        let w = mode.width() as i32;
        let old = self.clone();
        for y in 0..8 {
            for x in 0..w {
                let sx = (x - dx).rem_euclid(w) as usize;
                let sy = (y - dy).rem_euclid(8) as usize;
                self.set_pixel(code, mode, x as usize, y as usize, old.pixel(code, mode, sx, sy));
            }
        }
    }

    /// Turned a quarter clockwise (hires: 8 x 8 is square).
    pub fn rotate(&mut self, code: u8) {
        let old = self.clone();
        for y in 0..8 {
            for x in 0..8 {
                self.set_pixel(code, Mode::Hires, x, y, old.pixel(code, Mode::Hires, y, 7 - x));
            }
        }
    }
}

// --- sprites ----------------------------------------------------------------------------------------

/// The pixels of a C64 sprite: 24 x 21, 3 bytes a row, 64 bytes a block (63 and a pad byte).
pub const SPRITE_COLS: usize = 24;
pub const SPRITE_ROWS: usize = 21;
pub const SPRITE_BLOCK: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFrame {
    pub name: String,
    pub default: bool,
    /// w x h sprite blocks, part by part, row by row
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteLayer {
    pub name: String,
    pub overlay: bool,
    pub multicolor: bool,
    pub color: Option<u8>,
    pub frames: Vec<SpriteFrame>,
}

/// A `.spr` of nerdsuite2 and yopaca (`YSPR`, version 1): the frames of a costume, w x h sprites
/// each, and its layers. The tag of the editor that wrote it (a trailer: the tag, its length, `YTAG`)
/// is kept and written again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprites {
    pub multicolor: bool,
    pub w: usize,
    pub h: usize,
    /// a color a part
    pub colors: Vec<u8>,
    pub frames: Vec<SpriteFrame>,
    pub layers: Vec<SpriteLayer>,
    pub tag: Option<String>,
}

/// The data of a file without the tag of the editor that wrote it, and the tag.
pub(crate) fn read_tag(bytes: &[u8]) -> (&[u8], Option<String>) {
    let n = bytes.len();
    if n < 5 || &bytes[n - 4..] != b"YTAG" {
        return (bytes, None);
    }
    let len = bytes[n - 5] as usize;
    let Some(start) = (n - 5).checked_sub(len).filter(|&s| len <= 64 && s >= 5) else {
        return (bytes, None);
    };
    let tag = &bytes[start..start + len];
    if tag.iter().any(|&b| !(0x20..=0x7e).contains(&b)) {
        return (bytes, None);
    }
    (&bytes[..start], Some(String::from_utf8_lossy(tag).into_owned()))
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.d.get(self.p).ok_or("the file ends too early")?;
        self.p += 1;
        Ok(b)
    }

    fn bytes(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let b = self.d.get(self.p..self.p + n).ok_or("the file ends too early")?.to_vec();
        self.p += n;
        Ok(b)
    }

    fn name(&mut self) -> Result<String, String> {
        let n = self.byte()? as usize;
        Ok(self.bytes(n)?.into_iter().map(|b| b as char).collect())
    }
}

impl Sprites {
    /// A costume of one frame of w x h sprites.
    pub fn new(multicolor: bool, w: usize, h: usize) -> Sprites {
        Sprites {
            multicolor,
            w,
            h,
            colors: vec![],
            frames: vec![SpriteFrame { name: "frame1".into(), default: true, data: vec![0; w * h * SPRITE_BLOCK] }],
            layers: vec![],
            tag: None,
        }
    }

    pub fn decode(data: &[u8]) -> Result<Sprites, String> {
        let (data, tag) = read_tag(data);
        let mut r = Reader { d: data, p: 0 };
        if r.bytes(4)? != b"YSPR" {
            return Err("not a YSPR file".into());
        }
        let version = r.byte()?;
        let flags = r.byte()?;
        if version == 2 {
            let (cols, rows) = (r.byte()? as usize, r.byte()? as usize);
            if (cols, rows) != (SPRITE_COLS, SPRITE_ROWS) {
                return Err(format!("sprites of {cols} x {rows} pixels - the C64's are 24 x 21"));
            }
        } else if version != 1 {
            return Err(format!("version {version}, the editor reads 1 to 2"));
        }
        let (w, h) = (r.byte()? as usize, r.byte()? as usize);
        let n = r.byte()? as usize;
        let colors = r.bytes(n)?;
        let frames = |r: &mut Reader| -> Result<Vec<SpriteFrame>, String> {
            let mut out = Vec::new();
            for _ in 0..r.byte()? {
                let name = r.name()?;
                let default = r.byte()? & 1 != 0;
                out.push(SpriteFrame { name, default, data: r.bytes(w * h * SPRITE_BLOCK)? });
            }
            Ok(out)
        };
        let main = frames(&mut r)?;
        let mut layers = Vec::new();
        for _ in 0..r.byte()? {
            let name = r.name()?;
            let (flags, color) = (r.byte()?, r.byte()?);
            layers.push(SpriteLayer {
                name,
                overlay: flags & 1 != 0,
                multicolor: flags & 2 != 0,
                color: (flags & 4 != 0).then_some(color),
                frames: frames(&mut r)?,
            });
        }
        Ok(Sprites { multicolor: flags & 1 != 0, w, h, colors, frames: main, layers, tag })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut o = b"YSPR".to_vec();
        o.extend([1, self.multicolor as u8, self.w as u8, self.h as u8, self.colors.len() as u8]);
        o.extend(&self.colors);
        let frames = |o: &mut Vec<u8>, list: &[SpriteFrame]| {
            o.push(list.len() as u8);
            for f in list {
                o.push(f.name.len() as u8);
                o.extend(f.name.bytes());
                o.push(f.default as u8);
                o.extend(&f.data);
            }
        };
        frames(&mut o, &self.frames);
        o.push(self.layers.len() as u8);
        for l in &self.layers {
            o.push(l.name.len() as u8);
            o.extend(l.name.bytes());
            o.push(l.overlay as u8 | (l.multicolor as u8) << 1 | (l.color.is_some() as u8) << 2);
            o.push(l.color.unwrap_or(0));
            frames(&mut o, &l.frames);
        }
        if let Some(tag) = &self.tag {
            o.extend(tag.bytes());
            o.push(tag.len() as u8);
            o.extend(b"YTAG");
        }
        o
    }

    /// The pixels of a frame: (columns, rows); a multicolor pixel is two wide.
    pub fn size(&self) -> (usize, usize) {
        let pw = if self.multicolor { SPRITE_COLS / 2 } else { SPRITE_COLS };
        (pw * self.w, SPRITE_ROWS * self.h)
    }

    /// Where the bits of a pixel are: (byte, shift, mask).
    fn locate(&self, x: usize, y: usize) -> (usize, u32, u8) {
        let pw = self.size().0 / self.w;
        let block = ((y / SPRITE_ROWS) * self.w + x / pw) * SPRITE_BLOCK + (y % SPRITE_ROWS) * 3;
        let px = x % pw;
        if self.multicolor { (block + px / 4, (6 - 2 * (px % 4)) as u32, 3) } else { (block + px / 8, (7 - px % 8) as u32, 1) }
    }

    /// The pen of a pixel of a frame: 0/1 in hires; 0 clear, 1 $d025, 2 the sprite color, 3 $d026 in multicolor.
    pub fn pixel(&self, frame: usize, x: usize, y: usize) -> u8 {
        let (at, shift, mask) = self.locate(x, y);
        (self.frames[frame].data[at] >> shift) & mask
    }

    pub fn set_pixel(&mut self, frame: usize, x: usize, y: usize, pen: u8) {
        let (at, shift, mask) = self.locate(x, y);
        let b = &mut self.frames[frame].data[at];
        *b = (*b & !(mask << shift)) | ((pen & mask) << shift);
    }

    /// A frame changed pixel by pixel: `f(x, y)` gives the pen of (x, y) from the frame as it was.
    fn remap(&mut self, frame: usize, f: impl Fn(&Sprites, usize, usize) -> u8) {
        let (w, h) = self.size();
        let old = self.clone();
        for y in 0..h {
            for x in 0..w {
                let pen = f(&old, x, y);
                self.set_pixel(frame, x, y, pen);
            }
        }
    }

    pub fn clear(&mut self, frame: usize) {
        self.frames[frame].data.fill(0);
    }

    pub fn flip_x(&mut self, frame: usize) {
        let w = self.size().0;
        self.remap(frame, |s, x, y| s.pixel(frame, w - 1 - x, y));
    }

    pub fn flip_y(&mut self, frame: usize) {
        let h = self.size().1;
        self.remap(frame, |s, x, y| s.pixel(frame, x, h - 1 - y));
    }

    pub fn shift(&mut self, frame: usize, dx: i32, dy: i32) {
        let (w, h) = self.size();
        self.remap(frame, |s, x, y| {
            s.pixel(frame, (x as i32 - dx).rem_euclid(w as i32) as usize, (y as i32 - dy).rem_euclid(h as i32) as usize)
        });
    }

    /// A name for a new frame: frame1, frame2 … the first free one.
    pub fn free_name(&self, stem: &str) -> String {
        (1..).map(|n| format!("{stem}{n}")).find(|n| !self.frames.iter().any(|f| &f.name == n)).unwrap()
    }
}

// --- screens -----------------------------------------------------------------------------------------

/// A map of characters: w x h codes, and their colors when it has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharMap {
    pub w: usize,
    pub h: usize,
    pub chars: Vec<u8>,
    pub colors: Option<Vec<u8>>,
}

impl CharMap {
    pub fn new(w: usize, h: usize, code: u8, color: Option<u8>) -> CharMap {
        CharMap { w, h, chars: vec![code; w * h], colors: color.map(|c| vec![c; w * h]) }
    }
}

/// How a screen is shown (version 3): its colors, multicolor or extended color mode, its charset
/// (a file of the project, empty: the ROM font), its tileset, where its patches are.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Look {
    pub bg: u8,
    pub border: u8,
    pub text: u8,
    /// $d022, $d023
    pub multicolor: Option<[u8; 2]>,
    /// $d022, $d023, $d024
    pub ecm: Option<[u8; 3]>,
    pub charset: String,
    pub tiles: String,
    /// name, column, row
    pub patches: Vec<(String, u8, u8)>,
    /// the color registers of an Atari screen
    pub registers: Option<Vec<u8>>,
}

/// A `.scr` of nerdsuite2 and yopaca (`YSCR`, versions 1 to 3): the screen, its patches (named
/// maps: doors open and closed …), a tile map, the look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub map: CharMap,
    pub patches: Vec<(String, CharMap)>,
    /// size, w, h, cells
    pub tiles: Option<(u8, u8, u8, Vec<u8>)>,
    pub look: Option<Look>,
    pub tag: Option<String>,
}

impl Reader<'_> {
    fn map(&mut self) -> Result<CharMap, String> {
        let (w, h, flags) = (self.byte()? as usize, self.byte()? as usize, self.byte()?);
        let chars = self.bytes(w * h)?;
        let colors = if flags & 1 != 0 { Some(self.bytes(w * h)?) } else { None };
        Ok(CharMap { w, h, chars, colors })
    }
}

fn put_map(o: &mut Vec<u8>, m: &CharMap) {
    o.extend([m.w as u8, m.h as u8, m.colors.is_some() as u8]);
    o.extend(&m.chars);
    if let Some(c) = &m.colors {
        o.extend(c);
    }
}

fn put_name(o: &mut Vec<u8>, s: &str) {
    o.push(s.len() as u8);
    o.extend(s.bytes());
}

impl Screen {
    pub fn decode(data: &[u8]) -> Result<Screen, String> {
        let (data, tag) = read_tag(data);
        let mut r = Reader { d: data, p: 0 };
        if r.bytes(4)? != b"YSCR" {
            return Err("not a YSCR file".into());
        }
        let v = r.byte()?;
        if !(1..=3).contains(&v) {
            return Err(format!("version {v}, the editor reads 1 to 3"));
        }
        let map = r.map()?;
        let mut patches = Vec::new();
        for _ in 0..r.byte()? {
            let name = r.name()?;
            patches.push((name, r.map()?));
        }
        let mut tiles = None;
        if v >= 2 {
            let (size, w, h) = (r.byte()?, r.byte()?, r.byte()?);
            let cells = r.bytes(w as usize * h as usize)?;
            if size != 0 {
                tiles = Some((size, w, h, cells));
            }
        }
        let mut look = None;
        if v >= 3 {
            let flags = r.byte()?;
            let b = r.bytes(8)?;
            let mut l = Look {
                bg: b[0],
                border: b[1],
                text: b[2],
                multicolor: (flags & 1 != 0).then_some([b[3], b[4]]),
                ecm: (flags & 2 != 0).then_some([b[5], b[6], b[7]]),
                charset: r.name()?,
                tiles: r.name()?,
                ..Default::default()
            };
            for _ in 0..r.byte()? {
                let name = r.name()?;
                l.patches.push((name, r.byte()?, r.byte()?));
            }
            if r.p < r.d.len() {
                let n = r.byte()? as usize;
                l.registers = Some(r.bytes(n)?).filter(|regs| !regs.is_empty());
            }
            look = Some(l);
        }
        Ok(Screen { map, patches, tiles, look, tag })
    }

    pub fn encode(&self) -> Vec<u8> {
        let version = if self.look.is_some() { 3 } else if self.tiles.is_some() { 2 } else { 1 };
        let mut o = b"YSCR".to_vec();
        o.push(version);
        put_map(&mut o, &self.map);
        o.push(self.patches.len() as u8);
        for (name, m) in &self.patches {
            put_name(&mut o, name);
            put_map(&mut o, m);
        }
        match &self.tiles {
            Some((size, w, h, cells)) => {
                o.extend([*size, *w, *h]);
                o.extend(cells);
            }
            None if self.look.is_some() => o.extend([0, 0, 0]),
            None => {}
        }
        if let Some(l) = &self.look {
            let [mc1, mc2] = l.multicolor.unwrap_or([0, 0]);
            let [e1, e2, e3] = l.ecm.unwrap_or([0, 0, 0]);
            o.extend([
                l.multicolor.is_some() as u8 | (l.ecm.is_some() as u8) << 1,
                l.bg,
                l.border,
                l.text,
                mc1,
                mc2,
                e1,
                e2,
                e3,
            ]);
            put_name(&mut o, &l.charset);
            put_name(&mut o, &l.tiles);
            o.push(l.patches.len() as u8);
            for (name, col, row) in &l.patches {
                put_name(&mut o, name);
                o.extend([*col, *row]);
            }
            let regs = l.registers.clone().unwrap_or_default();
            o.push(regs.len() as u8);
            o.extend(regs.iter().map(|v| v & 0x7f));
        }
        if let Some(tag) = &self.tag {
            o.extend(tag.bytes());
            o.push(tag.len() as u8);
            o.extend(b"YTAG");
        }
        o
    }

    /// Where a patch is: its column and row (0, 0 when the look does not say).
    pub fn patch_at(&self, name: &str) -> (usize, usize) {
        self.look
            .as_ref()
            .and_then(|l| l.patches.iter().find(|(n, _, _)| n == name))
            .map_or((0, 0), |(_, c, r)| (*c as usize, *r as usize))
    }

    /// The screen as the VIC-II shows it, a color index a pixel (w*8 x h*8): the charset (2048 bytes),
    /// the cell colors (text color without them), multicolor for a cell whose color has bit 3, extended
    /// color mode by the top bits of a code. `patch`: a patch drawn over the screen at its place.
    pub fn render(&self, charset: &[u8], patch: Option<usize>) -> Vec<u8> {
        let look = self.look.clone().unwrap_or(Look { text: 1, ..Default::default() });
        let (w, h) = (self.map.w, self.map.h);
        let mut out = vec![look.bg & 15; w * 8 * h * 8];
        let mut draw = |m: &CharMap, ox: usize, oy: usize| {
            for cy in 0..m.h {
                for cx in 0..m.w {
                    let (x0, y0) = (ox + cx, oy + cy);
                    if x0 >= w || y0 >= h {
                        continue;
                    }
                    let mut code = m.chars[cy * m.w + cx];
                    let color = m.colors.as_ref().map_or(look.text, |c| c[cy * m.w + cx]) & 15;
                    let mut bg = look.bg & 15;
                    if let Some(ecm) = look.ecm {
                        bg = match code >> 6 {
                            0 => look.bg,
                            k => ecm[k as usize - 1],
                        } & 15;
                        code &= 63;
                    }
                    let glyph = &charset[code as usize * 8..code as usize * 8 + 8];
                    let mc = look.multicolor.filter(|_| color & 8 != 0 && look.ecm.is_none());
                    for (y, row) in glyph.iter().enumerate() {
                        for x in 0..8 {
                            let pen = match mc {
                                Some([m1, m2]) => match (row >> (6 - (x & 6))) & 3 {
                                    0 => bg,
                                    1 => m1 & 15,
                                    2 => m2 & 15,
                                    _ => color & 7,
                                },
                                None if row & (0x80 >> x) != 0 => color,
                                None => bg,
                            };
                            out[(y0 * 8 + y) * w * 8 + x0 * 8 + x] = pen;
                        }
                    }
                }
            }
        };
        draw(&self.map, 0, 0);
        if let Some((name, m)) = patch.and_then(|p| self.patches.get(p)) {
            let (col, row) = self.patch_at(name);
            draw(m, col, row);
        }
        out
    }
}

/// The C64's character ROM (4K: the upper case set, then the lower case one), read from an
/// emulator on this computer: VICE's `C64` folder or mimic's `roms/c64`. NerdSuite has none of its own.
pub fn c64_rom_font() -> Option<Vec<u8>> {
    let apps = crate::run::tools::apps_dir();
    let settings = crate::run::tools::ToolSettings::load();
    let mut dirs = Vec::new();
    if let Some(vice) = crate::run::tools::vice_exe(&settings, "c64") {
        // VICE: bin/x64sc.exe and its C64 folder next to bin
        if let Some(root) = vice.parent().and_then(|bin| bin.parent()) {
            dirs.push(root.join("C64"));
        }
    }
    dirs.push(apps.join("GTK3VICE-3.10-win64-r45942").join("C64"));
    if let Some(mimic) = crate::run::tools::resolve_exe(&settings, &crate::run::tools::mimic_tool()) {
        dirs.push(mimic.parent().unwrap_or(&apps).join("roms").join("c64"));
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("chargen")
                && let Ok(data) = std::fs::read(entry.path())
                && data.len() == 4096
            {
                return Some(data);
            }
        }
    }
    None
}

// --- bitmaps -----------------------------------------------------------------------------------------

pub const KOALA_SIZE: usize = 2 + 8000 + 1000 + 1000 + 1;
pub const ART_SIZE: usize = 2 + 8000 + 1000 + 7;

/// A C64 bitmap picture with the color rules of the VIC-II's bitmap modes (nerdsuite2's `bitmap.js`):
/// hires, 320 x 200, every 8 x 8 cell two colors - the nibbles of its screen byte, high for a set
/// pixel; multicolor, 160 x 200 pixels of double width, every cell three colors of its own - 01 the
/// high nibble of the screen byte, 10 the low one, 11 the color RAM - and 00 the background of the
/// whole picture. Koala (.kla) for multicolor, Art Studio (.art) for hires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub multicolor: bool,
    pub bitmap: Vec<u8>,
    pub screen: Vec<u8>,
    pub color: Vec<u8>,
    pub bg: u8,
    pub load_address: [u8; 2],
    /// the padding of an Art Studio file, written again as it was
    pub padding: Vec<u8>,
}

impl Bitmap {
    pub fn new(multicolor: bool) -> Bitmap {
        Bitmap {
            multicolor,
            bitmap: vec![0; 8000],
            screen: vec![0x10; 1000],
            color: vec![0; 1000],
            bg: 0,
            load_address: if multicolor { [0x00, 0x60] } else { [0x00, 0x20] },
            padding: if multicolor { vec![] } else { vec![0; 7] },
        }
    }

    pub fn from_file(bytes: &[u8]) -> Result<Bitmap, String> {
        let multicolor = match bytes.len() {
            KOALA_SIZE => true,
            ART_SIZE => false,
            n => return Err(format!("{n} bytes: not a Koala ({KOALA_SIZE}) or Art Studio ({ART_SIZE}) picture")),
        };
        Ok(Bitmap {
            multicolor,
            load_address: [bytes[0], bytes[1]],
            bitmap: bytes[2..8002].to_vec(),
            screen: bytes[8002..9002].to_vec(),
            color: if multicolor { bytes[9002..10002].to_vec() } else { vec![0; 1000] },
            bg: if multicolor { bytes[10002] } else { 0 },
            padding: if multicolor { vec![] } else { bytes[9002..].to_vec() },
        })
    }

    pub fn to_file(&self) -> Vec<u8> {
        let mut out = self.load_address.to_vec();
        out.extend(&self.bitmap);
        out.extend(&self.screen);
        if self.multicolor {
            out.extend(&self.color);
            out.push(self.bg);
        } else {
            out.extend(&self.padding);
        }
        out
    }

    /// The pixels: (320, 200) hires, (160, 200) multicolor.
    pub fn size(&self) -> (usize, usize) {
        (if self.multicolor { 160 } else { 320 }, 200)
    }

    /// The byte of the bitmap, the shift of the pixel in it, and its cell.
    fn locate(&self, x: usize, y: usize) -> (usize, u32, usize) {
        let px = if self.multicolor { x * 2 } else { x };
        let cell = (y / 8) * 40 + px / 8;
        let byte = cell * 8 + y % 8;
        let shift = if self.multicolor { 6 - 2 * (x % 4) as u32 } else { 7 - (x % 8) as u32 };
        (byte, shift, cell)
    }

    /// The bits of a pixel: 0/1 hires, 0-3 multicolor.
    pub fn bits(&self, x: usize, y: usize) -> u8 {
        let (byte, shift, _) = self.locate(x, y);
        (self.bitmap[byte] >> shift) & if self.multicolor { 3 } else { 1 }
    }

    fn set_bits(&mut self, x: usize, y: usize, v: u8) {
        let (byte, shift, _) = self.locate(x, y);
        let mask = if self.multicolor { 3u8 } else { 1 } << shift;
        self.bitmap[byte] = (self.bitmap[byte] & !mask) | ((v << shift) & mask);
    }

    /// The color of the slot of a cell: hires 0 background, 1 foreground; multicolor 0-3.
    fn slot(&self, cell: usize, slot: u8) -> u8 {
        match (self.multicolor, slot) {
            (_, 0) if self.multicolor => self.bg & 15,
            (false, 0) => self.screen[cell] & 15,
            (false, _) | (true, 1) => self.screen[cell] >> 4,
            (true, 2) => self.screen[cell] & 15,
            _ => self.color[cell] & 15,
        }
    }

    fn set_slot(&mut self, cell: usize, slot: u8, color: u8) {
        let c = color & 15;
        match (self.multicolor, slot) {
            (true, 0) => self.bg = c,
            (false, 0) | (true, 2) => self.screen[cell] = (self.screen[cell] & 0xf0) | c,
            (false, _) | (true, 1) => self.screen[cell] = (self.screen[cell] & 0x0f) | c << 4,
            _ => self.color[cell] = c,
        }
    }

    /// The color a pixel shows.
    pub fn color_at(&self, x: usize, y: usize) -> u8 {
        let (_, _, cell) = self.locate(x, y);
        self.slot(cell, self.bits(x, y))
    }

    /// Paints a pixel. Hires: `foreground` sets it, else clears it, and its cell takes the color
    /// for that slot. Multicolor: the background (or `!foreground`) clears it; a color uses the
    /// slot of the cell that holds it, else a free one (a slot no pixel of the cell shows), else
    /// recolors the slot it paints over - all its pixels in the cell. Over the background with three
    /// other colors in the cell it leaves the pixel alone: false, a color clash.
    pub fn paint(&mut self, x: usize, y: usize, color: u8, foreground: bool) -> bool {
        let color = color & 15;
        let (_, _, cell) = self.locate(x, y);
        if !self.multicolor {
            let v = foreground as u8;
            self.set_bits(x, y, v);
            self.set_slot(cell, v, color);
            return true;
        }
        if !foreground || color == self.bg & 15 {
            self.set_bits(x, y, 0);
            return true;
        }
        if let Some(slot) = (1..=3).find(|&s| self.slot(cell, s) == color) {
            self.set_bits(x, y, slot);
            return true;
        }
        // the slots the pixels of the cell show
        let (cx, cy) = ((cell % 40) * 4, (cell / 40) * 8);
        let mut used = [false; 4];
        for yy in cy..cy + 8 {
            for xx in cx..cx + 4 {
                used[self.bits(xx, yy) as usize] = true;
            }
        }
        if let Some(slot) = (1..=3u8).find(|&s| !used[s as usize]) {
            self.set_slot(cell, slot, color);
            self.set_bits(x, y, slot);
            return true;
        }
        match self.bits(x, y) {
            0 => false,
            slot => {
                self.set_slot(cell, slot, color);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_picture_round_trips() {
        let data = demo("gfx/bitmap/yopaca.kla");
        let b = Bitmap::from_file(&data).unwrap();
        assert!(b.multicolor);
        assert_eq!(b.size(), (160, 200));
        assert_eq!(b.to_file(), data);
        assert!(Bitmap::from_file(&data[..100]).is_err());
        let art = Bitmap::new(false);
        assert_eq!(art.to_file().len(), ART_SIZE);
        assert_eq!(Bitmap::from_file(&art.to_file()).unwrap(), art);
    }

    #[test]
    fn hires_cells_have_two_colors() {
        let mut b = Bitmap::new(false);
        b.paint(9, 0, 2, true);
        assert_eq!(b.bitmap[8], 0x40);
        assert_eq!(b.color_at(9, 0), 2);
        assert_eq!(b.color_at(10, 0), 0); // the background of the cell
        b.paint(10, 0, 5, false); // the background slot of the cell
        assert_eq!(b.color_at(10, 0), 5);
        assert_eq!(b.color_at(9, 0), 2);
    }

    #[test]
    fn multicolor_slots_and_clashes() {
        let mut b = Bitmap::new(true);
        b.screen.fill(0);
        b.bg = 6;
        // three colors take the three free slots of the cell
        assert!(b.paint(0, 0, 2, true));
        assert!(b.paint(1, 0, 5, true));
        assert!(b.paint(2, 0, 7, true));
        assert_eq!((b.color_at(0, 0), b.color_at(1, 0), b.color_at(2, 0)), (2, 5, 7));
        // a color the cell has: its slot
        assert!(b.paint(3, 0, 5, true));
        assert_eq!(b.bits(3, 0), b.bits(1, 0));
        // a fourth color over the background: a clash, the pixel stays
        assert!(!b.paint(0, 1, 1, true));
        assert_eq!(b.color_at(0, 1), 6);
        // over a color: its slot is recolored, all its pixels in the cell
        assert!(b.paint(1, 0, 1, true));
        assert_eq!((b.color_at(1, 0), b.color_at(3, 0)), (1, 1));
        // the background
        assert!(b.paint(2, 0, 6, true));
        assert_eq!(b.bits(2, 0), 0);
    }

    fn demo(path: &str) -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo").join(path)).unwrap()
    }

    #[test]
    fn the_demo_screens_round_trip() {
        for name in ["gfx/screen/hall.scr", "gfx/screen/town.scr"] {
            let data = demo(name);
            let s = Screen::decode(&data).unwrap();
            assert_eq!((s.map.w, s.map.h), (40, 17), "{name}");
            assert_eq!(s.encode(), data, "{name}");
        }
        let hall = Screen::decode(&demo("gfx/screen/hall.scr")).unwrap();
        assert_eq!(hall.patches.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["door_open", "door_closed"]);
        assert_eq!(hall.look.as_ref().unwrap().charset, "gfx/charset/chars.chr");
        assert_eq!(hall.patch_at("door_open"), (25, 3));
    }

    #[test]
    fn rendering_hires_multicolor_ecm() {
        let mut charset = vec![0u8; 2048];
        charset[8] = 0b1100_0000; // char 1: the top row: a pixel pair
        let mut s = Screen {
            map: CharMap { w: 1, h: 1, chars: vec![1], colors: Some(vec![2]) },
            patches: vec![],
            tiles: None,
            look: Some(Look { bg: 6, text: 1, ..Default::default() }),
            tag: None,
        };
        let px = s.render(&charset, None);
        assert_eq!(&px[..3], &[2, 2, 6]);
        // multicolor: a color with bit 3, the pair 11 is the cell color 0-7
        s.map.colors = Some(vec![8 + 5]);
        s.look.as_mut().unwrap().multicolor = Some([11, 12]);
        assert_eq!(&s.render(&charset, None)[..3], &[5, 5, 6]);
        // ecm: the top bits of the code choose the background
        s.look.as_mut().unwrap().multicolor = None;
        s.look.as_mut().unwrap().ecm = Some([3, 4, 7]);
        s.map.chars = vec![0x41];
        s.map.colors = Some(vec![2]);
        assert_eq!(&s.render(&charset, None)[..3], &[2, 2, 3]);
    }

    #[test]
    fn the_demo_sprites_round_trip() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/gfx/sprite/Dave.spr");
        let data = std::fs::read(path).unwrap();
        let s = Sprites::decode(&data).unwrap();
        assert!(s.multicolor);
        assert_eq!((s.w, s.h), (1, 2));
        assert_eq!(s.frames[0].name, "left3");
        assert_eq!(s.tag.as_deref(), Some("v1.7-6-ged7395b"));
        assert_eq!(s.size(), (12, 42));
        assert_eq!(s.encode(), data);
    }

    #[test]
    fn sprite_pixels_and_operations() {
        let mut s = Sprites::new(false, 2, 1);
        assert_eq!(s.size(), (48, 21));
        s.set_pixel(0, 0, 0, 1);
        s.set_pixel(0, 24, 20, 1); // the second sprite: its block, its last row
        assert_eq!(s.frames[0].data[0], 0x80);
        assert_eq!(s.frames[0].data[64 + 20 * 3], 0x80);
        s.flip_x(0);
        assert_eq!(s.pixel(0, 47, 0), 1);
        assert_eq!(s.pixel(0, 23, 20), 1);
        s.shift(0, 1, 1);
        assert_eq!(s.pixel(0, 0, 1), 1); // wrapped around
        let mut m = Sprites::new(true, 1, 1);
        m.set_pixel(0, 1, 0, 2);
        assert_eq!(m.frames[0].data[0], 0b0010_0000);
        assert_eq!(m.free_name("frame"), "frame2");
        // a file without a tag stays without one
        assert_eq!(Sprites::decode(&m.encode()).unwrap(), m);
    }

    #[test]
    fn files_and_load_addresses() {
        let c = Charset::from_file(&[0u8; 2048]);
        assert_eq!(c.load_address, None);
        let mut prg = vec![0x00, 0x30];
        prg.extend([0xffu8; 2048]);
        let c = Charset::from_file(&prg);
        assert_eq!(c.load_address, Some([0x00, 0x30]));
        assert_eq!(c.glyph(0), &[0xff; 8]);
        assert_eq!(c.to_file(), prg);
        // a half charset is padded
        assert_eq!(Charset::from_file(&[1u8; 1024]).bytes.len(), 2048);
    }

    #[test]
    fn hires_and_multicolor_pixels() {
        let mut c = Charset::default();
        c.set_pixel(1, Mode::Hires, 0, 0, 1);
        c.set_pixel(1, Mode::Hires, 7, 7, 1);
        assert_eq!(c.glyph(1)[0], 0x80);
        assert_eq!(c.glyph(1)[7], 0x01);
        assert_eq!(c.pixel(1, Mode::Hires, 7, 7), 1);
        c.set_pixel(2, Mode::Multicolor, 1, 3, 2);
        assert_eq!(c.glyph(2)[3], 0b0010_0000);
        assert_eq!(c.pixel(2, Mode::Multicolor, 1, 3), 2);
        c.set_pixel(2, Mode::Multicolor, 1, 3, 0);
        assert_eq!(c.glyph(2)[3], 0);
    }

    #[test]
    fn operations() {
        let mut c = Charset::default();
        c.glyph_mut(0).copy_from_slice(&[0x80, 0, 0, 0, 0, 0, 0, 0x01]);
        c.flip_x(0, Mode::Hires);
        assert_eq!(c.glyph(0), &[0x01, 0, 0, 0, 0, 0, 0, 0x80]);
        c.flip_y(0);
        assert_eq!(c.glyph(0), &[0x80, 0, 0, 0, 0, 0, 0, 0x01]);
        c.shift(0, Mode::Hires, 1, 0);
        assert_eq!(c.glyph(0), &[0x40, 0, 0, 0, 0, 0, 0, 0x80]);
        c.shift(0, Mode::Hires, 0, -1);
        assert_eq!(c.glyph(0), &[0, 0, 0, 0, 0, 0, 0x80, 0x40]);
        c.invert(0);
        assert_eq!(c.glyph(0)[0], 0xff);
        c.clear(0);
        c.glyph_mut(0)[0] = 0xff; // the top row turns into the right column
        c.rotate(0);
        assert!(c.glyph(0).iter().all(|&r| r == 0x01));
        // multicolor: pairs stay pairs
        let mut m = Charset::default();
        m.glyph_mut(0)[0] = 0b0110_0000;
        m.flip_x(0, Mode::Multicolor);
        assert_eq!(m.glyph(0)[0], 0b0000_1001);
    }
}
