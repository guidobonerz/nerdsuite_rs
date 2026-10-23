//! The Composer, after nerdsuite2's `sound.js`: a tracker for the three voices of the SID - or more,
//! of an imported MIDI file: then the game plays up to three of them, chosen in the head of the
//! pattern, and the editor plays all of them (a SID for every three) or only those.
//!   left    the orders: the patterns in the order the song plays them
//!   middle  the pattern of the selected order: a row is a step of the song, a column a voice, a
//!           cell a note, the instrument that plays it and a command with its value
//!   right   the song (tempo, rows), the instruments, the tables of the song (GoatTracker's: wave,
//!           pulse, filter, speed) and the selection
//! Notes are typed like on a piano, by the place of the keys (QWERTZ and QWERTY play the same):
//! Z S X D C ... is the octave, Q 2 W 3 E ... the next one. The song plays on the model of the SID
//! and its player (`nerdsuite_core::sid`) in the audio thread (`audio.rs`). The song is a file of
//! the project (.sng); GoatTracker songs, SID tunes and MIDI files are imported
//! (`nerdsuite_core::songimport`) as a new song.

use std::{cell::Cell, collections::HashMap, rc::Rc, time::Duration};

use gpui::{
    App, Bounds, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, ScrollHandle, SharedString, Task, Window, anchored, canvas, deferred, div, point, prelude::*, px,
};
use nerdsuite_core::{
    song::{
        CELL, COMMANDS, FILTERS, Instrument, MAX_INSTRUMENTS, MAX_ORDERS, MAX_PATTERNS, MAX_TABLE_ROWS, NOTE_NAMES, NOTE_OFF, NOTES, RING,
        ROW_CHOICES, SYNC, Song, Table, VOICES, WAVES, cell_offset, new_pattern, note_name, presets,
    },
    songimport::{self, MidiOptions, SongKind},
};

use crate::{
    audio::Audio,
    theme::{Theme, mono_font, theme},
};

pub enum ComposerEvent {
    OpenFile(String),
    Changed,
}

impl EventEmitter<ComposerEvent> for ComposerView {}

const UNDO_LIMIT: usize = 100;
const HEX: &[u8; 16] = b"0123456789ABCDEF";
/// the folder of the music of a project (nerdsuite2's `layout.js`)
const MUSIC: &str = "audio/music";
/// the columns of a cell: note, instrument, command, value
const COLS: usize = 4;
/// the width of the column of a voice
const VOICE_WIDTH: f32 = 160.;
const EMPTY: [u8; CELL] = [0; CELL];
/// the milliseconds of an attack per rate; decay and release take three times as long
const ATTACK_MS: [u32; 16] = [2, 8, 16, 24, 38, 56, 68, 80, 100, 250, 500, 800, 1000, 3000, 5000, 8000];
/// the waveforms the popup of the wave table offers (the gate bit is the note's)
const WAVE_CHOICES: [u8; 13] = [0x11, 0x21, 0x41, 0x81, 0x31, 0x51, 0x61, 0x71, 0x13, 0x15, 0x17, 0x23, 0x43];
const WAVE_BITS: [(u8, &str); 7] = [(0x10, "triangle"), (0x20, "sawtooth"), (0x40, "pulse"), (0x80, "noise"), (0x02, "sync"), (0x04, "ring"), (0x08, "test")];
const FILTER_BITS: [(u8, &str); 3] = [(1, "low pass"), (2, "band pass"), (4, "high pass")];
/// the choices of the popups: frames of a sweep, its speeds, the speeds of slides
const FRAME_PICKS: [u8; 24] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 20, 24, 32, 40, 48, 64, 96, 127];
const SPEED_PICKS: [i32; 24] = [-128, -96, -64, -48, -32, -24, -16, -12, -8, -4, -2, -1, 1, 2, 4, 8, 12, 16, 24, 32, 48, 64, 96, 127];
const SLIDE_PICKS: [u16; 16] = [4, 8, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 2048];
const FILTER_TYPES: [u8; 8] = [0, 1, 2, 4, 3, 5, 6, 7];
/// the semitone of a key, by its place on a QWERTY keyboard
const NOTE_KEYS: [(&str, u8); 32] = [
    ("z", 0), ("s", 1), ("x", 2), ("d", 3), ("c", 4), ("v", 5), ("g", 6), ("b", 7), ("h", 8), ("n", 9), ("j", 10), ("m", 11), (",", 12), ("l", 13), (".", 14),
    ("q", 12), ("2", 13), ("w", 14), ("3", 15), ("e", 16), ("r", 17), ("5", 18), ("t", 19), ("6", 20), ("y", 21), ("7", 22), ("u", 23),
    ("i", 24), ("9", 25), ("o", 26), ("0", 27), ("p", 28),
];

fn hex2(n: usize) -> String {
    format!("{:02X}", n & 255)
}

fn hex1(n: usize) -> char {
    HEX[n & 15] as char
}

fn ms(v: u32) -> String {
    if v < 1000 { format!("{v} ms") } else { format!("{} s", v as f32 / 1000.) }
}

fn wave_name(l: u8) -> String {
    WAVE_BITS.iter().filter(|(b, _)| l & b != 0).map(|(_, t)| *t).collect::<Vec<_>>().join(" + ")
}

fn filter_name(m: u8) -> String {
    if m == 0 {
        return "off".into();
    }
    FILTER_BITS.iter().filter(|(b, _)| m & b != 0).map(|(_, t)| *t).collect::<Vec<_>>().join(" + ")
}

fn cutoff_text(v: u8) -> String {
    format!("{} Hz", ((30. + v as f64 * 8. * 5.8) / 10.).round() as u32 * 10)
}

fn signed_text(v: u8) -> String {
    let s = v as i8;
    if s > 0 { format!("+{s}") } else { s.to_string() }
}

fn percent(l: u8, r: u8) -> u32 {
    ((((l as u32 & 15) << 8) | r as u32) as f64 / 40.95).round() as u32
}

fn is_wait(l: u8) -> bool {
    (1..=0x0f).contains(&l)
}

fn is_set(l: u8) -> bool {
    (0x80..0xff).contains(&l)
}

fn to_row(r: u8) -> String {
    if r != 0 { format!("to row {}", hex2(r as usize)) } else { "stop".into() }
}

/// What the two bytes of a row of a table do, by name.
fn row_text(k: Table, l: u8, r: u8) -> [String; 2] {
    match k {
        Table::Wave => {
            let left = match l {
                0 => "waveform stays".into(),
                1..=0x0f => format!("wait {l}"),
                0x10..=0xef => wave_name(l),
                0xf0..=0xfe => "GoatTracker command".into(),
                _ => "jump".into(),
            };
            let right = if l == 0xff {
                to_row(r)
            } else if is_wait(l) || (0xf0..0xff).contains(&l) {
                String::new()
            } else if r < 0x60 {
                if r != 0 { format!("+{r}") } else { "the note".into() }
            } else if r < 0x80 {
                format!("-{}", 0x80 - r as u32)
            } else if r == 0x80 {
                "pitch stays".into()
            } else {
                note_name((r - 0x80).min(NOTES - 1) + 1)
            };
            [left, right]
        }
        Table::Speed => [format!("{l} frames"), format!("depth {r}")],
        _ if l == 0xff => ["jump".into(), to_row(r)],
        _ if l != 0 && l < 0x80 => [format!("{l} frames"), format!("{} a frame", signed_text(r))],
        Table::Pulse if l != 0 => ["width".into(), format!("{} %", percent(l, r))],
        Table::Pulse => ["nothing".into(), String::new()],
        _ if l != 0 => [filter_name((l >> 4) & 7), format!("resonance {}", r >> 4)],
        _ => ["cutoff".into(), cutoff_text(r)],
    }
}

fn table_hint(k: Table) -> &'static str {
    match k {
        Table::Wave => "A step a frame: a waveform and a pitch (semitones above the note, below it, or a fixed note), a wait, a jump. An instrument's wave table takes the place of its waveform and arpeggio.",
        Table::Pulse => "A pulse width, or a sweep: so many frames by so much a frame; a jump. It takes the place of the instrument's width and sweep.",
        Table::Filter => "The type and resonance of the filter, its cutoff, or a sweep of the cutoff; a jump. The note takes the filter.",
        Table::Speed => "A vibrato (the frames of half a wave, its depth) for the vibrato row of an instrument and command D, or a 16 bit speed for the commands A-C.",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlayMode {
    Song,
    Pattern,
}

#[derive(Clone)]
struct Clip {
    rows: usize,
    voices: usize,
    data: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Selection {
    r0: usize,
    r1: usize,
    v0: usize,
    v1: usize,
}

type Act = Rc<dyn Fn(&mut ComposerView, &mut Context<ComposerView>)>;

fn act(f: impl Fn(&mut ComposerView, &mut Context<ComposerView>) + 'static) -> Act {
    Rc::new(f)
}

/// What a popup shows: headers, items to choose, grids of keys.
enum Entry {
    Header(String),
    Item { label: String, checked: bool, act: Option<Act> },
    Grid { width: f32, keys: Vec<Key> },
    /// the grid of the notes: 8 octaves of 12
    Notes { keys: Vec<Option<Key>> },
    Divider,
}

struct Key {
    label: String,
    checked: bool,
    sharp: bool,
    act: Act,
}

fn item(label: impl Into<String>, checked: bool, act: Act) -> Entry {
    Entry::Item { label: label.into(), checked, act: Some(act) }
}

fn key(label: impl Into<String>, checked: bool, act: Act) -> Key {
    Key { label: label.into(), checked, sharp: false, act }
}

struct Popup {
    at: Point<Pixels>,
    entries: Vec<Entry>,
}

/// A song of another program that waits for its choices.
struct PendingImport {
    bytes: Vec<u8>,
    name: String,
    /// the folder of the project it is in
    folder: Option<String>,
    kind: SongKind,
    info: String,
    // MIDI
    rows_per_quarter: usize,
    drums: Option<bool>,
    all: bool,
    // SID
    subtune: usize,
    subtunes: usize,
    seconds: u32,
}

type SetFn = Rc<dyn Fn(&mut ComposerView, i32, &mut Context<ComposerView>)>;

/// A slider being dragged.
struct Drag {
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    min: i32,
    max: i32,
    set: SetFn,
}

pub struct ComposerView {
    focus: FocusHandle,
    files: Vec<String>,
    /// the file of the song; None: a song not saved yet
    path: Option<String>,
    song: Song,
    saved: Song,
    /// a new or imported song: not in its file yet
    fresh: bool,
    undo: Vec<Song>,
    redo: Vec<Song>,
    /// the cursor: the position in the orders, the cell, 0 note / 1 instrument / 2 command / 3 its value
    order: usize,
    row: usize,
    voice: usize,
    col: usize,
    /// the selection reaches from (row, voice) to the cursor
    anchor: Option<(usize, usize)>,
    ins: usize,
    octave: u8,
    step: usize,
    playing: Option<PlayMode>,
    /// the row that plays: order, row
    pos: Option<(usize, usize)>,
    muted: Vec<bool>,
    /// all voices of the song are heard, not only the game's
    hear_all: bool,
    clip: Option<Clip>,
    table: Table,
    popup: Option<Popup>,
    pending: Option<PendingImport>,
    /// the keys that hold a note: key -> voice
    held: HashMap<String, usize>,
    audio: Option<Audio>,
    poll: Option<Task<()>>,
    sliders: HashMap<SharedString, Rc<Cell<Option<Bounds<Pixels>>>>>,
    drag: Option<Drag>,
    /// the name of the instrument while it is typed
    renaming: Option<String>,
    message: Option<(String, bool)>,
    rows_scroll: ScrollHandle,
    pattern_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// the keyboard has Y and Z swapped: their notes are by the place of the key
    qwertz: bool,
}

impl ComposerView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let layout = cx.keyboard_layout();
        let qwertz = is_qwertz(layout.id(), layout.name());
        let song = Song::default();
        ComposerView {
            focus: cx.focus_handle(),
            files: vec![],
            path: None,
            saved: song.clone(),
            fresh: false,
            song,
            undo: vec![],
            redo: vec![],
            order: 0,
            row: 0,
            voice: 0,
            col: 0,
            anchor: None,
            ins: 1,
            octave: 3,
            step: 1,
            playing: None,
            pos: None,
            muted: vec![],
            hear_all: true,
            clip: None,
            table: Table::Wave,
            popup: None,
            pending: None,
            held: HashMap::new(),
            audio: None,
            poll: None,
            sliders: HashMap::new(),
            drag: None,
            renaming: None,
            message: None,
            rows_scroll: ScrollHandle::new(),
            pattern_bounds: Default::default(),
            qwertz,
        }
    }

    pub fn set_files(&mut self, files: Vec<String>, cx: &mut Context<Self>) {
        if self.files != files {
            self.files = files;
            cx.notify();
        }
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// A song has been opened or made here (the first song of the project is not opened over it).
    pub fn has_song(&self) -> bool {
        self.path.is_some() || self.is_dirty()
    }

    pub fn is_dirty(&self) -> bool {
        self.fresh || self.song != self.saved
    }

    /// The song into its file: a new one into a free name of the music folder.
    pub fn unsaved(&self) -> Option<(String, Vec<u8>)> {
        self.is_dirty().then(|| (self.path.clone().unwrap_or_else(|| self.free_path("song")), self.song.encode()))
    }

    pub fn mark_saved(&mut self, path: String, cx: &mut Context<Self>) {
        self.saved = self.song.clone();
        self.fresh = false;
        self.path = Some(path);
        cx.emit(ComposerEvent::Changed);
        cx.notify();
    }

    /// The view closes: the song stops.
    pub fn leave(&mut self, cx: &mut Context<Self>) {
        if self.playing.is_some() {
            self.stop(cx);
        }
        self.release_all();
    }

    /// A path of the music folder no song has: base.sng, base-2.sng ...
    fn free_path(&self, base: &str) -> String {
        self.free_path_in(&format!("{MUSIC}/"), base)
    }

    fn free_path_in(&self, folder: &str, base: &str) -> String {
        (1..)
            .map(|n| if n == 1 { format!("{folder}{base}.sng") } else { format!("{folder}{base}-{n}.sng") })
            .find(|p| !self.files.iter().any(|f| f.eq_ignore_ascii_case(p)))
            .unwrap()
    }

    // --- the sound ---------------------------------------------------------------------------------

    /// The engine, started with the first sound.
    fn audio(&mut self) -> Option<&Audio> {
        if self.audio.is_none() {
            match Audio::start() {
                Ok(a) => {
                    a.with(|e| {
                        e.set_song(&self.song, self.hear_all);
                        for (v, &on) in self.muted.iter().enumerate() {
                            e.mute(v, on);
                        }
                    });
                    self.audio = Some(a);
                }
                Err(e) => self.message = Some((format!("No sound: {e:#}"), true)),
            }
        }
        self.audio.as_ref()
    }

    fn sync_song(&mut self) {
        if let Some(a) = &self.audio {
            a.with(|e| e.set_song(&self.song, self.hear_all));
        }
    }

    fn play(&mut self, mode: PlayMode, cx: &mut Context<Self>) {
        let (order, row) = (self.order, if mode == PlayMode::Pattern { 0 } else { self.row });
        let Some(a) = self.audio() else {
            cx.notify();
            return;
        };
        a.with(|e| e.play(order, row, mode == PlayMode::Pattern));
        self.playing = Some(mode);
        self.pos = None;
        // the row that plays, from the engine
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(30)).await;
                let Ok(go_on) = this.update(cx, |this, cx| this.on_pos(cx)) else { break };
                if !go_on {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(a) = &self.audio {
            a.with(|e| e.stop());
        }
        self.playing = None;
        self.pos = None;
        self.poll = None;
        cx.notify();
    }

    /// Whether the song still plays.
    fn on_pos(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(mode) = self.playing else { return false };
        let Some((pos, playing)) = self.audio.as_ref().map(|a| a.with(|e| (e.position, e.playing()))) else { return false };
        if !playing {
            self.playing = None;
            self.pos = None;
            cx.notify();
            return false;
        }
        if pos != self.pos {
            self.pos = pos;
            if let Some((order, row)) = pos {
                // follow the song
                if mode == PlayMode::Song && order != self.order && order < self.song.orders.len() {
                    self.order = order;
                }
                if order == self.order {
                    self.rows_scroll.scroll_to_item(row);
                }
            }
            cx.notify();
        }
        true
    }

    fn jam(&mut self, key: &str, note: u8) {
        if self.held.contains_key(key) {
            return;
        }
        let (voice, ins) = (self.voice, self.ins as u8);
        self.held.insert(key.to_owned(), voice);
        if let Some(a) = self.audio() {
            a.with(|e| e.note_on(voice, note, ins));
        }
    }

    fn release(&mut self, key: &str) {
        let Some(voice) = self.held.remove(key) else { return };
        if !self.held.values().any(|&v| v == voice)
            && let Some(a) = &self.audio
        {
            a.with(|e| e.note_off(voice));
        }
    }

    fn release_all(&mut self) {
        for key in self.held.keys().cloned().collect::<Vec<_>>() {
            self.release(&key);
        }
    }

    /// A note of the popup sounds for a moment.
    fn preview(&mut self, note: u8, cx: &mut Context<Self>) {
        let (voice, ins) = (self.voice, self.ins as u8);
        let Some(a) = self.audio() else { return };
        a.with(|e| e.note_on(voice, note, ins));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(350)).await;
            this.update(cx, |this, _| {
                if let Some(a) = &this.audio {
                    a.with(|e| e.note_off(voice));
                }
            })
            .ok();
        })
        .detach();
    }

    // --- undo and files ------------------------------------------------------------------------------

    fn snapshot(&mut self) {
        self.undo.push(self.song.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, redo: bool, cx: &mut Context<Self>) {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let Some(s) = from.pop() else { return };
        to.push(std::mem::replace(&mut self.song, s));
        self.clamp_cursor();
        self.sync_song();
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(ComposerEvent::Changed);
        cx.notify();
    }

    fn clamp_cursor(&mut self) {
        self.order = self.order.min(self.song.orders.len() - 1);
        self.row = self.row.min(self.song.rows - 1);
        self.voice = self.voice.min(self.song.voices - 1);
        self.ins = self.ins.clamp(1, self.song.instruments.len().max(1));
        self.anchor = None;
    }

    /// Another song in the editor: `path` its file, `saved` whether it is as the file has it.
    fn set_song(&mut self, mut song: Song, path: Option<String>, saved: bool, cx: &mut Context<Self>) {
        self.stop(cx);
        // the editor needs an instrument to play the notes with
        if song.instruments.is_empty() {
            song.instruments.push(Instrument::default());
        }
        self.saved = song.clone();
        self.fresh = !saved;
        self.song = song;
        self.path = path;
        self.undo.clear();
        self.redo.clear();
        (self.order, self.row, self.voice, self.col, self.anchor, self.ins) = (0, 0, 0, 0, None, 1);
        self.muted.clear();
        self.popup = None;
        self.renaming = None;
        self.sync_song();
        self.changed(cx);
    }

    /// A song of the project: one of this editor, or a GoatTracker song to import.
    pub fn open(&mut self, path: String, bytes: &[u8], cx: &mut Context<Self>) {
        self.message = None;
        match songimport::song_kind(bytes) {
            Some(SongKind::Song) => match Song::decode(bytes) {
                Ok(song) => self.set_song(song, Some(path), true, cx),
                Err(e) => self.message = Some((format!("{path}: {e}"), true)),
            },
            _ => self.import(path, bytes.to_vec(), true, cx),
        }
        cx.notify();
    }

    /// A song of another program - a GoatTracker song, a SID tune, a MIDI file - or of this editor
    /// from outside the project: a new, unsaved song; the file it comes from stays as it is.
    pub fn import(&mut self, name: String, bytes: Vec<u8>, in_project: bool, cx: &mut Context<Self>) {
        if self.is_dirty() {
            self.message = Some(("Save the song before another one is opened (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        let folder = in_project.then(|| name.rfind('/').map_or(String::new(), |i| name[..=i].to_owned()));
        let base = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_owned();
        let stem = base.rsplit_once('.').map_or(base.as_str(), |(s, _)| s).to_owned();
        let Some(kind) = songimport::song_kind(&bytes) else {
            self.message = Some((format!("{name}: not a song of this editor, a GoatTracker song, a SID tune or a MIDI file"), true));
            cx.notify();
            return;
        };
        let mut pending = PendingImport {
            bytes,
            name: name.clone(),
            folder,
            kind,
            info: String::new(),
            rows_per_quarter: 4,
            drums: None,
            all: true,
            subtune: 1,
            subtunes: 1,
            seconds: 90,
        };
        match kind {
            SongKind::Song => match Song::decode(&pending.bytes) {
                Ok(song) => {
                    let path = self.free_path(&stem);
                    self.set_song(song, Some(path), false, cx);
                }
                Err(e) => self.message = Some((format!("{name}: {e}"), true)),
            },
            SongKind::Goat => self.finish_import(pending, cx),
            SongKind::Midi => match songimport::midi_info(&pending.bytes) {
                Ok(info) => {
                    let parts = info.parts.iter().map(|p| format!("{} ({})", p.name, p.notes)).collect::<Vec<_>>().join(", ");
                    let title = if info.title.is_empty() { base } else { info.title };
                    pending.info = format!("{title}: {parts}, {} bpm.", info.bpm);
                    pending.drums = info.parts.iter().any(|p| p.drums).then_some(true);
                    self.pending = Some(pending);
                }
                Err(e) => self.message = Some((format!("{name}: {e}"), true)),
            },
            SongKind::Sid => match songimport::sid_info(&pending.bytes) {
                Ok(info) => {
                    let title = if info.title.is_empty() { base } else { info.title };
                    let by = if info.author.is_empty() { String::new() } else { format!(" by {}", info.author) };
                    pending.info = format!("{title}{by}: a SID tune is a program - it runs for some seconds and what it plays becomes notes.");
                    pending.subtunes = info.songs.max(1);
                    pending.subtune = info.start.clamp(1, pending.subtunes);
                    self.pending = Some(pending);
                }
                Err(e) => self.message = Some((format!("{name}: {e}"), true)),
            },
        }
        cx.notify();
    }

    fn finish_import(&mut self, p: PendingImport, cx: &mut Context<Self>) {
        self.pending = None;
        let result = match p.kind {
            SongKind::Goat => songimport::import_goat(&p.bytes),
            SongKind::Midi => songimport::import_midi(
                &p.bytes,
                &MidiOptions { rows_per_quarter: p.rows_per_quarter, drums: p.drums.unwrap_or(false), all: p.all },
            ),
            SongKind::Sid => songimport::import_sid(&p.bytes, Some(p.subtune), p.seconds),
            SongKind::Song => return,
        };
        match result {
            Ok(r) => {
                let base = p.name.rsplit(['/', '\\']).next().unwrap_or(&p.name);
                let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
                let stem = if p.kind == SongKind::Goat { format!("{stem}-import") } else { stem.to_owned() };
                let folder = p.folder.unwrap_or_else(|| format!("{MUSIC}/"));
                let path = self.free_path_in(&folder, &stem);
                self.set_song(r.song, Some(path.clone()), false, cx);
                let notes = if r.notes.is_empty() { String::new() } else { format!(": {}", r.notes.join(" · ")) };
                self.message = Some((format!("{} imported as {path} (not saved yet){notes}", p.name), false));
            }
            Err(e) => self.message = Some((format!("{}: {e}", p.name), true)),
        }
        cx.notify();
    }

    fn import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import a song (.sng, .sid, .mid)".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let read = std::fs::read(&path);
                this.update(cx, |this, cx| match read {
                    Ok(bytes) => this.import(path.display().to_string(), bytes, false, cx),
                    Err(e) => {
                        this.message = Some((format!("{}: {e}", path.display()), true));
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn new_song(&mut self, cx: &mut Context<Self>) {
        if self.is_dirty() {
            self.message = Some(("Save the song before a new one is made (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        let path = self.free_path("song");
        self.set_song(Song::default(), Some(path), false, cx);
        self.message = None;
    }

    // --- editing the pattern -------------------------------------------------------------------------

    fn pattern_index(&self) -> usize {
        self.song.orders[self.order]
    }

    fn at(&self, row: usize, voice: usize) -> usize {
        cell_offset(row, voice, self.song.voices)
    }

    /// The game does not play every voice: the editor can play all or the game's.
    fn choosing(&self) -> bool {
        self.song.voices > VOICES || self.song.game.len() < VOICES
    }

    fn selection(&self) -> Option<Selection> {
        self.anchor.map(|(row, voice)| Selection {
            r0: row.min(self.row),
            r1: row.max(self.row),
            v0: voice.min(self.voice),
            v1: voice.max(self.voice),
        })
    }

    /// The selection, else the cell at the cursor.
    fn selection_or_cell(&self) -> Selection {
        self.selection().unwrap_or(Selection { r0: self.row, r1: self.row, v0: self.voice, v1: self.voice })
    }

    /// An edit of the pattern of the selected order.
    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Vec<u8>, &dyn Fn(usize, usize) -> usize)) {
        self.snapshot();
        let voices = self.song.voices;
        let p = self.pattern_index();
        f(&mut self.song.patterns[p], &|r, v| cell_offset(r, v, voices));
        self.sync_song();
        self.changed(cx);
    }

    /// An edit of the song: the cursor is kept in it.
    fn song_edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Song)) {
        self.snapshot();
        f(&mut self.song);
        let anchor = self.anchor;
        self.clamp_cursor();
        self.anchor = anchor.filter(|&(r, v)| r < self.song.rows && v < self.song.voices);
        self.sync_song();
        self.changed(cx);
    }

    fn move_to(&mut self, row: isize, voice: isize, col: usize, extend: bool, cx: &mut Context<Self>) {
        if extend {
            self.anchor.get_or_insert((self.row, self.voice));
        } else {
            self.anchor = None;
        }
        let rows = self.song.rows as isize;
        self.row = row.rem_euclid(rows) as usize;
        self.voice = voice.clamp(0, self.song.voices as isize - 1) as usize;
        self.col = col;
        self.rows_scroll.scroll_to_item(self.row);
        cx.notify();
    }

    fn move_rows(&mut self, by: isize, cx: &mut Context<Self>) {
        self.move_to(self.row as isize + by, self.voice as isize, self.col, false, cx);
    }

    /// To the next column, over the voices.
    fn move_column(&mut self, dir: isize, extend: bool, cx: &mut Context<Self>) {
        if extend {
            return self.move_to(self.row as isize, self.voice as isize + dir, self.col, true, cx);
        }
        let k = (self.voice as isize * COLS as isize + self.col as isize + dir).clamp(0, (self.song.voices * COLS) as isize - 1) as usize;
        self.move_to(self.row as isize, (k / COLS) as isize, k % COLS, false, cx);
    }

    fn enter_note(&mut self, note: u8, cx: &mut Context<Self>) {
        let (row, voice, ins) = (self.row, self.voice, self.ins as u8);
        self.edit(cx, |p, at| {
            let o = at(row, voice);
            p[o] = note;
            p[o + 1] = if note == NOTE_OFF { 0 } else { ins };
        });
        self.move_rows(self.step as isize, cx);
    }

    fn enter_instrument(&mut self, n: usize, cx: &mut Context<Self>) {
        if n > self.song.instruments.len() {
            self.message = Some((format!("There is no instrument {}.", hex1(n)), true));
            return cx.notify();
        }
        let (row, voice) = (self.row, self.voice);
        self.edit(cx, |p, at| p[at(row, voice) + 1] = n as u8);
        if n > 0 {
            self.ins = n;
        }
        self.move_rows(self.step as isize, cx);
    }

    /// The command of the cell; the cursor goes on to its value.
    fn enter_command(&mut self, n: u8, cx: &mut Context<Self>) {
        if n != 0 && !COMMANDS.iter().any(|c| c.0 == n) {
            self.message = Some((format!("There is no command {}.", hex1(n as usize)), true));
            return cx.notify();
        }
        let (row, voice) = (self.row, self.voice);
        self.edit(cx, |p, at| {
            let o = at(row, voice);
            p[o + 2] = n;
            if n == 0 {
                p[o + 3] = 0;
            }
        });
        if n != 0 {
            self.move_to(self.row as isize, self.voice as isize, 3, false, cx);
        } else {
            self.move_rows(self.step as isize, cx);
        }
    }

    /// A digit of the value of the command: the digits move to the left.
    fn enter_value(&mut self, digit: u8, cx: &mut Context<Self>) {
        let (row, voice) = (self.row, self.voice);
        self.edit(cx, |p, at| {
            let o = at(row, voice) + 3;
            p[o] = (p[o] << 4) | digit;
        });
    }

    /// A selection: its cells. Without one the part of the cell the cursor is in: the note with its
    /// instrument, the instrument, the command with its value.
    fn clear_cells(&mut self, cx: &mut Context<Self>) {
        let s = self.selection_or_cell();
        let part: &[usize] = if self.anchor.is_some() { &[0, 1, 2, 3] } else { [&[0, 1][..], &[1], &[2, 3], &[2, 3]][self.col] };
        let part = part.to_vec();
        self.edit(cx, |p, at| {
            for r in s.r0..=s.r1 {
                for v in s.v0..=s.v1 {
                    for &k in &part {
                        p[at(r, v) + k] = 0;
                    }
                }
            }
        });
    }

    /// The cells of the voice from the cursor on move a row down (dir 1) or up (-1).
    fn shift_track(&mut self, dir: isize, cx: &mut Context<Self>) {
        let (row, voice, rows) = (self.row, self.voice, self.song.rows);
        self.edit(cx, |p, at| {
            let mut cells: Vec<[u8; CELL]> = (row..rows).map(|r| p[at(r, voice)..at(r, voice) + CELL].try_into().unwrap()).collect();
            if dir > 0 {
                cells.insert(0, EMPTY);
                cells.pop();
            } else {
                cells.remove(0);
                cells.push(EMPTY);
            }
            for (k, c) in cells.iter().enumerate() {
                let o = at(row + k, voice);
                p[o..o + CELL].copy_from_slice(c);
            }
        });
    }

    /// The selected cells; without a selection the cell at the cursor.
    fn copy(&mut self, cut: bool, cx: &mut Context<Self>) {
        let s = self.selection_or_cell();
        let (rows, voices) = (s.r1 - s.r0 + 1, s.v1 - s.v0 + 1);
        let p = &self.song.patterns[self.pattern_index()];
        let mut data = Vec::new();
        for r in s.r0..=s.r1 {
            for v in s.v0..=s.v1 {
                let o = self.at(r, v);
                data.extend_from_slice(&p[o..o + CELL]);
            }
        }
        let first = note_name(p[self.at(s.r0, s.v0)]);
        self.clip = Some(Clip { rows, voices, data });
        if cut {
            self.edit(cx, |q, at| {
                for r in s.r0..=s.r1 {
                    for v in s.v0..=s.v1 {
                        q[at(r, v)..at(r, v) + CELL].copy_from_slice(&EMPTY);
                    }
                }
            });
        }
        let what = if cut { "Cut" } else { "Copied" };
        let text = if self.anchor.is_some() {
            let of = if voices > 1 { format!("of {voices} voices") } else { format!("of voice {}", s.v0 + 1) };
            format!("{what} {rows} row{} {of}", if rows > 1 { "s" } else { "" })
        } else {
            format!("{what} {first} of row {}", hex2(s.r0))
        };
        self.message = Some((text, false));
        cx.notify();
    }

    /// At the cursor; a copied track goes to the top of the voice.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.clip.clone() else { return };
        let (rows, voices) = (self.song.rows, self.song.voices);
        let row = if c.rows >= rows { 0 } else { self.row };
        let voice = self.voice;
        self.edit(cx, |p, at| {
            for r in 0..c.rows.min(rows.saturating_sub(row)) {
                for v in 0..c.voices.min(voices - voice) {
                    let from = (r * c.voices + v) * CELL;
                    let o = at(row + r, voice + v);
                    p[o..o + CELL].copy_from_slice(&c.data[from..from + CELL]);
                }
            }
        });
    }

    /// The note at the cursor a semitone up or down; a note off and an empty cell stay.
    fn note_by(&mut self, d: i32, cx: &mut Context<Self>) {
        let o = self.at(self.row, self.voice);
        let n = self.song.patterns[self.pattern_index()][o] as i32;
        if n < 1 || n > NOTES as i32 || !(1..=NOTES as i32).contains(&(n + d)) {
            return;
        }
        self.edit(cx, |p, _| p[o] = (n + d) as u8);
    }

    fn transpose_by(&mut self, semitones: i32, cx: &mut Context<Self>) {
        let s = self.selection().unwrap_or(Selection { r0: 0, r1: self.song.rows - 1, v0: self.voice, v1: self.voice });
        let p = self.pattern_index();
        self.snapshot();
        self.song.transpose(p, (s.r0, s.r1), (s.v0, s.v1), semitones);
        self.sync_song();
        self.changed(cx);
    }

    /// The track, then the pattern.
    fn select_all(&mut self, cx: &mut Context<Self>) {
        let track = self.anchor.is_some_and(|(r, v)| r == 0 && v == self.voice) && self.row == self.song.rows - 1;
        self.anchor = Some((0, if track { 0 } else { self.voice }));
        self.row = self.song.rows - 1;
        if track {
            self.voice = self.song.voices - 1;
        }
        cx.notify();
    }

    fn is_muted(&self, voice: usize) -> bool {
        self.muted.get(voice).copied().unwrap_or(false)
    }

    fn set_muted(&mut self, voice: usize, on: bool, cx: &mut Context<Self>) {
        if self.muted.len() <= voice {
            self.muted.resize(voice + 1, false);
        }
        self.muted[voice] = on;
        if let Some(a) = &self.audio {
            a.with(|e| e.mute(voice, on));
        }
        cx.notify();
    }

    fn all_muted(&self) -> bool {
        (0..self.song.voices).all(|v| self.is_muted(v))
    }

    /// Every voice muted or heard: while one is heard they are all muted, else all heard again.
    fn toggle_all_muted(&mut self, cx: &mut Context<Self>) {
        let on = !self.all_muted();
        for v in 0..self.song.voices {
            self.set_muted(v, on, cx);
        }
    }

    /// The voices the game plays: up to three.
    fn toggle_game(&mut self, voice: usize, cx: &mut Context<Self>) {
        let on = self.song.game.contains(&voice);
        if !on && self.song.game.len() >= VOICES {
            self.message = Some(("The game plays three voices: take one of them away first.".into(), true));
            return cx.notify();
        }
        self.song_edit(cx, |s| {
            if on {
                s.game.retain(|&v| v != voice);
            } else {
                s.game.push(voice);
                s.game.sort();
            }
        });
    }

    fn hear(&mut self, all: bool, cx: &mut Context<Self>) {
        self.hear_all = all;
        self.sync_song();
        cx.notify();
    }

    /// The song becomes the three voices of the SID the game plays.
    fn keep_game_voices(&mut self, cx: &mut Context<Self>) {
        let game = self.song.game.clone();
        self.song_edit(cx, |s| *s = s.pick_voices(&game, false));
        for v in 0..self.muted.len() {
            if self.muted[v] {
                self.set_muted(v, false, cx);
            }
        }
        self.muted.clear();
        let kept = game.iter().map(|v| (v + 1).to_string()).collect::<Vec<_>>().join(", ");
        self.message = Some((format!("The song has the voices {} of the game now.", if kept.is_empty() { "none".into() } else { kept }), false));
    }

    // --- the orders ------------------------------------------------------------------------------------

    /// A position after the selected one: the same pattern, a new one or a copy of it.
    fn add_order(&mut self, kind: &'static str, cx: &mut Context<Self>) {
        if self.song.orders.len() >= MAX_ORDERS {
            self.message = Some((format!("A song has {MAX_ORDERS} positions at most."), true));
            return cx.notify();
        }
        if kind != "same" && self.song.patterns.len() >= MAX_PATTERNS {
            self.message = Some((format!("A song has {MAX_PATTERNS} patterns at most."), true));
            return cx.notify();
        }
        let at = self.order;
        let current = self.pattern_index();
        self.song_edit(cx, |s| {
            let mut p = s.orders[at];
            if kind != "same" {
                let q = if kind == "copy" { s.patterns[current].clone() } else { new_pattern(s.rows, s.voices) };
                s.patterns.push(q);
                p = s.patterns.len() - 1;
            }
            s.orders.insert(at + 1, p);
            if let Some(l) = s.loop_to.as_mut()
                && *l > at
            {
                *l += 1;
            }
        });
        self.order = at + 1;
        cx.notify();
    }

    fn remove_order(&mut self, cx: &mut Context<Self>) {
        if self.song.orders.len() < 2 {
            return;
        }
        let at = self.order;
        self.song_edit(cx, |s| {
            s.orders.remove(at);
            if let Some(l) = s.loop_to.as_mut()
                && (*l > at || *l >= s.orders.len())
            {
                *l = l.saturating_sub(1);
            }
        });
    }

    /// The pattern of the selected position; one past the last pattern makes a new one.
    fn step_pattern(&mut self, dir: isize, cx: &mut Context<Self>) {
        let p = self.pattern_index() as isize + dir;
        if p < 0 {
            return;
        }
        let p = p as usize;
        if p >= self.song.patterns.len() && self.song.patterns.len() >= MAX_PATTERNS {
            self.message = Some((format!("A song has {MAX_PATTERNS} patterns at most."), true));
            return cx.notify();
        }
        let at = self.order;
        self.song_edit(cx, |s| {
            if p >= s.patterns.len() {
                s.patterns.push(new_pattern(s.rows, s.voices));
            }
            s.orders[at] = p;
        });
    }

    // --- the instruments ---------------------------------------------------------------------------------

    /// preset: a ready instrument, else a copy of the selected one.
    fn add_instrument(&mut self, preset: Option<Instrument>, cx: &mut Context<Self>) {
        if self.song.instruments.len() >= MAX_INSTRUMENTS {
            self.message = Some((format!("A song has {MAX_INSTRUMENTS} instruments at most."), true));
            return cx.notify();
        }
        let ins = self.ins;
        self.song_edit(cx, |s| {
            let n = s.instruments.len() + 1;
            let i = preset.unwrap_or_else(|| Instrument { name: format!("instrument {n}"), ..s.instruments.get(ins - 1).cloned().unwrap_or_default() });
            s.instruments.push(i);
        });
        self.ins = self.song.instruments.len();
        cx.notify();
    }

    /// The cells that play it lose their instrument, the instruments after it move up.
    fn remove_instrument(&mut self, cx: &mut Context<Self>) {
        if self.song.instruments.len() < 2 {
            return;
        }
        let n = self.ins as u8;
        self.song_edit(cx, |s| {
            s.instruments.remove(n as usize - 1);
            for p in &mut s.patterns {
                for c in p.chunks_mut(CELL) {
                    c[1] = if c[1] == n { 0 } else if c[1] > n { c[1] - 1 } else { c[1] };
                }
            }
        });
    }

    fn instrument(&self) -> &Instrument {
        &self.song.instruments[self.ins - 1]
    }

    // --- popups ------------------------------------------------------------------------------------------

    fn open_popup(&mut self, at: Point<Pixels>, entries: Vec<Entry>, cx: &mut Context<Self>) {
        self.popup = Some(Popup { at, entries });
        cx.notify();
    }

    /// Where a popup of the cursor opens: under its row.
    fn cursor_point(&self) -> Point<Pixels> {
        let b = self.pattern_bounds.get().unwrap_or_default();
        let row = self.rows_scroll.bounds_for_item(self.row).map_or(b.origin.y + px(40.), |r| r.origin.y + r.size.height);
        point(b.origin.x + px(60. + self.voice as f32 * VOICE_WIDTH), row)
    }

    /// The popup of a cell: every note by octave, the note off and the empty cell - or, in the
    /// instrument column, the instruments, in the columns of the command the commands. A note sounds
    /// when it is chosen.
    fn cell_popup(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let o = self.at(self.row, self.voice);
        let cell: [u8; CELL] = self.song.patterns[self.pattern_index()][o..o + CELL].try_into().unwrap();
        let place = format!("row {}, voice {}", hex2(self.row), self.voice + 1);
        let entries = match self.col {
            2 | 3 => {
                let mut e = vec![Entry::Header(format!("The command of {place}")), item("none", cell[2] == 0, act(|t, cx| t.enter_command(0, cx)))];
                e.extend(COMMANDS.iter().map(|&(v, label, _)| item(format!("{} {label}", hex1(v as usize)), cell[2] == v, act(move |t, cx| t.enter_command(v, cx)))));
                e
            }
            1 => {
                let mut e = vec![Entry::Header(format!("The instrument of {place}")), item("none", cell[1] == 0, act(|t, cx| t.enter_instrument(0, cx)))];
                e.extend(self.song.instruments.iter().enumerate().map(|(k, x)| {
                    item(format!("{} {}", hex1(k + 1), x.name), cell[1] as usize == k + 1, act(move |t, cx| t.enter_instrument(k + 1, cx)))
                }));
                e
            }
            _ => {
                let keys = (0..8 * 12)
                    .map(|k| {
                        let note = k as u8 + 1;
                        (note <= NOTES).then(|| Key {
                            label: note_name(note),
                            checked: cell[0] == note,
                            sharp: NOTE_NAMES[k % 12].ends_with('#'),
                            act: act(move |t, cx| {
                                t.preview(note, cx);
                                t.enter_note(note, cx);
                            }),
                        })
                    })
                    .collect();
                vec![
                    Entry::Header(format!("The note of {place} · instrument {} {}", hex1(self.ins), self.instrument().name)),
                    Entry::Notes { keys },
                    Entry::Divider,
                    item("note off", cell[0] == NOTE_OFF, act(|t, cx| t.enter_note(NOTE_OFF, cx))),
                    item(
                        "empty",
                        false,
                        act(|t, cx| {
                            t.anchor = None;
                            t.col = 0;
                            t.clear_cells(cx);
                        }),
                    ),
                ]
            }
        };
        self.open_popup(at, entries, cx);
    }

    fn add_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let mut e = vec![item("a copy of the selected one", false, act(|t, cx| t.add_instrument(None, cx)))];
        for (group, list) in presets() {
            e.push(Entry::Divider);
            e.push(Entry::Header(group.into()));
            for i in list {
                let wave = WAVES.iter().find(|w| w.0 == i.wave & 0xf0).map_or("", |w| w.1);
                let label = format!("{} · {wave}", i.name);
                e.push(item(label, false, act(move |t, cx| t.add_instrument(Some(i.clone()), cx))));
            }
        }
        self.open_popup(at, e, cx);
    }

    /// A byte of a row of a table, chosen in a popup: what it does, by name.
    fn table_popup(&mut self, at: Point<Pixels>, k: Table, n: usize, side: usize, cx: &mut Context<Self>) {
        let rows = self.song.table(k).clone();
        let [l, r] = rows[n];
        let cur = if side == 1 { r } else { l };
        let set_row = move |a: u8, b: u8| act(move |t, cx| t.song_edit(cx, |s| s.table_mut(k)[n] = [a, b]));
        let set = move |v: u8| if side == 1 { set_row(l, v) } else { set_row(v, r) };
        let title = Entry::Header(format!("{} byte of row {}", if side == 1 { "The right" } else { "The left" }, hex2(n + 1)));
        let head = |t: &str| Entry::Header(t.into());
        let grid = |keys: Vec<Key>| Entry::Grid { width: 52., keys };
        let jump = item("jump", l == 0xff, if l == 0xff { set_row(l, r) } else { set_row(0xff, 0) });
        let unused = || vec![Entry::Item { label: "This byte is not used by the left one.".into(), checked: false, act: None }];
        let frames = |keep: u8| vec![head("frames"), grid(FRAME_PICKS.iter().map(|&f| key(f.to_string(), l == f, set_row(f, keep))).collect())];
        let speeds = || {
            vec![
                head("a frame"),
                grid(SPEED_PICKS.iter().map(|&s| key(if s > 0 { format!("+{s}") } else { s.to_string() }, r as i8 as i32 == s, set(s as u8))).collect()),
            ]
        };
        let mut e = vec![title];
        // the right byte of a jump: the end, or a row of the table
        if side == 1 && l == 0xff && k != Table::Speed {
            e[0] = head("jump to");
            e.push(item("stop", r == 0, set(0)));
            for (j, x) in rows.iter().enumerate() {
                e.push(item(format!("row {} · {}", hex2(j + 1), row_text(k, x[0], x[1])[0]), r as usize == j + 1, set(j as u8 + 1)));
            }
            return self.open_popup(at, e, cx);
        }
        match k {
            Table::Wave if side == 0 => {
                e.push(item("the waveform stays", cur == 0, set(0)));
                e.push(head("waveform"));
                let mut shown = WAVE_CHOICES.to_vec();
                if !(shown.contains(&cur) || cur < 0x10 || cur >= 0xf0) {
                    shown.push(cur);
                }
                e.extend(shown.into_iter().map(|v| item(wave_name(v), cur == v, set(v))));
                e.push(Entry::Divider);
                e.push(jump);
                if (0xf0..0xff).contains(&cur) {
                    e.push(item("GoatTracker command", true, set(cur)));
                }
                e.push(head("wait frames"));
                e.push(grid((1..=15).map(|j| key(j.to_string(), cur == j, set(j))).collect()));
            }
            Table::Wave => {
                if is_wait(l) || (0xf0..0xff).contains(&l) {
                    e.extend(unused());
                } else {
                    e.push(item("the note", cur == 0, set(0)));
                    e.push(item("the pitch stays", cur == 0x80, set(0x80)));
                    e.push(head("semitones"));
                    let mut keys: Vec<Key> = (0..24u8).map(|j| key(format!("-{}", 24 - j), cur == 0x68 + j, set(0x68 + j))).collect();
                    keys.extend((1..=36u8).map(|j| key(format!("+{j}"), cur == j, set(j))));
                    e.push(grid(keys));
                    e.push(head("a fixed note"));
                    let keys = (0..8 * 12)
                        .map(|j| {
                            (j < NOTES as usize && j <= 0x5f).then(|| Key {
                                label: note_name(j as u8 + 1),
                                checked: cur == 0x80 + j as u8,
                                sharp: NOTE_NAMES[j % 12].ends_with('#'),
                                act: set(0x80 + j as u8),
                            })
                        })
                        .collect();
                    e.push(Entry::Notes { keys });
                }
            }
            Table::Pulse if side == 0 => {
                e.push(item("nothing", l == 0, set_row(0, 0)));
                e.push(item("a width", is_set(l), if is_set(l) { set_row(l, r) } else { set_row(0x88, 0) }));
                e.push(jump);
                e.extend(frames(if l != 0 && l < 0x80 { r } else { 0x10 }));
            }
            Table::Pulse => {
                if is_set(l) {
                    let width = |p: u32| {
                        let w = ((p as f64 * 40.95).round() as u32).min(0xfff);
                        (0x80 | (w >> 8) as u8, w as u8)
                    };
                    e.push(head("width"));
                    e.push(grid(
                        (1..=19)
                            .map(|j| {
                                let p = j * 5;
                                let (a, b) = width(p);
                                key(format!("{p} %"), percent(l, r) == p, set_row(a, b))
                            })
                            .collect(),
                    ));
                } else if l != 0 {
                    e.extend(speeds());
                } else {
                    e.extend(unused());
                }
            }
            Table::Filter if side == 0 => {
                e.push(head("type"));
                for &m in &FILTER_TYPES {
                    e.push(item(filter_name(m), is_set(l) && (l >> 4) & 7 == m, set_row(0x80 | (m << 4), if is_set(l) { r } else { 0x80 })));
                }
                e.push(Entry::Divider);
                e.push(item("cutoff", l == 0, if l == 0 { set_row(0, r) } else { set_row(0, 0x40) }));
                e.push(jump);
                e.extend(frames(if l != 0 && l < 0x80 { r } else { 0x02 }));
            }
            Table::Filter => {
                if is_set(l) {
                    e.push(head("resonance"));
                    e.push(grid((0..16u8).map(|j| key(j.to_string(), r >> 4 == j, set(j << 4))).collect()));
                } else if l == 0 {
                    e.push(head("cutoff"));
                    let mut keys: Vec<Key> = (0..16u16).map(|j| (j * 16) as u8).chain([255]).map(|v| key(cutoff_text(v), r == v, set(v))).collect();
                    keys.shrink_to_fit();
                    e.push(Entry::Grid { width: 76., keys });
                } else {
                    e.extend(speeds());
                }
            }
            // the speed table: a vibrato (frames of a half wave, depth) or a speed of a slide or glide
            Table::Speed => {
                if side == 0 {
                    e.push(head("vibrato: frames of half a wave"));
                    e.push(grid((0..=16u8).chain([24, 32, 48, 64, 96, 128]).map(|f| key(f.to_string(), l == f, set(f))).collect()));
                } else {
                    e.push(head("vibrato: depth"));
                    e.push(grid([0u8, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 255].iter().map(|&v| key(v.to_string(), r == v, set(v))).collect()));
                }
                e.push(head("or the speed of a slide or glide"));
                let speed = (l as u16) << 8 | r as u16;
                e.push(grid(SLIDE_PICKS.iter().map(|&v| key(v.to_string(), speed == v, set_row((v >> 8) as u8, v as u8))).collect()));
            }
        }
        self.open_popup(at, e, cx);
    }

    // --- keys ------------------------------------------------------------------------------------------

    /// The key of a note by its place: on a QWERTZ keyboard Y and Z change places.
    fn note_key<'a>(&self, key: &'a str) -> &'a str {
        match (self.qwertz, key) {
            (true, "y") => "z",
            (true, "z") => "y",
            _ => key,
        }
    }

    fn on_key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        let m = ks.modifiers;
        let key = ks.key.as_str();
        let typed = ks.key_char.clone().unwrap_or_default();
        cx.stop_propagation();
        if let Some(name) = self.renaming.as_mut() {
            match key {
                "enter" => self.commit_rename(cx),
                "escape" => self.renaming = None,
                "backspace" => {
                    name.pop();
                }
                _ if !m.control && !m.platform && typed.chars().all(|c| (' '..='~').contains(&c)) && name.len() + typed.len() <= 12 => name.push_str(&typed),
                _ => {}
            }
            return cx.notify();
        }
        if self.popup.is_some() || self.pending.is_some() {
            if key == "escape" {
                self.popup = None;
                self.pending = None;
                cx.notify();
            }
            return;
        }
        let extend = m.shift;
        if m.control || m.platform {
            match key {
                "z" if m.shift => self.restore(true, cx),
                "z" => self.restore(false, cx),
                "y" => self.restore(true, cx),
                "c" => self.copy(false, cx),
                "x" => self.copy(true, cx),
                "v" => self.paste(cx),
                "a" => self.select_all(cx),
                _ => cx.propagate(),
            }
            return;
        }
        // Alt + up / down: the note at the cursor a semitone up or down (a selection: all of it)
        if m.alt {
            let d = match key {
                "up" => 1,
                "down" => -1,
                _ => return cx.propagate(),
            };
            if self.anchor.is_some() { self.transpose_by(d, cx) } else { self.note_by(d, cx) }
            return;
        }
        let (row, voice, col) = (self.row as isize, self.voice as isize, self.col);
        let page = 16.min(self.song.rows / 2) as isize;
        let last = self.song.rows as isize - 1;
        match key {
            "up" => return self.move_to(row - 1, voice, col, extend, cx),
            "down" => return self.move_to(row + 1, voice, col, extend, cx),
            "left" => return self.move_column(-1, extend, cx),
            "right" => return self.move_column(1, extend, cx),
            "pageup" => return self.move_to((row - page).max(0), voice, col, extend, cx),
            "pagedown" => return self.move_to((row + page).min(last), voice, col, extend, cx),
            "home" => return self.move_to(0, voice, col, extend, cx),
            "end" => return self.move_to(last, voice, col, extend, cx),
            "tab" => {
                let n = self.song.voices as isize;
                return self.move_to(row, (voice + if m.shift { n - 1 } else { 1 }) % n, 0, false, cx);
            }
            "space" => {
                return if self.playing.is_some() { self.stop(cx) } else { self.play(if m.shift { PlayMode::Pattern } else { PlayMode::Song }, cx) };
            }
            "escape" => {
                self.anchor = None;
                return cx.notify();
            }
            "enter" => {
                let at = self.cursor_point();
                return self.cell_popup(at, cx);
            }
            "delete" => {
                if m.shift {
                    self.shift_track(-1, cx);
                } else {
                    self.clear_cells(cx);
                    if self.anchor.is_none() {
                        self.move_rows(self.step as isize, cx);
                    }
                }
                return;
            }
            "backspace" => {
                self.anchor = None;
                self.clear_cells(cx);
                return self.move_rows(-1, cx);
            }
            "insert" => return self.shift_track(1, cx),
            _ => {}
        }
        if typed == "+" || typed == "-" {
            self.octave = if typed == "+" { (self.octave + 1).min(7) } else { self.octave.saturating_sub(1) };
            return cx.notify();
        }
        if self.col >= 1 {
            if let Some(n) = (key.len() == 1).then(|| HEX.iter().position(|&h| h == key.as_bytes()[0].to_ascii_uppercase())).flatten() {
                match self.col {
                    1 => self.enter_instrument(n, cx),
                    2 => self.enter_command(n as u8, cx),
                    _ => self.enter_value(n as u8, cx),
                }
                return;
            }
            return cx.propagate();
        }
        if key == "1" {
            if !e.is_held {
                self.enter_note(NOTE_OFF, cx);
            }
            return;
        }
        let place = self.note_key(key);
        let Some(&(_, semitone)) = NOTE_KEYS.iter().find(|(k, _)| *k == place) else { return cx.propagate() };
        let note = self.octave * 12 + semitone + 1;
        if e.is_held || note > NOTES {
            return;
        }
        self.jam(key, note);
        self.enter_note(note, cx);
        window.focus(&self.focus, cx);
    }

    fn on_key_up(&mut self, e: &KeyUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.release(&e.keystroke.key);
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.renaming.take() else { return };
        let name = name.trim().to_owned();
        let name = if name.is_empty() { "instrument".to_owned() } else { name };
        let ix = self.ins - 1;
        if self.song.instruments[ix].name != name {
            self.song_edit(cx, |s| s.instruments[ix].name = name);
        }
    }

    // --- sliders -----------------------------------------------------------------------------------------

    fn on_drag_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            if self.drag.take().is_some() {
                cx.notify();
            }
            return;
        }
        self.drag_to(e.position, cx);
    }

    fn drag_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(d) = &self.drag else { return };
        let Some(b) = d.bounds.get() else { return };
        let f = ((at.x - b.origin.x) / b.size.width).clamp(0., 1.);
        let v = d.min + (f * (d.max - d.min) as f32).round() as i32;
        let set = d.set.clone();
        set(self, v, cx);
        cx.notify();
    }

    /// A slider: its label and value, the bar; a drag is one undo step when it changes the song.
    fn slider(&mut self, id: &str, label: &str, value: i32, (min, max): (i32, i32), text: String, undoable: bool, set: SetFn, theme: Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let id: SharedString = id.to_owned().into();
        let bounds = self.sliders.entry(id.clone()).or_default().clone();
        let f = if max > min { (value - min).clamp(0, max - min) as f32 / (max - min) as f32 } else { 0. };
        let cell = bounds.clone();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(130.))
            .gap_0p5()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .child(div().text_color(theme.on_surface_variant).child(label.to_owned()))
                    .child(text),
            )
            .child(
                div()
                    .id(id)
                    .relative()
                    .h(px(14.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .h(px(4.))
                            .rounded_sm()
                            .bg(theme.container_highest)
                            .child(div().h_full().rounded_sm().bg(theme.caret).w(gpui::relative(f))),
                    )
                    .child(canvas(move |b, _, _| cell.set(Some(b)), |_, _, _, _| {}).absolute().size_full())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if undoable {
                                this.snapshot();
                            }
                            this.drag = Some(Drag { bounds: bounds.clone(), min, max, set: set.clone() });
                            this.drag_to(e.position, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    /// A slider of a value of the selected instrument.
    fn ins_slider(
        &mut self,
        label: &str,
        (min, max): (i32, i32),
        get: fn(&Instrument) -> i32,
        put: fn(&mut Instrument, i32),
        text: impl Fn(i32) -> String,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let value = get(self.instrument());
        let set: SetFn = Rc::new(move |this: &mut ComposerView, v, cx: &mut Context<ComposerView>| {
            let ix = this.ins - 1;
            if get(&this.song.instruments[ix]) != v {
                put(&mut this.song.instruments[ix], v);
                this.sync_song();
                this.changed(cx);
            }
        });
        self.slider(&format!("ins-{label}"), label, value, (min, max), text(value), true, set, theme, cx)
    }
}

/// A keyboard with Y and Z swapped (German, Austrian, Swiss, Czech, Slovak, Hungarian, Croatian,
/// Slovenian ...): by the layout's id on Windows, its name elsewhere.
fn is_qwertz(id: &str, name: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    const IDS: [&str; 10] = ["0407", "0807", "0c07", "1007", "1407", "100c", "0405", "041b", "040e", "041a"];
    let windows = id.len() == 8 && IDS.iter().any(|x| id.ends_with(x)) && !id.ends_with("10405");
    windows || ["german", "deutsch", "swiss", "czech", "slovak", "hungarian", "croatian", "slovenian", "qwertz"].iter().any(|n| name.contains(n) || id.contains(n))
}

impl Focusable for ComposerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn chip(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, active: bool, theme: Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px_2()
        .py_0p5()
        .rounded_md()
        .border_1()
        .border_color(theme.outline)
        .cursor_pointer()
        .text_xs()
        .when(active, |el| el.bg(theme.container_highest))
        .hover(|el| el.bg(theme.container))
        .child(label.into())
}

fn section_title(text: impl Into<SharedString>, theme: Theme) -> gpui::Div {
    div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.on_surface_variant).child(text.into())
}

fn hint(text: impl Into<SharedString>, theme: Theme) -> gpui::Div {
    div().text_xs().text_color(theme.on_surface_variant).child(text.into())
}

impl ComposerView {
    fn render_bar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let dirty = self.is_dirty();
        let files = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(theme.on_surface_variant).pr_1().child("SONGS"))
            .children(self.files.iter().cloned().map(|path| {
                let active = self.path.as_deref() == Some(path.as_str());
                let label = if active && dirty { format!("{path} ●") } else { path.clone() };
                chip(SharedString::from(format!("sng-{path}")), label, active, theme).on_click(cx.listener(move |_, _, _, cx| cx.emit(ComposerEvent::OpenFile(path.clone()))))
            }))
            .when(self.path.as_ref().is_some_and(|p| !self.files.contains(p)), |el| {
                el.child(chip("sng-new-file", format!("{} ●", self.path.as_deref().unwrap_or_default()), true, theme))
            })
            .when(self.files.is_empty() && self.path.is_none(), |el| el.child(hint("No songs (.sng) in the project yet: a new song is saved into audio/music.", theme)));
        let playing = self.playing;
        let muted = self.all_muted();
        let tools = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(chip("sng-new", "New song", false, theme).on_click(cx.listener(|this, _, _, cx| this.new_song(cx))))
            .child(chip("sng-import", "Import…", false, theme).on_click(cx.listener(|this, _, window, cx| this.import_dialog(window, cx))))
            .child(div().w(px(10.)))
            .child(chip("sng-undo", "Undo", false, theme).when(self.undo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.restore(false, cx))))
            .child(chip("sng-redo", "Redo", false, theme).when(self.redo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.restore(true, cx))))
            .child(div().w(px(10.)))
            .child(chip("sng-play", "▶ Song", playing == Some(PlayMode::Song), theme).on_click(cx.listener(|this, _, _, cx| this.play(PlayMode::Song, cx))))
            .child(chip("sng-loop", "↻ Pattern", playing == Some(PlayMode::Pattern), theme).on_click(cx.listener(|this, _, _, cx| this.play(PlayMode::Pattern, cx))))
            .child(chip("sng-stop", "■ Stop", false, theme).on_click(cx.listener(|this, _, _, cx| this.stop(cx))))
            .child(chip("sng-mute", if muted { "All muted" } else { "Mute all" }, muted, theme).on_click(cx.listener(|this, _, _, cx| this.toggle_all_muted(cx))));
        div().flex().flex_col().gap_2().child(files).child(tools)
    }

    fn render_orders(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let s = &self.song;
        let list = div().id("trk-orders").flex_1().min_h_0().overflow_y_scroll().children(s.orders.iter().enumerate().map(|(k, &p)| {
            let selected = k == self.order;
            let playing = self.pos.is_some_and(|(o, _)| o == k);
            div()
                .id(("trk-order", k))
                .flex()
                .gap_2()
                .px_2()
                .rounded_sm()
                .cursor_pointer()
                .font_family(mono_font())
                .text_sm()
                .when(selected, |el| el.bg(theme.container_highest))
                .when(playing, |el| el.text_color(theme.caret))
                .hover(|el| el.bg(theme.container))
                .child(div().text_color(theme.on_surface_variant).child(hex2(k)))
                .child(hex2(p))
                .when(s.loop_to == Some(k), |el| el.child(div().text_color(theme.on_surface_variant).child("↺")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.order = k;
                    this.anchor = None;
                    cx.notify();
                }))
        }));
        let tool = |id: &'static str, label: &'static str, tip: &'static str| {
            chip(id, label, false, theme).tooltip(move |_, cx| cx.new(|_| Tip(tip.into())).into())
        };
        let loop_here = self.order;
        div()
            .w(px(116.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .child(section_title("ORDERS", theme))
            .child(list)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(tool("ord-prev", "−", "the pattern before").on_click(cx.listener(|this, _, _, cx| this.step_pattern(-1, cx))))
                    .child(tool("ord-next", "+", "the next pattern (a new one past the last)").on_click(cx.listener(|this, _, _, cx| this.step_pattern(1, cx))))
                    .child(tool("ord-same", "dup", "a position after this one with the same pattern").on_click(cx.listener(|this, _, _, cx| this.add_order("same", cx))))
                    .child(tool("ord-new", "new", "a position after this one with a new pattern").on_click(cx.listener(|this, _, _, cx| this.add_order("new", cx))))
                    .child(tool("ord-copy", "copy", "a position after this one with a copy of its pattern").on_click(cx.listener(|this, _, _, cx| this.add_order("copy", cx))))
                    .child(tool("ord-loop", "↺", "the song loops to this position (again: it ends)").on_click(cx.listener(move |this, _, _, cx| {
                        this.song_edit(cx, |s| s.loop_to = if s.loop_to == Some(loop_here) { None } else { Some(loop_here) })
                    })))
                    .child(tool("ord-del", "del", "remove this position").on_click(cx.listener(|this, _, _, cx| this.remove_order(cx)))),
            )
    }

    fn render_pattern(&self, theme: Theme, focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let cx = &*cx;
        let s = &self.song;
        let p = &s.patterns[self.pattern_index()];
        let sel = self.selection();
        let play_row = self.pos.filter(|&(o, _)| o == self.order).map(|(_, r)| r);
        let voices = s.voices;
        let head = div()
            .flex()
            .font_family(mono_font())
            .text_sm()
            .border_b_1()
            .border_color(theme.outline_variant)
            .child(div().w(px(28.)).text_color(theme.on_surface_variant).child(hex2(self.pattern_index())))
            .children((0..voices).map(|v| {
                let muted = self.is_muted(v);
                let game = s.game.iter().position(|&g| g == v);
                div()
                    .w(px(VOICE_WIDTH))
                    .flex()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .when(game.is_some() && self.choosing(), |el| el.bg(theme.container))
                    .child(
                        div()
                            .id(("trk-mute", v))
                            .cursor_pointer()
                            .px_1()
                            .rounded_sm()
                            .hover(|el| el.bg(theme.container_highest))
                            .when(muted, |el| el.text_color(theme.on_surface_variant))
                            .child(format!("{} voice {}", if muted { "🔇" } else { "🔊" }, v + 1))
                            .on_click(cx.listener(move |this, _, _, cx| this.set_muted(v, !this.is_muted(v), cx))),
                    )
                    .child(
                        div()
                            .id(("trk-game", v))
                            .cursor_pointer()
                            .px_1()
                            .rounded_sm()
                            .text_xs()
                            .when(game.is_some(), |el| el.bg(theme.tab_active).text_color(theme.tab_active_text))
                            .when(game.is_none(), |el| el.text_color(theme.on_surface_variant))
                            .hover(|el| el.bg(theme.container_highest))
                            .child(game.map_or("game".into(), |g| format!("game {}", g + 1)))
                            .tooltip(|_, cx| cx.new(|_| Tip("the game plays this voice (up to three)".into())).into())
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_game(v, cx))),
                    )
            }));
        let rows = (0..s.rows).map(|r| {
            let line = div()
                .flex()
                .when(r % 16 == 0, |el| el.border_t_1().border_color(theme.outline))
                .when(r % 4 == 0 && r % 16 != 0, |el| el.bg(theme.container_low))
                .when(play_row == Some(r), |el| el.bg(theme.caret_line))
                .child(div().w(px(28.)).text_color(theme.on_surface_variant).child(hex2(r)));
            line.children((0..voices).map(move |v| {
                let o = cell_offset(r, v, voices);
                let [note, ins, cmd, value] = [p[o], p[o + 1], p[o + 2], p[o + 3]];
                let selected = sel.is_some_and(|s| (s.r0..=s.r1).contains(&r) && (s.v0..=s.v1).contains(&v));
                let texts = [
                    (note_name(note), note == 0, 34.),
                    (if ins != 0 { hex2(ins as usize) } else { "--".into() }, ins == 0, 22.),
                    (if cmd != 0 { hex1(cmd as usize).to_string() } else { "-".into() }, cmd == 0, 14.),
                    (if cmd != 0 { hex2(value as usize) } else { "--".into() }, cmd == 0, 22.),
                ];
                let muted = self.is_muted(v);
                div()
                    .w(px(VOICE_WIDTH))
                    .flex()
                    .gap_1()
                    .px_1()
                    .when(selected, |el| el.bg(theme.selection))
                    .when(muted, |el| el.opacity(0.5))
                    .children(texts.into_iter().enumerate().map(move |(col, (text, empty, w))| {
                        let cursor = r == self.row && v == self.voice && col == self.col;
                        div()
                            .id(("trk-cell", (r * 64 + v) * COLS + col))
                            .w(px(w))
                            .when(empty, |el| el.text_color(theme.on_surface_variant))
                            .when(cursor, |el| {
                                el.bg(if focused { theme.caret } else { theme.outline }).text_color(theme.tab_active_text).rounded_sm()
                            })
                            .child(text)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                    window.focus(&this.focus, cx);
                                    this.popup = None;
                                    this.move_to(r as isize, v as isize, col, e.modifiers.shift, cx);
                                    if e.click_count == 2 {
                                        this.cell_popup(e.position, cx);
                                    }
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                    window.focus(&this.focus, cx);
                                    this.move_to(r as isize, v as isize, col, false, cx);
                                    this.cell_popup(e.position, cx);
                                }),
                            )
                    }))
            }))
        });
        let bounds = self.pattern_bounds.clone();
        div()
            .id("trk-x")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll()
            .relative()
            .child(canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {}).absolute().size_full())
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_col()
                    .font_family(mono_font())
                    .text_sm()
                    .child(head)
                    .child(div().id("trk-rows").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.rows_scroll).children(rows)),
            )
    }

    fn render_side(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let s = &self.song;
        let tempo = s.tempo as i32;
        let set_tempo: SetFn = Rc::new(|this: &mut ComposerView, v, cx: &mut Context<ComposerView>| {
            if this.song.tempo as i32 != v {
                this.song.tempo = v as u8;
                this.sync_song();
                this.changed(cx);
            }
        });
        let tempo = self.slider("song-tempo", "tempo", tempo, (1, 24), format!("{tempo} frames a row · {} bpm", (750. / tempo as f64).round()), true, set_tempo, theme, cx);
        let octave = self.octave as i32;
        let set_octave: SetFn = Rc::new(|this: &mut ComposerView, v, _: &mut Context<ComposerView>| this.octave = v as u8);
        let octave = self.slider("song-octave", "octave", octave, (0, 7), octave.to_string(), false, set_octave, theme, cx);
        let step = self.step as i32;
        let set_step: SetFn = Rc::new(|this: &mut ComposerView, v, _: &mut Context<ComposerView>| this.step = v as usize);
        let step = self.slider("song-step", "step", step, (0, 8), step.to_string(), false, set_step, theme, cx);
        let s = &self.song;
        let mut row_choices: Vec<usize> = ROW_CHOICES.to_vec();
        if !row_choices.contains(&s.rows) {
            row_choices.push(s.rows);
            row_choices.sort();
        }
        let game_list = if s.game.is_empty() { "none".into() } else { s.game.iter().map(|v| (v + 1).to_string()).collect::<Vec<_>>().join(", ") };
        let song = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("SONG", theme))
            .child(tempo)
            .child(
                div().flex().gap_1().items_center().child(hint("rows", theme)).children(row_choices.into_iter().map(|r| {
                    chip(("song-rows", r), r.to_string(), s.rows == r, theme).on_click(cx.listener(move |this, _, _, cx| this.song_edit(cx, |s| s.set_rows(r))))
                })),
            )
            .child(div().flex().gap_3().child(octave).child(step))
            .when(self.choosing(), |el| {
                el.child(hint(format!("A song of {} voices: the game plays {game_list}.", s.voices), theme)).child(
                    div()
                        .flex()
                        .gap_1()
                        .child(chip("hear-all", "hear all", self.hear_all, theme).on_click(cx.listener(|this, _, _, cx| this.hear(true, cx))))
                        .child(chip("hear-game", "hear the game's", !self.hear_all, theme).on_click(cx.listener(|this, _, _, cx| this.hear(false, cx))))
                        .when(s.voices > VOICES, |el| {
                            el.child(chip("keep-game", "keep only the game's", false, theme).on_click(cx.listener(|this, _, _, cx| this.keep_game_voices(cx))))
                        }),
                )
            });

        let instruments = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div().flex().justify_between().items_center().child(section_title("INSTRUMENTS", theme)).child(
                    chip("ins-add", "+ add", false, theme).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.add_menu(e.position, cx)
                    })),
                ),
            )
            .children(s.instruments.iter().enumerate().map(|(k, x)| {
                let wave = WAVES.iter().find(|w| w.0 == x.wave & 0xf0).map_or("", |w| w.1);
                div()
                    .id(("ins", k))
                    .flex()
                    .gap_2()
                    .px_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_sm()
                    .when(k + 1 == self.ins, |el| el.bg(theme.container_highest))
                    .hover(|el| el.bg(theme.container))
                    .child(div().font_family(mono_font()).text_color(theme.on_surface_variant).child(hex1(k + 1).to_string()))
                    .child(div().flex_1().child(x.name.clone()))
                    .child(div().text_xs().text_color(theme.on_surface_variant).child(wave))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ins = k + 1;
                        this.renaming = None;
                        cx.notify();
                    }))
            }))
            .child(hint("The selected instrument plays the notes typed; its sliders are heard at once.", theme));

        let ins_view = self.render_instrument(theme, cx);
        let tables = self.render_tables(theme, cx);
        let selection = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("SELECTION", theme))
            .child(div().flex().flex_wrap().gap_1().children([(-12, "− octave"), (-1, "− 1"), (1, "+ 1"), (12, "+ octave")].into_iter().map(|(n, label)| {
                chip(("tr", (n + 12) as usize), label, false, theme).on_click(cx.listener(move |this, _, _, cx| this.transpose_by(n, cx)))
            })))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(chip("clip-copy", "Copy", false, theme).on_click(cx.listener(|this, _, _, cx| this.copy(false, cx))))
                    .child(chip("clip-cut", "Cut", false, theme).on_click(cx.listener(|this, _, _, cx| this.copy(true, cx))))
                    .child(chip("clip-paste", "Paste", false, theme).when(self.clip.is_none(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.paste(cx)))),
            )
            .child(hint(
                "Notes: Z S X D C V G B H N J M and Q 2 W 3 E R 5 T 6 Y 7 U, an octave higher; 1 is the note off. In the instrument column: 0-F, \
                 in the command columns: 0-F, then the value. + and - change the octave. A double click, the right button or Enter opens the \
                 popup of the cell. Space plays the song (Shift: the pattern), Delete clears (Shift: pulls the track up), Insert pushes it down, \
                 Alt+up/down moves a note a semitone, Shift+arrows select, Ctrl+A the track, again the pattern; Ctrl+C, X, V, Z, Y.",
                theme,
            ));
        div()
            .id("trk-side")
            .w(px(340.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .pl_3()
            .border_l_1()
            .border_color(theme.outline_variant)
            .child(song)
            .child(instruments)
            .child(ins_view)
            .child(tables)
            .child(selection)
    }

    fn render_instrument(&mut self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let i = self.instrument().clone();
        let n = self.ins;
        let semitones = |v: i32| if v != 0 { format!("{v} semitone{}", if v > 1 { "s" } else { "" }) } else { "off".into() };
        let off_or = |v: i32| if v != 0 { v.to_string() } else { "off".into() };
        let a = self.ins_slider("attack", (0, 15), |i| i.a as i32, |i, v| i.a = v as u8, |v| ms(ATTACK_MS[v as usize]), theme, cx);
        let d = self.ins_slider("decay", (0, 15), |i| i.d as i32, |i, v| i.d = v as u8, |v| ms(ATTACK_MS[v as usize] * 3), theme, cx);
        let s = self.ins_slider("sustain", (0, 15), |i| i.s as i32, |i, v| i.s = v as u8, |v| format!("{v} / 15"), theme, cx);
        let r = self.ins_slider("release", (0, 15), |i| i.r as i32, |i, v| i.r = v as u8, |v| ms(ATTACK_MS[v as usize] * 3), theme, cx);
        let pulse = (i.wave & 0x40 != 0).then(|| {
            (
                self.ins_slider("pulse width", (0, 4095), |i| i.pulse as i32, |i, v| i.pulse = v as u16, |v| format!("{} %", (v as f64 / 40.95).round()), theme, cx),
                self.ins_slider("pulse sweep", (0, 255), |i| i.pwm as i32, |i, v| i.pwm = v as u8, |v| if v != 0 { format!("{v} a frame") } else { "off".into() }, theme, cx),
            )
        });
        let vd = self.ins_slider("vibrato depth", (0, 15), |i| i.vib_depth as i32, |i, v| i.vib_depth = v as u8, off_or, theme, cx);
        let vs = self.ins_slider("vibrato speed", (0, 15), |i| i.vib_speed as i32, |i, v| i.vib_speed = v as u8, off_or, theme, cx);
        let a1 = self.ins_slider("arpeggio 1", (0, 15), |i| i.arp1 as i32, |i, v| i.arp1 = v as u8, semitones, theme, cx);
        let a2 = self.ins_slider("arpeggio 2", (0, 15), |i| i.arp2 as i32, |i, v| i.arp2 = v as u8, semitones, theme, cx);
        let filter = (i.filter != 0).then(|| {
            (
                self.ins_slider("cutoff", (0, 255), |i| i.cutoff as i32, |i, v| i.cutoff = v as u8, |v| cutoff_text(v as u8), theme, cx),
                self.ins_slider("resonance", (0, 15), |i| i.res as i32, |i, v| i.res = v as u8, |v| format!("{v} / 15"), theme, cx),
                self.ins_slider("cutoff sweep", (-128, 127), |i| i.sweep as i32, |i, v| i.sweep = v as i8, |v| if v != 0 { format!("{v:+} a frame") } else { "off".into() }, theme, cx),
            )
        });
        // the row of a table an instrument starts at: up to the rows the table has
        let row_of = |v: i32| if v != 0 { format!("row {}", hex2(v as usize)) } else { "off".into() };
        let max = |t: Table, v: u8| self.song.table(t).len().max(1).max(v as usize) as i32;
        let (mw, mp, mf, ms_) = (max(Table::Wave, i.wave_tab), max(Table::Pulse, i.pulse_tab), max(Table::Filter, i.filter_tab), max(Table::Speed, i.vib_tab));
        let wt = self.ins_slider("wave table", (0, mw), |i| i.wave_tab as i32, |i, v| i.wave_tab = v as u8, row_of, theme, cx);
        let pt = self.ins_slider("pulse table", (0, mp), |i| i.pulse_tab as i32, |i, v| i.pulse_tab = v as u8, row_of, theme, cx);
        let ft = self.ins_slider("filter table", (0, mf), |i| i.filter_tab as i32, |i, v| i.filter_tab = v as u8, row_of, theme, cx);
        let vt = self.ins_slider("vibrato (speed table)", (0, ms_), |i| i.vib_tab as i32, |i, v| i.vib_tab = v as u8, row_of, theme, cx);
        let vdl = (i.vib_tab != 0).then(|| {
            self.ins_slider("vibrato delay", (0, 255), |i| i.vib_delay as i32, |i, v| i.vib_delay = v as u8, |v| if v != 0 { format!("{v} frames") } else { "off".into() }, theme, cx)
        });
        let two = |a: gpui::AnyElement, b: gpui::AnyElement| div().flex().gap_3().child(a).child(b);
        let name = match &self.renaming {
            Some(text) => div()
                .id("ins-name")
                .px_2()
                .border_1()
                .border_color(theme.caret)
                .rounded_sm()
                .text_sm()
                .child(format!("{text}▏"))
                .child(hint("Enter keeps it, Escape not", theme)),
            None => div()
                .id("ins-name")
                .px_2()
                .border_1()
                .border_color(theme.outline_variant)
                .rounded_sm()
                .cursor_text()
                .text_sm()
                .child(i.name.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.renaming = Some(this.instrument().name.clone());
                    window.focus(&this.focus, cx);
                    cx.notify();
                })),
        };
        let wave = i.wave;
        let waves = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(WAVES.iter().map(|&(v, label)| {
                chip(("wave", v as usize), label, wave & 0xf0 == v, theme).on_click(cx.listener(move |this, _, _, cx| {
                    let ix = this.ins - 1;
                    this.song_edit(cx, |s| s.instruments[ix].wave = v | (s.instruments[ix].wave & (SYNC | RING)));
                }))
            }))
            // sync and ring modulation with the voice before (the SID's: voice 3 for voice 1); ring
            // modulates the triangle - with another waveform the SID plays it without
            .children([(SYNC, "sync", "the oscillator starts again with the one of the voice before"), (RING, "ring", "the triangle is ring modulated by the voice before")].into_iter().map(
                |(bit, label, tip)| {
                    chip(("wave-bit", bit as usize), label, wave & bit != 0, theme)
                        .tooltip(move |_, cx| cx.new(|_| Tip(tip.into())).into())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let ix = this.ins - 1;
                            this.song_edit(cx, |s| s.instruments[ix].wave ^= bit);
                        }))
                },
            ));
        let filters = div().flex().gap_1().children(FILTERS.iter().map(|&(v, label)| {
            chip(("filter", v as usize), label, i.filter == v, theme).on_click(cx.listener(move |this, _, _, cx| {
                let ix = this.ins - 1;
                this.song_edit(cx, |s| s.instruments[ix].filter = v);
            }))
        }));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div().flex().justify_between().items_center().child(section_title(format!("INSTRUMENT {}", hex1(n)), theme)).when(self.song.instruments.len() > 1, |el| {
                    el.child(chip("ins-del", "delete", false, theme).on_click(cx.listener(|this, _, _, cx| this.remove_instrument(cx))))
                }),
            )
            .child(name)
            .child(waves)
            .child(two(a, d))
            .child(two(s, r))
            .when_some(pulse, |el, (p, w)| el.child(two(p, w)))
            .child(two(vd, vs))
            .child(two(a1, a2))
            .child(hint("filter: the SID has one, the last note of an instrument with a filter sets it", theme))
            .child(filters)
            .when_some(filter, |el, (c, r, sw)| el.child(two(c, r)).child(sw))
            .child(hint("tables: the rows the note starts them at", theme))
            .child(two(wt, pt))
            .child(two(ft, vt))
            .when_some(vdl, |el, v| el.child(v))
    }

    fn render_tables(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let cx = &*cx;
        let k = self.table;
        let rows = self.song.table(k);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("TABLES", theme))
            .child(div().flex().gap_1().children(Table::ALL.into_iter().map(|t| {
                chip(("table", t as usize), t.label(), t == k, theme).on_click(cx.listener(move |this, _, _, cx| {
                    this.table = t;
                    cx.notify();
                }))
            })))
            .children(rows.iter().enumerate().map(|(n, &[l, r])| {
                let texts = row_text(k, l, r);
                div().flex().gap_2().text_sm().child(div().w(px(22.)).font_family(mono_font()).text_color(theme.on_surface_variant).child(hex2(n + 1))).children(
                    texts.into_iter().enumerate().map(move |(side, text)| {
                        div()
                            .id(("table-byte", n * 2 + side))
                            .flex_1()
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .border_1()
                            .border_color(theme.outline_variant)
                            .hover(|el| el.bg(theme.container))
                            .child(if text.is_empty() { "-".into() } else { text })
                            .when(k == Table::Speed, |el| {
                                let speed = (l as u16) << 8 | r as u16;
                                el.tooltip(move |_, cx| cx.new(|_| Tip(format!("as a speed: {speed}").into())).into())
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.table_popup(e.position, k, n, side, cx);
                                }),
                            )
                    }),
                )
            }))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(chip("table-add", "+ row", false, theme).on_click(cx.listener(move |this, _, _, cx| {
                        if this.song.table(k).len() < MAX_TABLE_ROWS {
                            this.song_edit(cx, |s| s.table_mut(k).push([0, 0]));
                        }
                    })))
                    .child(chip("table-del", "− last row", false, theme).when(rows.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(move |this, _, _, cx| {
                        if !this.song.table(k).is_empty() {
                            this.song_edit(cx, |s| {
                                s.table_mut(k).pop();
                            });
                        }
                    }))),
            )
            .child(hint(table_hint(k), theme))
    }

    fn render_popup(&self, theme: Theme, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let popup = self.popup.as_ref()?;
        let run = |a: Act| {
            cx.listener(move |this: &mut ComposerView, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.popup = None;
                a(this, cx);
                cx.notify();
            })
        };
        let mut n = 0usize;
        let mut id = || {
            n += 1;
            n
        };
        let key_el = |k: &Key, w: f32, id: usize| {
            div()
                .id(("pop-key", id))
                .w(px(w))
                .text_center()
                .text_xs()
                .rounded_sm()
                .cursor_pointer()
                .border_1()
                .border_color(if k.checked { theme.caret } else { theme.outline_variant })
                .when(k.sharp, |el| el.bg(theme.container_highest))
                .hover(|el| el.bg(theme.container))
                .child(k.label.clone())
        };
        let mut body = div().flex().flex_col().gap_0p5().text_sm();
        for e in &popup.entries {
            body = match e {
                Entry::Header(t) => body.child(div().pt_1().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.on_surface_variant).child(t.clone())),
                Entry::Divider => body.child(div().my_1().h(px(1.)).bg(theme.outline_variant)),
                Entry::Item { label, checked, act } => {
                    let el = div()
                        .id(("pop-item", id()))
                        .flex()
                        .gap_2()
                        .px_2()
                        .rounded_sm()
                        .child(div().w(px(12.)).child(if *checked { "✓" } else { "" }))
                        .child(label.clone());
                    body.child(match act {
                        Some(a) => el.cursor_pointer().hover(|el| el.bg(theme.container_highest)).on_mouse_down(MouseButton::Left, run(a.clone())),
                        None => el.text_color(theme.on_surface_variant),
                    })
                }
                Entry::Grid { width, keys } => body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_0p5()
                        .max_w(px(8. * (width + 2.)))
                        .children(keys.iter().map(|k| key_el(k, *width, id()).on_mouse_down(MouseButton::Left, run(k.act.clone())))),
                ),
                Entry::Notes { keys } => {
                    let mut grid = div().flex().flex_col().gap_0p5().font_family(mono_font());
                    grid = grid.child(div().flex().gap_0p5().child(div().w(px(16.))).children(NOTE_NAMES.iter().map(|n| {
                        div().w(px(34.)).text_center().text_xs().text_color(theme.on_surface_variant).child(n.replace('-', ""))
                    })));
                    for octave in 0..8 {
                        let row = div().flex().gap_0p5().child(div().w(px(16.)).text_xs().text_color(theme.on_surface_variant).child(octave.to_string()));
                        grid = grid.child(row.children((0..12).map(|j| match &keys[octave * 12 + j] {
                            Some(k) => key_el(k, 34., id()).on_mouse_down(MouseButton::Left, run(k.act.clone())).into_any_element(),
                            None => div().w(px(34.)).into_any_element(),
                        })));
                    }
                    body.child(grid)
                }
            };
        }
        Some(
            deferred(
                anchored().position(popup.at).snap_to_window_with_margin(px(8.)).child(
                    div()
                        .id("popup")
                        .occlude()
                        .max_h(px(560.))
                        .max_w(px(560.))
                        .overflow_y_scroll()
                        .p_2()
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.outline)
                        .rounded_md()
                        .shadow_lg()
                        .child(body)
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.popup = None;
                            cx.notify();
                        })),
                ),
            )
            .with_priority(1),
        )
    }

    /// The choices of an import of a MIDI file or a SID tune.
    fn render_import(&self, theme: Theme, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let p = self.pending.as_ref()?;
        let opt = |id: &'static str, ix: usize, label: String, on: bool, f: fn(&mut PendingImport, usize)| {
            chip((id, ix), label, on, theme).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(p) = this.pending.as_mut() {
                    f(p, ix);
                }
                cx.notify();
            }))
        };
        let mut body = div().flex().flex_col().gap_2().child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Import {}", p.name))).child(hint(p.info.clone(), theme));
        match p.kind {
            SongKind::Midi => {
                body = body
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(opt("mid-all", 1, "every voice (the game plays three)".into(), p.all, |p, _| p.all = true))
                            .child(opt("mid-all", 0, "shared out to three voices".into(), !p.all, |p, _| p.all = false)),
                    )
                    .child(div().flex().gap_1().items_center().child(hint("a quarter note is", theme)).children([2usize, 4, 8].map(|r| {
                        opt("mid-rows", r, format!("{r} rows"), p.rows_per_quarter == r, |p, r| p.rows_per_quarter = r)
                    })))
                    .when_some(p.drums, |el, drums| {
                        el.child(
                            div()
                                .flex()
                                .gap_1()
                                .child(opt("mid-drums", 1, "with the drums".into(), drums, |p, _| p.drums = Some(true)))
                                .child(opt("mid-drums", 0, "without the drums".into(), !drums, |p, _| p.drums = Some(false))),
                        )
                    });
            }
            SongKind::Sid => {
                let tunes = p.subtunes;
                body = body
                    .child(div().flex().flex_wrap().gap_1().items_center().child(hint(format!("subtune (1-{tunes})"), theme)).children((1..=tunes.min(32)).map(|t| {
                        opt("sid-tune", t, t.to_string(), p.subtune == t, |p, t| p.subtune = t)
                    })))
                    .child(div().flex().gap_1().items_center().child(hint("seconds", theme)).children([30usize, 60, 90, 120, 180, 300].map(|s| {
                        opt("sid-secs", s, s.to_string(), p.seconds as usize == s, |p, s| p.seconds = s as u32)
                    })));
            }
            _ => {}
        }
        body = body.child(
            div()
                .flex()
                .gap_2()
                .pt_1()
                .child(chip("imp-go", "Import", true, theme).on_click(cx.listener(|this, _, _, cx| {
                    if let Some(p) = this.pending.take() {
                        this.finish_import(p, cx);
                    }
                })))
                .child(chip("imp-cancel", "Cancel", false, theme).on_click(cx.listener(|this, _, _, cx| {
                    this.pending = None;
                    cx.notify();
                }))),
        );
        Some(
            div()
                .absolute()
                .top(px(90.))
                .left(px(160.))
                .w(px(520.))
                .p_4()
                .bg(theme.surface)
                .border_1()
                .border_color(theme.outline)
                .rounded_md()
                .shadow_lg()
                .occlude()
                .child(body),
        )
    }

    fn status_text(&self) -> String {
        let s = &self.song;
        let mut parts = vec![
            format!("{}{}", self.path.as_deref().unwrap_or("new song"), if self.is_dirty() { " · changed" } else { "" }),
            format!(
                "order {}/{} · pattern {} · row {} · voice {}",
                hex2(self.order),
                s.orders.len(),
                hex2(self.pattern_index()),
                hex2(self.row),
                self.voice + 1
            ),
            format!("octave {} · instrument {} {}", self.octave, hex1(self.ins), self.instrument().name),
        ];
        if let Some(sel) = self.selection() {
            parts.push(format!("{} rows selected", sel.r1 - sel.r0 + 1));
        }
        parts.push(format!("{} bytes", s.encode().len()));
        parts.join(" · ")
    }
}

/// A tooltip: its text.
struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        div().px_2().py_1().text_xs().bg(theme.container_highest).border_1().border_color(theme.outline).rounded_sm().child(self.0.clone())
    }
}

impl Render for ComposerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let focused = self.focus.contains_focused(window, cx);
        let bar = self.render_bar(theme, cx);
        let orders = self.render_orders(theme, cx);
        let pattern = self.render_pattern(theme, focused, cx);
        let side = self.render_side(theme, cx);
        let popup = self.render_popup(theme, cx);
        let import = self.render_import(theme, cx);
        let message = self.message.clone();
        div()
            .id("composer")
            .key_context("Composer")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_mouse_move(cx.listener(Self::on_drag_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .child(bar)
            .when_some(message, |el, (text, error)| el.child(div().text_xs().text_color(if error { theme.error } else { theme.on_surface_variant }).child(text)))
            .child(div().flex_1().min_h_0().flex().gap_3().child(orders).child(pattern).child(side))
            .child(div().text_xs().text_color(theme.on_surface_variant).child(self.status_text()))
            .children(popup)
            .children(import)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboards() {
        assert!(is_qwertz("00000407", "German"));
        assert!(!is_qwertz("00000409", "US"));
        assert!(is_qwertz("com.apple.keylayout.German", "German"));
    }

    #[test]
    fn table_rows_by_name() {
        assert_eq!(row_text(Table::Wave, 0x41, 0x0c), ["pulse".to_string(), "+12".into()]);
        assert_eq!(row_text(Table::Wave, 0xff, 1), ["jump".to_string(), "to row 01".into()]);
        assert_eq!(row_text(Table::Pulse, 0x88, 0), ["width".to_string(), "50 %".into()]);
        assert_eq!(row_text(Table::Filter, 0x90, 0xf0), ["low pass".to_string(), "resonance 15".into()]);
        assert_eq!(row_text(Table::Filter, 4, 0xfe), ["4 frames".to_string(), "-2 a frame".into()]);
    }
}
