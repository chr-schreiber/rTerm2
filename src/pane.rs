use std::borrow::Cow;
use std::sync::Arc;

use alacritty_terminal::event::{Event as TermEvent, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term};
use alacritty_terminal::tty;
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);

/// Leitet Ereignisse des Terminal-Kerns (aus dem PTY-Thread) an die winit-Eventloop weiter.
#[derive(Clone)]
pub struct EventProxy {
    proxy: EventLoopProxy<UserEvent>,
    pane: PaneId,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: TermEvent) {
        let ev = match event {
            TermEvent::Wakeup => UserEvent::Wakeup(self.pane),
            TermEvent::PtyWrite(text) => UserEvent::PtyWrite(self.pane, text),
            TermEvent::Title(title) => UserEvent::Title(self.pane, title),
            TermEvent::ResetTitle => UserEvent::Title(self.pane, crate::config::APP_NAME.into()),
            TermEvent::Exit | TermEvent::ChildExit(_) => UserEvent::Exit(self.pane),
            TermEvent::Bell => UserEvent::Bell(self.pane),
            _ => return,
        };
        let _ = self.proxy.send_event(ev);
    }
}

/// Grid-Größe in Zellen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Ein Terminal = ein PTY + Terminalzustand. Später Blatt im Split-Baum.
pub struct Pane {
    pub id: PaneId,
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    notifier: Notifier,
    size: WindowSize,
    /// Zuletzt per OSC gesetzter Fenstertitel
    pub title: String,
    /// PID der Shell, über die das Vordergrund-Programm ermittelt wird
    shell_pid: u32,
    /// Name des Programms im Vordergrund (zsh, nvim, htop …), leer = unbekannt
    pub process: String,
}

impl Pane {
    pub fn spawn(
        id: PaneId,
        proxy: EventLoopProxy<UserEvent>,
        size: WindowSize,
        shell: Option<&str>,
    ) -> anyhow::Result<Self> {
        let listener = EventProxy { proxy, pane: id };
        let grid = GridSize {
            cols: size.num_cols as usize,
            lines: size.num_lines as usize,
        };
        let term = Term::new(term::Config::default(), &grid, listener.clone());
        let term = Arc::new(FairMutex::new(term));

        let mut options = tty::Options::default();
        if let Some(shell) = shell {
            let mut parts = shell.split_whitespace().map(String::from);
            if let Some(program) = parts.next() {
                options.shell = Some(tty::Shell::new(program, parts.collect()));
            }
        }

        let pty = tty::new(&options, size, id.0)?;
        let shell_pid = pty.child().id();
        let event_loop = PtyEventLoop::new(Arc::clone(&term), listener, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();

        Ok(Self {
            id,
            term,
            notifier,
            size,
            title: crate::config::APP_NAME.into(),
            shell_pid,
            process: crate::procname::foreground_name(shell_pid).unwrap_or_default(),
        })
    }

    /// Vordergrund-Programm neu bestimmen. `true`, wenn es sich geändert hat.
    pub fn refresh_process(&mut self) -> bool {
        match crate::procname::foreground_name(self.shell_pid) {
            Some(name) if name != self.process => {
                self.process = name;
                true
            }
            _ => false,
        }
    }

    /// Beschriftung im Tab: das laufende Programm (wie iTerm2), sonst der OSC-Titel.
    pub fn tab_label(&self) -> &str {
        if self.process.is_empty() {
            &self.title
        } else {
            &self.process
        }
    }

    /// Fenstertitel: ein vom Programm per OSC gesetzter Titel, sonst das laufende Programm.
    pub fn window_title(&self) -> &str {
        if self.title != crate::config::APP_NAME {
            &self.title
        } else {
            self.tab_label()
        }
    }

    pub fn size(&self) -> WindowSize {
        self.size
    }

    pub fn write<B: Into<Cow<'static, [u8]>>>(&self, bytes: B) {
        self.notifier.notify(bytes);
    }

    pub fn resize(&mut self, size: WindowSize) {
        if size.num_cols == self.size.num_cols
            && size.num_lines == self.size.num_lines
            && size.cell_width == self.size.cell_width
            && size.cell_height == self.size.cell_height
        {
            return;
        }
        self.size = size;
        self.notifier.on_resize(size);
        self.term.lock().resize(GridSize {
            cols: size.num_cols as usize,
            lines: size.num_lines as usize,
        });
    }
}

impl Drop for Pane {
    /// Beendet die PTY-Eventloop; beim Drop des PTY bekommt die Shell SIGHUP.
    fn drop(&mut self) {
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}
