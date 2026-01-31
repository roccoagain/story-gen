mod api;
mod app;
mod config;
mod input;
mod ui;

use std::env;
use std::io;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use crate::api::advance_turn;
use crate::app::App;
use crate::config::load_or_prompt_api_key;
use crate::input::handle_key_event;
use crate::ui::draw_ui;

fn main() -> Result<()> {
    let debug = env::args().any(|arg| arg == "--debug" || arg == "-d");
    let api_key = load_or_prompt_api_key()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, api_key, debug);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    api_key: String,
    debug: bool,
) -> Result<()> {
    let mut app = App::new();

    loop {
        terminal.draw(|frame| draw_ui(frame, &mut app))?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                event::Event::Key(key) => {
                    if handle_key_event(key, &mut app)? {
                        break;
                    }
                }
                event::Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if app.busy {
            if let Some(rx) = &app.pending_response {
                match rx.try_recv() {
                    Ok(result) => {
                        app.pending_response = None;
                        app.busy = false;
                        match result {
                            Ok((reply, output_items, debug_summary)) => {
                                app.push_assistant_reply(&reply);
                                app.push_history_chunk(output_items);
                                if debug {
                                    app.push_log(app::LogKind::System, debug_summary);
                                }
                                app.state.turn = app.state.turn.saturating_add(1);
                                app.status = "Ready".to_string();
                            }
                            Err(err) => {
                                app.push_log(app::LogKind::Error, err.to_string());
                                app.status = "Error".to_string();
                            }
                        }
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        app.pending_response = None;
                        app.busy = false;
                        app.push_log(app::LogKind::Error, "Response channel disconnected.");
                        app.status = "Error".to_string();
                    }
                }
            }
            continue;
        }

        if let Some(_user_input) = app.pending_input.take() {
            let api_key = api_key.clone();
            let history = app.history.clone();
            let state = app.state.clone();
            let (tx, rx) = mpsc::channel();
            app.pending_response = Some(rx);
            app.busy = true;
            app.status = "Thinking...".to_string();
            terminal.draw(|frame| draw_ui(frame, &mut app))?;

            thread::spawn(move || {
                let result = advance_turn(&api_key, &history, &state, debug);
                let _ = tx.send(result);
            });
        }
    }

    Ok(())
}
