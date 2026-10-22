//! Run: a build in an emulator - mimic (through its debug protocol), VICE (autostart), or another
//! emulator of the launcher (Altirra, Stella, Mesen, Xemu, WinUAE …), as nerdsuite2 chooses them.

pub mod debug;
pub mod mimic;
pub mod tools;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{Context as _, Result, anyhow, bail};

use crate::{
    build::Build,
    platform::{Emulator, Platform},
};
use mimic::Mimic;
use tools::{ToolSettings, emulators, mimic_tool, preferred_order, resolve_exe, vice_exe};

/// The emulator that runs a platform.
#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    Mimic(PathBuf),
    Vice(PathBuf),
    /// an emulator of the launcher: its id and program
    Other(&'static str, PathBuf),
}

impl Choice {
    pub fn name(&self) -> String {
        match self {
            Choice::Mimic(_) => "mimic".into(),
            Choice::Vice(_) => "VICE".into(),
            Choice::Other(id, _) => emulators().into_iter().find(|e| e.id == *id).map_or(id.to_string(), |e| e.name.into()),
        }
    }
}

/// The emulator of the settings for a platform, else mimic when it runs the machine, else VICE (the
/// Commodore machines) or the first emulator of the launcher that is found.
pub fn choose(platform: &Platform, settings: &ToolSettings) -> Result<Choice> {
    let mimic = platform.mimic.and_then(|_| resolve_exe(settings, &mimic_tool()));
    match platform.emulator {
        Emulator::Vice { machine, .. } => {
            let vice = || vice_exe(settings, machine).map(Choice::Vice);
            let choice = match settings.preferred(platform.id) {
                Some("vice") => vice(),
                Some("mimic") => mimic.map(Choice::Mimic),
                _ => mimic.map(Choice::Mimic).or_else(vice),
            };
            choice.ok_or_else(|| anyhow!("{}: neither mimic nor VICE found (set MIMIC or VICE)", platform.name))
        }
        Emulator::Other(id) => {
            let pref = settings.preferred(id);
            if let Some(mimic) = mimic.filter(|_| pref.is_none() || pref == Some("mimic")) {
                return Ok(Choice::Mimic(mimic));
            }
            let all = emulators();
            let order = pref.into_iter().chain(preferred_order(id).iter().copied());
            for emu in order {
                if let Some(def) = all.iter().find(|d| d.id == emu && d.platforms.contains(&id))
                    && let Some(exe) = resolve_exe(settings, def)
                {
                    return Ok(Choice::Other(def.id, exe));
                }
            }
            bail!("{}: no emulator found ({})", platform.name, preferred_order(id).join(", "))
        }
    }
}

/// The format of a build as the launcher and mimic name it: xex, atascii-lst, amsdos-bin, cpc-ascii,
/// a26, nes, sfc, zx81-p, trs80-cmd, trs80-ascii, bbc-bin, bbc-ascii, cbm, adf.
pub fn run_format(platform: &Platform, build_format: &str) -> &'static str {
    let emu = match platform.emulator {
        Emulator::Vice { .. } => return "cbm",
        Emulator::Other(id) => id,
    };
    match (emu, build_format) {
        (_, "xex") => "xex",
        (_, "atascii-lst") => "atascii-lst",
        (_, "amsdos") => "amsdos-bin",
        (_, "cpc-ascii") => "cpc-ascii",
        (_, "zx81" | "zx81-p") => "zx81-p",
        (_, "trs80") => "trs80-cmd",
        (_, "trs80-ascii") => "trs80-ascii",
        (_, "bbc-ascii") => "bbc-ascii",
        (_, "adf") => "adf",
        ("vcs", _) => "a26",
        ("nes", _) => "nes",
        ("snes", _) => "sfc",
        ("bbc", _) => "bbc-bin",
        _ => "cbm",
    }
}

/// An 8.3 name for DOS, H: and AMSDOS: upper case, letters and digits ("PROGRAM" when none).
pub fn dos_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let base = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    let stem: String = base.to_uppercase().chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect();
    let stem: String = stem.trim_start_matches(|c: char| c.is_ascii_digit()).chars().take(8).collect();
    if stem.is_empty() { "PROGRAM".into() } else { stem }
}

/// ATASCII text: `\n` and `\r\n` become EOL ($9b), and the last line has one.
fn to_atascii(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 1);
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            0x0d => {
                if data.get(i + 1) == Some(&0x0a) {
                    i += 1;
                }
                out.push(0x9b);
            }
            0x0a => out.push(0x9b),
            c => out.push(c),
        }
        i += 1;
    }
    if out.last().is_some_and(|&c| c != 0x9b) {
        out.push(0x9b);
    }
    out
}

/// What the launcher starts: the arguments, and the files to write first.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub args: Vec<String>,
    pub files: Vec<(PathBuf, Vec<u8>)>,
}

/// The command line of an emulator of the launcher (nerdsuite2's `planRun`).
pub fn plan(platform: &Platform, emulator: &str, name: &str, format: &str, data: &[u8], dir: &Path) -> Result<Plan> {
    let emu_platform = match platform.emulator {
        Emulator::Other(id) => id,
        Emulator::Vice { .. } => bail!("{} runs in VICE or mimic", platform.name),
    };
    let stem = dos_stem(name);
    let dir = dir.join(emu_platform);
    let p = |file: String| dir.join(file);
    let s = |p: &PathBuf| p.to_string_lossy().into_owned();
    let model = platform.model.unwrap_or_default();
    let (args, files): (Vec<String>, Vec<(PathBuf, Vec<u8>)>) = match (emu_platform, emulator, format) {
        ("atari", _, "xex") => {
            let file = p(format!("{stem}.XEX"));
            let args = if emulator == "altirra" { vec!["/nosingleinstance".into(), "/run".into(), s(&file)] } else { vec!["-run".into(), s(&file)] };
            (args, vec![(file, data.to_vec())])
        }
        ("atari", _, "atascii-lst") => {
            let hdir = dir.join("H");
            let file = hdir.join(format!("{stem}.LST"));
            let args = if emulator == "altirra" {
                vec!["/nosingleinstance".into(), "/basic".into(), "/hdpath".into(), s(&hdir), "/type".into(), format!("ENTER \"H:{stem}.LST\"~RUN~")]
            } else {
                // atari800 has an H: device but types nothing: ENTER "H1:..." and RUN are up to you
                vec!["-basic".into(), "-H1".into(), s(&hdir)]
            };
            (args, vec![(file, to_atascii(data))])
        }
        ("cpc", _, _) | ("bbc", _, _) => {
            // TODO: the disc images of the launcher (CPC .dsk, BBC .ssd)
            bail!("{} in {emulator}: the disc images of the launcher are not ported yet - choose mimic in the settings", platform.name)
        }
        ("vcs", _, "a26") => {
            if data.is_empty() {
                bail!("empty cartridge image");
            }
            let file = p(format!("{stem}.a26"));
            let args = if emulator == "stella" { vec![s(&file)] } else { vec!["a2600".into(), "-cart".into(), s(&file), "-window".into()] };
            (args, vec![(file, data.to_vec())])
        }
        ("nes", _, "nes") => {
            if data.len() < 16 || &data[..4] != b"NES\x1a" {
                bail!("no iNES header (NES and $1a)");
            }
            let file = p(format!("{stem}.nes"));
            let args = if emulator == "mame" { vec!["nes".into(), "-cart".into(), s(&file), "-window".into()] } else { vec![s(&file)] };
            (args, vec![(file, data.to_vec())])
        }
        ("snes", _, "sfc") => {
            if data.len() < 0x8000 {
                bail!("a SNES ROM has 32K at least");
            }
            let file = p(format!("{stem}.sfc"));
            let args = if emulator == "mame" { vec!["snes".into(), "-cart".into(), s(&file), "-window".into()] } else { vec![s(&file)] };
            (args, vec![(file, data.to_vec())])
        }
        ("zx81", _, "zx81-p") => {
            if data.len() < 0x407d - 0x4009 {
                bail!("no .p file (the system variables are missing)");
            }
            let file = p(format!("{}.p", stem.to_lowercase()));
            let args = if emulator == "zesarux" { vec!["--machine".into(), "ZX81".into(), s(&file)] } else { vec![s(&file)] };
            (args, vec![(file, data.to_vec())])
        }
        ("trs80", _, "trs80-cmd" | "trs80-ascii") => {
            let model = if model == "3" { "3" } else { "1" };
            if format == "trs80-ascii" && emulator == "mame" {
                bail!("MAME loads /CMD programs: a BASIC listing runs in trs80gp");
            }
            let file = p(format!("{stem}.{}", if format == "trs80-cmd" { "CMD" } else { "BAS" }));
            let args = if emulator == "mame" {
                vec![if model == "3" { "trs80m3" } else { "trs80l2" }.into(), "-quik".into(), s(&file), "-window".into()]
            } else {
                vec![format!("-m{model}"), s(&file)]
            };
            (args, vec![(file, data.to_vec())])
        }
        ("mega65", _, _) => {
            if data.len() < 3 {
                bail!("no .prg (a load address and a program)");
            }
            let file = p(format!("{}.prg", stem.to_lowercase()));
            (vec!["-prg".into(), s(&file)], vec![(file, data.to_vec())])
        }
        ("amiga", _, "adf") => {
            if data.len() != 901120 {
                bail!("an ADF is 901120 bytes (a DD disk), not {}", data.len());
            }
            let file = p(format!("{stem}.adf"));
            let args = if emulator == "winuae" {
                vec!["-s".into(), "use_gui=no".into(), "-s".into(), format!("floppy0={}", s(&file))]
            } else {
                vec!["--amiga_model=A500".into(), format!("--floppy_drive_0={}", s(&file))]
            };
            (args, vec![(file, data.to_vec())])
        }
        _ => bail!("{emulator} does not run {format} programs of the {}", platform.name),
    };
    Ok(Plan { args, files })
}

/// The emulator a Run started.
enum Session {
    Mimic { mimic: Mimic, platform: &'static str },
    Process(Child),
}

/// Keeps the emulator of the last Run: a new Run ends it (mimic of the same machine stays and
/// gets the new program).
#[derive(Default)]
pub struct Runner {
    session: Option<Session>,
}

impl Runner {
    pub fn stop(&mut self) {
        if let Some(Session::Process(mut child)) = self.session.take() {
            child.kill().ok();
            child.wait().ok();
        }
    }

    /// Runs a build; gives what was started, for the status line.
    pub fn run(&mut self, platform: &'static Platform, build: &Build, settings: &ToolSettings) -> Result<String> {
        let file = build.file.as_ref().ok_or_else(|| anyhow!("the build has no program"))?;
        let choice = choose(platform, settings)?;
        let format = run_format(platform, &file.format);
        let tmp = std::env::temp_dir().join("nerdsuite");
        let write = |path: &Path, data: &[u8]| -> Result<()> {
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, data).with_context(|| format!("writing {}", path.display()))
        };
        match &choice {
            Choice::Mimic(exe) => {
                let (machine, model) = platform.mimic.unwrap();
                let reuse = match &mut self.session {
                    Some(Session::Mimic { mimic, platform: p }) => *p == platform.id && mimic.is_running(),
                    _ => false,
                };
                if !reuse {
                    self.stop();
                    self.session = None;
                    let mimic = Mimic::start(exe, machine, model)?;
                    self.session = Some(Session::Mimic { mimic, platform: platform.id });
                }
                let Some(Session::Mimic { mimic, .. }) = &mut self.session else { unreachable!() };
                let path = tmp.join(platform.id).join(&file.name);
                match format {
                    "cbm" | "xex" | "a26" => {
                        // a cbm file has its load address; a plain one of a Commodore machine gets it
                        let data = build.prg.as_deref().unwrap_or(&file.bytes);
                        write(&path, data)?;
                        mimic.load_program(&path)?;
                    }
                    "adf" => {
                        write(&path, &file.bytes)?;
                        mimic.load_adf(&path)?;
                    }
                    // TODO: the keyboards of the CPC, the ZX81 and the Atari (typing RUN, LOAD, a listing)
                    _ => bail!("mimic: {format} programs are not ported yet - choose another emulator in the settings"),
                }
                Ok(format!("{} runs in mimic ({machine})", file.name))
            }
            Choice::Vice(exe) => {
                self.stop();
                self.session = None;
                let Emulator::Vice { args, .. } = platform.emulator else { unreachable!() };
                let prg = build.prg.as_ref().ok_or_else(|| anyhow!("VICE runs .prg files: the build has none"))?;
                let path = tmp.join(platform.id).join(format!("{}.prg", build.name));
                write(&path, prg)?;
                let child = Command::new(exe)
                    .args(["-autostartprgmode", "1"])
                    .args(args.iter())
                    .arg("-autostart")
                    .arg(&path)
                    .current_dir(exe.parent().unwrap_or(Path::new(".")))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .with_context(|| format!("failed to start {}", exe.display()))?;
                self.session = Some(Session::Process(child));
                Ok(format!("{} runs in VICE", file.name))
            }
            Choice::Other(id, exe) => {
                self.stop();
                self.session = None;
                let plan = plan(platform, id, &file.name, format, &file.bytes, &tmp)?;
                for (path, data) in &plan.files {
                    write(path, data)?;
                }
                let child = Command::new(exe)
                    .args(&plan.args)
                    .current_dir(exe.parent().unwrap_or(Path::new(".")))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .with_context(|| format!("failed to start {}", exe.display()))?;
                self.session = Some(Session::Process(child));
                Ok(format!("{} runs in {}", file.name, choice.name()))
            }
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::platform;

    #[test]
    fn stems_and_formats() {
        assert_eq!(dos_stem("build/hello world.xex"), "HELLOWOR");
        assert_eq!(dos_stem("123.bin"), "PROGRAM");
        assert_eq!(run_format(platform("c64").unwrap(), "cbm"), "cbm");
        assert_eq!(run_format(platform("vcs").unwrap(), "plain"), "a26");
        assert_eq!(run_format(platform("cpc6128").unwrap(), "amsdos"), "amsdos-bin");
        assert_eq!(run_format(platform("zx81").unwrap(), "zx81"), "zx81-p");
        assert_eq!(to_atascii(b"10 PRINT\r\n20 END"), b"10 PRINT\x9b20 END\x9b");
    }

    #[test]
    fn plans_of_the_launcher() {
        let dir = Path::new("/tmp/ns");
        let atari = platform("atari8").unwrap();
        let p = plan(atari, "altirra", "game.xex", "xex", &[0xff, 0xff], dir).unwrap();
        let file = dir.join("atari").join("GAME.XEX");
        assert_eq!(p.args, ["/nosingleinstance", "/run", &file.to_string_lossy()]);
        assert_eq!(p.files, vec![(file, vec![0xff, 0xff])]);
        let p = plan(atari, "altirra", "hello.lst", "atascii-lst", b"10 ?1", dir).unwrap();
        assert_eq!(p.args.last().unwrap(), "ENTER \"H:HELLO.LST\"~RUN~");
        let trs = platform("trs80m3").unwrap();
        assert_eq!(plan(trs, "trs80gp", "x.cmd", "trs80-cmd", &[1], dir).unwrap().args[0], "-m3");
        assert!(plan(platform("nes").unwrap(), "mesen", "x.nes", "nes", &[0; 16], dir).is_err());
        assert!(plan(platform("amiga500").unwrap(), "winuae", "x.adf", "adf", &[0; 10], dir).is_err());
    }

    /// The demo in mimic-headless: its text on the screen. `MIMIC_TEST=1 cargo test -p nerdsuite_core mimic_runs`
    #[test]
    fn mimic_runs_the_demo() {
        if std::env::var("MIMIC_TEST").is_err() {
            return;
        }
        let exe = tools::apps_dir().join("mimic").join(tools::exe_name("mimic-headless"));
        let project = crate::project::Project::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo")).unwrap();
        let build = crate::build::build_program(&project, "src/main.asm", &Default::default());
        assert!(build.ok, "{:?}", build.problems);
        let path = std::env::temp_dir().join("nerdsuite-test").join("demo.prg");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, build.prg.as_ref().unwrap()).unwrap();

        let mut m = Mimic::start(&exe, "c64", None).unwrap();
        assert!(m.slots.program.is_some());
        m.load_program(&path).unwrap();
        // "hello from nerdsuite" in screen codes at row 12, column 10
        let want: Vec<u64> = "hello from nerdsuite".bytes().map(|c| if c == b' ' { 32 } else { (c - 0x60) as u64 }).collect();
        let t0 = std::time::Instant::now();
        loop {
            let r = m.client.call("peek", serde_json::json!({ "addr": 0x400 + 12 * 40 + 10, "len": want.len() }), None).unwrap();
            let got: Vec<u64> = r["bytes"].as_array().unwrap().iter().map(|b| b.as_u64().unwrap()).collect();
            if got == want {
                break;
            }
            assert!(t0.elapsed().as_secs() < 20, "the text did not appear: {got:?}");
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    #[test]
    fn choice_follows_the_settings() {
        let settings = ToolSettings {
            paths: [("MIMIC".to_owned(), "/nowhere".to_owned()), ("VICE".to_owned(), "/nowhere".to_owned())].into(),
            ..Default::default()
        };
        // what is found depends on the machine; an unknown path in the settings never panics
        let _ = choose(platform("c64").unwrap(), &settings);
        let _ = choose(platform("nes").unwrap(), &settings);
    }
}
