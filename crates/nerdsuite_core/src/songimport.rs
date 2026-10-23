//! Songs of other programs become songs of the Composer (song.rs), after nerdsuite2's
//! `songimport.js`:
//!   GoatTracker 2 (.sng, `GTS5`)  the patterns, the orderlists of the three channels with their
//!       repeats and transposes, the instruments with their envelope and their rows of the four
//!       tables, the tables as they are, and the pattern commands the player knows: portamento up
//!       and down, tone portamento and vibrato of the speedtable, cutoff, volume, tempo. Not kept:
//!       the other commands, the first waveform and gate timer of an instrument, the channels of a
//!       filtertable row (the filter takes the voice of the note), the depth of a vibrato relative
//!       to its note (its depth is taken as it is).
//!   SID tunes (.sid, PSID / RSID)  a tune is a program, not notes: it runs on a 6502
//!       (cpu6502.rs) for some seconds, and what it writes to the SID every frame is heard as notes -
//!       a gate that rises is a note, the distance of the notes is the tempo. Its sounds become
//!       instruments with tables ([`transcribe_sid`]): what the waveform, the pitch, the pulse width
//!       and the filter do frame by frame, a vibrato. Not kept: slides between notes.
//!   MIDI files (.mid, .midi, RIFF .rmi)  the notes of all tracks on a grid of rows (a quarter note
//!       is some rows). All of them: a voice for every note that sounds at once, up to
//!       [`MAX_VOICES`] - a voice plays one part (a track's channel), the first voices the most
//!       played parts; the game plays three of them, at first the first voice of the three most
//!       played parts. Or shared out to three voices: a part keeps its voice while it can, of a
//!       chord the highest note wins, a voice that still sounds is not taken. The General MIDI
//!       program chooses a ready instrument, the drums (channel 10) become kick, snare, tom and
//!       hats. Not kept: velocity, pitch bends, controllers, tempo changes.
//! All give an [`Imported`]: its notes tell what was cut or left out.

use std::collections::{HashMap, VecDeque};

use indexmap::IndexMap;

use crate::cpu6502::{Call, Cpu, Io};
use crate::song::{
    cell_offset, new_pattern, presets, Instrument, Song, Table, CLOCK, CMD_CUTOFF, CMD_DOWN_T, CMD_GLIDE_T, CMD_TEMPO,
    CMD_UP_T, CMD_VIB_T, CMD_VOLUME, MAX_INSTRUMENTS, MAX_ORDERS, MAX_PATTERNS, MAX_TABLE_ROWS, MAX_VOICES, NOTES,
    NOTE_OFF, VOICES,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SongKind {
    /// of this editor
    Song,
    Goat,
    Sid,
    Midi,
}

#[derive(Clone, Debug)]
pub struct Imported {
    pub song: Song,
    /// what was cut or left out
    pub notes: Vec<String>,
}

/// A byte of the file; 0 past its end.
fn at(bytes: &[u8], o: usize) -> u8 {
    bytes.get(o).copied().unwrap_or(0)
}

/// The bytes from..to, as far as there are.
fn sub(bytes: &[u8], from: usize, to: usize) -> &[u8] {
    let from = from.min(bytes.len());
    &bytes[from..to.clamp(from, bytes.len())]
}

fn text(bytes: &[u8], from: usize, length: usize) -> String {
    let s: String = bytes.iter().skip(from).take(length).take_while(|&&b| b != 0).map(|&b| b as char).collect();
    s.trim_matches(|c| matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}')).to_string()
}

fn magic(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.len().min(4)]
}

/// Math.round: halves round up.
fn round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1. } else { f }
}

fn plural(n: usize) -> &'static str {
    if n > 1 { "s" } else { "" }
}

pub fn song_kind(bytes: &[u8]) -> Option<SongKind> {
    match magic(bytes) {
        b"YSNG" => Some(SongKind::Song),
        b"GTS2" | b"GTS3" | b"GTS4" | b"GTS5" => Some(SongKind::Goat),
        b"PSID" | b"RSID" => Some(SongKind::Sid),
        b"MThd" => Some(SongKind::Midi),
        b"RIFF" if text(bytes, 8, 4) == "RMID" => Some(SongKind::Midi),
        _ => None,
    }
}

// --- the song from rows -------------------------------------------------------------------------

/// A row of a voice: note, instrument index + 1 or 0, command, value.
type Cell = [usize; 4];
const OFF: usize = NOTE_OFF as usize;

/// The instruments a song can keep: the most played ones; the others play the most similar one.
/// used: how often each is played. Gives the kept instruments and the new number (1-15) of each.
fn reduce_instruments(instruments: &[Instrument], used: &[usize], notes: &mut Vec<String>) -> (Vec<Instrument>, Vec<usize>) {
    let mut order: Vec<usize> = (0..instruments.len()).filter(|&k| used[k] > 0).collect();
    order.sort_by(|&a, &b| used[b].cmp(&used[a]));
    let mut kept: Vec<usize> = order.iter().copied().take(MAX_INSTRUMENTS).collect();
    kept.sort();
    let distance = |a: &Instrument, b: &Instrument| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).abs();
        (if a.wave == b.wave { 0 } else { 100 }) + d(a.a, b.a) + d(a.d, b.d) + d(a.s, b.s) + d(a.r, b.r)
    };
    let map = instruments
        .iter()
        .enumerate()
        .map(|(k, ins)| {
            if used[k] == 0 {
                return 0;
            }
            if let Some(at) = kept.iter().position(|&c| c == k) {
                return at + 1;
            }
            1 + kept.iter().enumerate().fold(0, |best, (n, &c)| {
                if distance(ins, &instruments[c]) < distance(ins, &instruments[kept[best]]) { n } else { best }
            })
        })
        .collect();
    if order.len() > kept.len() {
        notes.push(format!("{} instruments: the {MAX_INSTRUMENTS} most played stay, the others play the most similar one", order.len()));
    }
    (kept.iter().map(|&k| instruments[k].clone()).collect(), map)
}

struct Build {
    tempo: u8,
    rows: usize,
    /// per voice the rows of the whole song - three, or more
    voices: Vec<Vec<Cell>>,
    instruments: Vec<Instrument>,
    loop_row: usize,
    /// the voices the game plays
    game: Vec<usize>,
    tables: [Vec<[u8; 2]>; 4],
}

fn build_song(b: Build, notes: &mut Vec<String>) -> Song {
    let Build { tempo, rows, mut voices, instruments, loop_row, game, tables } = b;
    let mut used = vec![0; instruments.len()];
    for c in voices.iter_mut().flatten() {
        if c[1] > instruments.len() {
            c[1] = 0;
        }
        if c[0] != 0 && c[0] != OFF && c[1] != 0 {
            used[c[1] - 1] += 1;
        }
    }
    let (kept, map) = reduce_instruments(&instruments, &used, notes);
    let length = voices.iter().map(Vec::len).max().unwrap_or(0);
    let nv = VOICES.max(voices.len());
    let mut song = Song {
        tempo,
        rows,
        loop_to: Some(0),
        voices: nv,
        game,
        orders: vec![],
        patterns: vec![],
        instruments: if kept.is_empty() { vec![Instrument::default()] } else { kept },
        tables,
        tag: None,
    };
    let mut known: HashMap<Vec<u8>, usize> = HashMap::new();
    for start in (0..length).step_by(rows) {
        let mut p = new_pattern(rows, nv);
        for r in 0..rows {
            for (v, voice) in voices.iter().enumerate() {
                let [note, ins, cmd, value] = voice.get(start + r).copied().unwrap_or_default();
                let ins = if note != 0 && note != OFF && ins != 0 { map[ins - 1] } else { 0 };
                let o = cell_offset(r, v, nv);
                p[o..o + 4].copy_from_slice(&[note as u8, ins as u8, cmd as u8, value as u8]);
            }
        }
        let index = match known.get(&p) {
            Some(&k) => k,
            None => {
                if song.patterns.len() >= MAX_PATTERNS {
                    notes.push(format!("more than {MAX_PATTERNS} different patterns: the song ends after {} positions", song.orders.len()));
                    break;
                }
                known.insert(p.clone(), song.patterns.len());
                song.patterns.push(p);
                song.patterns.len() - 1
            }
        };
        if song.orders.len() >= MAX_ORDERS {
            notes.push(format!("more than {MAX_ORDERS} positions: the end of the song is cut"));
            break;
        }
        song.orders.push(index);
    }
    if song.orders.is_empty() {
        song.orders = vec![0];
        song.patterns = vec![new_pattern(rows, nv)];
    }
    song.loop_to = Some((loop_row / rows).min(song.orders.len() - 1));
    song
}

// --- GoatTracker ----------------------------------------------------------------------------------

const GT_FIRST_NOTE: u8 = 0x60;
const GT_LAST_NOTE: u8 = 0xbc;
const GT_KEYOFF: u8 = 0xbe;
const GT_END: u8 = 0xff;
const MAX_ROWS: usize = 8192;

struct GtInstrument {
    ad: u8,
    sr: u8,
    wave: u8,
    pulse: u8,
    filter: u8,
    vib: u8,
    vib_delay: u8,
    name: String,
}

/// A vibrato of the speedtable: bit 7 of its left byte (a depth relative to the note) is dropped.
fn vibrato_row(speed: &mut [[u8; 2]], n: u8, relative: &mut bool) {
    if let Some(r) = speed.get_mut(n as usize - 1).filter(|r| r[0] & 0x80 != 0) {
        r[0] &= 0x7f;
        *relative = true;
    }
}

/// The orderlists of a song as rows, and what they tell on the way.
struct Goat<'a> {
    patterns: Vec<&'a [u8]>,
    tables: [Vec<[u8; 2]>; 4],
    relative: bool,
    dropped: Vec<String>,
    tempo: Option<u8>,
    /// the rows of the patterns played and how often
    lengths: Vec<(i64, usize)>,
}

/// Where a channel is in its orderlist.
struct Pos {
    restart: usize,
    restart_row: usize,
    transpose_by: i32,
    repeat: u32,
}

impl Goat<'_> {
    // the commands of GoatTracker the player has: 1-4 of the speedtable, cutoff, volume, tempo
    fn command(&mut self, c: u8, d: u8) -> (u8, u8) {
        match c {
            0 => (0, 0),
            1 => (CMD_UP_T, d),
            2 => (CMD_DOWN_T, d),
            // 300: a tied note - none
            3 => if d != 0 { (CMD_GLIDE_T, d) } else { (0, 0) },
            4 => {
                if d != 0 {
                    vibrato_row(&mut self.tables[Table::Speed as usize], d, &mut self.relative);
                }
                (CMD_VIB_T, d)
            }
            0x0c => (CMD_CUTOFF, d),
            0x0d => (CMD_VOLUME, d & 15),
            0x0f if d & 0x7f >= 3 => (CMD_TEMPO, d & 0x7f),
            _ => {
                let name = if c == 0x0f { "funktempo".to_string() } else { format!("{c:X}") };
                if !self.dropped.contains(&name) {
                    self.dropped.push(name);
                }
                (0, 0)
            }
        }
    }

    fn run(&mut self, list: &[u8], from: usize, to: usize, rows: &mut Vec<Cell>, pos: &mut Pos) {
        for (k, &b) in list.iter().enumerate().take(to).skip(from) {
            if rows.len() >= MAX_ROWS {
                break;
            }
            if k == pos.restart && from == 0 {
                pos.restart_row = rows.len();
            }
            if b >= 0xe0 {
                pos.transpose_by = b as i32 - 0xf0;
            } else if b >= 0xd0 {
                pos.repeat = if b & 15 != 0 { (b & 15) as u32 } else { 16 };
            } else {
                let p = self.patterns.get(b as usize).copied();
                while pos.repeat > 0 {
                    let mut q = 0;
                    while let Some(p) = p.filter(|p| q < p.len() && p[q] != GT_END) {
                        let n = p[q];
                        if p[q + 2] == 0x0f && self.tempo.is_none() && p[q + 3] & 0x7f >= 3 {
                            self.tempo = Some(p[q + 3] & 0x7f);
                        }
                        let note = if (GT_FIRST_NOTE..=GT_LAST_NOTE).contains(&n) {
                            (n as i32 - GT_FIRST_NOTE as i32 + 1 + pos.transpose_by).clamp(1, NOTES as i32) as usize
                        } else if n == GT_KEYOFF {
                            OFF
                        } else {
                            0
                        };
                        let ins = if note != 0 && note != OFF { (p[q + 1] & 0x3f) as usize } else { 0 };
                        let (cmd, value) = self.command(p[q + 2], p[q + 3]);
                        rows.push([note, ins, cmd as usize, value as usize]);
                        q += 4;
                    }
                    pos.repeat -= 1;
                }
                if let Some(p) = p {
                    let key = p.len() as i64 / 4 - 1;
                    match self.lengths.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, n)) => *n += 1,
                        None => self.lengths.push((key, 1)),
                    }
                }
                pos.repeat = 1;
            }
        }
    }

    /// A channel as rows: its orderlist with the repeats and transposes, to its end mark; and the
    /// row it restarts at. wanted: a shorter channel goes on from its restart position.
    fn channel(&mut self, list: &[u8], wanted: usize) -> (Vec<Cell>, usize) {
        let mut rows = vec![];
        let end = list.iter().position(|&b| b == GT_END);
        let restart = end.map_or(0, |e| at(list, e + 1) as usize);
        let mut pos = Pos { restart, restart_row: 0, transpose_by: 0, repeat: 1 };
        self.run(list, 0, end.unwrap_or(list.len()), &mut rows, &mut pos);
        while let Some(end) = end.filter(|&e| wanted > 0 && rows.len() < wanted && restart < e && rows.len() > pos.restart_row) {
            let before = rows.len();
            self.run(list, restart, end, &mut rows, &mut pos);
            if rows.len() == before {
                break;
            }
        }
        if wanted > 0 {
            rows.truncate(wanted);
        }
        (rows, pos.restart_row)
    }
}

pub fn import_goat(bytes: &[u8]) -> Result<Imported, String> {
    if magic(bytes) != b"GTS5" {
        let m: String = magic(bytes).iter().map(|&b| b as char).collect();
        return Err(format!("a GoatTracker song of the old format {m}: load and save it with GoatTracker 2 first"));
    }
    let mut notes = vec![];
    let mut o = 4 + 32 * 3;
    let title = text(bytes, 4, 32);
    let subtunes = at(bytes, o) as usize;
    o += 1;
    let mut orderlists = vec![];
    for _ in 0..subtunes * VOICES {
        let n = at(bytes, o) as usize;
        o += 1;
        orderlists.push(sub(bytes, o, o + n + 1));
        o += n + 1;
    }
    if subtunes > 1 {
        notes.push(format!("{subtunes} subtunes: the first one is imported"));
    }

    let mut gt = vec![];
    let mut n = at(bytes, o);
    o += 1;
    while n > 0 {
        let b = |k: usize| at(bytes, o + k);
        gt.push(GtInstrument { ad: b(0), sr: b(1), wave: b(2), pulse: b(3), filter: b(4), vib: b(5), vib_delay: b(6), name: text(bytes, o + 9, 16) });
        n -= 1;
        o += 25;
    }
    // wave, pulse, filter, speed: the left and the right column
    let mut raw: Vec<(&[u8], &[u8])> = vec![];
    for _ in 0..4 {
        let n = at(bytes, o) as usize;
        o += 1;
        raw.push((sub(bytes, o, o + n), sub(bytes, o + n, o + 2 * n)));
        o += 2 * n;
    }
    let mut patterns = vec![];
    let mut n = at(bytes, o);
    o += 1;
    while n > 0 {
        let rows = at(bytes, o) as usize;
        o += 1;
        patterns.push(sub(bytes, o, o + rows * 4));
        o += rows * 4;
        n -= 1;
    }
    if o > bytes.len() {
        return Err("the GoatTracker song is cut short".into());
    }
    if orderlists.is_empty() {
        return Err("the GoatTracker song has no subtune".into());
    }

    // the tables as they are: rows [left, right]; the instruments point at their rows
    let mut tables: [Vec<[u8; 2]>; 4] = Default::default();
    for (t, (left, right)) in tables.iter_mut().zip(&raw) {
        *t = left.iter().zip(right.iter()).map(|(&l, &r)| [l, r]).collect();
    }
    let (wave_table, pulse_table) = (raw[0], raw[1]);
    let mut relative = false;
    let mut instruments = vec![];
    for (k, g) in gt.iter().enumerate() {
        let name = if g.name.is_empty() { format!("instrument {}", k + 1) } else { g.name.clone() };
        let mut ins = Instrument {
            name: name.chars().take(12).collect(),
            a: g.ad >> 4,
            d: g.ad & 15,
            s: g.sr >> 4,
            r: g.sr & 15,
            wave: 0x40,
            pulse: 2048,
            wave_tab: g.wave,
            pulse_tab: g.pulse,
            filter_tab: g.filter,
            vib_tab: g.vib,
            vib_delay: g.vib_delay,
            ..Default::default()
        };
        if g.vib != 0 {
            vibrato_row(&mut tables[Table::Speed as usize], g.vib, &mut relative);
        }
        // what the instrument shows without its tables. The wavetable: the waveform it settles on,
        // the last one before it jumps or ends
        for p in (g.wave as usize..).take(32) {
            if p < 1 || p > wave_table.0.len() {
                break;
            }
            let l = wave_table.0[p - 1];
            if l == 0xff {
                break;
            }
            if (0x10..=0xdf).contains(&l) && l & 0xf0 != 0 {
                ins.wave = l & 0xf0;
            }
        }
        // the pulsetable: the first width it sets, the first speed it moves with
        let mut width = None;
        for p in (g.pulse as usize..).take(32) {
            if p < 1 || p > pulse_table.0.len() {
                break;
            }
            let (l, r) = (pulse_table.0[p - 1], pulse_table.1[p - 1]);
            if l == 0xff {
                break;
            }
            if l >= 0x80 {
                width.get_or_insert(((l as u16 & 15) << 8) | r as u16);
            } else if l != 0 && ins.pwm == 0 {
                ins.pwm = (if r < 128 { r as i32 } else { r as i32 - 256 }).unsigned_abs() as u8;
            }
        }
        if let Some(w) = width {
            ins.pulse = w;
        }
        instruments.push(ins);
    }

    let mut goat = Goat { patterns, tables, relative, dropped: vec![], tempo: None, lengths: vec![] };
    let lists = &orderlists[..VOICES.min(orderlists.len())];
    let first: Vec<(Vec<Cell>, usize)> = lists.iter().map(|l| goat.channel(l, 0)).collect();
    let length = first.iter().map(|c| c.0.len()).max().unwrap_or(0);
    let voices = lists
        .iter()
        .zip(&first)
        .map(|(l, c)| if c.0.len() < length { goat.channel(l, length).0 } else { c.0.clone() })
        .collect();
    if length >= MAX_ROWS {
        notes.push(format!("the song is cut after {MAX_ROWS} rows"));
    }
    // the rows of a pattern: what most patterns of the song have
    let mut lengths = goat.lengths.clone();
    lengths.sort_by_key(|l| std::cmp::Reverse(l.1));
    let common = lengths.first().map_or(32, |l| l.0);
    let rows = if (8..=64).contains(&common) { common as usize } else { 32 };
    let Goat { tables, relative, dropped, tempo, .. } = goat;
    let build = Build { tempo: tempo.unwrap_or(6), rows, voices, instruments, loop_row: first[0].1, game: vec![0, 1, 2], tables };
    let song = build_song(build, &mut notes);
    let title = if title.is_empty() { String::new() } else { format!(" \"{title}\"") };
    notes.insert(0, format!("GoatTracker song{title}: the instruments and their tables as they are"));
    if !dropped.is_empty() {
        notes.push(format!("commands not imported: {}", dropped.join(", ")));
    }
    if relative {
        notes.push("a vibrato with a depth relative to its note plays its depth as it is".into());
    }
    Ok(Imported { song, notes })
}

// --- SID tunes ------------------------------------------------------------------------------------

fn word(bytes: &[u8], o: usize) -> usize {
    (at(bytes, o) as usize) << 8 | at(bytes, o + 1) as usize
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidInfo {
    pub title: String,
    pub author: String,
    pub songs: usize,
    pub start: usize,
}

pub fn sid_info(bytes: &[u8]) -> Result<SidInfo, String> {
    if song_kind(bytes) != Some(SongKind::Sid) {
        return Err("not a SID tune".into());
    }
    Ok(SidInfo { title: text(bytes, 0x16, 32), author: text(bytes, 0x36, 32), songs: word(bytes, 0x0e).max(1), start: word(bytes, 0x10).max(1) })
}

/// A voice of the SID in a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SidVoice {
    pub freq: u16,
    /// 12 bits
    pub pulse: u16,
    pub ctrl: u8,
    pub ad: u8,
    pub sr: u8,
}

/// The filter in a frame: the cutoff (its high 8 bits), the resonance, the voices through it, its type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SidFilter {
    pub cut: u8,
    pub res: u8,
    pub route: u8,
    pub mode: u8,
}

/// What the SID registers hold after a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub voices: [SidVoice; VOICES],
    pub filter: SidFilter,
}

/// The machine around the tune: the SID registers it writes, a raster line, the NMI of CIA 2.
struct Tune {
    regs: [u8; 32],
    raster: u32,
    nmi_mask: u8,
    timer_on: bool,
}

impl Tune {
    fn nmi(&mut self, mem: &[u8], mask: u8, on: bool) -> bool {
        let vector = mem[0xfffa] as u16 | (mem[0xfffb] as u16) << 8;
        let fire = mask & 1 != 0 && on && !(self.nmi_mask & 1 != 0 && self.timer_on) && !(0xd000..0xe000).contains(&vector);
        self.nmi_mask = mask;
        self.timer_on = on;
        fire
    }
}

impl Io for Tune {
    fn read(&mut self, mem: &[u8], a: u16) -> Option<u8> {
        if a != 0xd011 && a != 0xd012 {
            return None;
        }
        self.raster = (self.raster + 7) % 312;
        Some(if a == 0xd012 { self.raster as u8 } else { (mem[0xd011] & 0x7f) | if self.raster & 0x100 != 0 { 0x80 } else { 0 } })
    }

    fn write(&mut self, mem: &[u8], a: u16, v: u8) -> bool {
        match a {
            0xd400..=0xd7ff => {
                self.regs[a as usize & 31] = v;
                false
            }
            0xdd0d => {
                let mask = if v & 0x80 != 0 { self.nmi_mask | (v & 0x1f) } else { self.nmi_mask & !v };
                self.nmi(mem, mask, self.timer_on)
            }
            0xdd0e => self.nmi(mem, self.nmi_mask, v & 1 != 0),
            _ => false,
        }
    }
}

/// The frames of the tune: per frame and voice what the SID registers hold.
fn run_tune(bytes: &[u8], subtune: usize, frames: usize, notes: &mut Vec<String>) -> Result<Vec<Frame>, String> {
    let mut data = sub(bytes, word(bytes, 6), bytes.len());
    let mut load = word(bytes, 8);
    let (init, play) = (word(bytes, 0x0a), word(bytes, 0x0c));
    if load == 0 {
        load = at(data, 0) as usize | (at(data, 1) as usize) << 8;
        data = sub(data, 2, data.len());
    }
    if load + data.len() > 0x10000 {
        return Err("the tune does not fit the memory".into());
    }
    let mut mem = vec![0; 0x10000];
    // the end of the Kernal's interrupt, for tunes that leave through it
    mem[0xea31..0xea34].copy_from_slice(&[0x4c, 0x81, 0xea]);
    mem[0xea7e..0xea87].copy_from_slice(&[0xad, 0x0d, 0xdc, 0x68, 0xa8, 0x68, 0xaa, 0x68, 0x40]);
    mem[load..load + data.len()].copy_from_slice(data);
    mem[1] = 0x37;
    // a tune that waits for a raster line goes on: the line moves with every read of it - its low
    // byte in $d012, its 9th bit in bit 7 of $d011 (the 312 lines of PAL)
    // A tune that measures the machine (PAL or NTSC) with an NMI of timer A of CIA 2 gets one when
    // it lets the timer and its NMI run - once. The NMIs of samples are not heard here: an NMI
    // that runs code in the registers of a CIA (a vector to $dc04) does not come
    let mut cpu = Cpu::new(mem, Tune { regs: [0; 32], raster: 0, nmi_mask: 0, timer_on: false });
    let start = if init != 0 { init } else { load };
    let returned = cpu.call(start as u16, Call { a: (subtune - 1) as u8, max: 5_000_000, ..Default::default() });
    let (mut routine, mut as_irq, mut kernal) = (play, false, false);
    if routine == 0 {
        // the tune set up an interrupt: that is its player
        as_irq = true;
        let m = &cpu.mem;
        let soft = m[0x314] as usize | (m[0x315] as usize) << 8;
        let hard = m[0xfffe] as usize | (m[0xffff] as usize) << 8;
        if soft >= load && soft < load + data.len() {
            routine = soft;
            kernal = true;
        } else {
            routine = hard;
        }
        if routine == 0 {
            return Err("the tune has no play routine and sets no interrupt".into());
        }
    }
    if !returned && play != 0 {
        notes.push("the init routine of the tune did not end".into());
    }
    let mut out = Vec::with_capacity(frames);
    let mut failed = 0;
    for _ in 0..frames {
        if !cpu.call(routine as u16, Call { max: 200_000, as_irq, kernal, ..Default::default() }) {
            failed += 1;
        }
        let regs = &cpu.io.regs;
        let voices = std::array::from_fn(|v| {
            let b = v * 7;
            SidVoice {
                freq: regs[b] as u16 | (regs[b + 1] as u16) << 8,
                pulse: regs[b + 2] as u16 | (regs[b + 3] as u16 & 15) << 8,
                ctrl: regs[b + 4],
                ad: regs[b + 5],
                sr: regs[b + 6],
            }
        });
        let filter = SidFilter { cut: regs[22], res: regs[23] >> 4, route: regs[23] & 7, mode: (regs[24] >> 4) & 7 };
        out.push(Frame { voices, filter });
    }
    if failed * 2 > frames {
        return Err("the play routine of the tune does not run here".into());
    }
    if cpu.unknown > 0 {
        notes.push("the tune uses opcodes this 6502 does not know: notes may be wrong".into());
    }
    Ok(out)
}

fn note_of(freq: u16) -> usize {
    (round(12. * (freq.max(1) as f64 * CLOCK / 16777216. / 440.).log2()) as i32 + 58).clamp(1, NOTES as i32) as usize
}

/// The frames a row lasts: the longest that (nearly) all notes start on; and the first frame of a row.
fn find_tempo(onsets: &[usize]) -> (usize, usize) {
    if onsets.len() < 4 {
        return (6, onsets.first().copied().unwrap_or(0));
    }
    for t in (2..=24).rev() {
        let mut hits = vec![0usize; t];
        for &f in onsets {
            hits[f % t] += 1;
        }
        let max = *hits.iter().max().unwrap();
        let best = hits.iter().position(|&h| h == max).unwrap();
        if hits[best] as f64 >= onsets.len() as f64 * 0.95 {
            return (t, best);
        }
    }
    (1, 0)
}

// --- the sounds of a SID tune as instruments with tables ------------------------------------------
// The notes of the tune are grouped by their sound: the envelope, the waveforms and the pitches of
// their first frames, the width of a pulse and whether the voice goes through the filter. A group
// is an instrument; one of its notes, the longest, gives its tables - a step a frame, like the
// player of the editor runs them (song.rs):
//   wave    the waveform and the pitch of each frame: relative to the note, or fixed notes when
//           the notes of the group play the same pitches whatever their note (a drum); a value
//           that stays is a wait, an arpeggio that comes round again a jump back to it
//   pulse   the width of the first frame, then sweeps - so many frames of a step - and a jump
//           back when they come round again
//   filter  the type and the resonance, the cutoff of the first frame, then its sweeps
//   speed   a vibrato: the frames of a half wave and the step of the frequency a frame

/// the frames of a note looked at
const PROFILE: usize = 128;
/// ... and those that tell its sound
const SHAPE: usize = 8;
/// the rows of a wave table at most
const MAX_WAVE_ROWS: usize = 32;

/// C-0 = 0
fn semitone(freq: i32) -> f64 {
    12. * (freq.max(1) as f64 * CLOCK / 16777216. / 440.).log2() + 57.
}

/// The pitch of a frame as a note (C-0 = 0), or None between two notes (a slide, a wide vibrato).
fn pitch_of(freq: u16) -> Option<i32> {
    let s = semitone(freq as i32);
    let n = round(s);
    ((s - n).abs() < 0.3).then(|| (n as i32).clamp(0, NOTES as i32 - 1))
}

/// The waveform with sync, ring and test (all four waveforms are three: F0-FE are no waveforms in a
/// table); 0: none.
fn wave_of(ctrl: u8) -> u8 {
    if ctrl & 0xf0 == 0 {
        0
    } else if ctrl & 0xf0 == 0xf0 {
        0xe0 | (ctrl & 0x0e)
    } else {
        ctrl & 0xfe
    }
}

fn signed12(d: i32) -> i32 {
    ((d + 2048) & 4095) - 2048
}

fn signed8(d: i32) -> i32 {
    ((d + 128) & 255) - 128
}

/// The first entry from that a list repeats with a period: (start, period) - the shortest, at
/// least twice in the list.
fn cycle_of<T: PartialEq>(list: &[T], max_period: usize) -> Option<(usize, usize)> {
    for start in 0..list.len() {
        let mut period = 2;
        while period <= max_period && start + 2 * period <= list.len() {
            if (start + period..list.len()).all(|i| list[i] == list[i - period]) {
                return Some((start, period));
            }
            period += 1;
        }
    }
    None
}

/// The rows of a table and the row (from 1) its end jumps back to; 0: it ends.
struct Made {
    rows: Vec<[u8; 2]>,
    loop_to: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Segment {
    /// frames, step
    Sweep(i32, i32),
    Set(i32),
}

/// Sweeps: a value a frame (12 or 8 bits) as steps of up to 127 a frame, each for up to 127
/// frames - [frames, step]; a jump: a row that sets the value (set_row).
fn sweep_rows(values: &[i32], bits: u32, set_row: impl Fn(i32) -> [u8; 2]) -> Made {
    let wrap = if bits == 12 { signed12 } else { signed8 };
    let mut segments: Vec<Segment> = vec![];
    for i in 1..values.len() {
        let d = wrap(values[i] - values[i - 1]);
        if d.abs() > 127 {
            segments.push(Segment::Set(values[i]));
            continue;
        }
        match segments.last_mut() {
            Some(Segment::Sweep(frames, step)) if *step == d && *frames < 127 => *frames += 1,
            _ => segments.push(Segment::Sweep(1, d)),
        }
    }
    // it stays: the end
    while let Some(Segment::Sweep(_, 0)) = segments.last() {
        segments.pop();
    }
    // sweeps that come round again: the last one may be cut by the end of the note
    let cycle = if segments.len() >= 4 { cycle_of(&segments[..segments.len() - 1], 6) } else { None };
    let used = match cycle {
        Some((start, period)) => &segments[..start + period],
        None => &segments[..],
    };
    let mut rows = vec![set_row(values[0])];
    rows.extend(used.iter().map(|s| match *s {
        Segment::Set(v) => set_row(v),
        Segment::Sweep(frames, step) => [frames as u8, (step & 255) as u8],
    }));
    // the row to jump back to (the set row is the first)
    Made { rows, loop_to: cycle.map_or(0, |(start, _)| start + 2) }
}

/// The wave table of a note: (wave, pitch) a frame - pitch a note (fixed) or a step from the note.
fn wave_rows(seq: &[(u8, i32)], fixed: bool) -> Made {
    let code = |p: i32| -> u8 {
        if fixed {
            (0x81 + (p - 1).clamp(0, 0x5e)) as u8
        } else if p >= 0 {
            p.min(0x5f) as u8
        } else {
            (0x80 + p.max(-32)) as u8
        }
    };
    // the value it settles on
    let mut last = seq.len() - 1;
    while last > 0 && seq[last] == seq[last - 1] {
        last -= 1;
    }
    let steady = seq.len() - last >= 3;
    let cycle = if steady { None } else { cycle_of(seq, 16) };
    let mut rows: Vec<[u8; 2]> = vec![];
    let mut row_at = vec![0; seq.len()];
    // frames from..to-1: a row each value, a wait for the frames it stays
    let emit = |rows: &mut Vec<[u8; 2]>, row_at: &mut Vec<usize>, from: usize, to: usize| {
        let mut i = from;
        while i < to {
            let mut j = i + 1;
            while j < to && seq[j] == seq[i] {
                j += 1;
            }
            row_at[i] = rows.len();
            rows.push([seq[i].0, code(seq[i].1)]);
            let mut hold = j - i - 1;
            while hold > 0 {
                rows.push([hold.min(15) as u8, 0]);
                hold = hold.saturating_sub(15);
            }
            i = j;
        }
    };
    if let Some((start, period)) = cycle {
        emit(&mut rows, &mut row_at, 0, start);
        emit(&mut rows, &mut row_at, start, start + period);
        if rows.len() < MAX_WAVE_ROWS {
            return Made { rows, loop_to: row_at[start] + 1 };
        }
        rows.clear();
    }
    emit(&mut rows, &mut row_at, 0, seq.len().min((if steady { last } else { seq.len() - 1 }) + 1));
    rows.truncate(MAX_WAVE_ROWS);
    Made { rows, loop_to: 0 }
}

/// A vibrato of a note that stays on its pitch: [frames of a half wave, step a frame, delay].
fn vibrato_of(freqs: &[i32]) -> Option<[i32; 3]> {
    // around its note: a slide goes further than a semitone
    let pitches: Vec<f64> = freqs.iter().map(|&f| semitone(f)).collect();
    let (max, min) = pitches.iter().fold((f64::NEG_INFINITY, f64::INFINITY), |(a, b), &p| (a.max(p), b.min(p)));
    if max - min > 1.2 {
        return None;
    }
    let steps: Vec<i32> = freqs.windows(2).map(|w| w[1] - w[0]).collect();
    let first = steps.iter().position(|&d| d != 0)?;
    let mut runs = vec![];
    let (mut sign, mut run) = (0, 0);
    for &d in &steps[first..] {
        let s = if d.signum() != 0 { d.signum() } else { sign };
        if s == sign {
            run += 1;
        } else {
            if sign != 0 {
                runs.push(run);
            }
            sign = s;
            run = 1;
        }
    }
    // a slide, not a vibrato
    if runs.len() < 2 {
        return None;
    }
    let median = |list: &[i32]| {
        let mut l = list.to_vec();
        l.sort();
        l[l.len() >> 1]
    };
    let nonzero: Vec<i32> = steps.iter().filter(|&&d| d != 0).map(|d| d.abs()).collect();
    let step = median(&nonzero);
    Some([median(&runs).clamp(1, 127), step.clamp(1, 255), (first as i32).min(255)])
}

/// A note of a SID tune: its voice, its frames, its note, what its frames do.
struct SidNote {
    v: usize,
    f: usize,
    end: usize,
    len: usize,
    base: i32,
    kind: String,
    shape: usize,
    key_fix: String,
    waves: Vec<u8>,
    pitches: Vec<Option<i32>>,
    rel: Vec<Option<i32>>,
    main: u8,
    ad: u8,
    sr: u8,
    routed: bool,
}

struct Group {
    notes: Vec<usize>,
    /// the notes that play it but do not make its tables
    guests: Vec<usize>,
    fixed: bool,
    first: f64,
    pitch_moves: bool,
}

/// The tables of a transcribed song as they fill.
struct Tables {
    t: [Vec<[u8; 2]>; 4],
    full: bool,
}

impl Tables {
    /// The rows of a table, a jump back or the end; their first row, 0: no room.
    fn put(&mut self, table: Table, made: Made) -> u8 {
        if made.rows.is_empty() {
            return 0;
        }
        let t = &mut self.t[table as usize];
        if t.len() + made.rows.len() + 1 > MAX_TABLE_ROWS {
            self.full = true;
            return 0;
        }
        let first = t.len() + 1;
        t.extend(made.rows);
        t.push([0xff, if made.loop_to != 0 { (first + made.loop_to - 1) as u8 } else { 0 }]);
        first as u8
    }

    /// A row of the speed table, the same one once.
    fn speed_row(&mut self, row: [u8; 2]) -> u8 {
        let t = &mut self.t[Table::Speed as usize];
        if let Some(k) = t.iter().position(|r| *r == row) {
            return k as u8 + 1;
        }
        if t.len() >= MAX_TABLE_ROWS {
            self.full = true;
            return 0;
        }
        t.push(row);
        t.len() as u8
    }
}

/// A SID tune as a song, and what it took: the frames a row lasts, the tables it made.
#[derive(Clone, Debug)]
pub struct Transcribed {
    pub song: Song,
    pub notes: Vec<String>,
    pub tempo: usize,
    /// the names of the tables that have rows
    pub made: String,
}

/// The frames of a tune as a song; tempo: the frames a row lasts, None: as the notes tell.
pub fn transcribe_sid(frames: &[Frame], tempo: Option<usize>) -> Result<Transcribed, String> {
    let mut notes = vec![];
    let nf = frames.len();
    let ctrl = |f: usize, v: usize| frames[f].voices[v].ctrl;
    let gate = |f: i64, v: usize| f >= 0 && (f as usize) < nf && ctrl(f as usize, v) & 1 != 0;
    // a note starts when the gate rises - or when, the gate held, the waveform goes for a frame: a
    // restart of the oscillator (and its test bit) for the next note without a release
    let restarts = |f: usize, v: usize| {
        f > 0 && gate(f as i64, v) && gate(f as i64 - 1, v) && ctrl(f, v) & 0xf0 == 0 && ctrl(f - 1, v) & 0xf0 != 0
    };
    let mut rises: Vec<Vec<usize>> = vec![];
    let mut falls: Vec<Vec<usize>> = vec![];
    for v in 0..VOICES {
        rises.push(vec![]);
        falls.push(vec![]);
        for f in 0..nf {
            let fi = f as i64;
            if (gate(fi, v) && !gate(fi - 1, v)) || restarts(f, v) {
                rises[v].push(f);
            }
            if !gate(fi, v) && gate(fi - 1, v) {
                falls[v].push(f);
            }
        }
    }
    let mut onsets: Vec<usize> = rises.concat();
    onsets.sort();
    let (tempo, offset) = match tempo.filter(|&t| t > 0) {
        Some(t) => (t, onsets.first().copied().unwrap_or(0)),
        None => find_tempo(&onsets),
    };
    let (ti, oi) = (tempo as i64, offset as i64);
    let count = if nf > offset { MAX_ROWS.min((nf - offset).div_ceil(tempo)) } else { 0 };
    let row_of = |f: usize| (f as i64 - oi).div_euclid(ti);
    // the nearest row: a note a little early is that row's
    let row_near = |f: usize| (2 * (f as i64 - oi) + ti).div_euclid(2 * ti);

    // every note: its voice, its frames, its note (after the first frames: the click of a drum, a
    // hard restart), what its frames do
    let mut all: Vec<SidNote> = vec![];
    let mut keys_rel: Vec<String> = vec![];
    for v in 0..VOICES {
        for (k, &f) in rises[v].iter().enumerate() {
            let next = rises[v].get(k + 1).copied().unwrap_or(nf);
            let end = next.min(falls[v].iter().copied().find(|&x| x > f).unwrap_or(nf));
            // its sound lasts to the next note: a release goes on with its waveform and its pitch (a
            // gate of a frame, then the release, is a sound too) - but not the hard restart before it
            let mut len = PROFILE.min(next - f);
            let restart = |c: u8| c & 1 == 0 && (c & 0xf0 == 0 || c & 8 != 0);
            while len > 1 && f + len > end && restart(ctrl(f + len - 1, v)) {
                len -= 1;
            }
            let fr = |i: usize| frames[f + i].voices[v];
            let filt = frames[f + 1usize.min(len - 1)].filter;
            // a frame without a waveform or with the test bit is not heard: its pitch is none
            let waves: Vec<u8> = (0..len).map(|i| wave_of(fr(i).ctrl)).collect();
            let pitches: Vec<Option<i32>> =
                (0..len).map(|i| if waves[i] != 0 && waves[i] & 8 == 0 { pitch_of(fr(i).freq) } else { None }).collect();
            // its note: the pitch its first frames have most with a tone (not noise, not a hard
            // restart), the first of them - the root of an arpeggio; else after the first frames
            let mut heard: Vec<(i32, usize)> = vec![];
            for i in 0..len.min(SHAPE) {
                if let Some(p) = pitches[i].filter(|_| waves[i] != 0 && waves[i] & 0x88 == 0) {
                    match heard.iter_mut().find(|h| h.0 == p) {
                        Some(h) => h.1 += 1,
                        None => heard.push((p, 1)),
                    }
                }
            }
            let at = (end - 1).min(f + 2usize.min(tempo - 1));
            let base = match heard.first() {
                Some(&h) => heard.iter().fold(h, |a, &b| if b.1 > a.1 { b } else { a }).0,
                None => note_of(frames[at].voices[v].freq) as i32 - 1,
            };
            let s = fr(1usize.min(len - 1));
            let routed = filt.route & (1 << v) != 0 && filt.mode != 0;
            let shaped = len.min(SHAPE);
            let rel: Vec<Option<i32>> = pitches[..shaped].iter().map(|p| p.map(|p| p - base)).collect();
            let fix = &pitches[..shaped];
            let main = waves
                .iter()
                .copied()
                .find(|&w| w & 0xf0 != 0 && w & 8 == 0)
                .or_else(|| waves.iter().copied().find(|&w| w != 0))
                .unwrap_or(0x40);
            // the sound is told by the frames its gate is on: the release and the hard restart of the
            // next note come as early as the note is short - a note of a row is the same sound
            let shape = SHAPE.min(len).min(end - f).max(1);
            let pulse = if main & 0x40 != 0 { (s.pulse >> 8).to_string() } else { "-".into() };
            let kind = format!("{}:{}:{}:{}", s.ad, s.sr, pulse, if routed { filt.mode } else { 0 });
            let join = |l: &[Option<i32>]| l.iter().map(|p| p.map_or("x".to_string(), |p| p.to_string())).collect::<Vec<_>>().join(",");
            let sound = format!("{kind}:{}", waves[..shape].iter().map(u8::to_string).collect::<Vec<_>>().join(","));
            keys_rel.push(format!("{sound}|{}", join(&rel[..shape])));
            let key_fix = format!("{sound}|{}", join(&fix[..shape]));
            all.push(SidNote { v, f, end, len, base, kind, shape, key_fix, waves, pitches, rel, main, ad: s.ad, sr: s.sr, routed });
        }
    }
    // the groups: the same sound and the same steps from the note; groups whose notes play the same
    // pitches whatever their note are one, with fixed notes (a drum on several notes)
    let mut groups: IndexMap<String, Group> = IndexMap::new();
    for (k, n) in all.iter().enumerate() {
        groups
            .entry(keys_rel[k].clone())
            .or_insert_with(|| Group { notes: vec![], guests: vec![], fixed: false, first: n.f as f64 + n.v as f64 / 4., pitch_moves: false })
            .notes
            .push(k);
    }
    // a group of short notes is the sound of the longer ones that begin like them: into the most
    // played of those (the longest first, so a group a frame shorter does not take them)
    let begins = |short: &SidNote, long: &SidNote| {
        short.kind == long.kind
            && short.shape < long.shape
            && (0..short.shape).all(|i| Some(&short.waves[i]) == long.waves.get(i) && Some(&short.rel[i]) == long.rel.get(i))
    };
    let mut order: Vec<(String, usize)> = groups.iter().map(|(k, g)| (k.clone(), all[g.notes[0]].shape)).collect();
    order.sort_by_key(|o| std::cmp::Reverse(o.1));
    for (key, shape) in order {
        if shape >= SHAPE {
            continue;
        }
        let Some(gi) = groups.get_index_of(&key) else { continue };
        let n = &all[groups[gi].notes[0]];
        let mut into: Option<usize> = None;
        for (hi, h) in groups.values().enumerate() {
            if hi != gi && begins(n, &all[h.notes[0]]) && into.is_none_or(|j| h.notes.len() > groups[j].notes.len()) {
                into = Some(hi);
            }
        }
        let Some(j) = into else { continue };
        let (moved, first) = (groups[gi].notes.clone(), groups[gi].first);
        let h = &mut groups[j];
        h.notes.extend(moved);
        h.first = h.first.min(first);
        groups.shift_remove_index(gi);
    }
    let mut by_fix: HashMap<String, String> = HashMap::new();
    let mut i = 0;
    while i < groups.len() {
        let (key, g) = groups.get_index(i).unwrap();
        let fix = &all[g.notes[0]].key_fix;
        if !g.notes.iter().all(|&n| all[n].key_fix == *fix) || fix.contains('x') {
            i += 1;
            continue;
        }
        match by_fix.get(fix) {
            Some(into) => {
                let j = groups.get_index_of(into).unwrap();
                let (moved, first) = (g.notes.clone(), g.first);
                let h = &mut groups[j];
                h.notes.extend(moved);
                h.fixed = true;
                h.first = h.first.min(first);
                groups.shift_remove_index(i);
            }
            None => {
                by_fix.insert(fix.clone(), key.clone());
                i += 1;
            }
        }
    }
    // the instruments: the groups heard longest - their frames, not their notes: a long note of its
    // own sound counts as much as many short ones; a note of another one plays the most like it
    let mut groups: Vec<Group> = groups.into_values().collect();
    let heard_for = |g: &Group| g.notes.iter().map(|&n| all[n].end - all[n].f).sum::<usize>();
    let mut list: Vec<usize> = (0..groups.len()).collect();
    list.sort_by(|&a, &b| heard_for(&groups[b]).cmp(&heard_for(&groups[a])));
    let mut kept: Vec<usize> = list.iter().copied().take(MAX_INSTRUMENTS).collect();
    kept.sort_by(|&a, &b| groups[a].first.total_cmp(&groups[b].first));
    // how unlike two sounds are: their first frames - the waveforms, the steps from the note - then
    // the filter and the envelope
    let distance = |a: &SidNote, b: &SidNote| {
        let nib = |x: u8, y: u8| ((x >> 4) as i32 - (y >> 4) as i32).abs() + ((x & 15) as i32 - (y & 15) as i32).abs();
        let mut d = (if a.routed == b.routed { 0 } else { 20 }) + nib(a.ad, b.ad) + nib(a.sr, b.sr);
        for i in 0..SHAPE {
            d += if a.waves.get(i).copied().unwrap_or(0) == b.waves.get(i).copied().unwrap_or(0) { 0 } else { 12 };
            d += if a.rel.get(i) == b.rel.get(i) { 0 } else { 6 };
        }
        d
    };
    for &g in list.iter().skip(MAX_INSTRUMENTS) {
        let n = &all[groups[g].notes[0]];
        let like = kept.iter().fold(kept[0], |best, &k| {
            if distance(n, &all[groups[k].notes[0]]) < distance(n, &all[groups[best].notes[0]]) { k } else { best }
        });
        // they play it, they do not make it: its tables are its own notes'
        let moved = groups[g].notes.clone();
        groups[like].guests.extend(moved);
    }
    if list.len() > MAX_INSTRUMENTS {
        notes.push(format!(
            "{} sounds: the {} heard least play the most similar of {MAX_INSTRUMENTS}",
            list.len(),
            list.len() - MAX_INSTRUMENTS
        ));
    }

    let mut tables = Tables { t: Default::default(), full: false };
    let mut instruments = vec![];
    for (k, &gi) in kept.iter().enumerate() {
        let g = &groups[gi];
        let rep = g.notes.iter().map(|&n| &all[n]).fold(&all[g.notes[0]], |a, b| if b.len > a.len { b } else { a });
        let fr = |i: usize| frames[rep.f + i].voices[rep.v];
        let name = match rep.main & 0xf0 {
            0x10 => "tri",
            0x20 => "saw",
            0x40 => "pulse",
            0x80 => "noise",
            _ => "mix",
        };
        let mut ins = Instrument {
            name: format!("{name} {}", k + 1),
            wave: rep.main & 0xf0,
            a: rep.ad >> 4,
            d: rep.ad & 15,
            s: rep.sr >> 4,
            r: rep.sr & 15,
            pulse: if rep.main & 0x40 != 0 { fr(1usize.min(rep.len - 1)).pulse } else { 2048 },
            ..Default::default()
        };
        // the wave table: the waveform and the pitch of each frame - fixed notes for a drum (it starts
        // with noise and its pitch moves: its note does not matter) and for notes alike on any note
        let fixed = g.fixed || (rep.waves[0] & 0x80 != 0 && rep.pitches.iter().any(|p| p.is_some() && *p != rep.pitches[0]));
        // as far as the notes of the sound agree: beyond that the longest note does what only it does
        // (a slide of its own), and the table stays on its last step
        let mut agreed = rep.len;
        let mut i = SHAPE;
        while i < rep.len && agreed == rep.len {
            let pitch_at = |n: &SidNote| n.pitches[i].map(|p| if fixed { p } else { p - n.base });
            let there: Vec<&SidNote> = g.notes.iter().map(|&n| &all[n]).filter(|n| n.len > i).collect();
            let alike = there
                .iter()
                .filter(|n| {
                    n.waves[i] == rep.waves[i] && (pitch_at(n).is_none() || pitch_at(rep).is_none() || pitch_at(n) == pitch_at(rep))
                })
                .count();
            if there.len() >= 2 && (alike as f64) < there.len() as f64 * 0.8 {
                agreed = i;
            }
            i += 1;
        }
        let mut seq: Vec<(u8, i32)> = vec![];
        let mut wave = rep.main;
        let mut pitch = if fixed { rep.pitches.iter().flatten().next().copied().unwrap_or(rep.base) } else { 0 };
        for i in 0..agreed {
            if rep.waves[i] != 0 {
                wave = rep.waves[i];
            }
            if let Some(p) = rep.pitches[i] {
                pitch = if fixed { p } else { p - rep.base };
            }
            seq.push((wave, pitch));
        }
        // the instrument alone does it
        let plain = !fixed && seq.iter().all(|&(w, p)| w == rep.main & 0xf0 && p == 0);
        if !plain {
            ins.wave_tab = tables.put(Table::Wave, wave_rows(&seq, fixed));
        }
        let moves = seq.iter().any(|&(_, p)| p != seq[0].1);
        // the pulse table: when the width moves
        if seq.iter().any(|&(w, _)| w & 0x40 != 0) {
            let widths: Vec<i32> = (0..rep.len).map(|i| fr(i).pulse as i32).collect();
            if widths.iter().any(|&w| w != widths[0]) {
                ins.pulse_tab = tables.put(Table::Pulse, sweep_rows(&widths, 12, |w| [0x80 | (w >> 8) as u8, (w & 255) as u8]));
            }
            ins.pulse = widths[0] as u16;
        }
        // the filter table: the note goes through the filter
        if rep.routed {
            let filt = frames[rep.f + 1usize.min(rep.len - 1)].filter;
            let cuts: Vec<i32> = (0..rep.len).map(|i| frames[rep.f + i].filter.cut as i32).collect();
            let mut made = sweep_rows(&cuts, 8, |c| [0, c as u8]);
            made.rows.insert(0, [0x80 | (filt.mode << 4), filt.res << 4]);
            if made.loop_to != 0 {
                made.loop_to += 1;
            }
            ins.filter_tab = tables.put(Table::Filter, made);
        }
        // a vibrato of the speed table: the note stays on its pitch and its frequency swings
        if !moves && !fixed && rep.len > 6 {
            let freqs: Vec<i32> = (0..rep.len).map(|i| fr(i).freq as i32).collect();
            if let Some([half, step, delay]) = vibrato_of(&freqs) {
                ins.vib_tab = tables.speed_row([half as u8, step as u8]);
                ins.vib_delay = if ins.vib_tab != 0 { delay as u8 } else { 0 };
            }
        }
        instruments.push(ins);
        groups[gi].pitch_moves = moves;
    }
    if tables.full {
        notes.push("the tables are full: some sounds play without theirs".into());
    }
    let mut instrument_of: HashMap<usize, usize> = HashMap::new();
    let mut group_of: HashMap<usize, usize> = HashMap::new();
    for (k, &gi) in kept.iter().enumerate() {
        for &n in groups[gi].notes.iter().chain(&groups[gi].guests) {
            instrument_of.insert(n, k + 1);
            group_of.insert(n, gi);
        }
    }

    let mut voices = vec![];
    for v in 0..VOICES {
        let mut rows: Vec<Cell> = vec![[0; 4]; count];
        // the frames of notes whose tables move the pitch
        let mut moving = vec![];
        for (k, n) in all.iter().enumerate().filter(|(_, n)| n.v == v) {
            let r = row_near(n.f);
            if group_of.get(&k).is_some_and(|&g| groups[g].pitch_moves) {
                moving.push((n.f, n.end));
            }
            if r < 0 || r >= count as i64 {
                continue;
            }
            rows[r as usize] = [(n.base + 1) as usize, instrument_of.get(&k).copied().unwrap_or(0), 0, 0];
        }
        // the gate goes and no note follows at once: a note off - in its row, or the next free one
        for &f in &falls[v] {
            if rises[v].iter().find(|&&x| x >= f).is_some_and(|&next| next - f <= tempo.max(3)) {
                continue;
            }
            let rf = row_of(f);
            for r in rf.max(0)..(count as i64).min(rf + 2) {
                if rows[r as usize][0] == 0 {
                    rows[r as usize] = [OFF, 0, 0, 0];
                    break;
                }
            }
        }
        // a held note that changes its pitch for a whole row is a new note - not where its table moves it
        let mut last = 0;
        for (r, row) in rows.iter_mut().enumerate() {
            if row[0] != 0 {
                last = if row[0] == OFF { 0 } else { row[0] };
                continue;
            }
            let from = offset + r * tempo;
            let to = nf.min(from + tempo);
            if last == 0 || tempo < 2 || to < from + tempo || moving.iter().any(|&(a, b)| from >= a && from < b) {
                continue;
            }
            let n = note_of(frames[from].voices[v].freq);
            let mut steady = n != last;
            let mut f = from;
            while f < to && steady {
                steady = gate(f as i64, v) && note_of(frames[f].voices[v].freq) == n;
                f += 1;
            }
            if steady {
                *row = [n, 0, 0, 0];
                last = n;
            }
        }
        while rows.last().is_some_and(|c| c[0] == 0) {
            rows.pop();
        }
        voices.push(rows);
    }
    if voices.iter().all(Vec::is_empty) {
        return Err("the tune plays no note in this time".into());
    }
    let length = voices.iter().map(Vec::len).max().unwrap_or(0);
    let made = Table::ALL.iter().filter(|&&t| !tables.t[t as usize].is_empty()).map(|t| t.label()).collect::<Vec<_>>().join(", ");
    let build = Build {
        tempo: tempo as u8,
        rows: if length > 32 * MAX_ORDERS { 64 } else { 32 },
        voices,
        instruments,
        loop_row: 0,
        game: vec![0, 1, 2],
        tables: tables.t,
    };
    let song = build_song(build, &mut notes);
    Ok(Transcribed { song, notes, tempo, made })
}

/// The frames of a SID tune: per frame the registers of its voices and of the filter.
pub fn sid_frames(bytes: &[u8], subtune: Option<usize>, seconds: u32) -> Result<Vec<Frame>, String> {
    let info = sid_info(bytes)?;
    run_tune(bytes, subtune.unwrap_or(info.start).min(info.songs).max(1), seconds as usize * 50, &mut vec![])
}

/// A SID tune as a song: its subtune (None: the one it starts with) heard for some seconds.
pub fn import_sid(bytes: &[u8], subtune: Option<usize>, seconds: u32) -> Result<Imported, String> {
    let info = sid_info(bytes)?;
    let mut run_notes = vec![];
    let frames = run_tune(bytes, subtune.unwrap_or(info.start).min(info.songs).max(1), seconds as usize * 50, &mut run_notes)?;
    let Transcribed { song, notes: t_notes, tempo, made } = transcribe_sid(&frames, None)?;
    let title = if info.title.is_empty() { String::new() } else { format!(" \"{}\"", info.title) };
    let author = if info.author.is_empty() { String::new() } else { format!(" by {}", info.author) };
    let made = if made.is_empty() { made } else { format!(" ({made})") };
    let mut notes = vec![format!(
        "SID tune{title}{author}: {seconds} s heard as notes, a row is {tempo} frame{}; its sounds as instruments with tables{made} - slides between notes are not kept",
        plural(tempo)
    )];
    notes.extend(run_notes);
    notes.extend(t_notes);
    Ok(Imported { song, notes })
}

// --- MIDI files -----------------------------------------------------------------------------------

fn preset(name: &str) -> Option<Instrument> {
    presets().into_iter().flat_map(|(_, l)| l).find(|i| i.name == name)
}

/// The ready instrument of each family of 8 General MIDI programs, and the family's name.
const GM_FAMILIES: [(&str, &str); 16] = [
    ("piano", "e-piano"), ("mallets", "bell"), ("organ", "organ"), ("guitar", "pluck bass"),
    ("bass", "fat bass"), ("strings", "string pad"), ("ensemble", "choir pad"), ("brass", "saw lead"),
    ("reed", "pwm lead"), ("pipe", "flute"), ("synth lead", "pwm lead"), ("synth pad", "sweep pad"),
    ("synth fx", "ufo"), ("ethnic", "pluck bass"), ("percussive", "marimba"), ("sound fx", "explosion"),
];

/// The note a drum plays.
fn drum_note(drum: &str) -> u8 {
    match drum {
        "kick" => 25,
        "snare" => 61,
        "tom" => 37,
        _ => 85,
    }
}

/// The drums of channel 10 by their key.
fn drum_of(key: u8) -> &'static str {
    match key {
        35 | 36 => "kick",
        37..=40 => "snare",
        41 | 43 | 45 | 47 | 48 | 50 => "tom",
        46 | 49 | 52 | 55 | 57 => "open hat",
        _ => "closed hat",
    }
}

/// of drums at once, the first wins
const DRUM_RANK: [&str; 5] = ["kick", "snare", "tom", "open hat", "closed hat"];
/// channel 10
const DRUMS: u8 = 9;

fn be32(bytes: &[u8], o: usize) -> usize {
    (at(bytes, o) as usize) << 24 | (at(bytes, o + 1) as usize) << 16 | (at(bytes, o + 2) as usize) << 8 | at(bytes, o + 3) as usize
}

/// A note of a MIDI file, in ticks.
struct MidiNote {
    key: u8,
    start: u64,
    end: u64,
    program: u8,
}

/// The notes of a channel of a track.
struct Part {
    track: usize,
    name: String,
    drums: bool,
    program: u8,
    notes: Vec<MidiNote>,
}

struct Midi {
    /// ticks of a quarter note
    tpq: f64,
    /// (tick, microseconds of a quarter note)
    tempos: Vec<(u64, u32)>,
    /// the first one: (numerator, denominator)
    signature: (u32, f64),
    title: String,
    parts: Vec<Part>,
}

/// A variable length number of a track.
fn vlq(bytes: &[u8], o: &mut usize, stop: usize) -> u64 {
    let mut v: u64 = 0;
    loop {
        let b = at(bytes, *o);
        *o += 1;
        v = v.saturating_mul(128).saturating_add((b & 127) as u64);
        if b & 128 == 0 || *o >= stop {
            return v;
        }
    }
}

/// The notes of all tracks: a part is the notes of a channel of a track.
fn read_midi(bytes: &[u8]) -> Result<Midi, String> {
    let mut o = 0;
    // a RIFF (.rmi) holds the file
    while o + 4 <= bytes.len() && &bytes[o..o + 4] != b"MThd" {
        o += 1;
    }
    if o + 14 > bytes.len() {
        return Err("not a MIDI file".into());
    }
    let (division, count) = (word(bytes, o + 12), word(bytes, o + 10));
    // SMPTE time: ticks of a second, as if a quarter note were half a second (120 bpm)
    let tpq = if division & 0x8000 != 0 { (256 - (division >> 8)) as f64 * (division & 255) as f64 / 2. } else { division as f64 };
    if tpq == 0. {
        return Err("the MIDI file has no time division".into());
    }
    o += 8 + be32(bytes, o + 4);
    let (mut tempos, mut signatures) = (vec![], vec![]);
    let mut parts: IndexMap<(usize, u8), Part> = IndexMap::new();
    let mut title = String::new();
    let mut t = 0;
    while t < count && o + 8 <= bytes.len() {
        let (id, length) = (&bytes[o..o + 4], be32(bytes, o + 4));
        o += 8;
        let stop = bytes.len().min(o.saturating_add(length));
        if id != b"MTrk" {
            o = stop;
            continue;
        }
        let mut programs = [0u8; 16];
        let mut open: IndexMap<usize, VecDeque<MidiNote>> = IndexMap::new();
        let (mut tick, mut running, mut name) = (0u64, 0u8, String::new());
        let part = |parts: &mut IndexMap<(usize, u8), Part>, programs: &[u8; 16], ch: u8| {
            parts.entry((t, ch)).or_insert_with(|| Part { track: t, name: String::new(), drums: ch == DRUMS, program: programs[ch as usize], notes: vec![] });
        };
        while o < stop {
            tick = tick.saturating_add(vlq(bytes, &mut o, stop));
            let mut status = at(bytes, o);
            if status >= 0x80 {
                o += 1;
            } else if running != 0 {
                // running status: the last one again
                status = running;
            } else {
                break;
            }
            if status == 0xff {
                // a meta event
                let ty = at(bytes, o);
                o += 1;
                let n = vlq(bytes, &mut o, stop) as usize;
                let d = sub(bytes, o, o.saturating_add(n));
                o = o.saturating_add(n);
                running = 0;
                if ty == 0x51 && n >= 3 {
                    tempos.push((tick, (at(d, 0) as u32) << 16 | (at(d, 1) as u32) << 8 | at(d, 2) as u32));
                } else if ty == 0x58 && n >= 2 {
                    signatures.push((tick, at(d, 0) as u32, 2f64.powi(at(d, 1) as i32)));
                } else if ty == 0x03 && name.is_empty() {
                    name = text(d, 0, n);
                } else if ty == 0x2f {
                    break;
                }
                continue;
            }
            if status == 0xf0 || status == 0xf7 {
                // a system exclusive
                let n = vlq(bytes, &mut o, stop) as usize;
                o = o.saturating_add(n);
                running = 0;
                continue;
            }
            running = status;
            let (ty, ch, a, b) = (status >> 4, status & 15, at(bytes, o) & 127, at(bytes, o + 1) & 127);
            o += if ty == 0xc || ty == 0xd { 1 } else { 2 };
            if ty == 0xc {
                programs[ch as usize] = a;
            } else if ty == 9 && b != 0 {
                open.entry(ch as usize * 128 + a as usize).or_default().push_back(MidiNote { key: a, start: tick, end: 0, program: programs[ch as usize] });
                part(&mut parts, &programs, ch);
            } else if ty == 8 || ty == 9 {
                let Some(mut n) = open.get_mut(&(ch as usize * 128 + a as usize)).and_then(VecDeque::pop_front) else { continue };
                n.end = tick;
                part(&mut parts, &programs, ch);
                parts[&(t, ch)].notes.push(n);
            }
        }
        // a note that is never let go
        for (k, list) in open {
            let ch = (k >> 7) as u8;
            for mut n in list {
                n.end = tick;
                part(&mut parts, &programs, ch);
                parts[&(t, ch)].notes.push(n);
            }
        }
        for p in parts.values_mut().filter(|p| p.track == t) {
            p.name = name.clone();
        }
        if t == 0 && !name.is_empty() {
            title = name;
        }
        o = stop;
        t += 1;
    }
    let mut list: Vec<Part> = parts.into_values().filter(|p| !p.notes.is_empty()).collect();
    for p in &mut list {
        // of a chord the highest note first
        p.notes.sort_by(|a, b| a.start.cmp(&b.start).then(b.key.cmp(&a.key)));
        p.program = p.notes[0].program;
        if p.drums {
            p.name = "drums".into();
        } else if p.name.is_empty() {
            p.name = GM_FAMILIES[(p.program >> 3) as usize].0.into();
        }
    }
    tempos.sort_by_key(|t| t.0);
    signatures.sort_by_key(|s| s.0);
    let signature = signatures.first().map_or((4, 4.), |s| (s.1, s.2));
    Ok(Midi { tpq, tempos, signature, title, parts: list })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiPart {
    pub name: String,
    pub notes: usize,
    pub drums: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiInfo {
    pub title: String,
    pub bpm: u32,
    pub parts: Vec<MidiPart>,
}

pub fn midi_info(bytes: &[u8]) -> Result<MidiInfo, String> {
    let m = read_midi(bytes)?;
    if m.parts.is_empty() {
        return Err("the MIDI file plays no note".into());
    }
    let usq = m.tempos.first().map_or(500000, |t| t.1);
    Ok(MidiInfo {
        title: m.title,
        bpm: round(60e6 / usq as f64) as u32,
        parts: m.parts.iter().map(|p| MidiPart { name: p.name.clone(), notes: p.notes.len(), drums: p.drums }).collect(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiOptions {
    /// the rows of a quarter note (4: a row is a sixteenth)
    pub rows_per_quarter: usize,
    /// channel 10 too
    pub drums: bool,
    /// a voice for every note at once (up to MAX_VOICES), else the notes shared out to three
    pub all: bool,
}

impl Default for MidiOptions {
    fn default() -> Self {
        MidiOptions { rows_per_quarter: 4, drums: true, all: true }
    }
}

/// A note of a MIDI file on the rows.
struct Placed {
    part: usize,
    start: usize,
    end: usize,
    drum: Option<&'static str>,
    note: u8,
    ins: usize,
    voice: usize,
}

/// Whose voice a voice was last: a part's, or of the drums a drum's.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Who {
    Part(usize),
    Drum(&'static str),
}

pub fn import_midi(bytes: &[u8], o: &MidiOptions) -> Result<Imported, String> {
    let m = read_midi(bytes)?;
    let mut notes = vec![];
    if m.parts.is_empty() {
        return Err("the MIDI file plays no note".into());
    }
    let (rpq, all) = (o.rows_per_quarter, o.all);
    let usq = m.tempos.first().map_or(500000, |t| t.1) as f64;
    let tempo = 1f64.max(255f64.min(round(usq / 1e6 * 50. / rpq as f64))) as u8;
    let first = m.tempos.first().map(|t| t.1);
    if m.tempos.iter().any(|t| Some(t.1) != first) {
        notes.push(format!("the tempo changes: it stays {} bpm", round(60e6 / usq)));
    }
    let row_of = |tick: u64| round(tick as f64 * rpq as f64 / m.tpq) as usize;

    // the instruments: one of a part, one of each drum - without the filter: the SID has one for all
    let mut instruments: Vec<Instrument> = vec![];
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut instrument = |key: String, name: &str, from: &str| {
        *by_key.entry(key).or_insert_with(|| {
            let name: String = name.chars().take(12).collect();
            instruments.push(Instrument { name, filter: 0, cutoff: 128, res: 0, sweep: 0, ..preset(from).unwrap_or_default() });
            instruments.len()
        })
    };
    let pitch = |key: u8| {
        let mut n = key as i32 - 11;
        while n < 1 {
            n += 12;
        }
        while n > NOTES as i32 {
            n -= 12;
        }
        n as u8
    };

    // the parts, the most played first, the drums last: they fill what the others leave free
    let mut parts: Vec<&Part> = m.parts.iter().filter(|p| o.drums || !p.drums).collect();
    parts.sort_by(|a, b| a.drums.cmp(&b.drums).then(b.notes.len().cmp(&a.notes.len())));
    if parts.is_empty() {
        return Err("the MIDI file plays only drums".into());
    }
    let mut length = 0;
    let mut wanted: Vec<Vec<Placed>> = vec![];
    for (k, p) in parts.iter().enumerate() {
        let mut list: Vec<Placed> = p
            .notes
            .iter()
            .map(|n| {
                let start = row_of(n.start);
                if p.drums {
                    let drum = drum_of(n.key);
                    let ins = instrument(format!("drum {drum}"), drum, drum);
                    Placed { part: k, start, end: start + 1, drum: Some(drum), note: drum_note(drum), ins, voice: 0 }
                } else {
                    let (family, ready) = GM_FAMILIES[(n.program >> 3) as usize];
                    let name = if p.name.is_empty() { family } else { &p.name };
                    let ins = instrument(format!("{}:{}", p.track, n.program), name, ready);
                    Placed { part: k, start, end: (start + 1).max(row_of(n.end)), drum: None, note: pitch(n.key), ins, voice: 0 }
                }
            })
            .collect();
        // on three voices of drums at once one: the kick before the snare before the rest
        if p.drums && !all {
            let rank = |d: Option<&str>| DRUM_RANK.iter().position(|&r| Some(r) == d);
            list.sort_by(|a, b| a.start.cmp(&b.start).then(rank(a.drum).cmp(&rank(b.drum))));
            list.dedup_by(|b, a| a.start == b.start);
        }
        list.retain(|n| n.start < MAX_ROWS);
        for n in &list {
            length = length.max(MAX_ROWS.min(n.end));
        }
        wanted.push(list);
    }
    if m.parts.iter().any(|p| row_of(p.notes[p.notes.len() - 1].start) >= MAX_ROWS) {
        notes.push(format!("the song is cut after {MAX_ROWS} rows"));
    }

    // the voices: a row holds the note that sounds there. A note goes to a voice that is free while it
    // sounds - the voice its part played last first. A note of the same part that started before is
    // cut. All: a voice is of one part, a part takes another voice when its own ones sound
    let mut owner: Vec<Vec<i64>> = vec![];
    let mut part_of: Vec<i64> = vec![];
    let open = |owner: &mut Vec<Vec<i64>>, part_of: &mut Vec<i64>| {
        owner.push(vec![-1; length + 1]);
        part_of.push(-1);
    };
    if !all {
        for _ in 0..VOICES {
            open(&mut owner, &mut part_of);
        }
    }
    let mut placed: Vec<Placed> = vec![];
    // the voice a part last played on; of the drums: a drum
    let mut last: HashMap<Who, usize> = HashMap::new();
    let mut dropped = 0;
    for list in wanted {
        for mut n in list {
            let free = |owner: &[Vec<i64>], v: usize| {
                (n.start..n.end).all(|r| {
                    let q = owner[v].get(r).copied().unwrap_or(-1);
                    q < 0 || (placed[q as usize].part == n.part && placed[q as usize].start < n.start)
                })
            };
            let mine = |owner: &[Vec<i64>], part_of: &[i64], v: usize| {
                !all || part_of[v] == n.part as i64 || (part_of[v] < 0 && owner.len() >= MAX_VOICES)
            };
            let who = match n.drum {
                Some(d) => Who::Drum(d),
                None => Who::Part(n.part),
            };
            let mut v = std::iter::once(last.get(&who).copied().unwrap_or(0))
                .chain(0..owner.len())
                .find(|&x| x < owner.len() && mine(&owner, &part_of, x) && free(&owner, x));
            if v.is_none() && all && owner.len() < MAX_VOICES {
                open(&mut owner, &mut part_of);
                v = Some(owner.len() - 1);
            }
            // no voice left: any voice that is free then
            if v.is_none() && all {
                v = (0..owner.len()).find(|&x| free(&owner, x));
            }
            let Some(v) = v else {
                dropped += 1;
                continue;
            };
            if part_of[v] < 0 {
                part_of[v] = n.part as i64;
            }
            let q = owner[v][n.start];
            if q >= 0 {
                placed[q as usize].end = n.start;
                let mut r = n.start;
                while owner[v].get(r) == Some(&q) {
                    owner[v][r] = -1;
                    r += 1;
                }
            }
            n.voice = v;
            last.insert(who, v);
            let to = n.end.min(owner[v].len());
            owner[v][n.start..to].fill(placed.len() as i64);
            placed.push(n);
        }
    }
    if dropped > 0 {
        let on = if all { MAX_VOICES.to_string() } else { "three".into() };
        notes.push(format!("{dropped} of {} notes did not fit the {on} voices", dropped + placed.len()));
    }
    while owner.len() < VOICES {
        open(&mut owner, &mut part_of);
    }
    // the game plays the first voice of the three most played parts
    let mut game: Vec<usize> = vec![];
    for v in 0..owner.len() {
        if game.len() < VOICES && !game.iter().any(|&g| part_of[g] == part_of[v]) {
            game.push(v);
        }
    }
    for v in 0..owner.len() {
        if game.len() < VOICES && !game.contains(&v) {
            game.push(v);
        }
    }
    game.sort();

    let mut voices: Vec<Vec<Cell>> = vec![vec![[0; 4]; length]; owner.len()];
    for n in &placed {
        voices[n.voice][n.start] = [n.note as usize, n.ins, 0, 0];
    }
    // the end of a note that nothing follows at once: a note off (a drum just dies away)
    for n in &placed {
        if n.drum.is_none() && n.end < length && voices[n.voice][n.end][0] == 0 && owner[n.voice][n.end] < 0 {
            voices[n.voice][n.end] = [OFF, 0, 0, 0];
        }
    }

    // the rows of a pattern: whole bars, near 32
    let (num, den) = m.signature;
    let bar = rpq as f64 * 4. * num as f64 / den;
    let fits: Vec<usize> = [32, 64, 48, 16]
        .into_iter()
        .filter(|&r| bar.is_finite() && bar.fract() == 0. && bar > 0. && r as f64 % bar == 0.)
        .collect();
    let first_fit = fits.first().copied().unwrap_or(32);
    let rows = if length > first_fit * MAX_ORDERS && fits.contains(&64) { 64 } else { first_fit };
    let n_parts = parts.len();
    let build = Build { tempo, rows, voices, instruments, loop_row: 0, game: game.clone(), tables: Default::default() };
    let song = build_song(build, &mut notes);
    let on = if all {
        let game: Vec<String> = game.iter().map(|v| (v + 1).to_string()).collect();
        format!("{} voices, the game plays {}", song.voices, game.join(", "))
    } else {
        "three voices".into()
    };
    let title = if m.title.is_empty() { String::new() } else { format!(" \"{}\"", m.title) };
    notes.insert(
        0,
        format!(
            "MIDI file{title}: {n_parts} part{} on {on}, a row is {tempo} frame{} - without velocity, pitch bends and controllers",
            plural(n_parts),
            plural(tempo as usize)
        ),
    );
    Ok(Imported { song, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::{freq, CELL};

    fn cell(s: &Song, pattern: usize, row: usize, voice: usize) -> [u8; 4] {
        let o = cell_offset(row, voice, s.voices);
        s.patterns[pattern][o..o + CELL].try_into().unwrap()
    }

    // --- MIDI ---

    fn vl(mut v: u32) -> Vec<u8> {
        let mut out = vec![(v & 127) as u8];
        v >>= 7;
        while v > 0 {
            out.insert(0, (v & 127) as u8 | 128);
            v >>= 7;
        }
        out
    }

    /// A track of (delta ticks, event bytes), ended.
    fn track(events: &[(u32, &[u8])]) -> Vec<u8> {
        let mut body = vec![];
        for (d, e) in events {
            body.extend(vl(*d));
            body.extend_from_slice(e);
        }
        body.extend([0, 0xff, 0x2f, 0]);
        let mut t = b"MTrk".to_vec();
        t.extend((body.len() as u32).to_be_bytes());
        t.extend(body);
        t
    }

    fn midi(format: u16, tracks: &[Vec<u8>]) -> Vec<u8> {
        let mut m = b"MThd".to_vec();
        m.extend(6u32.to_be_bytes());
        m.extend(format.to_be_bytes());
        m.extend((tracks.len() as u16).to_be_bytes());
        m.extend(96u16.to_be_bytes());
        for t in tracks {
            m.extend(t);
        }
        m
    }

    const TEMPO_120: &[u8] = &[0xff, 0x51, 3, 0x07, 0xa1, 0x20];

    #[test]
    fn midi_format_0() {
        let bytes = midi(
            0,
            &[track(&[
                (0, TEMPO_120),
                (0, &[0xff, 0x03, 4, b't', b'e', b's', b't']),
                (0, &[0x90, 60, 100]),
                (0, &[0x99, 36, 100]),
                (24, &[0x89, 36, 0]),
                (72, &[0x80, 60, 0]),
                // running status: a note on of velocity 0 ends it
                (0, &[0x90, 64, 100]),
                (96, &[64, 0]),
            ])],
        );
        assert_eq!(song_kind(&bytes), Some(SongKind::Midi));
        let info = midi_info(&bytes).unwrap();
        assert_eq!(info.title, "test");
        assert_eq!(info.bpm, 120);
        assert_eq!(
            info.parts,
            [MidiPart { name: "test".into(), notes: 2, drums: false }, MidiPart { name: "drums".into(), notes: 1, drums: true }]
        );
        let i = import_midi(&bytes, &MidiOptions::default()).unwrap();
        let s = &i.song;
        // 120 bpm, a row a sixteenth: 6.25 frames
        assert_eq!((s.tempo, s.rows, s.voices, s.orders.len()), (6, 32, 3, 1));
        assert_eq!(s.game, [0, 1, 2]);
        assert_eq!(cell(s, 0, 0, 0), [49, 1, 0, 0]);
        assert_eq!(cell(s, 0, 4, 0), [53, 1, 0, 0]);
        assert_eq!(cell(s, 0, 8, 0), [0, 0, 0, 0]);
        assert_eq!(cell(s, 0, 0, 1), [25, 2, 0, 0]);
        assert_eq!(cell(s, 0, 1, 1), [0, 0, 0, 0]);
        assert_eq!(s.instruments.len(), 2);
        assert_eq!((s.instruments[0].name.as_str(), s.instruments[0].wave), ("test", 0x40));
        assert_eq!((s.instruments[1].name.as_str(), s.instruments[1].filter, s.instruments[1].cutoff), ("kick", 0, 128));
        assert!(i.notes[0].starts_with("MIDI file \"test\": 2 parts on 3 voices, the game plays 1, 2, 3, a row is 6 frames"), "{}", i.notes[0]);
    }

    #[test]
    fn midi_format_1_chords_and_drums() {
        let bytes = midi(
            1,
            &[
                track(&[(0, TEMPO_120)]),
                track(&[(0, &[0x90, 60, 100]), (0, &[0x90, 64, 100]), (96, &[0x80, 60, 0]), (0, &[0x80, 64, 0]), (0, &[0x90, 67, 90]), (48, &[0x80, 67, 0])]),
                track(&[(0, &[0x99, 36, 100]), (0, &[0x99, 38, 100]), (10, &[0x89, 36, 0]), (0, &[0x89, 38, 0])]),
            ],
        );
        // all: a voice for every note at once
        let i = import_midi(&bytes, &MidiOptions::default()).unwrap();
        let s = &i.song;
        assert_eq!(s.voices, 4);
        assert_eq!(s.game, [0, 1, 2]);
        // of a chord the highest note first
        assert_eq!(cell(s, 0, 0, 0)[0], 53);
        assert_eq!(cell(s, 0, 0, 1)[0], 49);
        // the G goes on in the voice of its part that is free; the E ends with nothing after it
        assert_eq!(cell(s, 0, 4, 1)[0], 56);
        assert_eq!(cell(s, 0, 4, 0)[0], NOTE_OFF);
        let drums: Vec<u8> = (2..4).map(|v| cell(s, 0, 0, v)[0]).collect();
        assert_eq!(drums, [61, 25]);
        assert_eq!(s.instruments[0].name, "piano");

        // three voices: the kick wins of the drums
        let i = import_midi(&bytes, &MidiOptions { all: false, ..Default::default() }).unwrap();
        let s = &i.song;
        assert_eq!(s.voices, 3);
        assert_eq!([cell(s, 0, 0, 0)[0], cell(s, 0, 0, 1)[0], cell(s, 0, 0, 2)[0]], [53, 49, 25]);
        assert!(i.notes[0].contains("on three voices"));

        // without the drums
        let i = import_midi(&bytes, &MidiOptions { drums: false, ..Default::default() }).unwrap();
        assert_eq!(i.song.voices, 3);
        assert_eq!(cell(&i.song, 0, 0, 2)[0], 0);
    }

    #[test]
    fn midi_errors() {
        assert!(midi_info(b"MThd").is_err());
        let only_drums = midi(0, &[track(&[(0, &[0x99, 36, 100]), (10, &[0x89, 36, 0])])]);
        assert_eq!(import_midi(&only_drums, &MidiOptions { drums: false, ..Default::default() }).unwrap_err(), "the MIDI file plays only drums");
        let none = midi(0, &[track(&[(0, TEMPO_120)])]);
        assert_eq!(midi_info(&none).unwrap_err(), "the MIDI file plays no note");
    }

    // --- SID ---

    /// A PSID at $1000: every 6 frames a note, A-4 and C-5 in turn, its gate on for 3 frames.
    fn psid() -> Vec<u8> {
        let mut h = vec![0u8; 0x7c];
        h[..4].copy_from_slice(b"PSID");
        h[4..6].copy_from_slice(&2u16.to_be_bytes());
        h[6..8].copy_from_slice(&0x7cu16.to_be_bytes());
        h[8..10].copy_from_slice(&0x1000u16.to_be_bytes());
        h[0x0a..0x0c].copy_from_slice(&0x1000u16.to_be_bytes());
        h[0x0c..0x0e].copy_from_slice(&0x1003u16.to_be_bytes());
        h[0x0e..0x10].copy_from_slice(&1u16.to_be_bytes());
        h[0x10..0x12].copy_from_slice(&1u16.to_be_bytes());
        h[0x16..0x1a].copy_from_slice(b"test");
        h[0x36..0x38].copy_from_slice(b"me");
        let mut code = vec![0u8; 0x104];
        let mut put = |at: usize, b: &[u8]| code[at - 0x1000..at - 0x1000 + b.len()].copy_from_slice(b);
        put(0x1000, &[0x4c, 0x06, 0x10, 0x4c, 0x20, 0x10]);
        // init: the volume, the envelope, the counters
        put(0x1006, &[0xa9, 0x0f, 0x8d, 0x18, 0xd4, 0xa9, 0x00, 0x85, 0xfb, 0x85, 0xfc, 0xa9, 0x09, 0x8d, 0x05, 0xd4, 0xa9, 0x00, 0x8d, 0x06, 0xd4, 0x60]);
        // play: at 0 the note and the gate, at 3 the gate off and the next note
        put(0x1020, &[
            0xa5, 0xfb, 0xd0, 0x13, 0xa6, 0xfc, 0xbd, 0x00, 0x11, 0x8d, 0x00, 0xd4, 0xbd, 0x02, 0x11, 0x8d, 0x01, 0xd4, 0xa9, 0x41, 0x8d, 0x04, 0xd4,
            0xa5, 0xfb, 0xc9, 0x03, 0xd0, 0x0b, 0xa9, 0x40, 0x8d, 0x04, 0xd4, 0xa5, 0xfc, 0x49, 0x01, 0x85, 0xfc,
            0xe6, 0xfb, 0xa5, 0xfb, 0xc9, 0x06, 0xd0, 0x04, 0xa9, 0x00, 0x85, 0xfb, 0x60,
        ]);
        let (a, c) = (freq(57), freq(60));
        put(0x1100, &[a as u8, c as u8, (a >> 8) as u8, (c >> 8) as u8]);
        h.extend(code);
        h
    }

    #[test]
    fn sid_tune() {
        let bytes = psid();
        assert_eq!(song_kind(&bytes), Some(SongKind::Sid));
        assert_eq!(sid_info(&bytes).unwrap(), SidInfo { title: "test".into(), author: "me".into(), songs: 1, start: 1 });
        let frames = sid_frames(&bytes, None, 1).unwrap();
        assert_eq!(frames.len(), 50);
        assert_eq!(frames[0].voices[0].ctrl, 0x41);
        assert_eq!(frames[3].voices[0].ctrl, 0x40);
        assert_eq!(frames[6].voices[0].freq, freq(60));

        let i = import_sid(&bytes, None, 2).unwrap();
        let s = &i.song;
        assert_eq!(s.tempo, 6);
        assert!(i.notes[0].starts_with("SID tune \"test\" by me: 2 s heard as notes, a row is 6 frames"), "{}", i.notes[0]);
        // A-4 and C-5 in turn, a row each
        for r in 0..17 {
            assert_eq!(cell(s, 0, r, 0), [if r % 2 == 0 { 58 } else { 61 }, 1, 0, 0], "row {r}");
        }
        assert_eq!(cell(s, 0, 17, 0), [0; 4]);
        assert_eq!(cell(s, 0, 0, 1), [0; 4]);
        assert_eq!(s.instruments.len(), 1);
        let ins = &s.instruments[0];
        assert_eq!((ins.name.as_str(), ins.wave, ins.a, ins.d, ins.s, ins.r), ("pulse 1", 0x40, 0, 9, 0, 0));
        assert_eq!(ins.wave_tab, 0);
    }

    #[test]
    fn sid_vibrato_and_arpeggio() {
        // a long note of voice 0 with a vibrato; voice 1 an arpeggio of 3 notes, a frame each
        let base = freq(48) as i32;
        let frames: Vec<Frame> = (0..240)
            .map(|f| {
                let mut fr = Frame::default();
                let on = f % 120 < 100;
                let swing = [0, 10, 20, 30, 20, 10, 0, -10, -20, -30, -20, -10][f % 12];
                fr.voices[0] = SidVoice { freq: (base + swing) as u16, pulse: 0, ctrl: 0x20 | on as u8, ad: 0x0a, sr: 0xa0 };
                let arp = [0, 4, 7][f % 3];
                fr.voices[1] = SidVoice { freq: freq(36 + arp), pulse: 0x800, ctrl: 0x40 | on as u8, ad: 0x00, sr: 0xf0 };
                fr
            })
            .collect();
        let t = transcribe_sid(&frames, Some(10)).unwrap();
        assert_eq!(t.tempo, 10);
        let s = &t.song;
        assert_eq!(cell(s, 0, 0, 0)[0], 49);
        assert_eq!(cell(s, 0, 0, 1)[0], 37);
        assert_eq!(cell(s, 0, 10, 0)[0], NOTE_OFF);
        assert_eq!(cell(s, 0, 12, 0)[0], 49);
        // the arpeggio is a wave table that comes round again, the vibrato a row of the speed table
        let arp = &s.instruments[cell(s, 0, 0, 1)[1] as usize - 1];
        assert_ne!(arp.wave_tab, 0);
        let wave = s.table(Table::Wave);
        let at = arp.wave_tab as usize - 1;
        assert_eq!(&wave[at..at + 4], &[[0x40, 0], [0x40, 4], [0x40, 7], [0xff, arp.wave_tab]]);
        let vib = &s.instruments[cell(s, 0, 0, 0)[1] as usize - 1];
        assert_eq!(vib.wave_tab, 0);
        assert_eq!(s.table_row(Table::Speed, vib.vib_tab), Some([6, 10]));
        assert!(t.made.contains("wave") && t.made.contains("speed"));
    }

    #[test]
    fn sid_errors() {
        assert_eq!(sid_info(b"MThd").unwrap_err(), "not a SID tune");
        let mut silent = psid();
        // play: RTS at once
        silent[0x7c + 0x20] = 0x60;
        silent[0x7c + 0x06] = 0x60;
        assert_eq!(import_sid(&silent, None, 1).unwrap_err(), "the tune plays no note in this time");
    }

    // --- GoatTracker ---

    fn gts5() -> Vec<u8> {
        let mut b = b"GTS5".to_vec();
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"goat");
        b.extend(name);
        b.extend([0u8; 64]);
        b.push(1);
        // the orderlists: pattern 0, end, restart at 0; the others pattern 1 (empty)
        b.extend([2, 0x00, 0xff, 0x00]);
        b.extend([2, 0x01, 0xff, 0x00]);
        b.extend([2, 0x01, 0xff, 0x00]);
        // an instrument
        b.push(1);
        b.extend([0x09, 0x00, 1, 0, 0, 0, 0, 2, 9]);
        let mut name = [0u8; 16];
        name[..4].copy_from_slice(b"bass");
        b.extend(name);
        // the tables: wave, pulse, filter, speed
        b.extend([2, 0x41, 0xff, 0x00, 0x00]);
        b.extend([0]);
        b.extend([0]);
        b.extend([1, 0x81, 0x20]);
        // the patterns
        b.push(2);
        b.push(5);
        b.extend([0x60 + 48, 1, 0x0f, 0x05]);
        b.extend([GT_KEYOFF, 0, 0, 0]);
        b.extend([0x60 + 50, 1, 0x04, 0x01]);
        b.extend([0xbd, 0, 0x0a, 0x00]);
        b.extend([0xff, 0, 0, 0]);
        b.push(1);
        b.extend([0xff, 0, 0, 0]);
        b
    }

    #[test]
    fn goattracker_song() {
        let bytes = gts5();
        assert_eq!(song_kind(&bytes), Some(SongKind::Goat));
        let i = import_goat(&bytes).unwrap();
        let s = &i.song;
        assert_eq!((s.tempo, s.rows, s.voices, s.loop_to), (5, 32, 3, Some(0)));
        assert_eq!(s.orders, [0]);
        assert_eq!(cell(s, 0, 0, 0), [49, 1, CMD_TEMPO, 5]);
        assert_eq!(cell(s, 0, 1, 0), [NOTE_OFF, 0, 0, 0]);
        assert_eq!(cell(s, 0, 2, 0), [51, 1, CMD_VIB_T, 1]);
        assert_eq!(cell(s, 0, 3, 0), [0; 4]);
        assert_eq!(cell(s, 0, 0, 1), [0; 4]);
        let ins = &s.instruments[0];
        assert_eq!((ins.name.as_str(), ins.wave, ins.d, ins.wave_tab, ins.pulse), ("bass", 0x40, 9, 1, 2048));
        assert_eq!(s.table(Table::Wave), &vec![[0x41, 0], [0xff, 0]]);
        // the relative depth is dropped
        assert_eq!(s.table(Table::Speed), &vec![[0x01, 0x20]]);
        assert_eq!(i.notes[0], "GoatTracker song \"goat\": the instruments and their tables as they are");
        assert!(i.notes.contains(&"commands not imported: A".to_string()));
        assert!(i.notes.iter().any(|n| n.starts_with("a vibrato with a depth relative")));

        assert!(import_goat(&bytes[..bytes.len() - 3]).unwrap_err().contains("cut short"));
        let mut old = bytes.clone();
        old[3] = b'4';
        assert!(import_goat(&old).unwrap_err().contains("old format GTS4"));
    }
}
