//! The sound of the Composer, after nerdsuite2's `sid.js` and `sidplayer.js`.
//!
//! [`Sid`] is a small model of the SID (6581/8580): three voices with their 24 bit oscillators, the
//! four waveforms (combined ones are their AND), the test bit, the ADSR envelopes with the chip's
//! rates and its exponential decay, the filter (a state variable filter: low, band, high pass, the
//! cutoff from about 30 Hz to 12 kHz in even steps as in the 8580, the resonance) and the volume.
//! It is written to like the chip, registers $d400-$d418 as 0-24. Not modelled: ring modulation and
//! sync.
//!
//! [`Player`] plays a song (`song.rs`) on it: [`Player::tick`] is the player's frame, 50 times a
//! second, and everything it does is a write to a SID register - what a player in the game does in
//! its raster interrupt.
//!   a row    lasts `tempo` frames. A note first takes the gate away for a frame, then the next
//!            frame sets the instrument and the gate: the envelope starts again.
//!   a frame  moves the sounding notes: a step of the wave table, the arpeggio, a slide or glide, the
//!            vibrato, the frequency, a step of the pulse table or the pulse sweep, a step of the
//!            filter table or the sweep of the cutoff - in this order, as the engine does it
//!   tables   a note of an instrument with a table runs it from the instrument's row: the wave
//!            table sets the waveform and the pitch, the pulse table the width, the filter table
//!            the filter; a vibrato of the speed table, after its delay
//!     wave   left $00 the waveform stays, $01-$0F wait that many frames, $10-$EF a waveform (the
//!            gate is the note's), $F0-$FE a command of GoatTracker (skipped), $FF jump to the row
//!            of the right byte (0: stop); right $00-$5F that many semitones up, $60-$7F $80 - right
//!            down, $80 the pitch stays, $81-$DF right - $80 semitones above C-0
//!     pulse  left $80-$8F the width (its high nibble the low nibble of left, its low byte the
//!            right one); $01-$7F for that many frames the width moves by the right byte (signed);
//!            $FF jump. The width wraps at 12 bits
//!     filter left $80-$F0 the type (bit 4 low, 5 band, 6 high pass) and the resonance (the high
//!            nibble of right); $00 the cutoff is right; $01-$7F for that many frames the cutoff
//!            moves by the right byte (signed); $FF jump. The cutoff wraps at 8 bits
//!     speed  of a vibrato left the frames of half a wave, right its depth; of a slide or glide left
//!            and right one 16 bit speed
//!   the filter  is one for the three voices: a note of an instrument with a filter sends its voice
//!            through it and sets it, a note of one without takes its voice out
//!   sounds   tracks 3 and 4 are two sounds that may play over the music: the first voice of a
//!            song, once, on the last voice the music holds no note on (else the last no other
//!            sound has), whose music is silent meanwhile.
//!
//! [`SidEngine`] is the players with their chips and a clock in samples - a chip and a player for
//! every three voices of a song of more.

use std::sync::Arc;

use crate::song::{
    CELL, CLOCK, CMD_ARP, CMD_BREAK, CMD_CUTOFF, CMD_DOWN, CMD_DOWN_T, CMD_GLIDE, CMD_GLIDE_T, CMD_PULSE, CMD_TEMPO, CMD_UP,
    CMD_UP_T, CMD_VIB_T, CMD_VOLUME, NOTE_OFF, NOTES, Song, Table, VOICES, cell_offset, freq,
};

/// oscillator steps per sample, averaged: less aliasing
const OVERSAMPLE: usize = 4;
/// filter steps per sample: it stays stable up to its highest cutoff
const FILTER_STEPS: usize = 2;
/// the clock cycles of one envelope step, per rate 0-15 of attack, decay and release
const PERIODS: [f64; 16] = [9., 32., 63., 95., 149., 220., 267., 313., 392., 977., 1954., 3126., 3907., 11720., 19532., 31251.];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Env {
    Attack,
    Decay,
    Release,
}

/// Decay and release slow down as the envelope falls: the exponential curve of the chip.
fn exp_period(env: i32) -> f64 {
    match env {
        94.. => 1.,
        55.. => 2.,
        27.. => 4.,
        15.. => 8.,
        7.. => 16.,
        _ => 30.,
    }
}

#[derive(Clone)]
struct Voice {
    freq: u32,
    pw: i32,
    ctrl: u8,
    ad: u8,
    sr: u8,
    acc: f64,
    lfsr: u32,
    env: i32,
    state: Env,
    env_acc: f64,
}

impl Voice {
    fn new() -> Voice {
        Voice { freq: 0, pw: 0, ctrl: 0, ad: 0, sr: 0, acc: 0., lfsr: 0x7ffff8, env: 0, state: Env::Release, env_acc: 0. }
    }

    /// One step of the envelope counter.
    fn env_step(&mut self) {
        match self.state {
            Env::Attack => {
                self.env += 1;
                if self.env >= 255 {
                    self.env = 255;
                    self.state = Env::Decay;
                }
            }
            Env::Decay => {
                if self.env > (self.sr >> 4) as i32 * 17 {
                    self.env -= 1;
                }
            }
            Env::Release => {
                if self.env > 0 {
                    self.env -= 1;
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Sid {
    sample_rate: f64,
    regs: [u8; 25],
    voices: [Voice; 3],
    volume: u8,
    /// $d417: the voices that go through the filter
    route: u8,
    damp: f64,
    /// $d418: 1 low, 2 band, 4 high pass, 8 voice 3 is not heard
    mode: u8,
    /// the coefficient of the cutoff frequency
    cutoff: f64,
    low: f64,
    band: f64,
}

impl Sid {
    pub fn new(sample_rate: f64) -> Sid {
        let mut sid = Sid {
            sample_rate,
            regs: [0; 25],
            voices: [Voice::new(), Voice::new(), Voice::new()],
            volume: 0,
            route: 0,
            damp: 1.4,
            mode: 0,
            cutoff: 0.,
            low: 0.,
            band: 0.,
        };
        sid.set_cutoff();
        sid
    }

    fn set_cutoff(&mut self) {
        let fc = ((self.regs[22] as u32) << 3) | (self.regs[21] as u32 & 7);
        self.cutoff = 2. * (std::f64::consts::PI * (30. + fc as f64 * 5.8) / (self.sample_rate * FILTER_STEPS as f64)).sin();
    }

    pub fn write(&mut self, reg: usize, value: u8) {
        if reg > 24 {
            return;
        }
        self.regs[reg] = value;
        match reg {
            24 => {
                self.volume = value & 15;
                self.mode = value >> 4;
            }
            23 => {
                self.route = value & 7;
                self.damp = 1.4 - (value >> 4) as f64 * 0.08;
            }
            21 | 22 => self.set_cutoff(),
            _ => {
                let b = reg - reg % 7;
                let r = self.regs;
                let v = &mut self.voices[reg / 7];
                match reg % 7 {
                    0 | 1 => v.freq = r[b] as u32 | (r[b + 1] as u32) << 8,
                    2 | 3 => v.pw = r[b + 2] as i32 | ((r[b + 3] as i32 & 15) << 8),
                    4 => {
                        // the gate changes: the envelope counts anew - the cycles a slow decay had
                        // counted would be a burst of fast attack steps
                        if value & 1 != 0 && v.ctrl & 1 == 0 {
                            v.state = Env::Attack;
                            v.env_acc = 0.;
                        }
                        if value & 1 == 0 && v.ctrl & 1 != 0 {
                            v.state = Env::Release;
                            v.env_acc = 0.;
                        }
                        v.ctrl = value;
                    }
                    5 => v.ad = value,
                    _ => v.sr = value,
                }
            }
        }
    }

    /// The samples of `out`, -1 to 1.
    pub fn render(&mut self, out: &mut [f32]) {
        let cycles = CLOCK / self.sample_rate;
        let sub = cycles / OVERSAMPLE as f64;
        // three voices at their loudest fit
        let scale = self.volume as f64 / 15. / (0x800 as f64 * 255. * 3. * OVERSAMPLE as f64);
        for sample in out.iter_mut() {
            let mut mix = 0.;
            let mut filtered = 0.;
            for n in 0..3 {
                let v = &mut self.voices[n];
                // the envelope
                let rate = match v.state {
                    Env::Attack => v.ad >> 4,
                    Env::Decay => v.ad & 15,
                    Env::Release => v.sr & 15,
                } as usize;
                v.env_acc += cycles;
                loop {
                    let period = PERIODS[rate] * if v.state == Env::Attack { 1. } else { exp_period(v.env) };
                    if v.env_acc < period {
                        break;
                    }
                    v.env_acc -= period;
                    v.env_step();
                    // it rests
                    if v.state != Env::Attack && (v.env == 0 || (v.state == Env::Decay && v.env <= (v.sr >> 4) as i32 * 17)) {
                        v.env_acc = 0.;
                        break;
                    }
                }
                // the oscillator
                let ctrl = v.ctrl;
                if ctrl & 0xf0 == 0 {
                    continue;
                }
                let mut sum = 0i64;
                for _ in 0..OVERSAMPLE {
                    if ctrl & 8 != 0 {
                        v.acc = 0.; // the test bit holds the oscillator
                    } else {
                        let next = v.acc + v.freq as f64 * sub;
                        // the noise register shifts when bit 19 of the oscillator rises
                        let mut clocks = ((next + 524288.) / 1048576.).floor() as i64 - ((v.acc + 524288.) / 1048576.).floor() as i64;
                        while clocks > 0 {
                            let l = v.lfsr;
                            v.lfsr = ((l << 1) | (((l >> 22) ^ (l >> 17)) & 1)) & 0x7fffff;
                            clocks -= 1;
                        }
                        v.acc = if next >= 16777216. { next - 16777216. } else { next };
                    }
                    let acc = v.acc as i32;
                    let mut w: i32 = 0xfff;
                    if ctrl & 0x10 != 0 {
                        w &= ((if acc & 0x800000 != 0 { !acc } else { acc }) >> 11) & 0xfff;
                    }
                    if ctrl & 0x20 != 0 {
                        w &= acc >> 12;
                    }
                    if ctrl & 0x40 != 0 {
                        w &= if (acc >> 12) >= v.pw { 0xfff } else { 0 };
                    }
                    if ctrl & 0x80 != 0 {
                        let l = v.lfsr;
                        let bit = |s: u32, d: u32| (((l >> s) & 1) << d) as i32;
                        w &= bit(20, 11) | bit(18, 10) | bit(14, 9) | bit(11, 8) | bit(9, 7) | bit(5, 6) | bit(2, 5) | bit(0, 4);
                    }
                    sum += (w - 0x800) as i64;
                }
                let s = sum as f64 * v.env as f64;
                if self.route & (1 << n) != 0 {
                    filtered += s;
                } else if n < 2 || self.mode & 8 == 0 {
                    mix += s;
                }
            }
            if self.route != 0 || self.low != 0. || self.band != 0. {
                let mut high = 0.;
                for _ in 0..FILTER_STEPS {
                    self.low += self.cutoff * self.band;
                    high = filtered - self.low - self.damp * self.band;
                    self.band += self.cutoff * high;
                }
                if self.low.abs() < 1e-3 && self.band.abs() < 1e-3 {
                    self.low = 0.; // it rests
                    self.band = 0.;
                }
                if self.mode & 1 != 0 {
                    mix += self.low;
                }
                if self.mode & 2 != 0 {
                    mix += self.band;
                }
                if self.mode & 4 != 0 {
                    mix += high;
                }
            }
            *sample = (mix * scale).clamp(-1., 1.) as f32;
        }
    }
}

const PULSE_MIN: i32 = 0x100;
const PULSE_MAX: i32 = 0xf00;
/// the step of an arpeggio a command ended
const ARP_ENDS: u8 = 0x80;
/// the track of the first sound (4: the second): it plays on the voice of a music track
const SOUND: usize = 3;
/// sounds over the music at a time
const SOUNDS: usize = 2;
/// the registers of each voice
const SID_VOICE: [usize; 3] = [0, 7, 14];
/// rows a table step reads at most: jumps and filter types before a step
const STEPS: usize = 8;

/// A track: `tnote` is the note the pitch is of (the wave table moves it), `wave` the waveform (the
/// table changes it); `wptr`/`wdelay` the wave table, `pptr`/`ptimer`/`pspeed` the pulse table,
/// `vtab` the row of a vibrato of the speed table (`vdelay` frames before it starts, `vcount` frames
/// of its half wave gone, `vdir` its way).
#[derive(Clone, Default)]
struct Track {
    /// the song of the instrument and its index
    ins: Option<(Arc<Song>, usize)>,
    /// the song of the tables
    song: Option<Arc<Song>>,
    note: i32,
    tnote: i32,
    wave: u8,
    trigger: bool,
    gate: bool,
    freq: i32,
    pulse: i32,
    pulse_dir: i32,
    vib: i32,
    vib_step: i32,
    vib_offset: i32,
    arp: u8,
    arp_step: u8,
    slide: u8,
    speed: i32,
    target: i32,
    wptr: u8,
    wdelay: u8,
    pptr: u8,
    ptimer: u8,
    pspeed: i32,
    vtab: u8,
    vdelay: u8,
    vcount: i32,
    vdir: i32,
}

impl Track {
    fn new() -> Track {
        Track { note: -1, tnote: -1, pulse: 0x800, pulse_dir: 1, vdir: 1, ..Default::default() }
    }
}

#[derive(Clone, Default)]
struct Seq {
    song: Option<Arc<Song>>,
    playing: bool,
    order: usize,
    row: usize,
    frame: u8,
    tempo: u8,
    brk: bool,
    loop_pattern: bool,
}

#[derive(Clone, Default)]
struct Filter {
    route: u8,
    res: u8,
    mode: u8,
    cutoff: i32,
    sweep: i32,
    owner: Option<usize>,
    ptr: u8,
    timer: u8,
    speed: i32,
    song: Option<Arc<Song>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SeqId {
    Music,
    Sound(usize),
}

/// A row of a table of a song, numbered from 1.
fn row(song: &Option<Arc<Song>>, table: Table, n: u8) -> Option<[u8; 2]> {
    song.as_ref()?.table_row(table, n)
}

/// A 16 bit speed of the speed table.
fn speed_of(song: &Option<Arc<Song>>, n: u8) -> i32 {
    row(song, Table::Speed, n).map_or(0, |r| ((r[0] as i32) << 8) | r[1] as i32)
}

fn freq_of(note: i32) -> i32 {
    freq(note.max(0) as usize) as i32
}

/// The player of a song on a SID. The numbers are whole numbers a 6502 counts as well, so game and
/// editor sound alike.
#[derive(Clone)]
pub struct Player {
    pub sid: Sid,
    muted: [bool; 3],
    music: Seq,
    effects: [Seq; SOUNDS],
    tracks: Vec<Track>,
    filter: Filter,
    volume: u8,
    /// the music track whose voice each sound plays on
    sound_voices: [usize; SOUNDS],
    /// the rows of the music that started since this was taken
    rows: Vec<(usize, usize)>,
}

impl Player {
    pub fn new(sid: Sid) -> Player {
        let mut p = Player {
            sid,
            muted: [false; 3],
            music: Seq::default(),
            effects: Default::default(),
            tracks: vec![],
            filter: Filter::default(),
            volume: 15,
            sound_voices: [2; SOUNDS],
            rows: vec![],
        };
        p.reset();
        p
    }

    pub fn playing(&self) -> bool {
        self.music.playing
    }

    /// The order and row the music is at.
    pub fn position(&self) -> (usize, usize) {
        (self.music.order, self.music.row)
    }

    /// The rows of the music that started since the last call.
    pub fn take_rows(&mut self) -> Vec<(usize, usize)> {
        std::mem::take(&mut self.rows)
    }

    fn seq(&mut self, id: SeqId) -> &mut Seq {
        match id {
            SeqId::Music => &mut self.music,
            SeqId::Sound(n) => &mut self.effects[n],
        }
    }

    pub fn reset(&mut self) {
        for s in std::iter::once(&mut self.music).chain(self.effects.iter_mut()) {
            s.playing = false;
            s.order = 0;
            s.row = 0;
            s.frame = 0;
            s.brk = false;
        }
        self.tracks = (0..VOICES + SOUNDS).map(|_| Track::new()).collect();
        self.filter = Filter::default();
        self.volume = 15;
        self.sound_voices = [2; SOUNDS];
        for r in 0..24 {
            self.sid.write(r, 0);
        }
        self.sid.write(24, 15);
    }

    /// The track is heard: the music's track is not while a sound plays on its voice.
    fn live(&self, track: usize) -> bool {
        track >= SOUND || !self.taken(track, None)
    }

    /// A sound plays on the voice of music track k (but sound `but`).
    fn taken(&self, k: usize, but: Option<usize>) -> bool {
        self.effects.iter().enumerate().any(|(n, e)| Some(n) != but && e.playing && self.sound_voices[n] == k)
    }

    fn voice(&self, track: usize) -> usize {
        if track >= SOUND { self.sound_voices[track - SOUND] } else { track }
    }

    /// A register of the voice of a track.
    fn write(&mut self, track: usize, reg: usize, value: u8) {
        if self.live(track)
            && let Some(&base) = SID_VOICE.get(self.voice(track))
        {
            self.sid.write(base + reg, value);
        }
    }

    /// The song changes while it plays: the position stays.
    pub fn set_song(&mut self, song: Arc<Song>) {
        let m = &mut self.music;
        m.tempo = song.tempo;
        if m.order >= song.orders.len() {
            m.order = 0;
        }
        if m.row >= song.rows {
            m.row = 0;
        }
        m.song = Some(song);
    }

    /// loop_pattern: the pattern of the order again and again.
    pub fn play(&mut self, order: usize, row: usize, loop_pattern: bool) {
        let Some(song) = self.music.song.clone() else { return };
        self.reset();
        let m = &mut self.music;
        m.order = order.min(song.orders.len() - 1);
        m.row = row.min(song.rows - 1);
        m.tempo = song.tempo;
        m.loop_pattern = loop_pattern;
        m.playing = true;
    }

    /// A song as a sound: its first voice, once, over the music - the second sound while the first
    /// plays; on the last voice the music holds no note on and the other sound does not have, else
    /// the last the other sound does not have.
    pub fn play_sound(&mut self, song: Arc<Song>) {
        let n = if self.effects[0].playing { 1 } else { 0 };
        self.stop_seq(SeqId::Sound(n));
        let voice = (0..3)
            .rev()
            .find(|&k| !self.tracks[k].gate && !self.taken(k, Some(n)))
            .or_else(|| (0..3).rev().find(|&k| !self.taken(k, Some(n))));
        // no voice: it plays on none of the music's
        self.sound_voices[n] = voice.unwrap_or(usize::MAX);
        let e = &mut self.effects[n];
        e.tempo = song.tempo;
        e.song = Some(song);
        e.order = 0;
        e.row = 0;
        e.frame = 0;
        e.brk = false;
        e.playing = true;
    }

    pub fn sound_playing(&self) -> bool {
        self.effects[0].playing
    }

    pub fn stop(&mut self) {
        self.reset();
    }

    /// The notes of a sequencer release; the volume is the music's: it ends with it.
    fn stop_seq(&mut self, id: SeqId) {
        self.seq(id).playing = false;
        let tracks = match id {
            SeqId::Music => vec![0, 1, 2],
            SeqId::Sound(n) => vec![SOUND + n],
        };
        for k in tracks {
            self.note_off(k);
            self.tracks[k] = Track::new();
        }
        if id == SeqId::Music {
            self.volume = 15;
            self.sid.write(24, (self.filter.mode << 4) | 15);
        }
    }

    pub fn mute(&mut self, voice: usize, on: bool) {
        self.muted[voice] = on;
        if on {
            self.note_off(voice);
        }
    }

    /// A note of an instrument (1-15) of `song` (the music's by default). glide: a sounding note does
    /// not start anew, its pitch moves to the note - with the instrument it has.
    pub fn note_on(&mut self, track: usize, note: u8, ins: u8, song: Option<Arc<Song>>, glide: bool) {
        let song = song.or_else(|| self.music.song.clone());
        let v = &mut self.tracks[track];
        if let Some(song) = song
            && ins > 0
            && (ins as usize) <= song.instruments.len()
            && !glide
        {
            v.ins = Some((song.clone(), ins as usize - 1));
            v.song = Some(song);
        }
        let Some((song, ix)) = v.ins.clone() else { return };
        let note = note as i32 - 1;
        if glide && v.note >= 0 {
            v.note = note;
            v.slide = CMD_GLIDE;
            v.target = freq_of(note);
            return;
        }
        // the tables of the instrument start again; with a wave table its arpeggio gives way
        let it = &song.instruments[ix];
        v.note = note;
        v.tnote = note;
        v.trigger = true;
        v.gate = true;
        v.slide = 0;
        v.arp = if it.wave_tab != 0 { 0 } else { (it.arp1 << 4) | it.arp2 };
        v.wave = it.wave;
        v.wptr = it.wave_tab;
        v.wdelay = 0;
        v.pptr = it.pulse_tab;
        v.ptimer = 0;
        v.vtab = it.vib_tab;
        v.vdelay = it.vib_delay;
        v.vdir = 1;
        let wave = v.wave & 0xfe;
        self.write(track, 4, wave); // the gate goes, the next frame sets it
    }

    pub fn note_off(&mut self, track: usize) {
        let v = &self.tracks[track];
        let value = if v.ins.is_some() { v.wave & 0xfe } else { 0 };
        self.write(track, 4, value);
    }

    /// The command of a cell, after its note.
    fn command(&mut self, track: usize, id: SeqId, cmd: u8, value: u8) {
        let song = self.seq(id).song.clone();
        let v = &mut self.tracks[track];
        match cmd {
            CMD_UP | CMD_DOWN => {
                v.slide = cmd;
                v.speed = value as i32;
            }
            CMD_GLIDE => v.speed = value as i32,
            // the speeds of the speed table: 16 bit; a vibrato of its row (0 ends it)
            CMD_UP_T | CMD_DOWN_T => {
                v.slide = if cmd == CMD_UP_T { CMD_UP } else { CMD_DOWN };
                v.speed = speed_of(&song, value);
            }
            CMD_GLIDE_T => v.speed = speed_of(&song, value),
            CMD_VIB_T => {
                v.vtab = value;
                v.vdelay = 0;
                v.vcount = row(&song, Table::Speed, value).map_or(0, |r| r[0] as i32) >> 1;
                v.vdir = 1;
                v.vib_offset = 0;
                if v.song.is_none() {
                    v.song = song;
                }
            }
            CMD_ARP => {
                // it ends: the next frame is on the note again
                if value == 0 && v.arp != 0 {
                    v.arp_step = ARP_ENDS;
                }
                v.arp = value;
            }
            CMD_CUTOFF => {
                self.filter.cutoff = value as i32;
                self.sid.write(22, value);
            }
            CMD_PULSE => {
                v.pulse = (value as i32) << 4;
                let p = v.pulse;
                self.set_pulse(track, p);
            }
            // the volume is the music's
            CMD_VOLUME => {
                if track < SOUND {
                    self.volume = value & 15;
                    self.sid.write(24, (self.filter.mode << 4) | self.volume);
                }
            }
            CMD_TEMPO => {
                if value != 0 {
                    self.seq(id).tempo = value;
                }
            }
            CMD_BREAK => self.seq(id).brk = true,
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        for k in (0..SOUND + SOUNDS).rev() {
            self.track_frame(k);
        }
        for id in [SeqId::Sound(1), SeqId::Sound(0), SeqId::Music] {
            let s = self.seq(id);
            if !s.playing || s.song.is_none() {
                continue;
            }
            if s.frame == 0 {
                self.play_row(id);
            }
            let s = self.seq(id);
            if s.playing {
                s.frame = if s.frame + 1 < s.tempo { s.frame + 1 } else { 0 };
            }
        }
    }

    fn play_row(&mut self, id: SeqId) {
        let seq = self.seq(id);
        let Some(song) = seq.song.clone() else { return };
        let (order, row) = (seq.order, seq.row);
        let pattern = &song.patterns[song.orders[order]];
        let sound = id != SeqId::Music;
        if !sound {
            self.rows.push((order, row));
        }
        for k in 0..if sound { 1 } else { VOICES.min(song.voices) } {
            let o = cell_offset(row, k, song.voices);
            let [note, ins, cmd, value] = [pattern[o], pattern[o + 1], pattern[o + 2], pattern[o + 3]];
            let track = match id {
                SeqId::Sound(n) => SOUND + n,
                SeqId::Music => k,
            };
            if !sound && self.muted[k] {
                // a muted voice still says how the song goes on
                if cmd == CMD_TEMPO || cmd == CMD_BREAK {
                    self.command(track, id, cmd, value);
                }
                continue;
            }
            if note == NOTE_OFF {
                self.tracks[track].gate = false;
                self.note_off(track);
            } else if note != 0 {
                self.note_on(track, note, ins, Some(song.clone()), cmd == CMD_GLIDE || cmd == CMD_GLIDE_T);
            }
            if cmd != 0 {
                self.command(track, id, cmd, value);
            }
        }
        let seq = self.seq(id);
        let end = seq.brk;
        seq.brk = false;
        seq.row += 1;
        if seq.row < song.rows && !end {
            return;
        }
        seq.row = 0;
        if seq.loop_pattern {
            return;
        }
        seq.order += 1;
        if seq.order < song.orders.len() {
            return;
        }
        match song.loop_to {
            Some(l) if !sound => seq.order = l.min(song.orders.len() - 1),
            _ => {
                seq.order = 0;
                self.stop_seq(id);
            }
        }
    }

    fn track_frame(&mut self, k: usize) {
        let Some((song, ix)) = self.tracks[k].ins.clone() else { return };
        if self.tracks[k].note < 0 {
            return;
        }
        let i = &song.instruments[ix];
        if self.tracks[k].trigger {
            let v = &mut self.tracks[k];
            v.trigger = false;
            v.pulse = i.pulse as i32;
            v.pulse_dir = 1;
            v.vib = 0;
            v.vib_offset = 0;
            v.arp_step = 0;
            wave_step(v); // the first step of the wave table: the waveform, the pitch
            v.freq = freq_of(v.tnote);
            // the step of the vibrato: up to about a semitone, a part of the distance to the next note
            v.vib_step = if v.vtab != 0 {
                0
            } else {
                ((freq_of(v.note + 1) - freq_of(v.note)) * i.vib_depth as i32 * i.vib_speed as i32) >> 8
            };
            v.vcount = row(&v.song, Table::Speed, v.vtab).map_or(0, |r| r[0] as i32) >> 1;
            let tsong = v.song.clone();
            if self.live(k) {
                self.set_filter(k, i, tsong);
                if self.filter.owner == Some(k) {
                    self.filter_step();
                }
            }
            self.write(k, 5, (i.a << 4) | i.d);
            self.write(k, 6, (i.s << 4) | i.r);
            let v = &mut self.tracks[k];
            pulse_step(v);
            let (pulse, f, wave) = (v.pulse, v.freq, v.wave);
            self.set_pulse(k, pulse);
            self.set_freq(k, f);
            self.write(k, 4, (wave & 0xfe) | 1);
            return;
        }
        let v = &mut self.tracks[k];
        let mut moved = false;
        let was = v.wave;
        let note = wave_step(v);
        if v.wave != was {
            let value = (v.wave & 0xfe) | v.gate as u8;
            self.write(k, 4, value);
        }
        let v = &mut self.tracks[k];
        if note {
            v.freq = freq_of(v.tnote);
            moved = true;
        }
        if v.arp != 0 {
            // the note, its first and its second other note, a frame each; without a second one two steps
            let steps = if v.arp & 15 != 0 { 3 } else { 2 };
            v.arp_step = if v.arp_step + 1 < steps { v.arp_step + 1 } else { 0 };
            let up = [0, v.arp >> 4, v.arp & 15][v.arp_step as usize] as i32;
            v.freq = freq_of((v.tnote + up).min(NOTES as i32 - 1));
            moved = true;
        } else if v.arp_step == ARP_ENDS {
            v.arp_step = 0;
            v.freq = freq_of(v.tnote);
            moved = true;
        }
        if v.slide != 0 {
            let (f, s, t) = (v.freq, v.speed, v.target);
            v.freq = match v.slide {
                CMD_UP => (f + s).min(0xffff),
                CMD_DOWN => (f - s).max(0),
                _ if f < t => (f + s).min(t),
                _ => (f - s).max(t),
            };
            moved = true;
        }
        if v.vtab != 0 {
            // a vibrato of the speed table: the depth a frame, the way turns after left frames
            let r = row(&v.song, Table::Speed, v.vtab);
            if v.vdelay != 0 {
                v.vdelay -= 1;
            } else if let Some(r) = r
                && r[0] != 0
            {
                v.vib_offset += v.vdir * r[1] as i32;
                v.vcount += 1;
                if v.vcount >= r[0] as i32 {
                    v.vcount = 0;
                    v.vdir = -v.vdir;
                }
                moved = true;
            }
        } else if v.vib_step != 0 {
            // a triangle: a quarter of the 64 steps up, a half down, a quarter up
            v.vib = (v.vib + i.vib_speed as i32) & 63;
            v.vib_offset += if v.vib < 16 || v.vib >= 48 { v.vib_step } else { -v.vib_step };
            moved = true;
        }
        if moved {
            let f = v.freq + v.vib_offset;
            self.set_freq(k, f);
        }
        let v = &mut self.tracks[k];
        if v.pptr != 0 || v.ptimer != 0 {
            if pulse_step(v) {
                let p = v.pulse;
                self.set_pulse(k, p);
            }
        } else if i.pwm != 0 && i.pulse_tab == 0 && v.wave & 0x40 != 0 {
            // a pulse table ends: the width stays
            v.pulse += i.pwm as i32 * v.pulse_dir;
            if v.pulse >= PULSE_MAX {
                v.pulse = PULSE_MAX;
                v.pulse_dir = -1;
            }
            if v.pulse < PULSE_MIN {
                v.pulse = PULSE_MIN;
                v.pulse_dir = 1;
            }
            let p = v.pulse;
            self.set_pulse(k, p);
        }
        let f = &mut self.filter;
        if f.owner == Some(k) && (f.ptr != 0 || f.timer != 0) {
            self.filter_step();
        } else if f.owner == Some(k) && f.sweep != 0 {
            f.cutoff = (f.cutoff + f.sweep).clamp(0, 255);
            let c = f.cutoff as u8;
            self.sid.write(22, c);
        }
    }

    /// A step of the filter table of the filter's note: its type, resonance, cutoff.
    fn filter_step(&mut self) {
        let f = &mut self.filter;
        if f.timer != 0 {
            f.timer -= 1;
            f.cutoff = (f.cutoff + f.speed) & 255;
            let c = f.cutoff as u8;
            self.sid.write(22, c);
            return;
        }
        let mut n = 0;
        while n < STEPS && self.filter.ptr != 0 {
            n += 1;
            let f = &mut self.filter;
            let Some([l, x]) = row(&f.song, Table::Filter, f.ptr) else {
                f.ptr = 0;
                return;
            };
            if l == 0xff {
                f.ptr = x;
                continue;
            }
            f.ptr = f.ptr.checked_add(1).unwrap_or(0);
            if l >= 0x80 {
                // the type and the resonance: the next row in the same frame
                f.mode = (l >> 4) & 7;
                f.res = x >> 4;
                let (a, b) = ((f.res << 4) | f.route, (f.mode << 4) | self.volume);
                self.sid.write(23, a);
                self.sid.write(24, b);
                continue;
            }
            if l == 0 {
                f.cutoff = x as i32;
            } else {
                f.timer = l - 1;
                f.speed = x as i8 as i32;
                f.cutoff = (f.cutoff + f.speed) & 255;
            }
            let c = f.cutoff as u8;
            self.sid.write(22, c);
            return;
        }
    }

    /// The note of track k starts: its voice goes through the filter, which is now the one of its
    /// instrument (one with a filter table too, which runs from its row) - or around it.
    fn set_filter(&mut self, k: usize, i: &crate::song::Instrument, song: Option<Arc<Song>>) {
        let bit = 1u8.checked_shl(self.voice(k) as u32).unwrap_or(0);
        let f = &mut self.filter;
        if i.filter != 0 || i.filter_tab != 0 {
            f.mode = i.filter & 7;
            f.res = i.res;
            f.cutoff = i.cutoff as i32;
            f.sweep = if i.filter_tab != 0 { 0 } else { i.sweep as i32 };
            f.owner = Some(k);
            f.route |= bit;
            f.ptr = i.filter_tab;
            f.timer = 0;
            f.speed = 0;
            f.song = song;
            let (c, m) = (f.cutoff as u8, (f.mode << 4) | self.volume);
            self.sid.write(22, c);
            self.sid.write(24, m);
        } else {
            f.route &= !bit;
        }
        let r = (self.filter.res << 4) | self.filter.route;
        self.sid.write(23, r);
    }

    fn set_freq(&mut self, k: usize, f: i32) {
        let f = f as u16;
        self.write(k, 0, f as u8);
        self.write(k, 1, (f >> 8) as u8);
    }

    fn set_pulse(&mut self, k: usize, p: i32) {
        self.write(k, 2, p as u8);
        self.write(k, 3, ((p >> 8) & 255) as u8);
    }
}

/// A step of the wave table of a track: its waveform and the note of its pitch. Whether the note changed.
fn wave_step(v: &mut Track) -> bool {
    if v.wptr == 0 {
        return false;
    }
    if v.wdelay != 0 {
        v.wdelay -= 1;
        return false;
    }
    for _ in 0..STEPS {
        let Some([l, x]) = row(&v.song, Table::Wave, v.wptr) else {
            v.wptr = 0;
            return false;
        };
        if l == 0xff {
            v.wptr = x;
            if x == 0 {
                return false;
            }
            continue;
        }
        v.wptr = v.wptr.checked_add(1).unwrap_or(0);
        if (1..=0x0f).contains(&l) {
            v.wdelay = l - 1;
            return false;
        }
        if l >= 0xf0 {
            return false; // a command of GoatTracker: its right byte is no note
        }
        if l >= 0x10 {
            v.wave = l & 0xfe; // the gate is the note's
        }
        if x == 0x80 {
            return false;
        }
        let to = if x < 0x60 {
            v.note + x as i32
        } else if x < 0x80 {
            v.note - (0x80 - x as i32)
        } else {
            x as i32 - 0x80
        };
        v.tnote = to.clamp(0, NOTES as i32 - 1);
        return true;
    }
    false
}

/// A step of the pulse table of a track: its width. Whether it changed.
fn pulse_step(v: &mut Track) -> bool {
    if v.ptimer != 0 {
        v.ptimer -= 1;
        v.pulse = (v.pulse + v.pspeed) & 0xfff;
        return true;
    }
    if v.pptr == 0 {
        return false;
    }
    for _ in 0..STEPS {
        let Some([l, x]) = row(&v.song, Table::Pulse, v.pptr) else {
            v.pptr = 0;
            return false;
        };
        if l == 0xff {
            v.pptr = x;
            if x == 0 {
                return false;
            }
            continue;
        }
        v.pptr = v.pptr.checked_add(1).unwrap_or(0);
        if l >= 0x80 {
            v.pulse = ((l as i32 & 15) << 8) | x as i32;
            return true;
        }
        if l == 0 {
            return false;
        }
        v.ptimer = l - 1;
        v.pspeed = x as i8 as i32;
        v.pulse = (v.pulse + v.pspeed) & 0xfff;
        return true;
    }
    false
}

/// The players of a song with their SIDs and a clock in samples. A song plays as the game plays it:
/// its game voices on one SID; or all: every voice of the song, a SID and a player for every three
/// (the voice of a call is a voice of the song).
pub struct SidEngine {
    sample_rate: f64,
    chips: Vec<Player>,
    /// per voice of the song: [chip, voice of its SID], or None
    slots: Vec<Option<(usize, usize)>>,
    muted: Vec<bool>,
    per_tick: f64,
    /// samples until the next frame of the players
    left: f64,
    buffer: Vec<f32>,
    /// the row that plays: order, row
    pub position: Option<(usize, usize)>,
}

impl SidEngine {
    pub fn new(sample_rate: f64) -> SidEngine {
        let mut e = SidEngine {
            sample_rate,
            chips: vec![],
            slots: vec![],
            muted: vec![],
            per_tick: sample_rate / 50.,
            left: 0.,
            buffer: vec![],
            position: None,
        };
        e.set_chips(1);
        e
    }

    pub fn player(&self) -> &Player {
        &self.chips[0]
    }

    pub fn playing(&self) -> bool {
        self.chips[0].playing()
    }

    fn set_chips(&mut self, n: usize) {
        self.chips.truncate(n);
        while self.chips.len() < n {
            self.chips.push(Player::new(Sid::new(self.sample_rate)));
        }
    }

    /// The voices of the song on the SIDs: the game voices on one, or all, three on each.
    pub fn set_song(&mut self, song: &Song, all: bool) {
        let groups: Vec<Vec<usize>> = if all {
            (0..song.voices).step_by(VOICES).map(|v| (v..(v + VOICES).min(song.voices)).collect()).collect()
        } else {
            vec![song.game.clone()]
        };
        let playing = self.chips[0].playing();
        let at = self.chips[0].position();
        let frame = self.chips[0].music.frame;
        self.set_chips(groups.len());
        self.slots = vec![None; song.voices];
        let many = groups.len() > 1;
        for (c, list) in groups.iter().enumerate() {
            let picked = Arc::new(song.pick_voices(list, many));
            let player = &mut self.chips[c];
            player.set_song(picked);
            for (k, &v) in list.iter().enumerate() {
                self.slots[v] = Some((c, k));
            }
            for k in 0..VOICES {
                let muted = list.get(k).is_some_and(|&v| self.muted.get(v).copied().unwrap_or(false));
                player.mute(k, muted);
            }
            // a SID that comes while the song plays joins it at the row of the others
            if playing && !player.playing() {
                player.play(at.0, at.1, false);
                player.music.frame = frame;
            }
        }
    }

    /// The SID voice of a voice of the song; a voice that is not heard plays its notes on the first SID.
    fn slot(&self, voice: usize) -> (usize, usize) {
        self.slots.get(voice).copied().flatten().unwrap_or((0, voice % VOICES))
    }

    pub fn play(&mut self, order: usize, row: usize, loop_pattern: bool) {
        for c in &mut self.chips {
            c.play(order, row, loop_pattern);
        }
        self.left = 0.;
        self.position = None;
    }

    pub fn stop(&mut self) {
        for c in &mut self.chips {
            c.stop();
        }
        self.position = None;
    }

    pub fn mute(&mut self, voice: usize, on: bool) {
        if self.muted.len() <= voice {
            self.muted.resize(voice + 1, false);
        }
        self.muted[voice] = on;
        if let Some(Some((c, k))) = self.slots.get(voice).copied() {
            self.chips[c].mute(k, on);
        }
    }

    /// A song as a sound over the music. polite: not over another sound.
    pub fn sound(&mut self, song: &Song, polite: bool) {
        if !(polite && self.chips[0].sound_playing()) {
            self.chips[0].play_sound(Arc::new(song.game_song()));
        }
    }

    pub fn note_on(&mut self, voice: usize, note: u8, ins: u8) {
        let (c, k) = self.slot(voice);
        self.chips[c].note_on(k, note, ins, None, false);
    }

    pub fn note_off(&mut self, voice: usize) {
        let (c, k) = self.slot(voice);
        self.chips[c].note_off(k);
    }

    /// The samples of `out`. The SIDs are mixed: louder than one, not as loud as their sum.
    pub fn render(&mut self, out: &mut [f32]) {
        let n = self.chips.len();
        let gain = 1. / (n as f32).sqrt();
        if n > 1 && self.buffer.len() < out.len() {
            self.buffer.resize(out.len(), 0.);
        }
        let mut i = 0;
        while i < out.len() {
            if self.left < 1. {
                for c in &mut self.chips {
                    c.tick();
                }
                if let Some(&last) = self.chips[0].take_rows().last() {
                    self.position = Some(last);
                }
                self.left += self.per_tick;
            }
            let len = (out.len() - i).min(self.left.ceil() as usize);
            self.chips[0].sid.render(&mut out[i..i + len]);
            for c in 1..n {
                self.chips[c].sid.render(&mut self.buffer[i..i + len]);
                for k in i..i + len {
                    out[k] += self.buffer[k];
                }
            }
            if n > 1 {
                for s in &mut out[i..i + len] {
                    *s = (*s * gain).clamp(-1., 1.);
                }
            }
            i += len;
            self.left -= len as f64;
        }
    }
}

/// The bytes of a cell: note, instrument, command, value.
pub fn cell(song: &Song, pattern: usize, row: usize, voice: usize) -> [u8; CELL] {
    let o = cell_offset(row, voice, song.voices);
    song.patterns[pattern][o..o + CELL].try_into().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::new_pattern;

    fn song_with_notes() -> Song {
        let mut s = Song { rows: 16, patterns: vec![new_pattern(16, VOICES)], ..Default::default() };
        for (r, note) in [(0, 49u8), (4, 53), (8, 56), (12, NOTE_OFF)] {
            let o = cell_offset(r, 0, VOICES);
            s.patterns[0][o] = note;
            s.patterns[0][o + 1] = 1;
        }
        s
    }

    #[test]
    fn a_song_sounds_and_names_its_rows() {
        let mut e = SidEngine::new(44100.);
        e.set_song(&song_with_notes(), true);
        e.play(0, 0, false);
        let mut out = vec![0f32; 44100 / 2];
        e.render(&mut out);
        let peak = out.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "peak {peak}");
        // half a second: 25 frames, rows of 6 frames
        assert_eq!(e.position, Some((0, 4)));
    }

    #[test]
    fn silence_without_notes() {
        let mut e = SidEngine::new(44100.);
        e.set_song(&Song::default(), true);
        e.play(0, 0, false);
        let mut out = vec![0f32; 4410];
        e.render(&mut out);
        assert!(out.iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn a_jammed_note_and_its_release() {
        let mut e = SidEngine::new(44100.);
        e.set_song(&Song::default(), true);
        e.note_on(0, 49, 1);
        let mut out = vec![0f32; 4410];
        e.render(&mut out);
        assert!(out.iter().any(|s| s.abs() > 0.05));
        e.note_off(0);
        let mut tail = vec![0f32; 44100 * 2];
        e.render(&mut tail);
        assert!(tail[tail.len() - 100..].iter().all(|s| s.abs() < 1e-3));
    }

    #[test]
    fn the_wave_table_sets_the_pitch() {
        let mut s = song_with_notes();
        // an arpeggio of the table: the note, +12, again
        s.table_mut(Table::Wave).extend([[0x41, 0], [0x00, 12], [0xff, 1]]);
        s.instruments[0].wave_tab = 1;
        s.instruments[0].vib_depth = 0;
        let mut p = Player::new(Sid::new(44100.));
        p.set_song(Arc::new(s));
        p.play(0, 0, false);
        p.tick(); // the row: the gate goes
        p.tick(); // the trigger: the first row of the table
        let f0 = p.sid.voices[0].freq;
        p.tick(); // +12: an octave
        let f1 = p.sid.voices[0].freq;
        assert_eq!(f0, freq(48) as u32);
        assert_eq!(f1, freq(60) as u32);
    }
}
