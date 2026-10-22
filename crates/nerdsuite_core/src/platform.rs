//! The machines nerdsuite makes programs for: their CPU, their BASIC, the file the assembler
//! writes and where a program starts. Ported from `public/js/platforms.js` of nerdsuite2.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Commodore,
    Amiga,
    Atari,
    Amstrad,
    Nintendo,
    Sinclair,
    Tandy,
    Acorn,
}

impl Family {
    pub const ALL: [Family; 8] = [
        Family::Commodore,
        Family::Amiga,
        Family::Atari,
        Family::Amstrad,
        Family::Nintendo,
        Family::Sinclair,
        Family::Tandy,
        Family::Acorn,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Family::Commodore => "Commodore",
            Family::Amiga => "Commodore Amiga",
            Family::Atari => "Atari",
            Family::Amstrad => "Amstrad / Schneider",
            Family::Nintendo => "Nintendo",
            Family::Sinclair => "Sinclair",
            Family::Tandy => "Tandy / Radio Shack",
            Family::Acorn => "Acorn",
        }
    }
}

/// The output of the assembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// A .prg with its load address.
    Cbm,
    /// An Atari binary load file.
    Xex,
    /// A CPC binary with its AMSDOS header.
    Amsdos,
    /// A cartridge image or raw code: .a26, .nes, .sfc, the BBC's code.
    Plain,
    /// A ZX81 .p file, the code in a REM line.
    Zx81,
    /// A TRS-80 /CMD file.
    Trs80,
    /// An Amiga disk, the code in its boot block.
    Adf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cpu {
    Mos6502,
    Mos6510,
    Wdc65816,
    Csg4510,
    Mega65,
    Z80,
    M68000,
}

impl Cpu {
    pub fn id(self) -> &'static str {
        match self {
            Cpu::Mos6502 => "6502",
            Cpu::Mos6510 => "6510",
            Cpu::Wdc65816 => "65816",
            Cpu::Csg4510 => "4502",
            Cpu::Mega65 => "m65",
            Cpu::Z80 => "z80",
            Cpu::M68000 => "68000",
        }
    }

    pub fn from_id(id: &str) -> Option<Cpu> {
        [
            Cpu::Mos6502,
            Cpu::Mos6510,
            Cpu::Wdc65816,
            Cpu::Csg4510,
            Cpu::Mega65,
            Cpu::Z80,
            Cpu::M68000,
        ]
        .into_iter()
        .find(|cpu| cpu.id().eq_ignore_ascii_case(id))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Platform {
    pub id: &'static str,
    pub family: Family,
    pub name: &'static str,
    pub short: &'static str,
    pub cpu: Cpu,
    /// The BASIC dialects of the machine, the first is the default.
    pub basic: &'static [&'static str],
    pub format: Format,
    pub ext: &'static str,
    /// Where a new assembler program starts.
    pub start: u32,
    /// The BASIC area (a SYS line), if the machine has one for machine code.
    pub basic_start: Option<u16>,
    pub screen: Option<u16>,
    /// The graphics editors (VIC-II and SID) make data for it as it is.
    pub commodore: bool,
    /// What runs it: VICE (its machine and arguments) or another emulator (the launcher's platform).
    pub emulator: Emulator,
    /// The machine and model of mimic that runs and debugs it.
    pub mimic: Option<(&'static str, Option<&'static str>)>,
    /// The model the launcher gives the emulator (CPC 464/6128, TRS-80 Model I/III, BBC).
    pub model: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emulator {
    Vice { machine: &'static str, args: &'static [&'static str] },
    /// one of the launcher's platforms: atari, vcs, cpc, nes, snes, zx81, trs80, bbc, mega65, amiga
    Other(&'static str),
}

macro_rules! platform {
    ($id:literal, $family:ident, $name:literal, $short:literal, $cpu:ident, [$($basic:literal),*],
     $format:ident, $ext:literal, $start:literal, $basic_start:expr, $screen:expr, $commodore:literal,
     $emulator:expr, $mimic:expr, $model:expr) => {
        Platform {
            id: $id,
            family: Family::$family,
            name: $name,
            short: $short,
            cpu: Cpu::$cpu,
            basic: &[$($basic),*],
            format: Format::$format,
            ext: $ext,
            start: $start,
            basic_start: $basic_start,
            screen: $screen,
            commodore: $commodore,
            emulator: $emulator,
            mimic: $mimic,
            model: $model,
        }
    };
}

const fn vice(machine: &'static str, args: &'static [&'static str]) -> Emulator {
    Emulator::Vice { machine, args }
}

pub static PLATFORMS: &[Platform] = &[
    platform!("c64", Commodore, "Commodore 64", "C64", Mos6510, ["v2", "simons"], Cbm, "prg", 0x0810, Some(0x0801), Some(0x0400), true, vice("c64", &[]), Some(("c64", None)), None),
    platform!("c128", Commodore, "Commodore 128", "C128", Mos6502, ["v7"], Cbm, "prg", 0x1c10, Some(0x1c01), Some(0x0400), true, vice("c128", &[]), Some(("c128", None)), None),
    platform!("vic20", Commodore, "VIC-20", "VIC-20", Mos6502, ["v2"], Cbm, "prg", 0x1010, Some(0x1001), Some(0x1e00), true, vice("vic20", &[]), Some(("vic20", None)), None),
    platform!("plus4", Commodore, "Plus/4", "Plus/4", Mos6502, ["v35"], Cbm, "prg", 0x1010, Some(0x1001), Some(0x0c00), true, vice("plus4", &[]), Some(("plus4", None)), None),
    platform!("c16", Commodore, "Commodore 16", "C16", Mos6502, ["v35"], Cbm, "prg", 0x1010, Some(0x1001), Some(0x0c00), true, vice("plus4", &["-model", "c16"]), Some(("c16", None)), None),
    platform!("pet", Commodore, "PET / CBM 4032", "PET", Mos6502, ["v4", "v2"], Cbm, "prg", 0x0410, Some(0x0401), Some(0x8000), true, vice("pet", &[]), Some(("pet", None)), None),
    platform!("mega65", Commodore, "MEGA65", "MEGA65", Mega65, ["v65"], Cbm, "prg", 0x2010, Some(0x2001), Some(0x0800), true, Emulator::Other("mega65"), None, None),
    platform!("amiga500", Amiga, "Amiga 500", "A500", M68000, [], Adf, "adf", 0x40000, None, None, false, Emulator::Other("amiga"), Some(("amiga", Some("500"))), None),
    platform!("atari8", Atari, "Atari 800 XL", "Atari XL", Mos6502, ["atari", "turbo"], Xex, "xex", 0x2000, None, None, false, Emulator::Other("atari"), Some(("atari8", Some("800xl"))), None),
    platform!("atari65xe", Atari, "Atari 65 XE", "Atari 65XE", Mos6502, ["atari", "turbo"], Xex, "xex", 0x2000, None, None, false, Emulator::Other("atari"), Some(("atari8", Some("65xe"))), None),
    platform!("atari130xe", Atari, "Atari 130 XE (128K)", "Atari 130XE", Mos6502, ["atari", "turbo"], Xex, "xex", 0x2000, None, None, false, Emulator::Other("atari"), Some(("atari8", Some("130xe"))), None),
    platform!("atari800", Atari, "Atari 400 / 800 (48K, OS-B)", "Atari 800", Mos6502, ["atari", "turbo"], Xex, "xex", 0x2000, None, None, false, Emulator::Other("atari"), Some(("atari8", Some("800"))), None),
    platform!("vcs", Atari, "Atari 2600 (VCS)", "VCS", Mos6502, [], Plain, "a26", 0xf000, None, None, false, Emulator::Other("vcs"), Some(("atari2600", None)), None),
    platform!("cpc464", Amstrad, "Amstrad / Schneider CPC 464", "CPC 464", Z80, ["loco"], Amsdos, "bin", 0x4000, None, Some(0xc000), false, Emulator::Other("cpc"), Some(("cpc", Some("464"))), Some("464")),
    platform!("cpc6128", Amstrad, "Amstrad / Schneider CPC 6128", "CPC 6128", Z80, ["loco"], Amsdos, "bin", 0x4000, None, Some(0xc000), false, Emulator::Other("cpc"), Some(("cpc", Some("6128"))), Some("6128")),
    platform!("nes", Nintendo, "Nintendo Entertainment System (NES)", "NES", Mos6502, [], Plain, "nes", 0xc000, None, None, false, Emulator::Other("nes"), None, None),
    platform!("snes", Nintendo, "Super Nintendo (SNES)", "SNES", Wdc65816, [], Plain, "sfc", 0x8000, None, None, false, Emulator::Other("snes"), None, None),
    platform!("zx81", Sinclair, "Sinclair ZX81 (16K)", "ZX81", Z80, ["zx81"], Zx81, "p", 0x4082, None, None, false, Emulator::Other("zx81"), Some(("zx81", Some("zx81"))), None),
    platform!("ts1000", Sinclair, "Timex Sinclair 1000 (16K, 60 Hz)", "TS1000", Z80, ["zx81"], Zx81, "p", 0x4082, None, None, false, Emulator::Other("zx81"), Some(("zx81", Some("ts1000"))), None),
    platform!("trs80", Tandy, "TRS-80 Model I (Level II, 48K)", "TRS-80", Z80, ["trs80"], Trs80, "cmd", 0x7000, None, Some(0x3c00), false, Emulator::Other("trs80"), None, Some("1")),
    platform!("trs80m3", Tandy, "TRS-80 Model III (48K)", "TRS-80 M3", Z80, ["trs80"], Trs80, "cmd", 0x7000, None, Some(0x3c00), false, Emulator::Other("trs80"), None, Some("3")),
    platform!("bbcb", Acorn, "BBC Micro Model B", "BBC B", Mos6502, ["bbc"], Plain, "bin", 0x2000, None, Some(0x7c00), false, Emulator::Other("bbc"), None, Some("bbcb")),
];

pub fn platform(id: &str) -> Option<&'static Platform> {
    PLATFORMS.iter().find(|platform| platform.id == id)
}

/// The machine of a project, the C64 when the id is unknown (as nerdsuite2 does).
pub fn platform_or_default(id: &str) -> &'static Platform {
    platform(id).unwrap_or(&PLATFORMS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        for (ix, a) in PLATFORMS.iter().enumerate() {
            assert!(PLATFORMS[ix + 1..].iter().all(|b| b.id != a.id), "{}", a.id);
        }
    }

    #[test]
    fn every_family_has_a_machine() {
        for family in Family::ALL {
            assert!(PLATFORMS.iter().any(|p| p.family == family), "{family:?}");
        }
    }

    #[test]
    fn unknown_machine_is_the_c64() {
        assert_eq!(platform_or_default("spectrum").id, "c64");
        assert_eq!(platform("cpc6128").unwrap().cpu, Cpu::Z80);
    }
}
