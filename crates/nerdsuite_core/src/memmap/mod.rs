//! The addresses of the machines: the registers of their chips, the routines of their ROMs, their
//! vectors and the areas of RAM that matter. The Disassembler names operands by them, the Hex view the
//! address at the cursor, and a new project gets them as `hardware.inc`. As nerdsuite2's `memmap.js`.

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

#[rustfmt::skip]
mod data;

/// An entry of a map: address, name, description, size, name when read, a Z80 I/O port.
type E = (u32, &'static str, &'static str, u32, Option<&'static str>, bool);
/// A field of a register: high bit, low bit, label, its values.
type F = (u8, u8, &'static str, V);
/// The registers of a machine and their fields.
type Table = &'static [(&'static str, &'static [F])];

/// The values of a field.
#[derive(Debug, Clone, Copy)]
enum V {
    /// A one bit field shows its label when set, a wider one its number.
    Plain,
    Map(&'static [(u32, &'static str)]),
    List(&'static [&'static str]),
    /// The color names of the machine.
    Colors,
    /// Each bit one thing: `sprite` gives `sprites 0, 3`, `BG1, BG2` names the bits.
    Mask,
    /// The value times a step as `$xxxx`, and a suffix.
    Hex(u32, &'static str),
}

/// A name of the map. `size > 1` is an area, named with an offset (`SCREEN_RAM+40`); `port` a Z80 I/O
/// port, in the include file only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sym {
    pub addr: u32,
    pub name: &'static str,
    pub desc: &'static str,
    pub size: u32,
    /// The name of the register when it is read (the TIA, GTIA, POKEY).
    pub read: Option<&'static str>,
    pub port: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymHit<'a> {
    pub name: &'a str,
    pub desc: &'a str,
    pub offset: u32,
    pub sym: &'a Sym,
}

/// The names of a machine.
#[derive(Debug, Default)]
pub struct Symbols {
    pub list: Vec<Sym>,
    exact: HashMap<u32, usize>,
    /// The areas, the smallest first.
    areas: Vec<usize>,
}

impl Symbols {
    fn new(entries: &[E]) -> Symbols {
        let list: Vec<Sym> = entries
            .iter()
            .map(|&(addr, name, desc, size, read, port)| Sym { addr, name, desc, size, read, port })
            .collect();
        let mut exact: HashMap<u32, usize> = HashMap::new();
        let mut areas = Vec::new();
        for (ix, s) in list.iter().enumerate() {
            if s.port {
                continue;
            }
            if s.size > 1 {
                areas.push(ix);
            }
            // a register wins over an area
            match exact.get(&s.addr) {
                Some(&old) if list[old].size <= s.size => {}
                _ => {
                    exact.insert(s.addr, ix);
                }
            }
        }
        areas.sort_by_key(|&ix| list[ix].size);
        Symbols { list, exact, areas }
    }

    /// The name of an address: the entry at it (`read` picks the read name), else the smallest area
    /// that holds it.
    pub fn at(&self, addr: u32, read: bool) -> Option<SymHit<'_>> {
        if let Some(&ix) = self.exact.get(&addr) {
            let s = &self.list[ix];
            let name = if read { s.read.unwrap_or(s.name) } else { s.name };
            return Some(SymHit { name, desc: s.desc, offset: 0, sym: s });
        }
        self.areas.iter().map(|&ix| &self.list[ix]).find(|a| addr > a.addr && addr < a.addr + a.size).map(|a| SymHit {
            name: a.name,
            desc: a.desc,
            offset: addr - a.addr,
            sym: a,
        })
    }
}

/// The machine a model is like (the 130 XE: the 800 XL), as `platformKin`.
fn kin(machine: &str) -> &str {
    match machine {
        "atari65xe" | "atari130xe" | "atari800" => "atari8",
        "ts1000" => "zx81",
        _ => machine,
    }
}

const MAPS: usize = data::MAPS.len();
static CACHE: [OnceLock<Symbols>; MAPS] = [const { OnceLock::new() }; MAPS];
static EMPTY: LazyLock<Symbols> = LazyLock::new(Symbols::default);

/// The names of a machine, none for one without a map.
pub fn symbols_for(machine: &str) -> &'static Symbols {
    let kin = kin(machine);
    match data::MAPS.iter().position(|(id, _)| *id == kin) {
        Some(ix) => CACHE[ix].get_or_init(|| Symbols::new(data::MAPS[ix].1)),
        None => &EMPTY,
    }
}

pub fn has_memory_map(machine: &str) -> bool {
    let kin = kin(machine);
    data::MAPS.iter().any(|(id, _)| *id == kin)
}

/// What comes before the first `:` of a description: its group.
fn group(desc: &str) -> &str {
    desc.split(':').next().unwrap_or(desc)
}

/// The include file of a machine: every name as an ACME constant, by group.
pub fn include_source(machine: &str, title: &str) -> String {
    let mut sorted: Vec<&Sym> = symbols_for(machine).list.iter().collect();
    sorted.sort_by_key(|s| s.addr);
    let mut out = vec![
        format!("; hardware.inc - the addresses of the {title}: chips, ROM routines, vectors, RAM areas"),
        "; made by nerdsuite - include it with !source \"hardware.inc\"".to_string(),
        String::new(),
    ];
    let mut seen = std::collections::HashSet::new();
    let mut current: Option<String> = None;
    for s in sorted {
        let read = s.read.map(|r| (r, format!("{}: read at {}", group(s.desc), s.name)));
        for (name, desc) in std::iter::once((s.name, s.desc.to_string())).chain(read) {
            if !seen.insert(name) {
                continue;
            }
            let g = group(&desc);
            if current.as_deref() != Some(g) && desc.contains(':') {
                out.push(String::new());
                out.push(format!("; {g}"));
                current = Some(g.to_string());
            }
            let width = if s.addr < 0x100 && !s.port { 2 } else { 4 };
            let what = match desc.find(':') {
                Some(ix) => desc[ix + 1..].trim_start(),
                None => &desc,
            };
            let size = if s.size > 1 { format!(" ({} bytes)", s.size) } else { String::new() };
            out.push(format!("{name:<20}= ${:0width$x}    ; {what}{size}", s.addr));
            if name == s.name {
                for (r#where, what) in describe_bits(machine, name) {
                    out.push(format!("{:28};   {where:<9} {what}", ""));
                }
            }
        }
    }
    out.join("\n") + "\n"
}

fn bits_of(machine: &str, name: &str) -> Option<&'static [F]> {
    let kin = kin(machine);
    let (_, table) = data::BITS.iter().find(|(id, _)| *id == kin)?;
    table.iter().find(|(n, _)| *n == name).map(|&(_, fields)| fields)
}

fn colors(machine: &str) -> &'static [&'static str] {
    data::COLOR_NAMES.iter().find(|(id, _)| *id == machine).map_or(data::C64_COLORS, |&(_, c)| c)
}

/// A value of a register in words: `y scroll 3 · rows 25 · screen on`; `None` for a register without fields.
pub fn decode_bits(machine: &str, name: &str, value: u32) -> Option<String> {
    let fields = bits_of(machine, name)?;
    let mut parts = Vec::new();
    for &(hi, lo, label, values) in fields {
        let v = (value >> lo) & ((1 << (hi - lo + 1)) - 1);
        match values {
            V::Mask => {
                let set: Vec<u32> = (0..=(hi - lo) as u32).filter(|k| v >> k & 1 != 0).collect();
                let names: Vec<&str> = label.split(", ").collect();
                parts.push(if names.len() > 1 {
                    if set.is_empty() {
                        "none".to_string()
                    } else {
                        set.iter().map(|&k| names.get(k as usize).copied().unwrap_or("undefined")).collect::<Vec<_>>().join(", ")
                    }
                } else if set.is_empty() {
                    format!("{label}s none")
                } else {
                    format!("{label}s {}", set.iter().map(u32::to_string).collect::<Vec<_>>().join(", "))
                });
            }
            V::Colors => parts.push(match colors(machine).get(v as usize) {
                Some(c) => format!("{label} {c}"),
                None => format!("{label} {v}"),
            }),
            V::Hex(step, suffix) => parts.push(format!("{label} ${:04x}{suffix}", v * step)),
            V::Map(map) => parts.push(match map.iter().find(|(k, _)| *k == v) {
                Some((_, t)) => format!("{label} {t}"),
                None => format!("{label} {v}"),
            }),
            V::List(list) => parts.push(match list.get(v as usize) {
                Some(t) => format!("{label} {t}"),
                None => format!("{label} {v}"),
            }),
            V::Plain if hi == lo => {
                if v != 0 {
                    parts.push(label.to_string());
                }
            }
            V::Plain => parts.push(format!("{label} {v}")),
        }
    }
    Some(if parts.is_empty() { "all bits clear".to_string() } else { parts.join(" · ") })
}

/// The fields of a register as rows: (`bits 2-0`, `y scroll`).
pub fn describe_bits(machine: &str, name: &str) -> Vec<(String, String)> {
    bits_of(machine, name)
        .unwrap_or(&[])
        .iter()
        .map(|&(hi, lo, label, values)| {
            let what = match values {
                V::Colors => format!("{label}: a color"),
                V::Mask => format!("{label}: a bit each"),
                V::List(list) => {
                    format!("{label}: {}", list.iter().enumerate().map(|(k, t)| format!("{k} {t}")).collect::<Vec<_>>().join(", "))
                }
                V::Map(map) => format!("{label}: {}", map.iter().map(|(k, t)| format!("{k} {t}")).collect::<Vec<_>>().join(", ")),
                V::Plain | V::Hex(..) => label.to_string(),
            };
            let r#where = if hi == lo { format!("bit {hi}") } else { format!("bits {hi}-{lo}") };
            (r#where, what)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c64_registers_and_their_bits() {
        let s = symbols_for("c64");
        assert_eq!(s.at(0xd011, false).unwrap().name, "VIC_CTRL1");
        assert_eq!(s.at(0xffd2, false).unwrap().name, "CHROUT");
        assert_eq!(decode_bits("c64", "VIC_CTRL1", 0x1b).unwrap(), "y scroll 3 · rows 25 · screen on");
        assert_eq!(decode_bits("c64", "VIC_BORDER", 6).unwrap(), "color blue");
        assert_eq!(decode_bits("c64", "SID_V1_CTRL", 0x41).unwrap(), "gate · pulse");
        assert_eq!(decode_bits("atari8", "DMACTL", 0x22).unwrap(), "playfield normal · display list DMA");
        assert_eq!(decode_bits("c64", "VIC_MEMORY", 0x15).unwrap(), "chars / bitmap at $1000 in the bank · screen at $0400 in the bank");
        assert_eq!(decode_bits("c64", "VIC_SPR_ENA", 0x09).unwrap(), "sprites 0, 3");
        assert_eq!(decode_bits("c64", "VIC_IRQ_MASK", 0).unwrap(), "all bits clear");
        assert_eq!(decode_bits("c64", "VIC_RASTER", 0), None);
    }

    #[test]
    fn a_register_wins_over_an_area() {
        let s = symbols_for("c64");
        assert_eq!(s.at(0xa000, false).unwrap().name, "BASIC_COLD");
        assert_eq!(s.at(0xa001, false).unwrap().offset, 1);
        let hit = s.at(0x0428, false).unwrap();
        assert_eq!((hit.name, hit.offset), ("SCREEN_RAM", 0x28));
        assert_eq!(s.at(0x07f9, false).unwrap().name, "SPRITE_PTRS");
    }

    #[test]
    fn vcs_read_names() {
        let s = symbols_for("vcs");
        assert_eq!(s.at(0x02, false).unwrap().name, "WSYNC");
        assert_eq!(s.at(0x0c, true).unwrap().name, "INPT4");
        assert_eq!(s.at(0x09, false).unwrap().name, "COLUBK");
        assert_eq!(s.at(0x09, true).unwrap().name, "INPT1");
        assert_eq!(s.at(0x0e, true).unwrap().name, "PF1");
    }

    #[test]
    fn snes_bits() {
        assert!(decode_bits("snes", "INIDISP", 0x80).unwrap().contains("forced blank"));
        assert_eq!(decode_bits("snes", "TM", 0x11).unwrap(), "BG1, sprites");
        assert_eq!(decode_bits("snes", "BG1SC", 0x05).unwrap(), "size 64x32 · tilemap at word $0400");
    }

    #[test]
    fn every_machine_has_a_map() {
        for p in crate::platform::PLATFORMS {
            assert!(has_memory_map(p.id), "{}", p.id);
            if p.id != "pet" {
                assert!(symbols_for(p.id).list.len() > 20, "{}", p.id);
            }
            assert!(include_source(p.id, p.name).starts_with("; hardware.inc - the addresses of the "));
        }
        assert!(!has_memory_map("spectrum"));
        assert!(symbols_for("spectrum").list.is_empty());
    }

    #[test]
    fn models_are_like_their_machine() {
        for (id, kin) in [("atari65xe", "atari8"), ("atari130xe", "atari8"), ("atari800", "atari8"), ("ts1000", "zx81")] {
            assert_eq!(symbols_for(id).list.len(), symbols_for(kin).list.len(), "{id}");
        }
    }

    #[test]
    fn mega65_map() {
        let s = symbols_for("mega65");
        assert_eq!(s.at(0xd020, false).unwrap().name, "VIC_BORDER");
        assert_eq!(s.at(0xd031, false).unwrap().name, "VIC3_CTRLB");
        assert_eq!(s.at(0xd700, false).unwrap().name, "DMA_ADDRLSB");
        let sid2 = s.at(0xd425, false).unwrap();
        assert_eq!((sid2.name, sid2.offset), ("SID2", 5));
        assert_eq!(s.at(0x2001, false).unwrap().name, "BASIC_START");
        let text = decode_bits("mega65", "VIC3_CTRLB", 0xc0).unwrap();
        assert!(text.find("fast CPU").unwrap() < text.find("80 columns").unwrap(), "{text}");
    }

    #[test]
    fn zx81_trs80_bbc_maps() {
        assert_eq!(symbols_for("zx81").at(0x4029, false).unwrap().name, "NXTLIN");
        assert_eq!(symbols_for("trs80").at(0x3c00 + 70, false).unwrap().name, "SCREEN_RAM");
        assert_eq!(symbols_for("bbcb").at(0xffee, false).unwrap().name, "OSWRCH");
    }

    #[test]
    fn include_file() {
        let src = include_source("c64", "Commodore 64");
        assert!(src.contains("\n; VIC-II\nVIC_SPR0_X          = $d000    ; sprite 0 x position\n"), "{src}");
        assert!(src.contains("SCREEN_RAM          = $0400    ; the screen (default) (1000 bytes)\n"));
        assert!(src.contains("VIC_CTRL1           = $d011    ; control 1"));
        assert!(src.contains("\n                            ;   bits 2-0  y scroll\n"));
        let vcs = include_source("vcs", "Atari 2600");
        assert!(vcs.contains("INPT4               = $0c    ; read at REFP1\n"), "{vcs}");
        let cpc = include_source("cpc464", "CPC 464");
        assert!(cpc.contains("GATE_ARRAY          = $7f00    ; Gate Array - pens, colors, mode, ROMs\n"));
        assert_eq!(describe_bits("c64", "CIA2_PRA")[0], ("bits 1-0".to_string(), "VIC bank: 0 $c000, 1 $8000, 2 $4000, 3 $0000".to_string()));
    }
}
