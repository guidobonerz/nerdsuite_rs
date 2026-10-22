//! A project: a folder with `nerdsuite.json` and its files.
//!
//! ```text
//! nerdsuite.json      the settings: the machine, the main program, the dialect of each BASIC file, the CPU
//! assets.json         the graphics model of the editors
//! src/                the sources: .asm .a .s .inc .bas
//! gfx/ audio/         screens, charsets, sprites, tiles, pictures, songs
//! ```

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::platform::{self, Platform};

pub const SETTINGS_FILE: &str = "nerdsuite.json";

/// `nerdsuite.json`. Keys this version does not know are kept as they are, so saving the
/// settings does not lose what nerdsuite2 wrote there (the marks of the Disassembler …).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    #[serde(default = "default_machine")]
    pub machine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dialects: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembler: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_machine() -> String {
    "c64".into()
}

/// What a file is, by its extension, and so the view that opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Assembler,
    Basic,
    Json,
    Text,
    Screen,
    Charset,
    Sprite,
    Tiles,
    Bitmap,
    Song,
    Binary,
}

impl FileKind {
    pub fn of(path: &Path) -> FileKind {
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ext.as_str() {
            "asm" | "a" | "s" | "inc" | "a65" => FileKind::Assembler,
            "bas" => FileKind::Basic,
            "json" => FileKind::Json,
            "txt" | "md" | "cfg" => FileKind::Text,
            "scr" => FileKind::Screen,
            "chr" => FileKind::Charset,
            "spr" => FileKind::Sprite,
            "til" => FileKind::Tiles,
            "kla" | "art" => FileKind::Bitmap,
            "sng" => FileKind::Song,
            _ => FileKind::Binary,
        }
    }

    /// Opened as text in the Code view.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            FileKind::Assembler | FileKind::Basic | FileKind::Json | FileKind::Text
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The path from the project folder, with `/` on every system.
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub settings: Settings,
    /// The tree of the folder, depth first, folders before files.
    pub files: Vec<FileEntry>,
}

impl Project {
    pub fn open(root: impl Into<PathBuf>) -> Result<Project> {
        let root = root.into();
        anyhow::ensure!(root.is_dir(), "{} is not a folder", root.display());
        let settings_path = root.join(SETTINGS_FILE);
        let settings = if settings_path.exists() {
            let json = fs::read_to_string(&settings_path)
                .with_context(|| format!("reading {}", settings_path.display()))?;
            serde_json::from_str(&json)
                .with_context(|| format!("parsing {}", settings_path.display()))?
        } else {
            Settings {
                machine: default_machine(),
                ..Default::default()
            }
        };
        let mut project = Project {
            root,
            settings,
            files: Vec::new(),
        };
        project.rescan()?;
        Ok(project)
    }

    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    pub fn platform(&self) -> &'static Platform {
        platform::platform_or_default(&self.settings.machine)
    }

    /// The BASIC dialect of a file: its own, else the machine's first.
    pub fn dialect(&self, path: &str) -> Option<&str> {
        self.settings
            .dialects
            .get(path)
            .map(String::as_str)
            .or_else(|| self.platform().basic.first().copied())
    }

    pub fn rescan(&mut self) -> Result<()> {
        self.files.clear();
        scan(&self.root, "", 0, &mut self.files)
    }

    pub fn abs_path(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let abs = self.abs_path(path);
        fs::read(&abs).with_context(|| format!("reading {}", abs.display()))
    }

    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let abs = self.abs_path(path);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&abs, bytes).with_context(|| format!("writing {}", abs.display()))
    }

    pub fn save_settings(&self) -> Result<()> {
        let mut json = serde_json::to_string_pretty(&self.settings)?;
        json.push('\n');
        self.write(SETTINGS_FILE, json.as_bytes())
    }
}

fn is_ignored(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules")
}

fn scan(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<FileEntry>) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().ok()?.is_dir();
            (!is_ignored(&name)).then_some((name, is_dir))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|(a, a_dir), (b, b_dir)| {
        b_dir
            .cmp(a_dir)
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });
    for (name, is_dir) in entries {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        out.push(FileEntry {
            path: path.clone(),
            name: name.clone(),
            depth,
            is_dir,
        });
        if is_dir {
            scan(&dir.join(&name), &path, depth + 1, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo")
    }

    #[test]
    fn opens_the_demo() {
        let project = Project::open(demo()).unwrap();
        assert_eq!(project.settings.machine, "c64");
        assert_eq!(project.settings.main.as_deref(), Some("src/main.asm"));
        assert_eq!(project.dialect("src/hello.bas"), Some("v2"));
        assert_eq!(project.platform().short, "C64");
        let paths = project.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>();
        assert!(paths.contains(&"src/main.asm"));
        // folders before files
        let audio = paths.iter().position(|p| *p == "audio").unwrap();
        let assets = paths.iter().position(|p| *p == "assets.json").unwrap();
        assert!(audio < assets);
    }

    #[test]
    fn unknown_settings_survive() {
        let json = r#"{"machine":"vic20","marks":{"a.prg":[1,2]},"cpu":"6502"}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.machine, "vic20");
        assert!(settings.extra.contains_key("marks"));
        let again: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(settings, again);
    }

    #[test]
    fn kinds() {
        assert_eq!(FileKind::of(Path::new("src/MAIN.ASM")), FileKind::Assembler);
        assert_eq!(FileKind::of(Path::new("gfx/bitmap/x.kla")), FileKind::Bitmap);
        assert_eq!(FileKind::of(Path::new("demo.prg")), FileKind::Binary);
        assert!(FileKind::of(Path::new("hello.bas")).is_text());
    }
}
