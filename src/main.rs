mod app;
mod config;
mod copymode;
mod font;
mod gpu;
mod input;
mod keymap;
mod layout;
mod mouse;
mod overlay;
mod pane;
mod persist;
mod picker;
mod poll;
mod procname;
mod quad;
mod renderer;
mod tab;
mod tabbar;
mod theme;

use winit::event_loop::EventLoop;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Setzt TERM / COLORTERM für die Kindprozesse (xterm-256color bzw. alacritty, falls terminfo vorhanden).
    alacritty_terminal::tty::setup_env();

    let config = config::Config::load()?;
    let mut themes = theme::ThemeLibrary::load();
    let default_idx = themes.index_of(theme::DEFAULT_THEME).unwrap_or(0);
    let theme_idx = match config.theme.as_deref() {
        Some(spec) => themes.resolve(spec).unwrap_or_else(|e| {
            eprintln!("{}: {e:#}", config::APP_NAME);
            default_idx
        }),
        None => default_idx,
    };

    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let mut app = app::App::new(event_loop.create_proxy(), config, themes, theme_idx);
    event_loop.run_app(&mut app)?;
    Ok(())
}
