//! mimic and its debug protocol "mimic-debug" (version 7): TCP, a JSON object per line. A request
//! is `{ id, cmd, ...fields }`, its response `{ id, ok, result | error }`; events have `event`
//! instead of `id` (hello, resumed, stopped …). Ported from nerdsuite2's `backend/mimic`.
//!
//! ```text
//! mimic.exe --machine c64 [--model 464] --rom-dir <its folder>/roms --debug 127.0.0.1:<port>
//! ```

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

type Pending = Arc<Mutex<HashMap<u64, Sender<Value>>>>;

/// A connection to mimic's debug port.
pub struct Client {
    stream: TcpStream,
    pending: Pending,
    next_id: u64,
    /// the events of mimic (resumed, stopped …), in their order
    pub events: Receiver<Value>,
    /// the machine mimic said hello with
    pub machine: String,
}

impl Client {
    /// Connects and waits for the hello of mimic.
    pub fn connect(port: u16, timeout: Duration) -> Result<Client> {
        let t0 = Instant::now();
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(e) if t0.elapsed() < timeout => {
                    let _ = e;
                    thread::sleep(Duration::from_millis(200));
                }
                Err(e) => return Err(e).context("cannot connect to mimic"),
            }
        };
        stream.set_nodelay(true).ok();
        let pending: Pending = Default::default();
        let (event_tx, events) = mpsc::channel();
        let (hello_tx, hello_rx) = mpsc::channel();
        let reader = BufReader::new(stream.try_clone()?);
        let reader_pending = pending.clone();
        thread::spawn(move || read_loop(reader, reader_pending, event_tx, hello_tx));
        let hello: Value = hello_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| anyhow!("no hello from mimic"))?;
        Ok(Client {
            stream,
            pending,
            next_id: 1,
            events,
            machine: hello["machine"].as_str().unwrap_or_default().to_owned(),
        })
    }

    /// A request and its result; an answer `ok: false` is an error.
    pub fn call(&mut self, cmd: &str, args: Value, timeout: Option<Duration>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut req = json!({ "id": id, "cmd": cmd });
        if let (Some(req), Value::Object(args)) = (req.as_object_mut(), args) {
            req.extend(args);
        }
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let mut line = serde_json::to_vec(&req)?;
        line.push(b'\n');
        self.stream.write_all(&line).context("connection to mimic lost")?;
        let reply = rx.recv_timeout(timeout.unwrap_or(REQUEST_TIMEOUT)).map_err(|_| {
            self.pending.lock().unwrap().remove(&id);
            anyhow!("timeout waiting for mimic's answer to {cmd}")
        })?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            bail!("{cmd}: {}", reply["error"].as_str().unwrap_or("failed"));
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn read_loop(reader: BufReader<TcpStream>, pending: Pending, events: Sender<Value>, hello: Sender<Value>) {
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else { continue };
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            if let Some(tx) = pending.lock().unwrap().remove(&id) {
                tx.send(msg).ok();
            }
        } else if msg.get("event").and_then(Value::as_str) == Some("hello") {
            hello.send(msg).ok();
        } else if msg.get("event").is_some() {
            events.send(msg).ok();
        }
    }
    // the connection is gone: the waiting requests fail
    pending.lock().unwrap().clear();
}

/// The media slots of the machine: the program, the cartridge, the disk and the tape.
#[derive(Debug, Default, Clone)]
pub struct Slots {
    pub program: Option<i64>,
    pub cartridge: Option<i64>,
    pub disk: Option<i64>,
    pub tape: Option<i64>,
}

/// A started mimic and its connection.
pub struct Mimic {
    pub child: Child,
    pub client: Client,
    pub machine: String,
    pub slots: Slots,
}

fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

impl Mimic {
    pub fn start(exe: &Path, machine: &str, model: Option<&str>) -> Result<Mimic> {
        let port = free_port()?;
        let mut cmd = Command::new(exe);
        cmd.arg("--machine").arg(machine);
        if let Some(model) = model {
            cmd.arg("--model").arg(model);
        }
        let dir = exe.parent().unwrap_or(Path::new("."));
        let roms = dir.join("roms");
        if roms.is_dir() {
            cmd.arg("--rom-dir").arg(roms);
        }
        cmd.arg("--debug").arg(format!("127.0.0.1:{port}"));
        // the desktop program runs from the start, mimic-headless only so
        if exe.file_name().is_some_and(|f| f.to_string_lossy().to_lowercase().contains("headless")) {
            cmd.arg("--run");
        }
        cmd.current_dir(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = cmd.spawn().with_context(|| format!("failed to start {}", exe.display()))?;
        let client = match Client::connect(port, CONNECT_TIMEOUT) {
            Ok(client) => client,
            Err(e) => {
                child.kill().ok();
                return Err(e);
            }
        };
        if !client.machine.is_empty() && client.machine != machine {
            child.kill().ok();
            bail!("mimic runs a {}, not a {machine}", client.machine);
        }
        let mut mimic = Mimic { child, client, machine: machine.to_owned(), slots: Slots::default() };
        if let Err(e) = mimic.handshake() {
            mimic.child.kill().ok();
            return Err(e);
        }
        Ok(mimic)
    }

    fn handshake(&mut self) -> Result<()> {
        let slots = self.client.call("list_media_slots", json!({}), None)?;
        for slot in slots.as_array().into_iter().flatten() {
            let id = slot["id"].as_i64();
            let name = slot["name"].as_str().unwrap_or_default();
            match slot["kind"].as_str().unwrap_or_default() {
                "Program" => self.slots.program = id,
                "Disk" if name == "drive A" || name == "DF0:" => self.slots.disk = id,
                "Cartridge" if self.machine == "atari2600" => self.slots.cartridge = id,
                "Tape" if self.machine == "zx81" => self.slots.tape = id,
                _ => {}
            }
        }
        Ok(())
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// A program into the program slot (or the cartridge of the 2600): a reset, the file in, run.
    /// mimic puts a .prg into the RAM once the machine has booted (RUN for a BASIC program) and
    /// loads a .xex as DOS does.
    pub fn load_program(&mut self, file: &Path) -> Result<()> {
        let slot = self
            .slots
            .program
            .or(self.slots.cartridge)
            .ok_or_else(|| anyhow!("this machine of mimic has no program slot"))?;
        self.client.call("reset", json!({ "kind": "soft" }), None)?;
        self.client.call(
            "insert_media",
            json!({ "slot": slot, "path": file.to_string_lossy() }),
            Some(Duration::from_secs(15)),
        )?;
        self.client.call("resume", json!({}), None)?;
        Ok(())
    }

    /// An ADF into DF0:, then a hard reset: Kickstart boots it (the Amiga).
    pub fn load_adf(&mut self, file: &Path) -> Result<()> {
        let slot = self.slots.disk.filter(|_| self.machine == "amiga").ok_or_else(|| anyhow!("ADF disks boot on the Amiga only"))?;
        self.client.call(
            "insert_media",
            json!({ "slot": slot, "path": file.to_string_lossy() }),
            Some(Duration::from_secs(15)),
        )?;
        self.client.call("reset", json!({ "kind": "hard" }), None)?;
        self.client.call("resume", json!({}), None)?;
        Ok(())
    }
}

impl Drop for Mimic {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}
