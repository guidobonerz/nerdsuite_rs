//! The song of the Composer, after nerdsuite2's `song.js`: a tracker song for the three voices of
//! the SID - or more, up to [`MAX_VOICES`], of an imported MIDI file: the editor plays them on a SID
//! for every three, the game plays up to three of them (`game`).
//!   instruments  1-15: a waveform, the envelope, the pulse width with a sweep, a vibrato, an
//!                arpeggio and the filter; rows of the four tables they start at
//!   patterns     rows of a cell per voice; a cell is a note (or the note off), an instrument and a
//!                command with its value
//!   orders       the patterns in the order the song plays them, with the position it loops to
//!   tempo        the frames (1/50 s) a row lasts
//!   tables       GoatTracker's four tables, shared by the instruments: rows of two bytes (left,
//!                right), numbered from 1 (see `sid.rs` for what they do)
//!
//! The file (.sng, `YSNG`) is the song as a player reads it:
//!   0 'YSNG'  4 version  5 tempo  6 rows of a pattern  7 instruments  8 patterns  9 orders
//!   10 loop position ($ff: the song ends)  11 version 3 and up: the voices, else 0 - three
//!   12 the orders; the instruments (12 bytes, 16 from version 4: the rows of their tables and
//!   the delay of the vibrato); the patterns (rows x voices x 4 bytes: note 0 none, 1-95 C-0 to
//!   A#7, $ff off; instrument; command; value); the names of the instruments (12 bytes each);
//!   version 3 and up: the 3 voices the game plays ($ff none); version 4 and up: the rows of the
//!   four tables (a byte each) and their rows (left, right).
//! Version 5 keeps the sync and ring bits ($02, $04) of an instrument's waveform. A song is written
//! as the lowest version that holds it; version 1 (no arpeggio, filter, commands: instruments of 8
//! bytes, cells of 2) is read. The tag of the editor that wrote a file is kept.

use crate::gfx::read_tag;

pub const VOICES: usize = 3;
pub const NOTE_OFF: u8 = 0xff;
/// C-0 to A#7: a B-7 does not fit the 16 bit frequency
pub const NOTES: u8 = 95;
/// of a song in the editor: eight SIDs
pub const MAX_VOICES: usize = 24;
pub const MAX_INSTRUMENTS: usize = 15;
pub const MAX_PATTERNS: usize = 64;
pub const MAX_ORDERS: usize = 128;
pub const MAX_TABLE_ROWS: usize = 255;
pub const ROW_CHOICES: [usize; 4] = [16, 32, 48, 64];
/// the bytes of a cell: note, instrument, command, value
pub const CELL: usize = 4;
/// the PAL clock the frequencies are counted with
pub const CLOCK: f64 = 985248.0;

pub const TRI: u8 = 0x10;
pub const SAW: u8 = 0x20;
pub const PULSE: u8 = 0x40;
pub const NOISE: u8 = 0x80;
/// the bits of a waveform besides the waveform: sync and ring modulation with the voice before
pub const SYNC: u8 = 0x02;
pub const RING: u8 = 0x04;
const WAVE_KEPT: u8 = 0xf0 | SYNC | RING;

pub const WAVES: [(u8, &str); 4] = [(TRI, "triangle"), (SAW, "sawtooth"), (PULSE, "pulse"), (NOISE, "noise")];
pub const FILTERS: [(u8, &str); 4] = [(0, "off"), (1, "low pass"), (2, "band pass"), (4, "high pass")];

// the commands of a cell. A slide and a glide go on until the next note or slide of the voice (a
// slide of 00 ends it); an arpeggio wins over them
pub const CMD_UP: u8 = 1;
pub const CMD_DOWN: u8 = 2;
pub const CMD_GLIDE: u8 = 3;
pub const CMD_ARP: u8 = 4;
pub const CMD_CUTOFF: u8 = 5;
pub const CMD_PULSE: u8 = 6;
pub const CMD_VOLUME: u8 = 7;
pub const CMD_TEMPO: u8 = 8;
pub const CMD_BREAK: u8 = 9;
// the commands of the speed table: the value is a row of it (a 16 bit speed, a vibrato)
pub const CMD_UP_T: u8 = 10;
pub const CMD_DOWN_T: u8 = 11;
pub const CMD_GLIDE_T: u8 = 12;
pub const CMD_VIB_T: u8 = 13;

/// The commands: value, name, what it does.
pub const COMMANDS: [(u8, &str, &str); 13] = [
    (CMD_UP, "slide up", "the pitch rises by the value every frame, until the next note"),
    (CMD_DOWN, "slide down", "the pitch falls by the value every frame, until the next note"),
    (CMD_GLIDE, "glide", "the note of the cell is not struck: the pitch moves to it by the value a frame"),
    (CMD_ARP, "arpeggio", "the note, then the two notes the nibbles of the value name above it, a frame each (00 ends it)"),
    (CMD_CUTOFF, "cutoff", "the cutoff of the filter"),
    (CMD_PULSE, "pulse width", "the pulse width: the value x 16"),
    (CMD_VOLUME, "volume", "the volume of the SID, 0-F"),
    (CMD_TEMPO, "tempo", "the frames a row lasts from here on"),
    (CMD_BREAK, "pattern end", "the next pattern of the orders starts after this row"),
    (CMD_UP_T, "slide up (table)", "a slide up by the 16 bit speed of the row of the speed table"),
    (CMD_DOWN_T, "slide down (table)", "a slide down by the 16 bit speed of the row of the speed table"),
    (CMD_GLIDE_T, "glide (table)", "a glide by the 16 bit speed of the row of the speed table"),
    (CMD_VIB_T, "vibrato (table)", "the vibrato of the row of the speed table (00 ends it)"),
];

/// The four tables, in the order of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    Wave,
    Pulse,
    Filter,
    Speed,
}

impl Table {
    pub const ALL: [Table; 4] = [Table::Wave, Table::Pulse, Table::Filter, Table::Speed];

    pub fn label(self) -> &'static str {
        match self {
            Table::Wave => "wave",
            Table::Pulse => "pulse",
            Table::Filter => "filter",
            Table::Speed => "speed",
        }
    }
}

const NAMES: [&str; 12] = ["C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-"];
pub const NOTE_NAMES: [&str; 12] = NAMES;

/// The name of a note of a cell: C-4, === (off), --- (none).
pub fn note_name(n: u8) -> String {
    match n {
        NOTE_OFF => "===".into(),
        0 => "---".into(),
        n => format!("{}{}", NAMES[(n as usize - 1) % 12], (n - 1) / 12),
    }
}

/// The frequency register of note 0 (C-0) and up, A-4 is 440 Hz; one more for the vibrato of the last.
pub fn freq(n: usize) -> u16 {
    static TABLE: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        (0..=NOTES as i32)
            .map(|n| (440. * 2f64.powf((n - 57) as f64 / 12.) * 16777216. / CLOCK).round().min(65535.) as u16)
            .collect()
    });
    t[n.min(NOTES as usize)]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instrument {
    pub name: String,
    pub wave: u8,
    pub a: u8,
    pub d: u8,
    pub s: u8,
    pub r: u8,
    /// 12 bits
    pub pulse: u16,
    /// the pulse sweep a frame
    pub pwm: u8,
    pub vib_depth: u8,
    pub vib_speed: u8,
    pub arp1: u8,
    pub arp2: u8,
    /// 0, 1 low, 2 band, 4 high pass
    pub filter: u8,
    pub cutoff: u8,
    pub res: u8,
    /// the cutoff a frame
    pub sweep: i8,
    pub wave_tab: u8,
    pub pulse_tab: u8,
    pub filter_tab: u8,
    pub vib_tab: u8,
    pub vib_delay: u8,
}

impl Default for Instrument {
    fn default() -> Self {
        Instrument {
            name: "instrument".into(),
            wave: PULSE,
            a: 0,
            d: 9,
            s: 8,
            r: 9,
            pulse: 2048,
            pwm: 0,
            vib_depth: 0,
            vib_speed: 0,
            arp1: 0,
            arp2: 0,
            filter: 0,
            cutoff: 128,
            res: 0,
            sweep: 0,
            wave_tab: 0,
            pulse_tab: 0,
            filter_tab: 0,
            vib_tab: 0,
            vib_delay: 0,
        }
    }
}

/// An instrument: its name, waveform and envelope, the rest as `rest` says.
fn ins(name: &str, wave: u8, [a, d, s, r]: [u8; 4], rest: impl FnOnce(&mut Instrument)) -> Instrument {
    let mut i = Instrument { name: name.into(), wave, a, d, s, r, ..Default::default() };
    rest(&mut i);
    i
}

/// The ready instruments the editor adds, by group. The SID has one filter: the last note of an
/// instrument with a filter sets it for every voice it sends through it.
pub fn presets() -> Vec<(&'static str, Vec<Instrument>)> {
    vec![
        ("Leads", vec![
            ins("pwm lead", PULSE, [0, 9, 10, 8], |i| { i.pulse = 0x400; i.pwm = 40; i.vib_depth = 3; i.vib_speed = 5; }),
            ins("saw lead", SAW, [0, 8, 11, 8], |i| { i.vib_depth = 2; i.vib_speed = 5; i.filter = 1; i.cutoff = 180; i.res = 6; }),
            ins("chip square", PULSE, [0, 0, 15, 4], |i| i.pulse = 0x800),
            ins("flute", TRI, [3, 8, 12, 8], |i| { i.vib_depth = 2; i.vib_speed = 4; }),
        ]),
        ("Basses", vec![
            ins("fat bass", PULSE, [0, 9, 8, 6], |i| { i.pulse = 0x300; i.pwm = 16; i.filter = 1; i.cutoff = 60; i.res = 10; }),
            ins("acid bass", SAW, [0, 6, 4, 4], |i| { i.filter = 1; i.cutoff = 160; i.res = 12; i.sweep = -4; }),
            ins("sub bass", TRI, [0, 10, 10, 6], |_| {}),
            ins("pluck bass", PULSE, [0, 6, 0, 6], |i| i.pulse = 0x800),
        ]),
        ("Arpeggios", vec![
            ins("major arp", PULSE, [0, 9, 10, 8], |i| { i.pulse = 0x600; i.pwm = 20; i.arp1 = 4; i.arp2 = 7; }),
            ins("minor arp", PULSE, [0, 9, 10, 8], |i| { i.pulse = 0x600; i.pwm = 20; i.arp1 = 3; i.arp2 = 7; }),
            ins("power chord", SAW, [0, 9, 10, 8], |i| { i.arp1 = 7; i.arp2 = 12; }),
        ]),
        ("Pads and keys", vec![
            ins("string pad", SAW, [8, 10, 12, 11], |i| { i.vib_depth = 2; i.vib_speed = 3; i.filter = 1; i.cutoff = 120; i.res = 4; }),
            ins("choir pad", PULSE, [9, 10, 12, 11], |i| { i.pulse = 0x800; i.pwm = 8; i.vib_depth = 1; i.vib_speed = 3; }),
            ins("sweep pad", PULSE, [6, 10, 12, 10], |i| { i.pulse = 0x500; i.pwm = 12; i.filter = 1; i.cutoff = 20; i.res = 8; i.sweep = 1; }),
            ins("e-piano", PULSE, [0, 10, 4, 9], |i| { i.pulse = 0x300; i.vib_depth = 1; i.vib_speed = 6; }),
            ins("organ", PULSE, [0, 0, 15, 4], |i| { i.pulse = 0x800; i.vib_depth = 1; i.vib_speed = 7; }),
            ins("bell", TRI, [0, 11, 0, 11], |i| { i.vib_depth = 1; i.vib_speed = 6; }),
            ins("marimba", TRI, [0, 6, 0, 6], |_| {}),
        ]),
        ("Drums", vec![
            ins("kick", TRI, [0, 5, 0, 5], |i| { i.filter = 1; i.cutoff = 60; i.res = 4; i.sweep = -8; }),
            ins("snare", NOISE, [0, 6, 0, 6], |i| { i.filter = 2; i.cutoff = 140; i.res = 4; }),
            ins("tom", TRI, [0, 7, 0, 7], |i| { i.filter = 1; i.cutoff = 90; i.sweep = -3; }),
            ins("closed hat", NOISE, [0, 2, 0, 2], |i| { i.filter = 4; i.cutoff = 200; }),
            ins("open hat", NOISE, [0, 7, 0, 7], |i| { i.filter = 4; i.cutoff = 200; }),
        ]),
        ("Effects", vec![
            ins("zap", PULSE, [0, 7, 0, 6], |i| { i.pulse = 0x200; i.filter = 2; i.cutoff = 255; i.res = 15; i.sweep = -12; }),
            ins("laser", SAW, [0, 6, 0, 5], |i| { i.arp1 = 12; i.arp2 = 7; i.filter = 1; i.cutoff = 255; i.res = 10; i.sweep = -20; }),
            ins("shot", NOISE, [0, 5, 0, 5], |i| { i.filter = 1; i.cutoff = 255; i.res = 6; i.sweep = -24; }),
            ins("explosion", NOISE, [0, 11, 0, 11], |i| { i.filter = 1; i.cutoff = 140; i.res = 10; i.sweep = -2; }),
            ins("hit", NOISE, [0, 4, 0, 4], |i| { i.filter = 1; i.cutoff = 90; i.res = 6; i.sweep = -6; }),
            ins("coin", PULSE, [0, 8, 0, 8], |i| { i.pulse = 0x800; i.arp1 = 5; }),
            ins("power up", PULSE, [0, 9, 6, 8], |i| { i.pulse = 0x400; i.pwm = 30; i.arp1 = 7; i.arp2 = 12; i.vib_depth = 2; i.vib_speed = 8; }),
            ins("blip", PULSE, [0, 2, 0, 2], |i| i.pulse = 0x800),
            ins("alarm", PULSE, [0, 0, 15, 3], |i| { i.pulse = 0x800; i.arp1 = 3; }),
            ins("ufo", PULSE, [2, 0, 15, 8], |i| { i.pulse = 0x400; i.pwm = 80; i.vib_depth = 15; i.vib_speed = 15; }),
        ]),
        ("Ambience", vec![
            ins("wind", NOISE, [10, 10, 10, 11], |i| { i.filter = 2; i.cutoff = 60; i.res = 12; i.sweep = 1; }),
            ins("thunder", NOISE, [2, 13, 0, 13], |i| { i.filter = 1; i.cutoff = 50; i.res = 12; i.sweep = -1; }),
            ins("surf", NOISE, [11, 11, 6, 12], |i| { i.filter = 1; i.cutoff = 30; i.res = 6; i.sweep = 1; }),
            ins("rain", NOISE, [4, 0, 6, 8], |i| { i.filter = 4; i.cutoff = 220; i.res = 2; }),
            ins("engine", PULSE, [2, 0, 15, 6], |i| { i.pulse = 0x100; i.pwm = 60; i.filter = 1; i.cutoff = 40; i.res = 10; }),
            ins("bubble", TRI, [0, 5, 0, 5], |i| { i.filter = 2; i.cutoff = 30; i.res = 15; i.sweep = 12; }),
            ins("bird", TRI, [1, 6, 0, 5], |i| { i.arp1 = 12; i.vib_depth = 15; i.vib_speed = 12; }),
            ins("heartbeat", TRI, [0, 4, 0, 4], |i| { i.filter = 1; i.cutoff = 30; i.res = 8; i.sweep = -2; }),
        ]),
    ]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    pub tempo: u8,
    pub rows: usize,
    /// the position the song loops to; None: it ends
    pub loop_to: Option<usize>,
    pub voices: usize,
    /// the voices of the song the game plays, up to three
    pub game: Vec<usize>,
    pub orders: Vec<usize>,
    /// rows x voices x [`CELL`] bytes
    pub patterns: Vec<Vec<u8>>,
    pub instruments: Vec<Instrument>,
    /// wave, pulse, filter, speed
    pub tables: [Vec<[u8; 2]>; 4],
    pub tag: Option<String>,
}

pub fn new_pattern(rows: usize, voices: usize) -> Vec<u8> {
    vec![0; rows * voices * CELL]
}

/// The cell of a voice in a row: its offset in a pattern of `voices` voices.
pub fn cell_offset(row: usize, voice: usize, voices: usize) -> usize {
    (row * voices + voice) * CELL
}

const MAGIC: &[u8; 4] = b"YSNG";
const NAME_LENGTH: usize = 12;
const HEADER: usize = 12;
const VERSION: u8 = 5;
const INS_BYTES: usize = 12;
const INS_BYTES_4: usize = 16;
const NO_VOICE: u8 = 0xff;
const NO_LOOP: u8 = 0xff;
/// the commands of the other voices a SID of a song of more voices keeps in step with
const GLOBAL_COMMANDS: [u8; 3] = [CMD_TEMPO, CMD_BREAK, CMD_VOLUME];

impl Default for Song {
    fn default() -> Self {
        Song {
            tempo: 6,
            rows: 32,
            loop_to: Some(0),
            voices: VOICES,
            game: vec![0, 1, 2],
            orders: vec![0],
            patterns: vec![new_pattern(32, VOICES)],
            tables: Default::default(),
            instruments: vec![
                ins("lead", PULSE, [0, 9, 10, 9], |i| { i.pulse = 2048; i.pwm = 24; i.vib_depth = 3; i.vib_speed = 5; }),
                ins("bass", SAW, [0, 8, 6, 6], |_| {}),
                ins("soft", TRI, [2, 8, 12, 10], |i| { i.vib_depth = 2; i.vib_speed = 4; }),
                ins("hihat", NOISE, [0, 3, 0, 3], |_| {}),
            ],
            tag: None,
        }
    }
}

impl Song {
    pub fn table(&self, t: Table) -> &Vec<[u8; 2]> {
        &self.tables[t as usize]
    }

    pub fn table_mut(&mut self, t: Table) -> &mut Vec<[u8; 2]> {
        &mut self.tables[t as usize]
    }

    /// A row of a table, numbered from 1; None past its end.
    pub fn table_row(&self, t: Table, n: u8) -> Option<[u8; 2]> {
        (n > 0).then(|| self.tables[t as usize].get(n as usize - 1).copied()).flatten()
    }

    pub fn uses_ring_sync(&self) -> bool {
        self.instruments.iter().any(|i| i.wave & (SYNC | RING) != 0)
    }

    /// A song that needs the tables: an instrument points into one, a table has rows, a command of
    /// the speed table plays - it is written as version 4.
    pub fn uses_tables(&self) -> bool {
        self.tables.iter().any(|t| !t.is_empty())
            || self.instruments.iter().any(|i| i.wave_tab != 0 || i.pulse_tab != 0 || i.filter_tab != 0 || i.vib_tab != 0)
            || self.patterns.iter().any(|p| p.chunks(CELL).any(|c| (CMD_UP_T..=CMD_VIB_T).contains(&c[2])))
    }

    /// The game plays the voices of the song as they are.
    pub fn plain(&self) -> bool {
        self.voices == VOICES && self.game == [0, 1, 2]
    }

    /// Patterns of `rows` rows: shorter ones are filled with empty rows, longer ones cut.
    pub fn set_rows(&mut self, rows: usize) {
        for p in &mut self.patterns {
            p.resize(rows * self.voices * CELL, 0);
        }
        self.rows = rows;
    }

    /// A song of three voices: the voices `list` of the song (None: a silent voice), with the same
    /// orders and instruments. globals: the tempo, pattern end and volume commands of the other
    /// voices come along, in a cell without a command - the SIDs of a song of more voices keep in step.
    pub fn pick_voices(&self, list: &[usize], globals: bool) -> Song {
        let n = self.voices;
        let voices: Vec<Option<usize>> = (0..VOICES).map(|k| list.get(k).copied().filter(|&v| v < n)).collect();
        let patterns = self
            .patterns
            .iter()
            .map(|p| {
                let mut q = new_pattern(self.rows, VOICES);
                for r in 0..self.rows {
                    for (k, v) in voices.iter().enumerate() {
                        if let Some(v) = *v {
                            let o = cell_offset(r, v, n);
                            q[cell_offset(r, k, VOICES)..][..CELL].copy_from_slice(&p[o..o + CELL]);
                        }
                    }
                    if !globals {
                        continue;
                    }
                    for v in 0..n {
                        let o = cell_offset(r, v, n);
                        if voices.contains(&Some(v)) || !GLOBAL_COMMANDS.contains(&p[o + 2]) {
                            continue;
                        }
                        if let Some(free) = (0..VOICES).find(|&k| q[cell_offset(r, k, VOICES) + 2] == 0) {
                            let f = cell_offset(r, free, VOICES) + 2;
                            q[f] = p[o + 2];
                            q[f + 1] = p[o + 3];
                        }
                    }
                }
                q
            })
            .collect();
        Song { voices: VOICES, game: vec![0, 1, 2], patterns, ..self.clone() }
    }

    /// The song as the game plays it: its game voices.
    pub fn game_song(&self) -> Song {
        if self.plain() { self.clone() } else { self.pick_voices(&self.game.clone(), false) }
    }

    /// The notes of the cells [r0..=r1] x [v0..=v1] of a pattern moved by semitones; notes that
    /// would leave the range stay.
    pub fn transpose(&mut self, pattern: usize, (r0, r1): (usize, usize), (v0, v1): (usize, usize), semitones: i32) {
        let voices = self.voices;
        let p = &mut self.patterns[pattern];
        for r in r0..=r1 {
            for v in v0..=v1 {
                let o = cell_offset(r, v, voices);
                let n = p[o] as i32;
                if n != 0 && n != NOTE_OFF as i32 && (1..=NOTES as i32).contains(&(n + semitones)) {
                    p[o] = (n + semitones) as u8;
                }
            }
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        // empty patterns at the end that no order plays are not written
        let mut np = self.patterns.len();
        while np > 1 && self.patterns[np - 1].iter().all(|&b| b == 0) && !self.orders.contains(&(np - 1)) {
            np -= 1;
        }
        let (ni, no, nv) = (self.instruments.len(), self.orders.len(), self.voices);
        let version = if self.uses_ring_sync() {
            5
        } else if self.uses_tables() {
            4
        } else if self.plain() {
            2
        } else {
            3
        };
        let mut o = Vec::new();
        o.extend_from_slice(MAGIC);
        let looped = match self.loop_to {
            None => NO_LOOP,
            Some(l) => l.min(no - 1) as u8,
        };
        o.extend([version, self.tempo, self.rows as u8, ni as u8, np as u8, no as u8, looped, if version > 2 { nv as u8 } else { 0 }]);
        o.extend(self.orders.iter().map(|&p| p as u8));
        for i in &self.instruments {
            let wave = i.wave & if version > 4 { WAVE_KEPT } else { 0xf0 };
            let filter = if i.filter != 0 { (i.res << 4) | (i.filter & 7) } else { 0 };
            o.extend([wave, (i.a << 4) | i.d, (i.s << 4) | i.r, i.pulse as u8, ((i.pulse >> 8) & 15) as u8, i.pwm]);
            o.extend([(i.vib_depth << 4) | i.vib_speed, (i.arp1 << 4) | i.arp2, filter, i.cutoff, i.sweep as u8]);
            if version > 3 {
                o.extend([i.wave_tab, i.pulse_tab, i.filter_tab, i.vib_tab, i.vib_delay]);
            } else {
                o.push(0);
            }
        }
        let size = self.rows * nv * CELL;
        for p in &self.patterns[..np] {
            o.extend_from_slice(&p[..size]);
        }
        for i in &self.instruments {
            let name = i.name.as_bytes();
            o.extend((0..NAME_LENGTH).map(|k| name.get(k).map_or(b' ', |&c| c & 127)));
        }
        if version > 2 {
            o.extend((0..VOICES).map(|k| self.game.get(k).map_or(NO_VOICE, |&v| v as u8)));
        }
        if version > 3 {
            let tables: Vec<&[[u8; 2]]> = self.tables.iter().map(|t| &t[..t.len().min(MAX_TABLE_ROWS)]).collect();
            o.extend(tables.iter().map(|t| t.len() as u8));
            for t in tables {
                o.extend(t.iter().flatten());
            }
        }
        if let Some(tag) = &self.tag {
            o.extend(tag.bytes());
            o.push(tag.len() as u8);
            o.extend_from_slice(b"YTAG");
        }
        o
    }

    pub fn decode(bytes: &[u8]) -> Result<Song, String> {
        let (b, tag) = read_tag(bytes);
        if b.len() < HEADER || &b[..4] != MAGIC {
            return Err("not a song of the Composer".into());
        }
        let version = b[4];
        if !(1..=VERSION).contains(&version) {
            return Err(format!("song version {version} is not known"));
        }
        let [tempo, rows, ni, np, no, looped] = [b[5], b[6], b[7], b[8], b[9], b[10]].map(usize::from);
        let voices = if version > 2 { b[11] as usize } else { VOICES };
        if !(VOICES..=MAX_VOICES).contains(&voices) {
            return Err(format!("a song of {voices} voices"));
        }
        let cell = if version == 1 { 2 } else { CELL };
        let ins_bytes = match version {
            1 => 8,
            4.. => INS_BYTES_4,
            _ => INS_BYTES,
        };
        let size = rows * voices * cell;
        let cut = || "the song file is cut short".to_string();
        if rows == 0 || np == 0 || no == 0 || b.len() < HEADER + no + ni * ins_bytes + np * size {
            return Err(cut());
        }
        let mut o = HEADER;
        let orders = b[o..o + no].iter().map(|&p| (p as usize).min(np - 1)).collect();
        o += no;
        let mut instruments = Vec::new();
        for k in 0..ni {
            let x = &b[o..o + ins_bytes];
            let mut i = Instrument {
                name: format!("instrument {}", k + 1),
                wave: x[0] & if version > 4 { WAVE_KEPT } else { 0xf0 },
                a: x[1] >> 4,
                d: x[1] & 15,
                s: x[2] >> 4,
                r: x[2] & 15,
                pulse: x[3] as u16 | ((x[4] as u16 & 15) << 8),
                pwm: x[5],
                vib_depth: x[6] >> 4,
                vib_speed: x[6] & 15,
                ..Default::default()
            };
            if version > 1 {
                i.arp1 = x[7] >> 4;
                i.arp2 = x[7] & 15;
                i.filter = if [1, 2, 4].contains(&(x[8] & 7)) { x[8] & 7 } else { 0 };
                i.res = x[8] >> 4;
                i.cutoff = x[9];
                i.sweep = x[10] as i8;
            }
            if version > 3 {
                [i.wave_tab, i.pulse_tab, i.filter_tab, i.vib_tab, i.vib_delay] = [x[11], x[12], x[13], x[14], x[15]];
            }
            instruments.push(i);
            o += ins_bytes;
        }
        let mut patterns = Vec::new();
        for _ in 0..np {
            let mut p = new_pattern(rows, voices);
            if version > 1 {
                p.copy_from_slice(&b[o..o + size]);
            } else {
                for c in 0..rows * VOICES {
                    p[c * CELL..c * CELL + 2].copy_from_slice(&b[o + c * 2..o + c * 2 + 2]);
                }
            }
            patterns.push(p);
            o += size;
        }
        for i in &mut instruments {
            if o + NAME_LENGTH > b.len() {
                break;
            }
            let name = String::from_utf8_lossy(&b[o..o + NAME_LENGTH]).trim_end().to_string();
            if !name.is_empty() {
                i.name = name;
            }
            o += NAME_LENGTH;
        }
        let mut game = vec![0, 1, 2];
        if version > 2 {
            game.clear();
            if o + 3 <= b.len() {
                for &v in &b[o..o + 3] {
                    if (v as usize) < voices && !game.contains(&(v as usize)) {
                        game.push(v as usize);
                    }
                }
            }
            o += 3;
        }
        let mut tables: [Vec<[u8; 2]>; 4] = Default::default();
        if version > 3 {
            let counts = b.get(o..o + 4).ok_or_else(cut)?.to_vec();
            o += 4;
            for (t, &n) in tables.iter_mut().zip(&counts) {
                let rows = b.get(o..o + n as usize * 2).ok_or_else(cut)?;
                t.extend(rows.chunks(2).map(|r| [r[0], r[1]]));
                o += n as usize * 2;
            }
        }
        Ok(Song {
            tempo: tempo.max(1) as u8,
            rows,
            loop_to: (looped != NO_LOOP as usize).then(|| looped.min(no - 1)),
            voices,
            game,
            orders,
            patterns,
            instruments,
            tables,
            tag,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequencies() {
        // A-4 (note 57) is 440 Hz on a PAL clock
        assert_eq!(freq(57), 7493);
        assert_eq!(note_name(58), "A-4");
        assert_eq!(note_name(NOTE_OFF), "===");
    }

    #[test]
    fn new_song_round_trip() {
        let s = Song::default();
        let bytes = s.encode();
        assert_eq!(bytes[4], 2);
        assert_eq!(Song::decode(&bytes).unwrap(), s);
    }

    #[test]
    fn tables_and_voices_round_trip() {
        let mut s = Song::default();
        s.table_mut(Table::Wave).extend([[0x41, 0], [0xff, 1]]);
        s.instruments[0].wave_tab = 1;
        s.instruments[1].wave |= RING;
        s.voices = 6;
        s.game = vec![4, 1];
        s.patterns = vec![new_pattern(32, 6)];
        s.patterns[0][cell_offset(3, 5, 6)] = 40;
        s.tag = Some("v1.0".into());
        let bytes = s.encode();
        assert_eq!(bytes[4], 5);
        assert_eq!(Song::decode(&bytes).unwrap(), s);
    }

    #[test]
    fn demo_song() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/demo/audio/music/gallop.sng");
        let bytes = std::fs::read(path).unwrap();
        let s = Song::decode(&bytes).unwrap();
        assert_eq!(s.voices, 3);
        assert!(!s.instruments.is_empty());
        assert_eq!(s.encode(), bytes);
    }

    #[test]
    fn picked_voices_keep_the_tempo() {
        let mut s = Song { voices: 4, game: vec![0, 1, 2], patterns: vec![new_pattern(32, 4)], ..Default::default() };
        s.patterns[0][cell_offset(0, 3, 4) + 2] = CMD_TEMPO;
        s.patterns[0][cell_offset(0, 3, 4) + 3] = 3;
        let g = s.pick_voices(&[0, 1, 2], true);
        assert_eq!(&g.patterns[0][2..4], &[CMD_TEMPO, 3]);
        assert_eq!(s.pick_voices(&[0, 1, 2], false).patterns[0][2], 0);
    }
}
