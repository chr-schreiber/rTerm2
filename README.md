# rterm2

[Deutsch](README.md) · **English**

iTerm2-inspired terminal emulator for Linux (X11 and Wayland), written in Rust: tabs, split panes, iTerm2 color schemes and GPU rendering – fully keyboard-driven. The terminal core (VT100/xterm) is provided by [`alacritty_terminal`](https://github.com/alacritty/alacritty).

## Features

- **Windows, tabs and split panes** via keyboard shortcuts, modeled after iTerm2
- **Keyboard pane navigation** (Ctrl+Shift+H/J/K/L), pane zoom, resizable dividers
- **Color schemes:** 10 built-in, any `.itermcolors` file, picker with live preview (Ctrl+Shift+P)
- **GPU rendering** with wgpu (Vulkan, falling back to OpenGL), CPU renderer as fallback
- **Scrollback search** and a vim-style **copy mode** – select and copy text without a mouse
- **Custom keybindings** in `config.toml`
- **Tab titles** show the program running in the active pane (`zsh`, `nvim`, `htop` …)
- Mouse is optional: selection, PRIMARY selection, mouse reporting for mc/htop/vim, draggable dividers

## Build & run

```sh
cargo run --release
RUST_LOG=info cargo run           # with log output
RUST_LOG=rterm2=debug cargo run   # logs every key with its modifiers
```

Requires: Rust ≥ 1.77 and `fc-match` (fontconfig) to find a font; alternatively set `[font] path`.

## Install

```sh
cargo install --path .                                                     # binary to ~/.cargo/bin/rterm2
install -Dm644 rterm2.desktop ~/.local/share/applications/rterm2.desktop   # application menu entry
```

## Configuration

`config.example.toml` is a complete template with every setting and its default value:

```sh
mkdir -p ~/.config/rterm2 && cp config.example.toml ~/.config/rterm2/config.toml
```

Short version of `~/.config/rterm2/config.toml`:

```toml
theme = "Dracula"   # name (built-in or from ~/.config/rterm2/themes/) or path to an .itermcolors file
shell = "/usr/bin/zsh -l"
padding = 6
renderer = "auto"   # "auto" (GPU, else CPU), "gpu" or "cpu"

[font]
path = "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf"
size = 14.0
```

## Keyboard shortcuts

| Action | rterm2 | iTerm2 |
|---|---|---|
| New window | Ctrl+Shift+N | ⌘N |
| New tab | Ctrl+Shift+T | ⌘T |
| Next / previous tab | Ctrl+PageDown / Ctrl+PageUp, Ctrl+Tab / Ctrl+Shift+Tab | ⌘⇧] / ⌘⇧[ |
| Select tab 1–9 | Alt+1 … Alt+9 or click the tab | ⌘1 … ⌘9 |
| Close tab | middle-click the tab | |
| Split vertically (new pane to the right) | Ctrl+Shift+D | ⌘D |
| Split horizontally (new pane below) | Ctrl+Shift+E | ⇧⌘D |
| Close pane (closing the last pane closes the tab or window) | Ctrl+Shift+W | ⌘W |
| Switch pane | Ctrl+Shift+H/J/K/L (left/down/up/right), Alt+arrows, Ctrl+Alt+arrows (AltGr works too) or click | ⌥⌘ arrows |
| Move divider | Ctrl+Shift+arrows | ⌃⌘ arrows |
| Zoom pane / restore | Ctrl+Shift+Enter | ⇧⌘↩ |
| Scroll back | Shift+PageUp/PageDown, mouse wheel | |
| Copy / paste | Ctrl+Shift+C / Ctrl+Shift+V | ⌘C / ⌘V |
| Choose color scheme | Ctrl+Shift+P | |
| Search scrollback | Ctrl+Shift+F | ⌘F |
| Copy mode | Ctrl+Shift+Space | ⇧⌘C |
| Font larger / smaller / reset | Ctrl++ (or Ctrl+=) / Ctrl+- / Ctrl+0 | ⌘+ / ⌘- / ⌘0 |

## Keyboard search and copy (copy mode)

**Ctrl+Shift+F** searches the scrollback (upwards), **Ctrl+Shift+Space** enters copy mode without a search. A status line appears at the bottom of the pane.

| Key | Effect |
|---|---|
| h j k l / arrows | move cursor |
| w b e (W B E) | next word / previous word / end of word (whitespace-separated WORDS) |
| 0 ^ $ | start of line / first character / end of line |
| gg / G | top / bottom |
| H M L | top / middle / bottom of the visible area |
| Ctrl+U / Ctrl+D, Ctrl+B / Ctrl+F, PageUp / PageDown | half / full page |
| { } / % | paragraph up / down, matching bracket |
| v / V / Ctrl+V | select: characters / lines / block (press again to cancel) |
| y or Enter | copy selection (clipboard + PRIMARY) and exit |
| / or ? | search down or up – literal text; all-lowercase input = case-insensitive |
| Enter / Esc (while searching) | accept match / jump back to start position |
| n / N | next / previous match |
| Esc | clear selection, exit on second press (also q, Ctrl+C) |

All visible matches are highlighted in yellow, the current one in orange. A mouse click exits copy mode.

## Custom keybindings

In `config.toml`, an entry under `[keybindings]` replaces **all** default shortcuts of that action. To keep a default shortcut, include it in the list:

```toml
[keybindings]
split_right = ["ctrl+shift+d", "super+d"]     # keep the default + add a shortcut
focus_left  = ["ctrl+shift+h", "alt+left"]    # several shortcuts
close_pane  = "none"                          # disable the action
```

Modifiers: `ctrl`, `shift`, `alt`, `altgr`, `super`. Keys: letters/digits/characters, `plus`, `minus`, `equal`, `enter`, `tab`, `space`, `escape`, `backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, `left`, `right`, `up`, `down`, `f1`–`f12`. If a shortcut is already bound to another action, the config entry wins. Errors (unknown action or key) are reported at startup.

Actions: `new_window`, `new_tab`, `next_tab`, `prev_tab`, `select_tab_1` … `select_tab_9`, `split_right`, `split_down`, `close_pane`, `toggle_zoom`, `focus_left/right/up/down`, `resize_left/right/up/down`, `scroll_page_up`, `scroll_page_down`, `copy`, `paste`, `font_bigger`, `font_smaller`, `font_reset`, `theme_picker`, `search`, `copy_mode`.

To see how rterm2 interprets a key: `RUST_LOG=rterm2=debug rterm2`.

## Color schemes

**Ctrl+Shift+P** opens the scheme picker – fully keyboard-driven:

- ↑/↓ (or Ctrl+J/K, Ctrl+N/P), PageUp/PageDown, Home/End: select – the scheme is applied **live** right away
- type to filter (fuzzy: "tn" finds "Tokyo Night"), Backspace deletes
- **Enter** applies it and saves `theme = "…"` to `config.toml` (comments and everything else are preserved)
- **Esc** returns to the previous scheme

Built in: iTerm2 Default, iTerm2 Light, Solarized Dark/Light, Dracula, Gruvbox Dark, Nord, One Dark, Tokyo Night, Catppuccin Mocha.

Your own schemes: every `.itermcolors` file in `~/.config/rterm2/themes/` shows up in the list under its file name (the same name replaces a built-in scheme). More than 450 ready-made schemes are available at https://iterm2colorschemes.com (folder `schemes/`):

```sh
mkdir -p ~/.config/rterm2/themes
git clone --depth 1 https://github.com/mbadolato/iTerm2-Color-Schemes /tmp/itcs
cp /tmp/itcs/schemes/*.itermcolors ~/.config/rterm2/themes/
```

Supported entries: Ansi 0–15, Foreground, Background, Cursor, Cursor Text, Selection, Selected Text.

## Tab and window titles

Each tab shows the program currently running in the foreground of its active pane – `zsh`, `nvim`, `htop` … (like iTerm2). Like iTerm2, rterm2 only checks `/proc` when something happens: new output, after Enter/Ctrl+C/Ctrl+Z (with short follow-up checks), and on tab or pane switches. At most every 0.5 s while an rterm2 window has focus, otherwise every 5 s. An idle terminal never wakes up. The window title shows a title set by the program itself (OSC 0/2, e.g. `user@host: ~/code` from many shell prompts), otherwise the program name as well.

## Mouse

- **Select:** drag; double-click selects a word, triple-click a line. The selection goes straight to the PRIMARY selection, **middle-click** pastes it.
- **Programs with mouse support** (mc, htop, vim with `mouse=a`) receive clicks, drags and the mouse wheel. Hold **Shift** to select text anyway.
- **Dividers** between panes can be dragged with the mouse.
- **Mouse wheel** in `less`/`man` (alternate screen without mouse mode) scrolls the content instead of the scrollback.

## Rendering

By default rterm2 renders on the **GPU** (wgpu: Vulkan, otherwise OpenGL ES). Each glyph is rasterized once and stored in a texture on the GPU; per frame only rectangles (backgrounds, cursor, glyphs) are sent to the GPU – in a single draw call.

- `renderer = "auto"` (default): GPU, automatic CPU fallback on problems
- `renderer = "gpu"`: GPU only, fail at startup instead of falling back
- `renderer = "cpu"`: always CPU (softbuffer)
- Which GPU is used: `RUST_LOG=rterm2=info rterm2` → line `GPU: …`
- Force a backend (wgpu): `WGPU_BACKEND=gl rterm2` or `WGPU_BACKEND=vulkan rterm2`

rterm2 prefers the low-power (integrated) GPU.

## Code structure

| File | Purpose |
|---|---|
| `main.rs` | startup, load config & theme, winit event loop |
| `app.rs` | windows, event dispatch, redraw |
| `tabbar.rs` | draw the tab bar, hit testing |
| `procname.rs` | foreground program of a pane via `/proc` – unit-tested |
| `poll.rs` | when to check: triggers, rate limit, follow-ups – unit-tested |
| `tab.rs` | panes of a tab, focus, zoom |
| `layout.rs` | split tree, layout into rectangles, neighbor lookup – unit-tested |
| `pane.rs` | PTY + `Term` + event proxy (one pane = one terminal) |
| `gpu.rs` + `shader.wgsl` | GPU renderer: glyph atlas, instances, one draw call |
| `quad.rs` | clipping, atlas packer, sRGB – unit-tested |
| `renderer.rs` | `Canvas` interface, CPU framebuffer, terminal drawing |
| `font.rs` | font loading, cell size, glyph cache |
| `keymap.rs` | actions, default shortcuts, `[keybindings]` – unit-tested |
| `copymode.rs` | copy mode and search (built on alacritty_terminal's vi mode) |
| `mouse.rs` | pixel → cell, mouse escape sequences (SGR/X10), paste preparation |
| `input.rs` | winit key → name/modifiers, keys → escape sequences |
| `theme.rs` | color schemes, built-in schemes, scheme library, `.itermcolors` import, 256-color palette |
| `picker.rs` | state of the scheme picker (filter, cursor) – unit-tested |
| `overlay.rs` | draws the scheme picker |
| `persist.rs` | writes `theme = …` back to config.toml – unit-tested |
| `config.rs` | TOML configuration |

## Acknowledgements

- **[alacritty](https://github.com/alacritty/alacritty)** – rterm2 uses `alacritty_terminal` (Apache-2.0) for escape sequences, grid, scrollback, selection, vi mode and search.
- **[iTerm2](https://github.com/gnachman/iTerm2)** – the model for interaction, keyboard shortcuts, the `.itermcolors` format and foreground program detection. No code was copied.
- The built-in color schemes (Solarized, Dracula, Gruvbox, Nord, One Dark, Tokyo Night, Catppuccin) are the work of their respective authors.

## License

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
