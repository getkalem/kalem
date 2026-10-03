//! Kalem's terminal frontend (design §7.6): the editor on ratatui and
//! crossterm, with the same commands, keymaps and settings as the
//! graphical one.

pub mod app;
pub mod caps;
pub mod editor;
pub mod input;
pub mod panels;
pub mod render;
pub mod terminal;
pub mod viewer;

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event;
use crossterm::execute;
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use kalem_core::settings::{self, Config};

/// Opens `path` (or an empty document) in the terminal editor.
pub fn run(path: Option<&Path>) -> io::Result<()> {
    let user = settings::config_dir().map(|d| d.join("settings.toml"));
    let dir = path
        .and_then(|p| std::path::absolute(p).ok())
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let workspace = dir.as_deref().and_then(settings::find_workspace_settings);
    let config = Config::load(user.as_deref(), workspace.as_deref());
    if let Err(e) =
        kalem_core::logging::init(&kalem_core::logging::LogOptions::standard(&config, false))
    {
        let _ = std::io::Write::write_fmt(&mut io::stderr(), format_args!("kalem: {e}\n"));
    }
    config.apply_process_settings();
    // Newer versions of the installed plugins, once a day.
    kalem_core::plugin_store::check_updates(&config);
    terminal::raw_mode()?;
    let mut caps = caps::query(Duration::from_millis(500));
    // Theme colors where the terminal shows them; its light or dark
    // background decides for `system`.
    if caps.true_color && !caps.no_color {
        let dark = kalem_core::theme::wants_dark(
            config.str("editor.theme"),
            caps.dark_background().unwrap_or(true),
        );
        let dir = kalem_core::theme::user_dir();
        caps.colors = Some(std::sync::Arc::new(kalem_core::theme::ThemeColors::load(
            dark,
            dir.as_deref(),
        )));
    }
    tracing::info!(terminal = ?caps.terminal, kitty_keyboard = caps.kitty_keyboard, "terminal");
    let mut app = match app::App::new(path, config, caps) {
        Ok(a) => a,
        Err(e) => {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(io::Error::other(e.to_string()));
        }
    };
    // The last session, when no file was given and the settings say so.
    let asked = kalem_core::sessions::take_restore_request();
    if asked || (path.is_none() && app.config_bool("editor.restore_session")) {
        app.run_command("session.restore", serde_json::Value::Null);
    }
    // Images need a graphics protocol; block characters are not used.
    // ratatui-image's own terminal query is not used: its reader thread
    // outlives its timeout and takes keystrokes.
    app.editor.images.borrow_mut().picker = picker(&app.caps);
    // Formulas in the terminal's own colors, else the theme's.
    let dark = app.caps.dark_background().unwrap_or(true);
    let bg = app.caps.background.map(|(r, g, b)| [r, g, b]);
    let (fg, theme_bg) = match &app.caps.colors {
        Some(t) => {
            let (f, b) = (t.foreground.rgb(), t.background.rgb());
            ([f.0, f.1, f.2], [b.0, b.1, b.2])
        }
        None if dark => ([0xdd; 3], [0; 3]),
        None => ([0x20; 3], [0xff; 3]),
    };
    app.editor.images.borrow_mut().math_colors = (fg, bg.unwrap_or(theme_bg));
    // Over SSH, kitty itself inflates zlib transmissions.
    let ssh = std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
    let kitty = app
        .caps
        .terminal
        .as_deref()
        .is_some_and(|t| t.to_lowercase().contains("kitty"));
    app.editor.images.borrow_mut().compress = ssh && kitty;
    tracing::info!(graphics = ?app.caps.graphics(), cell = ?app.caps.cell_size, "images");
    let (mut term, session) = terminal::start(&app.caps)?;
    // For start-up measurements (book/part-5/performance.org): quit after the
    // first frame.
    let exit_after_start = std::env::var_os("KALEM_EXIT_AFTER_START").is_some();
    loop {
        if app.dirty {
            let sync = app.caps.synchronized_output;
            if sync {
                execute!(io::stdout(), BeginSynchronizedUpdate)?;
            }
            term.draw(|f| app.draw(f))?;
            if sync {
                execute!(io::stdout(), EndSynchronizedUpdate)?;
            }
            if exit_after_start {
                break;
            }
        }
        if event::poll(app.timeout(Instant::now()))? {
            app.event(event::read()?);
            while event::poll(Duration::ZERO)? {
                app.event(event::read()?);
            }
        }
        for s in app.take_output() {
            use std::io::Write;
            io::stdout().write_all(s.as_bytes())?;
            io::stdout().flush()?;
        }
        app.tick(Instant::now());
        if app.quit {
            break;
        }
    }
    // The session `SPC q l` restores.
    let last = app.session();
    if !last.documents.is_empty() {
        let _ = kalem_core::sessions::save(kalem_core::sessions::LAST, &last);
    }
    // `SPC q r`, `SPC q R`: the terminal given back, then Kalem again with
    // the same arguments.
    if app.restart {
        drop(term);
        drop(session);
        let exe = std::env::current_exe()?;
        let status = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .status()?;
        std::process::exit(status.code().unwrap_or(0));
    }
    Ok(())
}

/// The image renderer for the terminal's protocol and cell size.
fn picker(caps: &caps::Caps) -> Option<ratatui_image::picker::Picker> {
    use ratatui_image::picker::{Picker, ProtocolType};
    let protocol = match caps.graphics()? {
        caps::Graphics::Kitty => ProtocolType::Kitty,
        caps::Graphics::Iterm2 => ProtocolType::Iterm2,
        caps::Graphics::Sixel => ProtocolType::Sixel,
    };
    let (width, height) = caps.cell_size.unwrap_or((10, 20));
    // The font size is known from Kalem's own query.
    #[allow(deprecated)]
    let mut p = Picker::from_fontsize(ratatui_image::FontSize { width, height });
    p.set_protocol_type(protocol);
    Some(p)
}

/// Prints the terminal's capabilities (`kalem tui --detect`).
pub fn detect() -> io::Result<()> {
    terminal::raw_mode()?;
    let c = caps::query(Duration::from_millis(1000));
    crossterm::terminal::disable_raw_mode()?;
    let report = c.report();
    std::io::Write::write_fmt(&mut io::stdout(), format_args!("{report}\n"))
}
