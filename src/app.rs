use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Point;
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::Match;
use alacritty_terminal::term::TermMode;
use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::config::{Config, APP_NAME};
use crate::copymode::{CopyState, Outcome};
use crate::font::FontAtlas;
use crate::gpu::{GpuContext, GpuRenderer};
use crate::input;
use crate::keymap::{Action, Keymap};
use crate::layout::{Divider, SplitDir};
use crate::mouse;
use crate::overlay;
use crate::pane::{Pane, PaneId};
use crate::persist;
use crate::picker::ThemePicker;
use crate::poll::{self, PollSchedule};
use crate::renderer::{self, Canvas, Frame, Highlights, Rect};
use crate::tab::Tab;
use crate::tabbar;
use crate::theme::{Theme, ThemeLibrary};

/// Ereignisse aus den PTY-Threads an die UI.
#[derive(Debug)]
pub enum UserEvent {
    Wakeup(PaneId),
    PtyWrite(PaneId, String),
    Title(PaneId, String),
    Bell(PaneId),
    Exit(PaneId),
}

/// Deckkraft, mit der inaktive Panes Richtung Hintergrund abgedunkelt werden (0 = aus).
const INACTIVE_DIM: u8 = 70;

type Surface = softbuffer::Surface<Arc<Window>, Arc<Window>>;

/// Womit ein Fenster zeichnet.
enum Backend {
    Gpu(Box<GpuRenderer>),
    Cpu {
        _context: softbuffer::Context<Arc<Window>>,
        surface: Surface,
    },
}

/// Alles, was für ein Bild gezeichnet wird – unabhängig vom Backend.
struct Scene<'a> {
    full: Rect,
    area: Rect,
    bar: Option<Rect>,
    titles: &'a [String],
    active: usize,
    panes: &'a [(PaneId, Rect)],
    dividers: &'a [Rect],
    tab: &'a Tab,
    theme: &'a Theme,
    padding: usize,
    picker: Option<(&'a ThemePicker, &'a ThemeLibrary)>,
    copy: Option<CopyView<'a>>,
}

/// Was der Copy-Mode zum Zeichnen braucht.
struct CopyView<'a> {
    pane: PaneId,
    marks: &'a [Match],
    current: Option<&'a Match>,
    /// Offene Suche: (nach oben?, Eingabe)
    prompt: Option<(bool, &'a str)>,
    no_match: bool,
}

/// Statuszeile unten in der Pane: Hilfe im Copy-Mode bzw. die Sucheingabe.
fn draw_copy_bar(
    canvas: &mut dyn Canvas,
    rect: Rect,
    font: &mut FontAtlas,
    theme: &Theme,
    c: &CopyView,
) {
    let h = font.cell_h + 8;
    if rect.h < h * 2 {
        return;
    }
    let bar = Rect {
        x: rect.x,
        y: rect.y + rect.h - h,
        w: rect.w,
        h,
    };
    canvas.fill(
        bar,
        bar.x,
        bar.y,
        bar.w,
        bar.h,
        theme.background.blend(theme.foreground, 40),
    );
    canvas.fill(
        bar,
        bar.x,
        bar.y,
        bar.w,
        1,
        theme.background.blend(theme.foreground, 110),
    );
    let text = match c.prompt {
        Some((backward, query)) => format!(
            "{} {query}_{}",
            if backward { "Suche ↑" } else { "Suche ↓" },
            if c.no_match { "    kein Treffer" } else { "" }
        ),
        None => "KOPIERMODUS   hjkl/wbe bewegen · v V markieren · y kopieren · / ? suchen · n N weiter · Esc Ende"
            .to_string(),
    };
    let max = bar.w.saturating_sub(16) / font.cell_w.max(1);
    let text: String = text.chars().take(max).collect();
    renderer::draw_text(
        canvas,
        bar,
        font,
        &text,
        bar.x + 8,
        bar.y + 4,
        theme.foreground,
    );
}

/// Zeichnet ein Bild auf eine beliebige Zeichenfläche (CPU oder GPU).
fn paint(canvas: &mut dyn Canvas, font: &mut FontAtlas, s: &Scene) {
    canvas.clear(s.theme.background);
    if let Some(bar) = s.bar {
        tabbar::draw(canvas, bar, s.titles, s.active, font, s.theme);
    }

    let multiple = s.panes.len() > 1;
    for (id, rect) in s.panes {
        let Some(pane) = s.tab.panes.get(id) else {
            continue;
        };
        let focused = *id == s.tab.focused;
        let highlights = match &s.copy {
            Some(c) if c.pane == *id => Highlights {
                marks: c.marks,
                current: c.current,
            },
            _ => Highlights::default(),
        };
        {
            let term = pane.term.lock();
            renderer::draw_terminal(
                canvas,
                rect.inset(s.padding),
                &term,
                font,
                s.theme,
                focused,
                &highlights,
            );
        }
        if multiple && !focused && INACTIVE_DIM > 0 {
            canvas.tint(*rect, s.theme.background, INACTIVE_DIM);
        }
    }

    let divider_color = s.theme.background.blend(s.theme.foreground, 90);
    for d in s.dividers {
        canvas.fill(s.area, d.x, d.y, d.w, d.h, divider_color);
    }

    if let Some(c) = &s.copy {
        if let Some((_, rect)) = s.panes.iter().find(|(id, _)| *id == c.pane) {
            draw_copy_bar(canvas, *rect, font, s.theme, c);
        }
    }

    if let Some((picker, lib)) = s.picker {
        overlay::draw_theme_picker(canvas, s.full, font, s.theme, lib, picker);
    }
}

struct WindowState {
    window: Arc<Window>,
    backend: Backend,
    font: FontAtlas,
    tabs: Vec<Tab>,
    active: usize,
    modifiers: ModifiersState,
    /// AltGr gedrückt (steht nicht in `ModifiersState`)
    altgr: bool,
    /// Fenster hat den Tastaturfokus (steuert das Abfrage-Intervall)
    focused: bool,
    cursor_pos: (f64, f64),
    /// Aktuelle Schriftgröße (pt), per Ctrl +/- änderbar
    font_size: f32,
    mouse: MouseState,
    /// Copy-Mode in einer Pane dieses Fensters
    copy: Option<CopyState>,
}

/// Was die Maus gerade tut.
#[derive(Default)]
struct MouseState {
    /// Trennlinie wird gezogen
    divider: Option<Divider>,
    /// Text wird in dieser Pane markiert
    selecting: Option<PaneId>,
    dragged: bool,
    last_click: Option<(Instant, PaneId, Point)>,
    clicks: u8,
    /// Gedrückte Taste, die an das Programm gemeldet wurde (Pane, Xterm-Code)
    reported: Option<(PaneId, u8)>,
    last_cell: Option<(usize, usize)>,
    icon: CursorIcon,
}

impl WindowState {
    fn full_area(&self) -> Rect {
        let s = self.window.inner_size();
        Rect {
            x: 0,
            y: 0,
            w: s.width as usize,
            h: s.height as usize,
        }
    }

    /// Tab-Leiste nur ab zwei Tabs (iTerm2-Standard).
    fn tabbar_rect(&self) -> Option<Rect> {
        if self.tabs.len() < 2 {
            return None;
        }
        let a = self.full_area();
        Some(Rect {
            x: 0,
            y: 0,
            w: a.w,
            h: tabbar::height(&self.font).min(a.h),
        })
    }

    /// Bereich unter der Tab-Leiste, in dem die Panes liegen.
    fn content_area(&self) -> Rect {
        let a = self.full_area();
        match self.tabbar_rect() {
            Some(bar) => Rect {
                x: 0,
                y: bar.h,
                w: a.w,
                h: a.h - bar.h,
            },
            None => a,
        }
    }

    fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// Passt die Panes aller Tabs an ihr Rechteck an.
    fn relayout(&mut self, padding: u32) {
        let area = self.content_area();
        for tab in &mut self.tabs {
            let (panes, _) = tab.layout(area);
            for (id, rect) in panes {
                let ws = grid_size(rect, &self.font, padding);
                if let Some(p) = tab.panes.get_mut(&id) {
                    p.resize(ws);
                }
            }
        }
        self.window.request_redraw();
    }

    fn update_title(&self) {
        self.window
            .set_title(self.tab().focused_pane().window_title());
    }

    /// Copy-Mode beenden (z. B. bei Mausklick).
    fn leave_copy(&mut self) {
        let Some(copy) = self.copy.take() else { return };
        if let Some(p) = self.tabs.iter().find_map(|t| t.panes.get(&copy.pane)) {
            CopyState::leave(&mut *p.term.lock());
        }
        self.window.request_redraw();
    }

    fn select_tab(&mut self, i: usize) {
        if i < self.tabs.len() && i != self.active {
            self.active = i;
            self.update_title();
            self.window.request_redraw();
        }
    }
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    keymap: Keymap,
    theme: Theme,
    next_pane_id: u64,
    windows: HashMap<WindowId, WindowState>,
    clipboard: Option<Clipboard>,
    themes: ThemeLibrary,
    theme_idx: usize,
    /// Offene Schema-Auswahl und das Fenster, in dem sie angezeigt wird
    picker: Option<(WindowId, ThemePicker)>,
    /// Geteilter GPU-Kontext (nach dem ersten Fenster mit GPU)
    gpu: Option<Arc<GpuContext>>,
    /// GPU-Start ist einmal fehlgeschlagen → weitere Fenster gleich mit der CPU
    gpu_failed: bool,
    /// Wann die Vordergrund-Programme (Tab-Beschriftung) abgefragt werden – nur nach Anlässen
    poll: PollSchedule,
}

fn clip_set(cb: &mut Option<Clipboard>, primary: bool, text: String) {
    let Some(cb) = cb.as_mut() else { return };
    let res = if primary {
        cb.set().clipboard(LinuxClipboardKind::Primary).text(text)
    } else {
        cb.set_text(text)
    };
    if let Err(e) = res {
        log::warn!("Zwischenablage schreiben: {e}");
    }
}

fn clip_get(cb: &mut Option<Clipboard>, primary: bool) -> Option<String> {
    let cb = cb.as_mut()?;
    let res = if primary {
        cb.get().clipboard(LinuxClipboardKind::Primary).text()
    } else {
        cb.get_text()
    };
    res.map_err(|e| log::warn!("Zwischenablage lesen: {e}"))
        .ok()
}

impl App {
    pub fn new(
        proxy: EventLoopProxy<UserEvent>,
        config: Config,
        themes: ThemeLibrary,
        theme_idx: usize,
    ) -> Self {
        let theme = themes.entries[theme_idx].theme.clone();
        let overrides = config.keybindings();
        let (keymap, warnings) = Keymap::new(&overrides);
        for w in warnings {
            eprintln!("{APP_NAME}: {w}");
        }
        let mut names: Vec<_> = overrides.iter().collect();
        names.sort();
        for (action, specs) in names {
            log::info!(
                "Kürzel aus der Config: {action} = {} (ersetzt die Standardbelegung)",
                specs.join(", ")
            );
        }
        Self {
            keymap,
            proxy,
            config,
            theme,
            next_pane_id: 0,
            windows: HashMap::new(),
            clipboard: Clipboard::new()
                .map_err(|e| log::warn!("Keine Zwischenablage: {e}"))
                .ok(),
            themes,
            theme_idx,
            picker: None,
            gpu: None,
            gpu_failed: false,
            poll: PollSchedule::default(),
        }
    }

    /// GPU-Backend für ein neues Fenster; `Err` → Aufrufer entscheidet über den CPU-Rückfall.
    fn gpu_backend(&mut self, window: &Arc<Window>) -> anyhow::Result<Backend> {
        let surface = match &self.gpu {
            Some(ctx) => ctx.create_surface(Arc::clone(window))?,
            None => {
                let (ctx, surface) = GpuContext::new(Arc::clone(window))?;
                self.gpu = Some(ctx);
                surface
            }
        };
        let ctx = Arc::clone(self.gpu.as_ref().expect("GPU-Kontext gerade angelegt"));
        let size = window.inner_size();
        Ok(Backend::Gpu(Box::new(GpuRenderer::new(
            ctx,
            surface,
            size.width,
            size.height,
        )?)))
    }

    fn cpu_backend(window: &Arc<Window>) -> anyhow::Result<Backend> {
        let context =
            softbuffer::Context::new(Arc::clone(window)).map_err(|e| anyhow::anyhow!("{e}"))?;
        let surface = softbuffer::Surface::new(&context, Arc::clone(window))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Backend::Cpu {
            _context: context,
            surface,
        })
    }

    /// Backend nach `renderer` aus der Config: "auto" probiert die GPU und fällt auf die CPU zurück.
    fn create_backend(&mut self, window: &Arc<Window>) -> anyhow::Result<Backend> {
        let mode = self.config.renderer.to_lowercase();
        if mode == "cpu" || (mode == "auto" && self.gpu_failed) {
            return Self::cpu_backend(window);
        }
        match self.gpu_backend(window) {
            Ok(b) => Ok(b),
            Err(e) if mode == "gpu" => {
                Err(e.context("renderer = \"gpu\" verlangt, GPU nicht nutzbar"))
            }
            Err(e) => {
                log::warn!("GPU nicht nutzbar, zeichne mit der CPU: {e:#}");
                self.gpu_failed = true;
                Self::cpu_backend(window)
            }
        }
    }

    /// Wechselt das Schema für alle Fenster.
    fn set_theme(&mut self, idx: usize) {
        if idx == self.theme_idx || idx >= self.themes.entries.len() {
            return;
        }
        self.theme_idx = idx;
        self.theme = self.themes.entries[idx].theme.clone();
        for w in self.windows.values() {
            w.window.request_redraw();
        }
    }

    /// Merkt sich das Schema in der config.toml. Dateien außerhalb des Theme-Ordners werden per Pfad gespeichert.
    fn save_theme(&self) {
        let entry = &self.themes.entries[self.theme_idx];
        let in_theme_dir =
            |p: &std::path::Path| ThemeLibrary::dir().is_some_and(|d| p.starts_with(d));
        let spec = match &entry.path {
            Some(p) if !in_theme_dir(p) => p.display().to_string(),
            _ => entry.name.clone(),
        };
        let Some(path) = Config::path() else { return };
        match persist::save_theme(&path, &spec) {
            Ok(()) => log::info!("Farbschema „{spec}“ in {} gespeichert", path.display()),
            Err(e) => log::error!("Config schreiben ({}): {e}", path.display()),
        }
    }

    /// Tastatur, solange die Schema-Auswahl offen ist: Vorschau sofort, Enter übernimmt, Esc setzt zurück.
    fn picker_key(&mut self, wid: WindowId, event: &KeyEvent) {
        enum Outcome {
            Stay,
            Apply,
            Cancel,
        }
        let ctrl = self
            .windows
            .get(&wid)
            .is_some_and(|w| w.modifiers.control_key());
        let Some((_, picker)) = self.picker.as_mut() else {
            return;
        };
        let names = self.themes.names();
        let mut outcome = Outcome::Stay;
        match &event.logical_key {
            Key::Named(NamedKey::ArrowUp) => picker.move_by(&names, -1),
            Key::Named(NamedKey::ArrowDown) => picker.move_by(&names, 1),
            Key::Named(NamedKey::PageUp) => picker.move_by(&names, -8),
            Key::Named(NamedKey::PageDown) => picker.move_by(&names, 8),
            Key::Named(NamedKey::Home) => picker.move_by(&names, -10_000),
            Key::Named(NamedKey::End) => picker.move_by(&names, 10_000),
            Key::Named(NamedKey::Enter) => outcome = Outcome::Apply,
            Key::Named(NamedKey::Escape) => outcome = Outcome::Cancel,
            Key::Named(NamedKey::Backspace) => picker.pop(),
            Key::Character(c) if ctrl => match c.to_lowercase().as_str() {
                "p" | "k" => picker.move_by(&names, -1),
                "n" | "j" => picker.move_by(&names, 1),
                _ => {}
            },
            _ => {
                if let Some(text) = &event.text {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        picker.push(c);
                    }
                }
            }
        }
        let selected = picker.selected(&names);
        let original = picker.original;

        match outcome {
            Outcome::Stay => {
                if let Some(i) = selected {
                    self.set_theme(i);
                }
            }
            Outcome::Apply => {
                self.picker = None;
                self.set_theme(selected.unwrap_or(original));
                self.save_theme();
            }
            Outcome::Cancel => {
                self.picker = None;
                self.set_theme(original);
            }
        }
        if let Some(w) = self.windows.get(&wid) {
            w.window.request_redraw();
        }
    }

    fn spawn_pane(&mut self, size: WindowSize) -> anyhow::Result<Pane> {
        let id = PaneId(self.next_pane_id);
        self.next_pane_id += 1;
        // Direkt nach dem Start läuft die Shell evtl. noch nicht (/proc meldet dann noch rterm2)
        self.poll.follow_up(Instant::now());
        Pane::spawn(id, self.proxy.clone(), size, self.config.shell.as_deref())
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let attrs = Window::default_attributes().with_title(APP_NAME);
        // App-ID (Wayland) bzw. WM_CLASS (X11), damit Desktop/Taskleiste das Programm erkennen
        let attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attrs, APP_NAME, APP_NAME,
        );
        let attrs =
            winit::platform::x11::WindowAttributesExtX11::with_name(attrs, APP_NAME, APP_NAME);
        let attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(900.0, 560.0));
        let window = Arc::new(event_loop.create_window(attrs)?);

        let font = FontAtlas::load(
            self.config.font.path.as_deref(),
            self.config.font.size,
            window.scale_factor(),
        )?;
        let backend = self.create_backend(&window)?;

        let s = window.inner_size();
        let area = Rect {
            x: 0,
            y: 0,
            w: s.width as usize,
            h: s.height as usize,
        };
        let pane = self.spawn_pane(grid_size(area, &font, self.config.padding))?;

        let st = WindowState {
            window,
            backend,
            font,
            tabs: vec![Tab::new(pane)],
            active: 0,
            modifiers: ModifiersState::empty(),
            altgr: false,
            focused: false,
            cursor_pos: (0.0, 0.0),
            font_size: self.config.font.size,
            mouse: MouseState::default(),
            copy: None,
        };
        self.windows.insert(st.window.id(), st);
        Ok(())
    }

    /// In welchem Fenster und Tab steckt die Pane?
    fn locate(&self, id: PaneId) -> Option<(WindowId, usize)> {
        self.windows.iter().find_map(|(wid, w)| {
            w.tabs
                .iter()
                .position(|t| t.panes.contains_key(&id))
                .map(|ti| (*wid, ti))
        })
    }

    fn perform(&mut self, event_loop: &ActiveEventLoop, wid: WindowId, action: Action) {
        // Tab-/Pane-Wechsel, neue oder geschlossene Pane: Beschriftung gleich aktualisieren.
        // (Die Abfrage selbst läuft in about_to_wait, also nach dieser Aktion.)
        self.poll.request_at(Instant::now());
        let padding = self.config.padding;
        let Some(st) = self.windows.get_mut(&wid) else {
            return;
        };
        match action {
            Action::NewWindow => {
                if let Err(e) = self.create_window(event_loop) {
                    log::error!("Neues Fenster: {e:#}");
                }
            }
            Action::NewTab => {
                let size = st.tab().focused_pane().size();
                match self.spawn_pane(size) {
                    Ok(pane) => {
                        let st = self.windows.get_mut(&wid).unwrap();
                        st.tabs.push(Tab::new(pane));
                        st.active = st.tabs.len() - 1;
                        st.relayout(padding);
                        st.update_title();
                    }
                    Err(e) => log::error!("Neuer Tab: {e:#}"),
                }
            }
            Action::NextTab => {
                let i = (st.active + 1) % st.tabs.len();
                st.select_tab(i);
            }
            Action::PrevTab => {
                let n = st.tabs.len();
                st.select_tab((st.active + n - 1) % n);
            }
            Action::SelectTab(i) => st.select_tab(i),
            Action::Split(dir) => {
                let mut size = st.tab().focused_pane().size();
                // Startgröße grob halbieren; relayout() setzt danach die exakte Größe.
                match dir {
                    SplitDir::Right => size.num_cols = (size.num_cols / 2).max(2),
                    SplitDir::Down => size.num_lines = (size.num_lines / 2).max(1),
                }
                match self.spawn_pane(size) {
                    Ok(pane) => {
                        let st = self.windows.get_mut(&wid).unwrap();
                        st.tab_mut().split(dir, pane);
                        st.relayout(padding);
                        st.update_title();
                    }
                    Err(e) => log::error!("Neue Pane: {e:#}"),
                }
            }
            Action::ClosePane => {
                let id = st.tab().focused;
                self.close_pane(event_loop, id);
            }
            Action::ToggleZoom => {
                let tab = st.tab_mut();
                if tab.panes.len() > 1 {
                    tab.zoomed = !tab.zoomed;
                    st.relayout(padding);
                }
            }
            Action::Focus(dir) => {
                let area = st.content_area();
                if st.tab_mut().focus_dir(area, dir) {
                    st.update_title();
                    st.window.request_redraw();
                }
            }
            Action::ResizeSplit(dir) => {
                if st.tab_mut().resize_split(dir) {
                    st.relayout(padding);
                }
            }
            Action::ScrollPageUp | Action::ScrollPageDown => {
                let s = if action == Action::ScrollPageUp {
                    Scroll::PageUp
                } else {
                    Scroll::PageDown
                };
                st.tab().focused_pane().term.lock().scroll_display(s);
                st.window.request_redraw();
            }
            Action::Search | Action::CopyMode => {
                let pane = st.tab().focused_pane();
                let id = pane.id;
                let state = CopyState::enter(id, &mut *pane.term.lock(), action == Action::Search);
                st.copy = Some(state);
                st.window.request_redraw();
            }
            Action::ThemePicker => {
                let names = self.themes.names();
                self.picker = Some((wid, ThemePicker::new(&names, self.theme_idx)));
                st.window.request_redraw();
            }
            Action::Copy => {
                let text = st.tab().focused_pane().term.lock().selection_to_string();
                if let Some(t) = text {
                    clip_set(&mut self.clipboard, false, t);
                }
            }
            Action::Paste => {
                if let Some(text) = clip_get(&mut self.clipboard, false) {
                    paste_into(st.tab().focused_pane(), &text);
                }
            }
            Action::FontBigger | Action::FontSmaller | Action::FontReset => {
                let size = match action {
                    Action::FontBigger => st.font_size + 1.0,
                    Action::FontSmaller => (st.font_size - 1.0).max(6.0),
                    _ => self.config.font.size,
                };
                match FontAtlas::load(
                    self.config.font.path.as_deref(),
                    size,
                    st.window.scale_factor(),
                ) {
                    Ok(font) => {
                        st.font = font;
                        st.font_size = size;
                        st.relayout(padding);
                    }
                    Err(e) => log::error!("Schriftgröße: {e:#}"),
                }
            }
        }
    }

    /// Tastatur im Copy-Mode.
    fn copy_key(&mut self, wid: WindowId, event: &KeyEvent) {
        let Some(st) = self.windows.get_mut(&wid) else {
            return;
        };
        let (ctrl, shift) = (st.modifiers.control_key(), st.modifiers.shift_key());
        let Some(copy) = st.copy.as_mut() else { return };
        let Some(pane) = st.tabs[st.active].panes.get(&copy.pane) else {
            // Pane gibt es nicht mehr (geschlossen oder anderer Tab)
            st.copy = None;
            return;
        };
        let outcome = {
            let mut term = pane.term.lock();
            let outcome = copy.handle_key(&mut *term, event, ctrl, shift);
            if !matches!(outcome, Outcome::Stay) {
                CopyState::leave(&mut *term);
            }
            outcome
        };
        match outcome {
            Outcome::Stay => {}
            Outcome::Exit => st.copy = None,
            Outcome::Yank(text) => {
                st.copy = None;
                clip_set(&mut self.clipboard, true, text.clone());
                clip_set(&mut self.clipboard, false, text);
            }
        }
        st.window.request_redraw();
    }

    fn mouse_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        wid: WindowId,
        button: MouseButton,
        pressed: bool,
    ) {
        let padding = self.config.padding as usize;
        let Some(st) = self.windows.get_mut(&wid) else {
            return;
        };
        if pressed {
            st.leave_copy();
            self.poll.request_at(Instant::now());
        }
        let (x, y) = st.cursor_pos;
        let mods = st.modifiers;

        if !pressed {
            if button == MouseButton::Left && st.mouse.divider.take().is_some() {
                return;
            }
            if let Some((id, code)) = st.mouse.reported.take() {
                if let Some(p) = st.tab().panes.get(&id) {
                    let mode = *p.term.lock().mode();
                    let (col, row) = st.mouse.last_cell.unwrap_or((0, 0));
                    p.write(mouse::encode(mode, code, col, row, false, mods));
                }
                return;
            }
            if button == MouseButton::Left {
                if let Some(id) = st.mouse.selecting.take() {
                    let dragged = st.mouse.dragged;
                    let text = st.tab().panes.get(&id).and_then(|p| {
                        let mut term = p.term.lock();
                        if dragged {
                            term.selection_to_string()
                        } else {
                            // Einfacher Klick ohne Ziehen hebt die Markierung auf.
                            term.selection = None;
                            None
                        }
                    });
                    st.window.request_redraw();
                    // Wie in iTerm2/X11 üblich: Markiertes landet sofort in der PRIMARY-Auswahl.
                    if let Some(t) = text {
                        clip_set(&mut self.clipboard, true, t);
                    }
                }
            }
            return;
        }

        // Tab-Leiste: Links = auswählen, Mitte = schließen
        if let Some(bar) = st.tabbar_rect() {
            if bar.contains(x, y) {
                let Some(i) = tabbar::hit(bar, st.tabs.len(), x, y) else {
                    return;
                };
                match button {
                    MouseButton::Left => st.select_tab(i),
                    MouseButton::Middle => {
                        let ids: Vec<PaneId> = st.tabs[i].panes.keys().copied().collect();
                        for id in ids {
                            self.close_pane(event_loop, id);
                        }
                    }
                    _ => {}
                }
                return;
            }
        }

        let area = st.content_area();
        if button == MouseButton::Left {
            if let Some(d) = st
                .tab()
                .dividers(area)
                .into_iter()
                .find(|d| d.rect.grow(3).contains(x, y))
            {
                st.mouse.divider = Some(d);
                return;
            }
        }

        let Some((id, rect)) = st.tab().pane_at(area, x, y) else {
            return;
        };
        if st.tab_mut().focus_at(area, x, y) {
            st.update_title();
            st.window.request_redraw();
        }

        let (cw, ch) = (st.font.cell_w, st.font.cell_h);
        let Some(pane) = st.tabs[st.active].panes.get(&id) else {
            return;
        };
        let mut term = pane.term.lock();
        let mode = *term.mode();
        let (col, row, side) = mouse::cell_at(
            rect.inset(padding),
            cw,
            ch,
            term.columns(),
            term.screen_lines(),
            x,
            y,
        );

        // Programm (mc, htop, vim …) will die Maus haben – außer Shift ist gedrückt.
        if mouse::reporting(mode, mods) {
            let code = match button {
                MouseButton::Left => mouse::BTN_LEFT,
                MouseButton::Middle => mouse::BTN_MIDDLE,
                MouseButton::Right => mouse::BTN_RIGHT,
                _ => return,
            };
            pane.write(mouse::encode(mode, code, col, row, true, mods));
            drop(term);
            st.mouse.reported = Some((id, code));
            st.mouse.last_cell = Some((col, row));
            return;
        }

        match button {
            MouseButton::Left => {
                let point = mouse::grid_point(col, row, term.grid().display_offset());
                let now = Instant::now();
                let clicks = match st.mouse.last_click {
                    Some((t, pid, p))
                        if pid == id
                            && p == point
                            && now.duration_since(t) < Duration::from_millis(400) =>
                    {
                        st.mouse.clicks % 3 + 1
                    }
                    _ => 1,
                };
                st.mouse.clicks = clicks;
                st.mouse.last_click = Some((now, id, point));
                let ty = match clicks {
                    1 => SelectionType::Simple,
                    2 => SelectionType::Semantic,
                    _ => SelectionType::Lines,
                };
                term.selection = Some(Selection::new(ty, point, side));
                st.mouse.selecting = Some(id);
                // Doppel-/Dreifachklick markiert sofort Wort bzw. Zeile.
                st.mouse.dragged = clicks > 1;
                drop(term);
                st.window.request_redraw();
            }
            MouseButton::Middle => {
                drop(term);
                if let Some(text) = clip_get(&mut self.clipboard, true) {
                    paste_into(pane, &text);
                }
            }
            _ => {}
        }
    }

    fn mouse_moved(&mut self, wid: WindowId, x: f64, y: f64) {
        let padding = self.config.padding;
        let Some(st) = self.windows.get_mut(&wid) else {
            return;
        };
        st.cursor_pos = (x, y);
        let mods = st.modifiers;

        if let Some(d) = st.mouse.divider.clone() {
            let ratio = d.ratio_at(x, y);
            st.tab_mut().set_ratio(&d.path, ratio);
            st.relayout(padding);
            return;
        }

        let area = st.content_area();
        let (cw, ch) = (st.font.cell_w, st.font.cell_h);
        let target = st.mouse.reported.map(|(id, _)| id).or(st.mouse.selecting);
        if let Some(id) = target {
            let (panes, _) = st.tab().layout(area);
            let Some(&(_, rect)) = panes.iter().find(|(p, _)| *p == id) else {
                return;
            };
            let Some(pane) = st.tabs[st.active].panes.get(&id) else {
                return;
            };
            let mut term = pane.term.lock();
            let mode = *term.mode();
            let (col, row, side) = mouse::cell_at(
                rect.inset(padding as usize),
                cw,
                ch,
                term.columns(),
                term.screen_lines(),
                x,
                y,
            );

            if let Some((_, code)) = st.mouse.reported {
                if mouse::reports_motion(mode) && st.mouse.last_cell != Some((col, row)) {
                    pane.write(mouse::encode(
                        mode,
                        code + mouse::MOTION,
                        col,
                        row,
                        true,
                        mods,
                    ));
                    st.mouse.last_cell = Some((col, row));
                }
                return;
            }
            let point = mouse::grid_point(col, row, term.grid().display_offset());
            if let Some(sel) = term.selection.as_mut() {
                sel.update(point, side);
            }
            st.mouse.dragged = true;
            drop(term);
            st.window.request_redraw();
            return;
        }

        // Mauszeiger passend zur Position
        let icon = match st
            .tab()
            .dividers(area)
            .iter()
            .find(|d| d.rect.grow(3).contains(x, y))
        {
            Some(d) if d.dir == SplitDir::Right => CursorIcon::ColResize,
            Some(_) => CursorIcon::RowResize,
            None if area.contains(x, y) => CursorIcon::Text,
            None => CursorIcon::Default,
        };
        if icon != st.mouse.icon {
            st.window.set_cursor(icon);
            st.mouse.icon = icon;
        }
    }

    fn mouse_wheel(&mut self, wid: WindowId, delta: MouseScrollDelta) {
        let padding = self.config.padding as usize;
        let Some(st) = self.windows.get_mut(&wid) else {
            return;
        };
        let (x, y) = st.cursor_pos;
        let mods = st.modifiers;
        let Some((id, rect)) = st.tab().pane_at(st.content_area(), x, y) else {
            return;
        };
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, dy) => (dy * 3.0) as i32,
            MouseScrollDelta::PixelDelta(p) => (p.y / st.font.cell_h as f64) as i32,
        };
        if lines == 0 {
            return;
        }
        let (cw, ch) = (st.font.cell_w, st.font.cell_h);
        let Some(pane) = st.tab().panes.get(&id) else {
            return;
        };
        let mut term = pane.term.lock();
        let mode = *term.mode();

        if mouse::reporting(mode, mods) {
            let (col, row, _) = mouse::cell_at(
                rect.inset(padding),
                cw,
                ch,
                term.columns(),
                term.screen_lines(),
                x,
                y,
            );
            let code = if lines > 0 {
                mouse::WHEEL_UP
            } else {
                mouse::WHEEL_DOWN
            };
            let mut buf = Vec::new();
            for _ in 0..lines.unsigned_abs() {
                buf.extend(mouse::encode(mode, code, col, row, true, mods));
            }
            pane.write(buf);
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
            && !mods.shift_key()
        {
            // less, man & Co. im Vollbild: Mausrad wird zu Pfeiltasten
            let seq: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            pane.write(seq.repeat(lines.unsigned_abs() as usize));
        } else {
            term.scroll_display(Scroll::Delta(lines));
            drop(term);
            st.window.request_redraw();
        }
    }

    /// Schließt eine Pane; leere Tabs, Fenster und zuletzt die App werden mit geschlossen.
    fn close_pane(&mut self, event_loop: &ActiveEventLoop, id: PaneId) {
        self.poll.request_at(Instant::now());
        let padding = self.config.padding;
        let Some((wid, ti)) = self.locate(id) else {
            return;
        };
        let st = self.windows.get_mut(&wid).unwrap();
        if st.tabs[ti].remove(id) {
            st.tabs.remove(ti);
            if st.tabs.is_empty() {
                self.close_window(event_loop, wid);
                return;
            }
            if st.active > ti {
                st.active -= 1;
            }
            st.active = st.active.min(st.tabs.len() - 1);
        }
        st.relayout(padding);
        st.update_title();
    }

    fn close_window(&mut self, event_loop: &ActiveEventLoop, wid: WindowId) {
        if let Some((pwid, picker)) = &self.picker {
            if *pwid == wid {
                let original = picker.original;
                self.picker = None;
                self.set_theme(original);
            }
        }
        // Drop der Panes beendet die Shells.
        self.windows.remove(&wid);
        if self.windows.is_empty() {
            event_loop.exit();
        }
    }

    fn redraw(&mut self, wid: WindowId) {
        let Some(st) = self.windows.get_mut(&wid) else {
            return;
        };
        let size = st.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let full = st.full_area();
        let area = st.content_area();
        let bar = st.tabbar_rect();
        let titles: Vec<String> = st
            .tabs
            .iter()
            .map(|t| t.focused_pane().tab_label().to_string())
            .collect();
        let active = st.active;
        let (panes, dividers) = st.tabs[active].layout(area);
        let picker = match &self.picker {
            Some((pwid, p)) if *pwid == wid => Some((p, &self.themes)),
            _ => None,
        };
        let mut marks = Vec::new();
        if let Some(copy) = st.copy.as_mut() {
            if let Some(pane) = st.tabs[active].panes.get(&copy.pane) {
                marks = copy.visible_matches(&*pane.term.lock());
            }
        }
        let copy = st.copy.as_ref().map(|c| CopyView {
            pane: c.pane,
            marks: &marks,
            current: c.current.as_ref(),
            prompt: c.prompt.as_ref().map(|p| (p.backward, p.query.as_str())),
            no_match: c.no_match(),
        });
        let scene = Scene {
            full,
            area,
            bar,
            titles: &titles,
            active,
            panes: &panes,
            dividers: &dividers,
            tab: &st.tabs[active],
            theme: &self.theme,
            padding: self.config.padding as usize,
            picker,
            copy,
        };
        let font = &mut st.font;

        match &mut st.backend {
            Backend::Gpu(gpu) => gpu.render(size.width, size.height, |canvas| {
                paint(canvas, font, &scene)
            }),
            Backend::Cpu { surface, .. } => {
                let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                else {
                    return;
                };
                if let Err(e) = surface.resize(w, h) {
                    log::error!("Surface resize: {e}");
                    return;
                }
                let mut buffer = match surface.buffer_mut() {
                    Ok(b) => b,
                    Err(e) => {
                        log::error!("Framebuffer: {e}");
                        return;
                    }
                };
                let mut frame = Frame {
                    buf: &mut buffer[..],
                    width: full.w,
                    height: full.h,
                };
                paint(&mut frame, font, &scene);
                if let Err(e) = buffer.present() {
                    log::error!("present: {e}");
                }
            }
        }
    }
}

/// Fügt Text in eine Pane ein (mit Bracketed Paste, falls das Programm es wünscht).
fn paste_into(pane: &Pane, text: &str) {
    let mode = {
        let mut term = pane.term.lock();
        term.scroll_display(Scroll::Bottom);
        *term.mode()
    };
    pane.write(mouse::paste_bytes(text, mode));
}

/// AltGr gedrückt/losgelassen? Beim Loslassen liefern manche Layouts (z. B. „Deutsch (Macintosh)“)
/// nicht `AltGraph` als logische Taste – deshalb zusätzlich die physischen Alt-Tasten.
fn is_altgr_key(event: &KeyEvent) -> bool {
    use winit::keyboard::{KeyCode, PhysicalKey};
    event.logical_key == Key::Named(NamedKey::AltGraph)
        || (event.state == ElementState::Released
            && matches!(
                event.physical_key,
                PhysicalKey::Code(KeyCode::AltLeft | KeyCode::AltRight)
            ))
}

/// Grid-Größe einer Pane, die das Pixel-Rechteck `rect` belegt.
fn grid_size(rect: Rect, font: &FontAtlas, padding: u32) -> WindowSize {
    let inner = rect.inset(padding as usize);
    WindowSize {
        num_cols: (inner.w / font.cell_w).max(2) as u16,
        num_lines: (inner.h / font.cell_h).max(1) as u16,
        cell_width: font.cell_w as u16,
        cell_height: font.cell_h as u16,
    }
}

impl App {
    /// Vordergrund-Programm der aktiven Pane jedes Tabs abfragen (nur deren Name wird angezeigt);
    /// Tab-Leiste und Titel bei Änderung neu zeichnen. Ca. 5 µs pro Pane.
    fn poll_processes(&mut self) {
        for st in self.windows.values_mut() {
            let mut changed = false;
            for tab in &mut st.tabs {
                let focused = tab.focused;
                if let Some(pane) = tab.panes.get_mut(&focused) {
                    changed |= pane.refresh_process();
                }
            }
            if changed {
                st.update_title();
                st.window.request_redraw();
            }
        }
    }
}

impl App {
    /// Rate-Limit wie iTerm2: 0,5 s, solange ein rterm2-Fenster den Fokus hat, sonst 5 s.
    fn poll_interval(&self) -> Duration {
        if self.windows.values().any(|w| w.focused) {
            poll::INTERVAL_FOCUSED
        } else {
            poll::INTERVAL_BACKGROUND
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    /// Läuft, bevor die Eventloop wartet: fällige Abfrage ausführen, dann bis zum nächsten Termin
    /// schlafen – oder ganz, wenn nichts geplant ist (untätiges Terminal = keine Aufwachvorgänge).
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.poll.take_due(now) {
            self.poll_processes();
        }
        use winit::event_loop::ControlFlow;
        event_loop.set_control_flow(match self.poll.next() {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        if let Err(e) = self.create_window(event_loop) {
            eprintln!("{APP_NAME}: {e:#}");
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        let padding = self.config.padding;
        let Some(st) = self.windows.get_mut(&wid) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => self.close_window(event_loop, wid),

            WindowEvent::Resized(_) => st.relayout(padding),

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                match FontAtlas::load(self.config.font.path.as_deref(), st.font_size, scale_factor)
                {
                    Ok(font) => st.font = font,
                    Err(e) => log::error!("Schrift neu laden: {e:#}"),
                }
                st.relayout(padding);
            }

            WindowEvent::ModifiersChanged(m) => st.modifiers = m.state(),

            WindowEvent::KeyboardInput { event, .. } if is_altgr_key(&event) => {
                st.altgr = event.state == ElementState::Pressed;
            }

            // Beim Fokuswechsel kann das Loslassen von AltGr verloren gehen.
            WindowEvent::Focused(focused) => {
                if !focused {
                    st.altgr = false;
                }
                st.focused = focused;
                // Beim Zurückkommen gleich nachsehen – im Hintergrund galt das lange Intervall.
                if focused {
                    self.poll.request_at(Instant::now());
                }
            }

            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if self.picker.as_ref().is_some_and(|(p, _)| *p == wid) {
                    self.picker_key(wid, &event);
                    return;
                }
                if st.copy.is_some() {
                    self.copy_key(wid, &event);
                    return;
                }
                let mods = input::mods(st.modifiers, st.altgr);
                let action = input::key_name(&event).and_then(|k| self.keymap.lookup(&k, mods));
                log::debug!(
                    "Taste {:?} → {:?}, {mods:?} → {action:?}",
                    event.logical_key,
                    input::key_name(&event)
                );
                if let Some(action) = action {
                    self.perform(event_loop, wid, action);
                    return;
                }
                // Schutz: Ctrl+Shift+Buchstabe ohne Belegung nicht als Ctrl+Buchstabe an die Shell
                // geben – sonst wird aus einem vertippten Kürzel z. B. Ctrl+D (Shell beendet sich).
                if mods.ctrl && mods.shift && !mods.alt {
                    if let Some(k) = input::key_name(&event) {
                        if k.len() == 1 && k.as_bytes()[0].is_ascii_lowercase() {
                            log::info!(
                                "Ctrl+Shift+{} ist nicht belegt – ignoriert",
                                k.to_uppercase()
                            );
                            return;
                        }
                    }
                }
                let pane = st.tab().focused_pane();
                let mode = *pane.term.lock().mode();
                if let Some(bytes) = input::key_to_bytes(&event, st.modifiers, mode) {
                    pane.term.lock().scroll_display(Scroll::Bottom);
                    // Enter, Ctrl+C, Ctrl+Z, Ctrl+D wechseln meist das Vordergrund-Programm
                    if bytes
                        .iter()
                        .any(|b| matches!(b, b'\r' | 0x03 | 0x1a | 0x04))
                    {
                        self.poll.follow_up(Instant::now());
                    }
                    pane.write(bytes);
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_moved(wid, position.x, position.y)
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.mouse_button(event_loop, wid, button, state == ElementState::Pressed)
            }

            WindowEvent::MouseWheel { delta, .. } => self.mouse_wheel(wid, delta),

            WindowEvent::RedrawRequested => self.redraw(wid),
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let interval = self.poll_interval();
        if let UserEvent::Exit(id) = event {
            self.close_pane(event_loop, id);
            return;
        }
        let id = match &event {
            UserEvent::Wakeup(id)
            | UserEvent::PtyWrite(id, _)
            | UserEvent::Title(id, _)
            | UserEvent::Bell(id) => *id,
            UserEvent::Exit(_) => unreachable!(),
        };
        let Some((wid, ti)) = self.locate(id) else {
            return;
        };
        let st = self.windows.get_mut(&wid).unwrap();
        match event {
            // Nur neu zeichnen, wenn der Tab gerade sichtbar ist.
            UserEvent::Wakeup(_) => {
                // Neue Ausgabe: Anlass zum Nachsehen (rate-limited, wie iTerm2)
                self.poll.request(Instant::now(), interval);
                if ti == st.active {
                    st.window.request_redraw();
                }
            }
            UserEvent::PtyWrite(_, text) => {
                if let Some(p) = st.tabs[ti].panes.get(&id) {
                    p.write(text.into_bytes());
                }
            }
            UserEvent::Title(_, title) => {
                if let Some(p) = st.tabs[ti].panes.get_mut(&id) {
                    p.title = title;
                }
                if ti == st.active && st.tab().focused == id {
                    st.update_title();
                }
                st.window.request_redraw(); // Tab-Leiste
            }
            UserEvent::Bell(_) => st
                .window
                .request_user_attention(Some(winit::window::UserAttentionType::Informational)),
            UserEvent::Exit(_) => {}
        }
    }
}
