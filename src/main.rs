mod app;
mod backend;
mod demo;
mod i18n;
mod theme;
mod ui;

use std::io;
use std::panic;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use ratatui::DefaultTerminal;
use tracing_subscriber::EnvFilter;

use crate::app::App;
use crate::i18n::Lang;
use crate::theme::ThemeKind;

struct Options {
    demo: bool,
    lang: Lang,
    theme: ThemeKind,
    mouse: bool,
}

fn main() -> Result<()> {
    let options = parse_args()?;
    init_logging().context("cannot initialize logging")?;

    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        ratatui::restore();
        original_hook(info);
    }));

    let mut terminal = ratatui::init();
    if options.mouse
        && let Err(error) = execute!(io::stdout(), EnableMouseCapture)
    {
        ratatui::restore();
        return Err(error.into());
    }

    let result = run(&mut terminal, options);

    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, options: Options) -> Result<()> {
    let mut app = App::new(options.lang, options.theme, options.mouse);
    app.demo = options.demo;
    if !options.demo {
        app.status = "TDLib backend is not wired yet — demo mode".into();
    }

    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
                Event::Mouse(mouse) => app.on_mouse(mouse),
                Event::Resize(_, _) => app.dirty = true,
                _ => {}
            }
        }

        app.on_tick();

        if let Some(enabled) = app.take_mouse_toggle() {
            if enabled {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn parse_args() -> Result<Options> {
    let mut options = Options {
        demo: false,
        lang: Lang::detect(),
        theme: ThemeKind::Mocha,
        mouse: true,
    };

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => options.demo = true,
            "--no-mouse" => options.mouse = false,
            "--lang" => {
                let value = args.next().context("--lang needs ru or en")?;
                options.lang =
                    Lang::parse(&value).with_context(|| format!("unknown language: {value}"))?;
            }
            "--theme" => {
                let value = args.next().context("--theme needs mocha or latte")?;
                options.theme =
                    ThemeKind::parse(&value).with_context(|| format!("unknown theme: {value}"))?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("gramterm {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument: {other}\n\n{}", help_text()),
        }
    }

    Ok(options)
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> &'static str {
    "Gramterm — Telegram in your terminal

Usage:
  gramterm [OPTIONS]

Options:
  --demo            run with demo data, no Telegram account
  --lang ru|en      interface language
  --theme mocha|latte
  --no-mouse        do not capture the mouse
  -h, --help        show this help
  -V, --version     show version

Keys:
  arrows / j k      move
  Tab / Shift+Tab   change focus
  Enter             open chat or send
  i                 focus the composer
  Esc               back / close
  /                 search chats
  :                 command
  F2                release/capture the mouse
  ?                 help
  q                 quit"
}

fn init_logging() -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("gramterm.log")?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(file)
        .init();
    Ok(())
}
