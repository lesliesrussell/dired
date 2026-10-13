// dired-7qs
//! Terminal frontend: crossterm input, ratatui rendering, and the event
//! loop that interleaves keys with background task events.

pub mod clipboard;
pub mod render;
pub mod theme;
pub mod widgets;

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use dired_app::{App, Effect};
use dired_config::{Key, KeyCode};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::theme::Theme;

/// Map a crossterm key event to Emacs-style key notation.
pub fn convert_key(ev: KeyEvent) -> Option<Key> {
    use crossterm::event::KeyCode as C;
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    let meta = ev.modifiers.contains(KeyModifiers::ALT);
    let code = match ev.code {
        C::Char(c) if ctrl && c.is_ascii_alphabetic() => KeyCode::Char(c.to_ascii_lowercase()),
        C::Char(c) => KeyCode::Char(c),
        C::Enter => KeyCode::Enter,
        C::Tab => KeyCode::Tab,
        C::BackTab => KeyCode::BackTab,
        C::Backspace => KeyCode::Backspace,
        C::Delete => KeyCode::Delete,
        C::Esc => KeyCode::Esc,
        C::Up => KeyCode::Up,
        C::Down => KeyCode::Down,
        C::Left => KeyCode::Left,
        C::Right => KeyCode::Right,
        C::Home => KeyCode::Home,
        C::End => KeyCode::End,
        C::PageUp => KeyCode::PageUp,
        C::PageDown => KeyCode::PageDown,
        C::F(n) => KeyCode::F(n),
        _ => return None,
    };
    // Terminals report C-/ and C-_ inconsistently; normalize to C-_.
    let (code, ctrl) = match (code, ctrl) {
        (KeyCode::Char('7'), true) | (KeyCode::Char('/'), true) => (KeyCode::Char('_'), true),
        other => other,
    };
    Some(Key { code, ctrl, meta })
}

enum Input {
    Key(Key),
    Resize,
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<TerminalGuard> {
        terminal::enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = io::stdout().execute(LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn spawn_input(paused: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> mpsc::UnboundedReceiver<Input> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if paused.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            match event::poll(Duration::from_millis(50)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => {
                        if let Some(key) = convert_key(k) {
                            if tx.send(Input::Key(key)).is_err() {
                                return;
                            }
                        }
                    }
                    Ok(Event::Resize(..)) => {
                        let _ = tx.send(Input::Resize);
                    }
                    Ok(_) => {}
                    Err(_) => return,
                },
                Ok(false) => {}
                Err(_) => return,
            }
        }
    });
    rx
}

fn run_foreground(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    paused: &AtomicBool,
    spec: dired_process::ProcessSpec,
) -> io::Result<Option<String>> {
    paused.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(60));
    io::stdout().execute(LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    let status = std::process::Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .status();
    terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    terminal.clear()?;
    paused.store(false, Ordering::Relaxed);
    Ok(match status {
        Ok(s) if s.success() => None,
        Ok(s) => Some(format!("{} exited with {s}", spec.program.to_string_lossy())),
        Err(e) => Some(format!("cannot run {}: {e}", spec.program.to_string_lossy())),
    })
}

/// Run the UI until the app quits.
pub async fn run(mut app: App) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut theme = Theme::from_config(&app.config.theme);
    // dired-7yu
    let mut theme_gen = app.config_generation();
    let paused = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let mut input = spawn_input(paused.clone(), stop.clone());
    let mut events = app.take_events();
    const FRAME: Duration = Duration::from_millis(16);
    let mut dirty = true;
    let mut last_draw = Instant::now() - FRAME;

    loop {
        // dired-7yu
        if app.config_generation() != theme_gen {
            theme = Theme::from_config(&app.config.theme);
            theme_gen = app.config_generation();
        }
        if dirty && last_draw.elapsed() >= FRAME {
            terminal.draw(|f| render::draw(f, &mut app, &theme))?;
            last_draw = Instant::now();
            dirty = false;
        }
        let wait = if dirty { FRAME.saturating_sub(last_draw.elapsed()) } else { Duration::from_secs(3600) };
        tokio::select! {
            k = input.recv() => match k {
                Some(Input::Key(k)) => { app.handle_key(k); dirty = true; }
                Some(Input::Resize) => { dirty = true; }
                None => break,
            },
            ev = events.recv() => {
                if let Some(ev) = ev {
                    app.handle_event(ev);
                    // Drain a bounded burst so input stays responsive.
                    let start = Instant::now();
                    while start.elapsed() < Duration::from_millis(8) {
                        match events.try_recv() {
                            Ok(ev) => app.handle_event(ev),
                            Err(_) => break,
                        }
                    }
                    dirty = true;
                }
            }
            _ = tokio::time::sleep(wait) => {}
        }
        for eff in app.take_effects() {
            match eff {
                Effect::Foreground(spec) => {
                    if let Some(err) = run_foreground(&mut terminal, &paused, spec)? {
                        app.error(err);
                    }
                    app.foreground_finished(); // dired-blw: show what the program changed
                    dirty = true;
                }
                // dired-vhh
                Effect::Clipboard(text) => {
                    if let Err(e) = clipboard::copy(&text) {
                        app.error(format!("cannot copy to the clipboard: {e}"));
                    }
                    dirty = true;
                }
            }
        }
        if app.should_quit() {
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode as C;

    #[test]
    fn converts_keys() {
        let k = convert_key(KeyEvent::new(C::Char('x'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(k, Key::ctrl('x'));
        let k = convert_key(KeyEvent::new(C::Char('<'), KeyModifiers::ALT | KeyModifiers::SHIFT)).unwrap();
        assert_eq!(k, Key::meta('<'));
        let k = convert_key(KeyEvent::new(C::Char('C'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(k, Key::ch('C'));
    }
}
