//! Where the emulators are: the settings of nerdsuite2 (`~/.nerdsuite/settings.json`, or the file
//! `NERDSUITE_SETTINGS` names) - a path there comes before the environment variable of the tool
//! (VICE, MIMIC, ALTIRRA …) - then the default folders in `~/applications`, then the PATH.

use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolSettings {
    /// tool key (VICE, MIMIC, ALTIRRA …) -> its program or folder
    #[serde(default)]
    pub paths: HashMap<String, String>,
    /// platform (c64, atari, cpc …) -> the emulator chosen for it
    #[serde(default)]
    pub preferred: HashMap<String, String>,
}

impl ToolSettings {
    pub fn file() -> PathBuf {
        match env::var_os("NERDSUITE_SETTINGS") {
            Some(file) => file.into(),
            None => home().join(".nerdsuite").join("settings.json"),
        }
    }

    /// The settings, empty when there are none (or they cannot be read).
    pub fn load() -> ToolSettings {
        fs::read_to_string(Self::file())
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// The path of a tool: the settings, else its environment variable.
    pub fn tool_path(&self, key: &str) -> Option<String> {
        self.paths
            .get(key)
            .filter(|p| !p.is_empty())
            .cloned()
            .or_else(|| env::var(key).ok().filter(|p| !p.is_empty()))
    }

    pub fn preferred(&self, platform: &str) -> Option<&str> {
        self.preferred.get(platform).map(String::as_str).filter(|p| !p.is_empty())
    }
}

pub fn home() -> PathBuf {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The folder the defaults of the tools look in.
pub fn apps_dir() -> PathBuf {
    home().join("applications")
}

pub fn exe_name(base: &str) -> String {
    if cfg!(windows) && !base.to_lowercase().ends_with(".exe") {
        format!("{base}.exe")
    } else {
        base.to_owned()
    }
}

/// The first of the names on the PATH (name.exe first on Windows).
pub fn search_path(names: &[&str]) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for name in names {
            let candidates = if cfg!(windows) { vec![format!("{name}.exe"), name.to_string()] } else { vec![name.to_string()] };
            for c in candidates {
                let p = dir.join(c);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// The first of the names in a folder: exact, else ignoring the case.
pub fn find_in_dir(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    for name in names {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if names.iter().any(|n| n.eq_ignore_ascii_case(&file)) && entry.path().is_file() {
            return Some(entry.path());
        }
    }
    None
}

/// A program nerdsuite starts: its key in the settings and its environment variable, the files
/// tried, a folder searched for some names, names on the PATH.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub id: &'static str,
    pub name: &'static str,
    pub env: &'static str,
    pub platforms: &'static [&'static str],
    pub defaults: Vec<PathBuf>,
    pub dir: Option<PathBuf>,
    pub dir_names: &'static [&'static str],
    pub path: &'static [&'static str],
}

/// Where a tool is: the path of the settings (a file, or a folder with one of its names), else its
/// defaults, its folder, the PATH.
pub fn resolve_exe(settings: &ToolSettings, def: &ToolDef) -> Option<PathBuf> {
    if let Some(v) = settings.tool_path(def.env) {
        let p = PathBuf::from(&v);
        if p.is_file() {
            return Some(p.canonicalize().unwrap_or(p));
        }
        if let Some(found) = find_in_dir(&p, def.dir_names) {
            return Some(found);
        }
    }
    if let Some(p) = def.defaults.iter().find(|p| p.is_file()) {
        return Some(p.clone());
    }
    if let Some(found) = def.dir.as_ref().and_then(|d| find_in_dir(d, def.dir_names)) {
        return Some(found);
    }
    search_path(def.path)
}

fn apps(parts: &[&str]) -> PathBuf {
    parts.iter().fold(apps_dir(), |p, part| p.join(part))
}

/// mimic: the settings (MIMIC: the program or its folder), else `~/applications/mimic`, else the PATH.
pub fn mimic_tool() -> ToolDef {
    ToolDef {
        id: "mimic",
        name: "mimic",
        env: "MIMIC",
        platforms: &[],
        defaults: Vec::new(),
        dir: Some(apps(&["mimic"])),
        dir_names: if cfg!(windows) { &["mimic.exe", "mimic-desktop.exe"] } else { &["mimic", "mimic-desktop"] },
        path: &["mimic", "mimic-desktop"],
    }
}

/// The program of VICE for a machine (x64sc, x128, xvic, xplus4, xpet): the folder of the settings
/// (VICE; a file there: its folder), else the default folder, else the PATH.
pub fn vice_exe(settings: &ToolSettings, machine: &str) -> Option<PathBuf> {
    let base = match machine {
        "c64" => "x64sc",
        "c128" => "x128",
        "vic20" => "xvic",
        "plus4" => "xplus4",
        "pet" => "xpet",
        _ => return None,
    };
    let mut dirs = Vec::new();
    if let Some(v) = settings.tool_path("VICE") {
        let p = PathBuf::from(v);
        dirs.push(if p.is_file() { p.parent().map(Path::to_path_buf).unwrap_or(p) } else { p });
    }
    dirs.push(apps(&["GTK3VICE-3.10-win64-r45942", "bin"]));
    dirs.iter()
        .map(|d| d.join(exe_name(base)))
        .find(|p| p.is_file())
        .or_else(|| search_path(&[base]))
}

/// The emulators of the launcher (nerdsuite2's `backend/emu`), in their order.
pub fn emulators() -> Vec<ToolDef> {
    let def = |id, name, env, platforms, defaults: Vec<PathBuf>, dir: Option<&str>, dir_names, path| ToolDef {
        id,
        name,
        env,
        platforms,
        defaults,
        dir: dir.map(|d| apps(&[d])),
        dir_names,
        path,
    };
    vec![
        def("altirra", "Altirra", "ALTIRRA", &["atari"], vec![apps(&["Altirra", "Altirra64.exe"]), apps(&["Altirra", "Altirra.exe"])], None, &[], &["Altirra64", "Altirra"]),
        def("atari800", "Atari800", "ATARI800", &["atari"], vec![], None, &[], &["atari800"]),
        def("winape", "WinAPE", "WINAPE", &["cpc"], vec![apps(&["winape", "WinApe.exe"])], None, &[], &["WinApe"]),
        def("caprice", "Caprice32", "CAPRICE", &["cpc"], vec![], None, &[], &["cap32"]),
        def("stella", "Stella", "STELLA", &["vcs"], vec![], Some("stella"), &["Stella.exe", "stella.exe", "Stella64.exe"], &["Stella", "stella"]),
        def("mesen", "Mesen", "MESEN", &["nes", "snes"], vec![], Some("mesen"), &["Mesen.exe", "Mesen2.exe"], &["Mesen", "mesen"]),
        def("fceux", "FCEUX", "FCEUX", &["nes"], vec![], Some("fceux"), &["fceux64.exe", "fceux.exe", "qfceux.exe"], &["fceux64", "fceux", "qfceux"]),
        def("nestopia", "Nestopia", "NESTOPIA", &["nes"], vec![], Some("nestopia"), &["nestopia.exe"], &["nestopia"]),
        def("bsnes", "bsnes", "BSNES", &["snes"], vec![], Some("bsnes"), &["bsnes.exe"], &["bsnes"]),
        def("snes9x", "Snes9x", "SNES9X", &["snes"], vec![], Some("snes9x"), &["snes9x-x64.exe", "snes9x.exe"], &["snes9x-x64", "snes9x"]),
        def("eightyone", "EightyOne", "EIGHTYONE", &["zx81"], vec![], Some("eightyone"), &["EightyOne.exe"], &["EightyOne"]),
        def("zesarux", "ZEsarUX", "ZESARUX", &["zx81"], vec![], Some("zesarux"), &["zesarux.exe"], &["zesarux"]),
        def("trs80gp", "trs80gp", "TRS80GP", &["trs80"], vec![apps(&["trs80gp.exe"])], Some("trs80gp"), &["trs80gp.exe"], &["trs80gp"]),
        def("beebem", "BeebEm", "BEEBEM", &["bbc"], vec![], Some("beebem"), &["BeebEm.exe"], &["BeebEm", "beebem"]),
        def("b2", "b2", "B2", &["bbc"], vec![], Some("b2"), &["b2.exe", "b2_Debug.exe"], &["b2"]),
        def("xemu", "Xemu", "XEMU", &["mega65"], vec![], Some("xemu"), &["xmega65.exe", "xmega65", "xemu-xmega65"], &["xmega65", "xemu-xmega65"]),
        def("winuae", "WinUAE", "WINUAE", &["amiga"], vec![], Some("winuae"), &["winuae64.exe", "winuae.exe"], &["winuae64", "winuae"]),
        def("fsuae", "FS-UAE", "FSUAE", &["amiga"], vec![], Some("fs-uae"), &["fs-uae.exe", "fs-uae"], &["fs-uae"]),
        def("mame", "MAME", "MAME", &["cpc", "vcs", "nes", "snes", "trs80", "bbc"], vec![apps(&["mame", "mame64.exe"]), apps(&["mame", "mame.exe"])], None, &[], &["mame64", "mame"]),
    ]
}

/// The order of the emulators of a platform (after the one chosen in the settings).
pub fn preferred_order(platform: &str) -> &'static [&'static str] {
    match platform {
        "atari" => &["altirra", "atari800"],
        "cpc" => &["winape", "caprice", "mame"],
        "vcs" => &["stella", "mame"],
        "nes" => &["mesen", "fceux", "nestopia", "mame"],
        "snes" => &["mesen", "bsnes", "snes9x", "mame"],
        "zx81" => &["eightyone", "zesarux"],
        "trs80" => &["trs80gp", "mame"],
        "bbc" => &["beebem", "b2", "mame"],
        "mega65" => &["xemu"],
        "amiga" => &["winuae", "fsuae"],
        _ => &[],
    }
}
