//! Debug in the workspace: F6 builds and starts the program in mimic with the breakpoints of the
//! sources, the Debugger view, the line of the CPU in the editor, the keys of nerdsuite2 (F5/F8 on,
//! F8 pause, F10 step over, F11 step into, Shift+F11 step out, Shift+F5 stop).

use std::path::PathBuf;

use gpui::{AnyElement, Context, FontWeight, Window, div, px};
use nerdsuite_core::{
    hex::CharSet,
    platform::Emulator,
    project::FileKind,
    run::{
        debug::{Command, Debugger},
        run_format,
        tools::{ToolSettings, mimic_tool, resolve_exe},
    },
    syntax::Language,
};

use super::*;
use crate::{debug_panel, theme::theme};

impl Workspace {
    pub(super) fn toggle_breakpoint(&mut self, path: &str, line: usize, cx: &mut Context<Self>) {
        let lines = self.breakpoints.entry(path.to_owned()).or_default();
        if !lines.remove(&line) {
            lines.insert(line);
        }
        let lines: Vec<usize> = lines.iter().copied().collect();
        self.code.update(cx, |code, cx| code.set_breakpoints(path, lines, cx));
        if let Some(debugger) = &self.debugger {
            debugger.send(Command::SetBreakpoints(self.breakpoint_addrs()));
        }
        cx.notify();
    }

    /// The breakpoints and the line of the CPU into the editor of a file just opened.
    pub(super) fn sync_editor(&mut self, path: &str, cx: &mut Context<Self>) {
        let lines: Vec<usize> = self.breakpoints.get(path).map(|l| l.iter().copied().collect()).unwrap_or_default();
        let at = self.exec_line();
        self.code.update(cx, |code, cx| {
            code.set_breakpoints(path, lines, cx);
            code.set_exec_line(at.as_ref().map(|(f, l)| (f.as_str(), *l)), cx);
        });
    }

    /// The addresses of the breakpoints: the first of each line that made bytes in the last build.
    fn breakpoint_addrs(&self) -> Vec<u32> {
        let Some(info) = &self.debug_info else { return Vec::new() };
        self.breakpoints
            .iter()
            .flat_map(|(file, lines)| lines.iter().filter_map(move |&line| info.addrs_of(file, line).first().copied()))
            .collect()
    }

    /// The source line the CPU stopped at.
    fn exec_line(&self) -> Option<(String, usize)> {
        let s = self.snapshot.as_ref().filter(|s| s.stopped)?;
        let (file, line) = self.debug_info.as_ref()?.line_of(s.pc?)?;
        Some((file.to_owned(), line))
    }

    pub(super) fn debug(&mut self, _: &Debug, window: &mut Window, cx: &mut Context<Self>) {
        if self.starting {
            return;
        }
        let Some(build) = self.build_now(cx) else { return };
        let Some(project) = self.project.as_ref() else { return };
        let platform = project.platform();
        let error = |this: &mut Self, msg: String, cx: &mut Context<Self>| {
            this.message = Some((msg.into(), true));
            cx.notify();
        };
        let Some((machine, model)) = platform.mimic else {
            return error(self, format!("Debug runs in mimic, which has no {}", platform.name), cx);
        };
        let Some(exe) = resolve_exe(&ToolSettings::load(), &mimic_tool()) else {
            return error(self, "Debug needs mimic: set MIMIC to mimic.exe or its folder".into(), cx);
        };
        let Some(file) = build.file.as_ref() else { return };
        let format = run_format(platform, &file.format);
        if !matches!(format, "cbm" | "xex" | "a26") {
            return error(self, format!("Debug: {format} programs are not ported yet"), cx);
        }
        let data = build.prg.clone().unwrap_or_else(|| file.bytes.clone());
        let path: PathBuf = std::env::temp_dir().join("nerdsuite").join(platform.id).join(&file.name);
        if let Err(e) = std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(&path, &data)) {
            return error(self, format!("{e:#}"), cx);
        }
        if let Ok(mut runner) = self.runner.lock() {
            runner.stop();
        }
        self.view = View::Debugger;
        let breakpoints = self.breakpoint_addrs();
        // a mimic of the same machine takes the new build
        if let Some(debugger) = self.debugger.as_ref().filter(|d| d.machine == machine && d.snapshot().alive) {
            debugger.send(Command::Load { file: path, breakpoints });
            self.message = Some((format!("Debugging {} in mimic ({machine})", file.name).into(), false));
            cx.notify();
            return;
        }
        self.debugger = None;
        self.snapshot = None;
        self.starting = true;
        self.message = Some((format!("Starting mimic ({machine}) for {}…", file.name).into(), false));
        cx.notify();
        let name = file.name.clone();
        let memory = build.start.unwrap_or(0x0801) & 0xff00;
        let task = cx
            .background_executor()
            .spawn(async move { Debugger::start(&exe, machine, model, path, breakpoints, memory) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.starting = false;
                match result {
                    Ok(debugger) => {
                        this.debugger = Some(debugger);
                        this.debug_version = 0;
                        this.message = Some((format!("Debugging {name} in mimic ({machine})").into(), false));
                    }
                    Err(e) => this.message = Some((format!("Debug: {e:#}").into(), true)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// What the debugger saw: the panels, and the line of the CPU in the editor.
    pub(super) fn poll_debugger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(debugger) = &self.debugger else { return };
        let version = debugger.version();
        if version == self.debug_version {
            return;
        }
        self.debug_version = version;
        let snapshot = debugger.snapshot();
        if !snapshot.alive {
            self.debugger = None;
            self.message = Some(("mimic has ended".into(), false));
        }
        self.snapshot = Some(snapshot);
        let at = self.exec_line();
        // the source of the stop opens in the Code view (the view in front stays)
        if let Some((file, _)) = &at
            && self.code.read(cx).editor_for(file, cx).is_none()
            && let Some(project) = &self.project
            && let Ok(bytes) = project.read(file)
        {
            let kind = FileKind::of(file.as_ref());
            let file = file.clone();
            self.code.update(cx, |code, cx| {
                code.open(&file, Language::of(kind), || String::from_utf8_lossy(&bytes).into_owned(), window, cx)
            });
            let lines: Vec<usize> = self.breakpoints.get(&file).map(|l| l.iter().copied().collect()).unwrap_or_default();
            self.code.update(cx, |code, cx| code.set_breakpoints(&file, lines, cx));
        }
        self.code.update(cx, |code, cx| code.set_exec_line(at.as_ref().map(|(f, l)| (f.as_str(), *l)), cx));
        cx.notify();
    }

    fn send(&self, command: Command) {
        if let Some(debugger) = &self.debugger {
            debugger.send(command);
        }
    }

    fn stopped(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| s.stopped)
    }

    pub(super) fn debug_continue(&mut self, _: &DebugContinue, _: &mut Window, _: &mut Context<Self>) {
        self.send(Command::Continue);
    }

    pub(super) fn debug_pause_or_continue(&mut self, _: &DebugPauseOrContinue, _: &mut Window, _: &mut Context<Self>) {
        self.send(if self.stopped() { Command::Continue } else { Command::Pause });
    }

    pub(super) fn step_over(&mut self, _: &StepOver, _: &mut Window, _: &mut Context<Self>) {
        self.send(Command::StepOver);
    }

    pub(super) fn step_into(&mut self, _: &StepInto, _: &mut Window, _: &mut Context<Self>) {
        self.send(Command::StepInto);
    }

    pub(super) fn step_out(&mut self, _: &StepOut, _: &mut Window, _: &mut Context<Self>) {
        self.send(Command::StepOut);
    }

    pub(super) fn stop_debugging(&mut self, _: &StopDebugging, _: &mut Window, cx: &mut Context<Self>) {
        if self.debugger.take().is_some() {
            self.snapshot = None;
            self.code.update(cx, |code, cx| code.set_exec_line(None, cx));
            self.message = Some(("Debugging stopped".into(), false));
            cx.notify();
        }
    }

    pub(super) fn render_debugger(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *theme(cx);
        let (Some(_), Some(s)) = (&self.debugger, &self.snapshot) else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(theme.on_surface_variant)
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).text_color(theme.on_surface).child("Debugger"))
                .child(if self.starting { "Starting mimic…" } else { "Debug (F6) builds the program and starts it in mimic." })
                .child("Click a line number in the editor for a breakpoint.")
                .child(
                    button("debug-start", "Debug", theme)
                        .on_click(cx.listener(|this, _, window, cx| this.debug(&Debug, window, cx))),
                )
                .into_any_element();
        };
        let project = self.project.as_ref();
        let cpu = project
            .and_then(|p| p.settings.cpu.clone())
            .unwrap_or_else(|| project.map_or("6502", |p| p.platform().cpu.id()).to_owned());
        let state = if s.stopped {
            match (s.pc, &s.reason) {
                (Some(pc), Some(reason)) => format!("stopped at ${pc:04x} ({reason})"),
                (Some(pc), None) => format!("stopped at ${pc:04x}"),
                _ => "stopped".into(),
            }
        } else {
            "running".into()
        };
        let stopped = s.stopped;
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(if stopped {
                button("dbg-continue", "Continue (F5)", theme)
                    .on_click(cx.listener(|this, _, w, cx| this.debug_continue(&DebugContinue, w, cx)))
            } else {
                button("dbg-pause", "Pause (F8)", theme)
                    .on_click(cx.listener(|this, _, w, cx| this.debug_pause_or_continue(&DebugPauseOrContinue, w, cx)))
            })
            .child(button("dbg-over", "Step over (F10)", theme).on_click(cx.listener(|this, _, w, cx| this.step_over(&StepOver, w, cx))))
            .child(button("dbg-into", "Step into (F11)", theme).on_click(cx.listener(|this, _, w, cx| this.step_into(&StepInto, w, cx))))
            .child(button("dbg-out", "Step out (⇧F11)", theme).on_click(cx.listener(|this, _, w, cx| this.step_out(&StepOut, w, cx))))
            .child(button("dbg-stop", "Stop (⇧F5)", theme).on_click(cx.listener(|this, _, w, cx| this.stop_debugging(&StopDebugging, w, cx))))
            .child(div().pl_2().text_color(if stopped { theme.warning } else { theme.string }).child(state));

        // the memory: where it shows, in which chars
        let platform = project.map(|p| p.platform());
        let screen = platform.and_then(|p| p.screen);
        let program = self.last_build.as_ref().and_then(|b| b.start);
        let addr = s.memory_addr;
        let mut places: Vec<(&'static str, u32)> = vec![("Zero page", 0), ("Stack", 0x100)];
        if let Some(screen) = screen {
            places.push(("Screen", screen as u32));
        }
        if let Some(program) = program {
            places.push(("Program", program & 0xfff0));
        }
        if let Some(pc) = s.pc {
            places.push(("PC", pc & 0xfff0));
        }
        let memory_bar = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .child(small_button("mem-back", "◀", theme).on_click(cx.listener(move |this, _, _, _| this.send(Command::ShowMemory(addr.saturating_sub(0x100))))))
            .child(small_button("mem-next", "▶", theme).on_click(cx.listener(move |this, _, _, _| this.send(Command::ShowMemory((addr + 0x100) & 0xffff)))))
            .children(places.into_iter().map(|(label, at)| {
                small_button(label, label, theme).on_click(cx.listener(move |this, _, _, _| this.send(Command::ShowMemory(at))))
            }))
            .child(div().w(px(12.)))
            .children(CharSet::ALL.into_iter().map(|charset| {
                small_button(charset.label(), charset.label(), theme)
                    .when(charset == self.memory_charset, |el| el.bg(theme.container_highest))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.memory_charset = charset;
                        cx.notify();
                    }))
            }));
        let emulator = match platform.map(|p| p.emulator) {
            Some(Emulator::Vice { machine, .. }) | Some(Emulator::Other(machine)) => machine,
            None => "",
        };

        div()
            .id("debugger")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .overflow_y_scroll()
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .w(px(250.))
                            .flex_none()
                            .child(debug_panel::registers(s, theme))
                            .child(debug_panel::stack(s, theme)),
                    )
                    .child(div().flex_1().min_w_0().child(debug_panel::disassembly(s, self.debug_info.as_ref(), &cpu, theme))),
            )
            .child(
                debug_panel::panel(&format!("Memory ${addr:04x} ({emulator})"), theme)
                    .child(memory_bar)
                    .child(debug_panel::memory(s, self.memory_charset, theme)),
            )
            .child(debug_panel::log(s, theme))
            .into_any_element()
    }
}

fn small_button(id: &'static str, label: &'static str, theme: crate::theme::Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(theme.outline)
        .cursor_pointer()
        .text_xs()
        .hover(|el| el.bg(theme.container))
        .child(label)
}
