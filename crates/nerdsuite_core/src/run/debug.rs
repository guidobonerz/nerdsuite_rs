//! Debug: a program in mimic under the control of the IDE - breakpoints, stepping, the registers,
//! the code at the PC, the stack and the memory. A thread owns mimic and its connection; the window
//! sends it commands and reads what it saw last ([`Snapshot`]), as nerdsuite2's `backend/mimic` and
//! `vice.js` do over HTTP.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use serde_json::{Value, json};

use super::mimic::Mimic;

/// What the window asks of the debugger.
#[derive(Debug, Clone)]
pub enum Command {
    Continue,
    Pause,
    StepInto,
    StepOver,
    /// until the subroutine returns (RTS)
    StepOut,
    /// the addresses of the breakpoints (exec)
    SetBreakpoints(Vec<u32>),
    /// the address of the memory panel
    ShowMemory(u32),
    /// a new build into the machine, with its breakpoints
    Load { file: PathBuf, breakpoints: Vec<u32> },
    Stop,
}

/// What the debugger saw last.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// counts up with every change
    pub version: u64,
    /// mimic runs (the process)
    pub alive: bool,
    /// the CPU is stopped (at a breakpoint, after a step, paused)
    pub stopped: bool,
    pub pc: Option<u32>,
    /// the registers in mimic's order, with the IDE's names (sp, fl for the 6502's S and P)
    pub registers: Vec<(String, u32)>,
    /// the bytes from the PC on, for the disassembly
    pub code: Vec<u8>,
    /// the stack of the 6502 (from SP + 1 to $01ff)
    pub stack: Vec<u8>,
    pub memory_addr: u32,
    pub memory: Vec<u8>,
    /// why it stopped: breakpoint, step, pause, cpu_jam …
    pub reason: Option<String>,
    pub log: Vec<String>,
}

impl Snapshot {
    pub fn register(&self, name: &str) -> Option<u32> {
        self.registers.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

pub const CODE_BYTES: usize = 96;
pub const MEMORY_BYTES: usize = 256;

/// A debug session: the thread that owns mimic.
pub struct Debugger {
    commands: Sender<Command>,
    thread: Option<thread::JoinHandle<()>>,
    snapshot: Arc<Mutex<Snapshot>>,
    pub machine: String,
}

impl Debugger {
    /// Starts mimic, sets the breakpoints and loads the program.
    pub fn start(exe: &Path, machine: &str, model: Option<&str>, file: PathBuf, breakpoints: Vec<u32>, memory_addr: u32) -> Result<Debugger> {
        let mimic = Mimic::start(exe, machine, model)?;
        let snapshot = Arc::new(Mutex::new(Snapshot { alive: true, memory_addr, ..Default::default() }));
        let (commands, rx) = mpsc::channel();
        let mut session = Session { mimic, snapshot: snapshot.clone(), bps: HashMap::new() };
        session.set_breakpoints(&breakpoints);
        if let Err(e) = session.mimic.load_program(&file) {
            return Err(e);
        }
        session.log(format!("{} loaded and started", file.file_name().unwrap_or_default().to_string_lossy()));
        let thread = thread::spawn(move || session.run(rx));
        Ok(Debugger { commands, thread: Some(thread), snapshot, machine: machine.to_owned() })
    }

    pub fn send(&self, command: Command) {
        self.commands.send(command).ok();
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().unwrap().clone()
    }

    pub fn version(&self) -> u64 {
        self.snapshot.lock().unwrap().version
    }
}

impl Drop for Debugger {
    /// The thread ends, and with it mimic (before the program does, when it quits).
    fn drop(&mut self) {
        self.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build, project::Project, run::tools};
    use std::time::Instant;

    fn wait(d: &Debugger, what: &str, f: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let t0 = Instant::now();
        loop {
            let s = d.snapshot();
            if f(&s) {
                return s;
            }
            assert!(t0.elapsed().as_secs() < 20, "{what}: {:?}", s.log);
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// The demo in mimic-headless: a breakpoint, a step, on. `MIMIC_TEST=1 cargo test -p nerdsuite_core debugger`
    #[test]
    fn debugger_stops_steps_and_goes_on() {
        if std::env::var("MIMIC_TEST").is_err() {
            return;
        }
        let exe = tools::apps_dir().join("mimic").join(tools::exe_name("mimic-headless"));
        let project = Project::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo")).unwrap();
        let b = build::build_program(&project, "src/main.asm", &Default::default());
        let info = build::DebugInfo::new(&b);
        let file = std::env::temp_dir().join("nerdsuite-test").join("debug.prg");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b.prg.as_ref().unwrap()).unwrap();
        // line 23: sta BORDER
        let bp = info.addrs_of("src/main.asm", 23)[0];
        let d = Debugger::start(&exe, "c64", None, file, vec![bp], 0x0400).unwrap();
        let s = wait(&d, "the breakpoint", |s| s.stopped && s.pc == Some(bp));
        assert_eq!(s.register("a"), Some(0)); // lda #BLACK
        assert_eq!(&s.code[..3], &[0x8d, 0x20, 0xd0]);
        assert_eq!(s.memory.len(), MEMORY_BYTES);
        d.send(Command::StepInto);
        let s = wait(&d, "the step", |s| s.stopped && s.pc == Some(bp + 3));
        assert_eq!(info.line_of(s.pc.unwrap()), Some(("src/main.asm", 24)));
        d.send(Command::SetBreakpoints(vec![]));
        d.send(Command::Continue);
        wait(&d, "running on", |s| !s.stopped);
        d.send(Command::Pause);
        wait(&d, "the pause", |s| s.stopped && s.reason.is_some());
    }
}

struct Session {
    mimic: Mimic,
    snapshot: Arc<Mutex<Snapshot>>,
    /// address -> the breakpoint of mimic
    bps: HashMap<u32, u64>,
}

/// The IDE's name of a register of mimic: S and P of the 6502 are sp and fl (as VICE names them).
fn ide_name(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "s" => "sp".into(),
        "p" => "fl".into(),
        n => n.into(),
    }
}

fn bytes_of(v: &Value) -> Vec<u8> {
    v["bytes"].as_array().into_iter().flatten().filter_map(|b| b.as_u64().map(|b| b as u8)).collect()
}

impl Session {
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        let mut s = self.snapshot.lock().unwrap();
        f(&mut s);
        s.version += 1;
    }

    fn log(&self, text: String) {
        self.update(|s| {
            s.log.push(text);
            if s.log.len() > 200 {
                s.log.remove(0);
            }
        });
    }

    fn run(mut self, rx: mpsc::Receiver<Command>) {
        loop {
            if !self.mimic.is_running() {
                self.update(|s| {
                    s.alive = false;
                    s.stopped = false;
                });
                self.log("mimic has ended".into());
                return;
            }
            // the events of mimic: resumed, stopped
            while let Ok(event) = self.mimic.client.events.try_recv() {
                match event["event"].as_str() {
                    Some("resumed") => self.update(|s| s.stopped = false),
                    Some("stopped") => {
                        let reason = event["reason"]["kind"].as_str().map(str::to_owned);
                        self.refresh(reason);
                    }
                    _ => {}
                }
            }
            match rx.recv_timeout(Duration::from_millis(30)) {
                Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(command) => {
                    if let Err(e) = self.command(command) {
                        self.log(format!("{e:#}"));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn command(&mut self, command: Command) -> Result<()> {
        let stopped = self.snapshot.lock().unwrap().stopped;
        let client = &mut self.mimic.client;
        match command {
            Command::Continue if stopped => {
                client.call("resume", json!({}), None)?;
            }
            Command::Pause if !stopped => {
                // the stopped event that follows refreshes
                client.call("pause", json!({}), None)?;
            }
            Command::StepInto if stopped => {
                // synchronous: no event
                client.call("step_instr", json!({ "count": 1 }), None)?;
                self.refresh(Some("step".into()));
            }
            Command::StepOver if stopped => {
                client.call("step_over", json!({}), None)?;
            }
            Command::StepOut if stopped => {
                client.call("step_out", json!({}), None)?;
            }
            Command::SetBreakpoints(addrs) => self.set_breakpoints(&addrs),
            Command::ShowMemory(addr) => {
                self.update(|s| s.memory_addr = addr);
                self.read_memory();
            }
            Command::Load { file, breakpoints } => {
                self.set_breakpoints(&breakpoints);
                self.mimic.load_program(&file)?;
                self.log(format!("{} loaded and started", file.file_name().unwrap_or_default().to_string_lossy()));
            }
            _ => {}
        }
        Ok(())
    }

    /// The breakpoints of mimic made the ones asked for: the others go, the new ones come.
    fn set_breakpoints(&mut self, addrs: &[u32]) {
        let gone: Vec<u32> = self.bps.keys().filter(|a| !addrs.contains(a)).copied().collect();
        for addr in gone {
            if let Some(bp) = self.bps.remove(&addr) {
                self.mimic.client.call("break_remove", json!({ "bp": bp }), None).ok();
            }
        }
        for &addr in addrs {
            if self.bps.contains_key(&addr) {
                continue;
            }
            match self.mimic.client.call("break_add", json!({ "kind": "exec", "addr": addr }), None) {
                Ok(r) => {
                    if let Some(bp) = r["bp"].as_u64() {
                        self.bps.insert(addr, bp);
                    }
                }
                Err(e) => self.log(format!("breakpoint at ${addr:04x}: {e:#}")),
            }
        }
    }

    fn read_memory(&mut self) {
        let addr = self.snapshot.lock().unwrap().memory_addr;
        if let Ok(r) = self.mimic.client.call("peek", json!({ "addr": addr, "len": MEMORY_BYTES }), None) {
            let bytes = bytes_of(&r);
            self.update(|s| s.memory = bytes);
        }
    }

    /// After a stop: the registers, the code at the PC, the stack and the memory.
    fn refresh(&mut self, reason: Option<String>) {
        let client = &mut self.mimic.client;
        let registers: Vec<(String, u32)> = client
            .call("regs", json!({}), None)
            .ok()
            .and_then(|r| {
                Some(
                    r["regs"]
                        .as_array()?
                        .iter()
                        .filter_map(|x| Some((ide_name(x["name"].as_str()?), x["value"].as_u64()? as u32)))
                        .collect(),
                )
            })
            .unwrap_or_default();
        let pc = registers.iter().find(|(n, _)| n == "pc").map(|(_, v)| *v);
        let code = match pc {
            Some(pc) => client.call("peek", json!({ "addr": pc, "len": CODE_BYTES }), None).map(|r| bytes_of(&r)).unwrap_or_default(),
            None => Vec::new(),
        };
        let stack = match registers.iter().find(|(n, _)| n == "sp").map(|(_, v)| *v) {
            Some(sp) if self.mimic.machine != "cpc" && self.mimic.machine != "zx81" && sp < 0xff => client
                .call("peek", json!({ "addr": 0x100 + sp + 1, "len": 0xff - sp }), None)
                .map(|r| bytes_of(&r))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let text = match (&reason, pc) {
            (Some(reason), Some(pc)) => format!("stopped ({reason}) at ${pc:04x}"),
            (None, Some(pc)) => format!("stopped at ${pc:04x}"),
            _ => "stopped".into(),
        };
        self.update(|s| {
            s.stopped = true;
            s.pc = pc;
            s.registers = registers;
            s.code = code;
            s.stack = stack;
            s.reason = reason;
        });
        self.read_memory();
        self.log(text);
    }
}
