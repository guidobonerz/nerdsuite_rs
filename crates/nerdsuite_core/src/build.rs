//! The build of a program, as nerdsuite2's `nsbuild.js`: an assembler source with the ACME
//! compatible assembler, a BASIC program with the tokenizer of its dialect.

use std::collections::HashMap;

use crate::{
    acme::{self, LineInfo, Symbol},
    basic::{self, TokenizeOptions},
    platform::Format,
    project::{FileKind, Project},
};

/// The folder of the project the build writes to.
pub const BUILD_DIR: &str = "build";

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub msg: String,
    pub warning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Assembler,
    Basic,
}

#[derive(Debug, Clone)]
pub struct BuildFile {
    pub name: String,
    pub bytes: Vec<u8>,
    pub format: String,
}

#[derive(Debug, Clone)]
pub struct Build {
    pub ok: bool,
    pub kind: Kind,
    pub main: String,
    /// the name of the program, without extension
    pub name: String,
    pub file: Option<BuildFile>,
    /// the program as VICE loads it: a .prg with its load address
    pub prg: Option<Vec<u8>>,
    pub start: Option<u32>,
    pub exec: Option<u32>,
    pub problems: Vec<Problem>,
    pub symbols: Vec<Symbol>,
    pub lines: Vec<LineInfo>,
    pub vice_labels: String,
    pub listing: String,
}

impl Build {
    fn failed(kind: Kind, main: &str, msg: String) -> Build {
        Build {
            ok: false,
            kind,
            main: main.to_owned(),
            name: base_name(main),
            file: None,
            prg: None,
            start: None,
            exec: None,
            problems: vec![Problem { file: Some(main.to_owned()), line: None, col: None, msg, warning: false }],
            symbols: Vec::new(),
            lines: Vec::new(),
            vice_labels: String::new(),
            listing: String::new(),
        }
    }

    pub fn errors(&self) -> impl Iterator<Item = &Problem> {
        self.problems.iter().filter(|p| !p.warning)
    }

    /// The files the build writes: the program, and the label list and listing of an assembly.
    pub fn outputs(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some(file) = &self.file {
            out.push((file.name.clone(), file.bytes.clone()));
        }
        if !self.vice_labels.is_empty() {
            out.push((format!("{}.lbl", self.name), self.vice_labels.clone().into_bytes()));
        }
        if !self.listing.is_empty() {
            out.push((format!("{}.lst", self.name), self.listing.clone().into_bytes()));
        }
        out
    }
}

fn base_name(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rfind('.') {
        Some(dot) if dot > 0 => file[..dot].to_owned(),
        _ => file.to_owned(),
    }
}

pub fn format_id(format: Format) -> &'static str {
    match format {
        Format::Cbm => "cbm",
        Format::Xex => "xex",
        Format::Amsdos => "amsdos",
        Format::Plain => "plain",
        Format::Zx81 => "zx81",
        Format::Trs80 => "trs80",
        Format::Adf => "adf",
    }
}

/// Whether a source has a `!to` of its own (then it decides the output format).
fn has_to(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        line.len() >= 3
            && line[..3].eq_ignore_ascii_case("!to")
            && !line[3..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    })
}

/// The files the assembler can read: everything of the project but the build, with the texts of
/// the editors that are not saved yet.
fn assembler_files(project: &Project, over: &HashMap<String, String>) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = project
        .files
        .iter()
        .filter(|f| !f.is_dir && !f.path.starts_with("reference/") && !f.path.starts_with("build/"))
        .filter_map(|f| {
            let bytes = match over.get(&f.path) {
                Some(text) => text.clone().into_bytes(),
                None => project.read(&f.path).ok()?,
            };
            Some((f.path.clone(), bytes))
        })
        .collect();
    for (path, text) in over {
        if !files.iter().any(|(p, _)| p == path) {
            files.push((path.clone(), text.clone().into_bytes()));
        }
    }
    files
}

/// Builds a program of the project; `over` are the texts of the editors.
pub fn build_program(project: &Project, main: &str, over: &HashMap<String, String>) -> Build {
    let kind = if FileKind::of(main.as_ref()) == FileKind::Basic { Kind::Basic } else { Kind::Assembler };
    let text = match over.get(main) {
        Some(text) => text.clone(),
        None => match project.read(main) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => return Build::failed(kind, main, format!("{main} is not in the project")),
        },
    };
    let platform = project.platform();
    let name = base_name(main);
    match kind {
        Kind::Basic => build_basic(project, main, &name, &text),
        Kind::Assembler => {
            let cpu = project.settings.cpu.clone().unwrap_or_else(|| platform.cpu.id().to_owned());
            let opts = acme::Options {
                cpu: Some(cpu),
                format: (!has_to(&text)).then(|| format_id(platform.format).to_owned()),
                ..Default::default()
            };
            let r = acme::assemble(main, assembler_files(project, over), &opts);
            let mut problems: Vec<Problem> = r
                .errors
                .iter()
                .map(|e| Problem { file: Some(e.file.clone()), line: e.line, col: e.col, msg: e.msg.clone(), warning: false })
                .collect();
            problems.extend(r.warnings.iter().map(|e| Problem {
                file: Some(e.file.clone()),
                line: e.line,
                col: e.col,
                msg: e.msg.clone(),
                warning: true,
            }));
            let out_name = match &r.output_name {
                Some(n) => n.rsplit('/').next().unwrap().to_owned(),
                None => format!("{name}.{}", if r.format == "cbm" { "prg" } else { platform.ext }),
            };
            // the program as VICE loads it: a cbm file has its load address, a plain one gets it
            let prg = match (&r.output, r.format.as_str()) {
                (Some(out), "cbm") => Some(out.clone()),
                (Some(out), "plain") if platform.commodore && platform.id != "mega65" => {
                    let mut prg = vec![r.start as u8, (r.start >> 8) as u8];
                    prg.extend(out);
                    Some(prg)
                }
                _ => None,
            };
            Build {
                ok: r.ok && r.output.is_some(),
                kind,
                main: main.to_owned(),
                name: base_name(&out_name),
                file: r.output.map(|bytes| BuildFile { name: out_name, bytes, format: r.format.clone() }),
                prg,
                start: Some(r.start),
                exec: r.exec.or(Some(r.start)),
                problems,
                symbols: r.symbols,
                lines: r.lines,
                vice_labels: r.vice_labels,
                listing: r.listing,
            }
        }
    }
}

fn build_basic(project: &Project, main: &str, name: &str, text: &str) -> Build {
    let dialect = project
        .settings
        .dialects
        .get(main)
        .filter(|d| basic::dialect(d).is_some())
        .cloned()
        .or_else(|| project.platform().basic.first().map(|d| d.to_string()))
        .unwrap_or_else(|| "v2".into());
    let machine = (project.settings.machine == "vic20").then_some("vic20");
    let opts = TokenizeOptions { dialect: &dialect, machine, ..Default::default() };
    let r = match basic::tokenize(text, &opts) {
        Ok(r) => r,
        Err(e) => return Build::failed(Kind::Basic, main, format!("{e:#}")),
    };
    let problems: Vec<Problem> = r
        .errors
        .iter()
        .map(|e| Problem {
            file: Some(main.to_owned()),
            line: (e.line > 0).then_some(e.line),
            col: None,
            msg: e.msg.clone(),
            warning: false,
        })
        .collect();
    let format = r.format.id();
    let ext = match format {
        "prg" => "prg",
        "atascii-lst" => "lst",
        "cpc-ascii" | "trs80-ascii" | "bbc-ascii" => "bas",
        "zx81-p" => "p",
        _ => "txt",
    };
    let bytes = r.bytes().map(<[u8]>::to_vec);
    Build {
        ok: r.ok && problems.is_empty() && bytes.is_some(),
        kind: Kind::Basic,
        main: main.to_owned(),
        name: name.to_owned(),
        file: bytes.map(|bytes| BuildFile {
            name: format!("{name}.{ext}"),
            bytes,
            format: if r.prg.is_some() { "cbm".into() } else { format.into() },
        }),
        prg: r.prg.clone(),
        start: r.start.map(u32::from),
        exec: None,
        problems,
        symbols: Vec::new(),
        lines: Vec::new(),
        vice_labels: String::new(),
        listing: String::new(),
    }
}

/// The debug info of a build: addresses <-> source lines, and the labels.
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    /// address -> (file, line, size): the first line that made bytes there
    by_addr: HashMap<u32, (String, usize, usize)>,
    /// (file, line) -> the addresses of the line
    by_line: HashMap<(String, usize), Vec<u32>>,
    labels: HashMap<u32, String>,
}

impl DebugInfo {
    pub fn new(build: &Build) -> DebugInfo {
        let mut info = DebugInfo::default();
        for l in build.lines.iter().filter(|l| !l.in_macro) {
            info.by_addr.entry(l.addr).or_insert_with(|| (l.file.clone(), l.line, l.size));
            info.by_line.entry((l.file.clone(), l.line)).or_default().push(l.addr);
        }
        for s in build.symbols.iter().filter(|s| s.is_label) {
            info.labels.entry(s.value as u32).or_insert_with(|| s.name.clone());
        }
        info
    }

    /// The source line of an address: exact, else the statement it lies in.
    pub fn line_of(&self, addr: u32) -> Option<(&str, usize)> {
        if let Some((file, line, _)) = self.by_addr.get(&addr) {
            return Some((file, *line));
        }
        (addr.saturating_sub(3)..addr).rev().find_map(|a| {
            let (file, line, size) = self.by_addr.get(&a)?;
            (a + (*size).max(1) as u32 > addr).then_some((file.as_str(), *line))
        })
    }

    pub fn addrs_of(&self, file: &str, line: usize) -> &[u32] {
        self.by_line.get(&(file.to_owned(), line)).map_or(&[], Vec::as_slice)
    }

    pub fn label(&self, addr: u32) -> Option<&str> {
        self.labels.get(&addr).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn debug_info_of_the_demo() {
        let b = build_program(&demo(), "src/main.asm", &HashMap::new());
        let info = DebugInfo::new(&b);
        // line 21 of main.asm: sei at $0810, the label start
        assert_eq!(info.addrs_of("src/main.asm", 21), [0x0810]);
        assert_eq!(info.line_of(0x0810), Some(("src/main.asm", 21)));
        assert_eq!(info.label(0x0810), Some("start"));
        // the second byte of lda #BLACK (line 22, $0811) lies in its statement
        assert_eq!(info.line_of(0x0812), Some(("src/main.asm", 22)));
        assert!(info.addrs_of("src/main.asm", 1).is_empty());
    }

    use super::*;

    fn demo() -> Project {
        Project::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo")).unwrap()
    }

    #[test]
    fn builds_the_demo_assembler_program() {
        let b = build_program(&demo(), "src/main.asm", &HashMap::new());
        assert!(b.ok, "{:?}", b.problems);
        let file = b.file.as_ref().unwrap();
        assert_eq!(file.name, "demo.prg");
        assert_eq!(file.bytes[..2], [0x01, 0x08]);
        assert_eq!(b.prg.as_deref(), Some(&file.bytes[..]));
        assert_eq!(b.start, Some(0x0801));
        assert!(b.vice_labels.contains(".start"));
        let names: Vec<_> = b.outputs().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["demo.prg", "demo.lbl", "demo.lst"]);
    }

    #[test]
    fn builds_the_demo_basic_program() {
        let b = build_program(&demo(), "src/hello.bas", &HashMap::new());
        assert!(b.ok, "{:?}", b.problems);
        let file = b.file.unwrap();
        assert_eq!(file.name, "hello.prg");
        assert_eq!(file.bytes[..2], [0x01, 0x08]);
    }

    #[test]
    fn unsaved_texts_and_errors() {
        let over = HashMap::from([("src/main.asm".to_owned(), "*= $1000\n lda nothing\n".to_owned())]);
        let b = build_program(&demo(), "src/main.asm", &over);
        assert!(!b.ok);
        let e = b.errors().next().unwrap();
        assert_eq!((e.file.as_deref(), e.line), (Some("src/main.asm"), Some(2)));
        assert_eq!(e.msg, "Symbol not defined (nothing).");
        let b = build_program(&demo(), "src/nope.asm", &HashMap::new());
        assert!(!b.ok);
    }

    #[test]
    fn to_decides_the_format() {
        assert!(has_to("; x\n  !to \"a.prg\", cbm\n"));
        assert!(!has_to("!token\n"));
    }
}
