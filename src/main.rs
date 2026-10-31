mod agent;
mod app;
mod git;
mod ui;

use std::{io, time::Duration};

use anyhow::Result;
use app::{App, DiffTarget, InputMode, View};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::force_color_output,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

fn main() -> Result<()> {
    force_color_output(true);
    let mut terminal = TerminalGuard::new()?;
    let result = run(terminal.terminal_mut());
    drop(terminal);
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();
    app.reload();
    let mut needs_draw = true;

    while !app.should_quit {
        needs_draw |= app.drain_history_events();
        needs_draw |= app.drain_diff_events();
        needs_draw |= app.drain_dispatch_events();
        needs_draw |= app.drain_session_events();
        if needs_draw {
            terminal.draw(|frame| ui::draw(frame, &app))?;
            needs_draw = false;
        }

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        needs_draw = true;
        app.dismiss_delivery_message();

        if app.input_mode == InputMode::ConfirmSend {
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                app.confirm_send_review();
            } else {
                app.cancel_send_review();
            }
            continue;
        }

        if app.input_mode == InputMode::Search {
            match key.code {
                KeyCode::Esc => app.cancel_search(),
                KeyCode::Enter => app.submit_search(),
                KeyCode::Backspace => app.search_backspace(),
                KeyCode::Char(character) => app.search_push(character),
                _ => {}
            }
            continue;
        }

        if app.view == View::Help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')
            ) {
                app.close_help();
            }
            continue;
        }

        if app.view == View::Sessions {
            match (key.code, key.modifiers) {
                (KeyCode::Esc | KeyCode::Char('q'), KeyModifiers::NONE) => app.close_sessions(),
                (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                    app.move_session_selection(1)
                }
                (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                    app.move_session_selection(-1)
                }
                (KeyCode::Enter, KeyModifiers::NONE) => app.select_agent_session(),
                (KeyCode::Char('r'), KeyModifiers::NONE) => app.open_sessions(),
                (KeyCode::Char('?'), _) => app.open_help(),
                _ => {}
            }
            continue;
        }

        if app.view == View::Comments {
            if app.input_mode == InputMode::Comment {
                match key.code {
                    KeyCode::Esc => app.cancel_comment(),
                    KeyCode::Enter => app.submit_comment(),
                    KeyCode::Backspace => app.comment_backspace(),
                    KeyCode::Char(character) => app.comment_push(character),
                    _ => {}
                }
            } else {
                match (key.code, key.modifiers) {
                    (KeyCode::Esc | KeyCode::Char('q'), KeyModifiers::NONE) => app.close_comments(),
                    (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                        app.move_comment_selection(1)
                    }
                    (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                        app.move_comment_selection(-1)
                    }
                    (KeyCode::Char('e'), KeyModifiers::NONE) => app.edit_selected_comment(),
                    (KeyCode::Char('d'), KeyModifiers::NONE) => app.delete_selected_comment(),
                    (KeyCode::Char('S'), _) => app.request_send_review(),
                    (KeyCode::Char('s'), KeyModifiers::NONE) => app.open_sessions(),
                    (KeyCode::Char('?'), _) => app.open_help(),
                    _ => {}
                }
            }
            continue;
        }

        if app.view == View::Diff {
            if app.input_mode == InputMode::Command {
                match key.code {
                    KeyCode::Esc => app.cancel_command(),
                    KeyCode::Enter => app.submit_command(),
                    KeyCode::Backspace => app.command_backspace(),
                    KeyCode::Char(character) => app.command_push(character),
                    _ => {}
                }
                continue;
            }
            if app.input_mode == InputMode::Comment {
                match key.code {
                    KeyCode::Esc => app.cancel_comment(),
                    KeyCode::Enter => app.submit_comment(),
                    KeyCode::Backspace => app.comment_backspace(),
                    KeyCode::Char(character) => app.comment_push(character),
                    _ => {}
                }
                continue;
            }
            let (_, terminal_height) = crossterm::terminal::size()?;
            let page = terminal_height.saturating_sub(1).max(1) as isize;
            let half_page = (page / 2).max(1);
            match (key.code, key.modifiers) {
                (KeyCode::Esc | KeyCode::Char('q'), KeyModifiers::NONE) => app.close_diff(),
                (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => app.move_diff_cursor(1),
                (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => app.move_diff_cursor(-1),
                (KeyCode::Char('J'), KeyModifiers::SHIFT) => app.move_diff(10),
                (KeyCode::Char('K'), KeyModifiers::SHIFT) => app.move_diff(-10),
                (KeyCode::Char('d'), KeyModifiers::CONTROL) => app.move_diff(half_page),
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => app.move_diff(-half_page),
                (KeyCode::Char('f'), KeyModifiers::CONTROL) | (KeyCode::PageDown, _) => {
                    app.move_diff(page)
                }
                (KeyCode::Char('b'), KeyModifiers::CONTROL) | (KeyCode::PageUp, _) => {
                    app.move_diff(-page)
                }
                (KeyCode::Char('G'), KeyModifiers::SHIFT) => app.select_diff_end(),
                (KeyCode::Char('g'), KeyModifiers::NONE) => app.handle_diff_g(),
                (KeyCode::Home, _) => {
                    app.diff_scroll = 0;
                    app.diff_comment_selected = None;
                    app.pending_g = false;
                }
                (KeyCode::End, _) => app.select_diff_end(),
                (KeyCode::Char(']'), KeyModifiers::NONE) => app.begin_diff_jump(1),
                (KeyCode::Char('['), KeyModifiers::NONE) => app.begin_diff_jump(-1),
                (KeyCode::Char('f'), KeyModifiers::NONE) => {
                    app.complete_diff_jump(DiffTarget::File)
                }
                (KeyCode::Char('c'), KeyModifiers::NONE) => {
                    app.complete_diff_jump(DiffTarget::Change)
                }
                (KeyCode::Char('}'), _) => app.jump_diff(DiffTarget::Change, 1),
                (KeyCode::Char('{'), _) => app.jump_diff(DiffTarget::Change, -1),
                (KeyCode::Tab, _) => app.jump_diff(DiffTarget::File, 1),
                (KeyCode::BackTab, _) => app.jump_diff(DiffTarget::File, -1),
                (KeyCode::Char('a'), KeyModifiers::NONE) => app.start_comment(),
                (KeyCode::Enter, KeyModifiers::NONE) => app.edit_inline_comment(),
                (KeyCode::Char('v'), KeyModifiers::NONE) => app.open_comments(),
                (KeyCode::Char(':'), KeyModifiers::NONE) => app.start_command(),
                (KeyCode::Char('S'), _) => app.request_send_review(),
                (KeyCode::Char('s'), KeyModifiers::NONE) => app.open_sessions(),
                (KeyCode::Char('/'), KeyModifiers::NONE) => app.start_search(),
                (KeyCode::Char('n'), KeyModifiers::NONE) => app.repeat_search(1),
                (KeyCode::Char('N'), KeyModifiers::SHIFT) => app.repeat_search(-1),
                (KeyCode::Char('r'), KeyModifiers::NONE) => app.reload_diff(),
                (KeyCode::Char('?'), _) => app.open_help(),
                _ => {
                    app.pending_g = false;
                    app.diff_jump_direction = None;
                }
            }
            continue;
        }

        match app.input_mode {
            InputMode::Normal => match (key.code, key.modifiers) {
                (KeyCode::Char('q'), KeyModifiers::NONE) => app.should_quit = true,
                (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => app.move_selection(1),
                (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => app.move_selection(-1),
                (KeyCode::Char('d'), KeyModifiers::CONTROL) => app.move_selection(10),
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => app.move_selection(-10),
                (KeyCode::Char('G'), KeyModifiers::SHIFT) => app.select_last(),
                (KeyCode::Char('g'), KeyModifiers::NONE) => app.handle_g(),
                (KeyCode::Char(':'), KeyModifiers::NONE) => app.start_command(),
                (KeyCode::Enter, KeyModifiers::NONE) => app.open_selected_commit(),
                (KeyCode::Char('/'), KeyModifiers::NONE) => app.start_search(),
                (KeyCode::Char('n'), KeyModifiers::NONE) => app.repeat_search(1),
                (KeyCode::Char('N'), KeyModifiers::SHIFT) => app.repeat_search(-1),
                (KeyCode::Char('r'), KeyModifiers::NONE) => app.reload(),
                (KeyCode::Char('s'), KeyModifiers::NONE) => app.open_sessions(),
                (KeyCode::Char('?'), _) => app.open_help(),
                _ => app.pending_g = false,
            },
            InputMode::Command => match key.code {
                KeyCode::Esc => app.cancel_command(),
                KeyCode::Enter => app.submit_command(),
                KeyCode::Backspace => app.command_backspace(),
                KeyCode::Char(character) => app.command_push(character),
                _ => {}
            },
            InputMode::Comment => unreachable!("comments are only entered in the diff view"),
            InputMode::ConfirmSend => unreachable!("send confirmation is handled first"),
            InputMode::Search => unreachable!("search input is handled first"),
        }
    }

    Ok(())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
