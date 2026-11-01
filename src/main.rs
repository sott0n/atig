mod agent;
mod app;
mod git;
mod review_store;
mod ui;

use std::{io, time::Duration};

use anyhow::Result;
use app::{App, DiffTarget, InputMode, RepositorySearchKind, View};
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
    app.enable_review_store();
    app.reload();
    let mut needs_draw = true;

    while !app.should_quit {
        needs_draw |= app.drain_history_events();
        needs_draw |= app.drain_diff_events();
        needs_draw |= app.drain_dispatch_events();
        needs_draw |= app.drain_session_events();
        needs_draw |= app.drain_repository_search_events();
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

        if app.input_mode == InputMode::RepositorySearch {
            match key.code {
                KeyCode::Esc => app.cancel_repository_search_input(),
                KeyCode::Enter => app.submit_repository_search(),
                KeyCode::Backspace => app.repository_search_backspace(),
                KeyCode::Char(character) => app.repository_search_push(character),
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

        if app.view == View::RepositorySearch {
            match (key.code, key.modifiers) {
                (KeyCode::Esc | KeyCode::Char('q'), KeyModifiers::NONE) => {
                    app.close_repository_search()
                }
                (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                    app.move_repository_search_selection(1)
                }
                (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                    app.move_repository_search_selection(-1)
                }
                (KeyCode::Enter, KeyModifiers::NONE) => app.select_repository_search_result(),
                (KeyCode::Char('?'), _) => app.open_help(),
                _ => {}
            }
            continue;
        }

        if app.view == View::Files {
            match (key.code, key.modifiers) {
                (KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t'), KeyModifiers::NONE) => {
                    app.close_files()
                }
                (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                    app.move_file_selection(1)
                }
                (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                    app.move_file_selection(-1)
                }
                (KeyCode::Enter, KeyModifiers::NONE) => app.select_diff_file(),
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

        if app.view == View::Context {
            if app.input_mode == InputMode::Question {
                match key.code {
                    KeyCode::Esc => app.cancel_understanding_question(),
                    KeyCode::Enter => app.submit_understanding_question(),
                    KeyCode::Backspace => app.question_backspace(),
                    KeyCode::Char(character) => app.question_push(character),
                    _ => {}
                }
                continue;
            }
            let (terminal_width, terminal_height) = crossterm::terminal::size()?;
            let page = terminal_height.saturating_sub(1).max(1) as isize;
            let half_page = (page / 2).max(1);
            match (key.code, key.modifiers) {
                (KeyCode::Esc, KeyModifiers::NONE) if app.cancel_agent_dispatch() => {}
                (KeyCode::Esc, KeyModifiers::NONE) if app.cancel_context_range() => {}
                (KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('b'), KeyModifiers::NONE) => {
                    app.close_context()
                }
                (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => app.move_context(1),
                (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => app.move_context(-1),
                (KeyCode::Char('h') | KeyCode::Left, KeyModifiers::NONE) => {
                    app.move_context_horizontal(-4)
                }
                (KeyCode::Char('l') | KeyCode::Right, KeyModifiers::NONE) => {
                    app.move_context_horizontal(4)
                }
                (KeyCode::Char('0'), KeyModifiers::NONE) => app.reset_context_horizontal(),
                (KeyCode::Char('$'), _) => {
                    app.select_context_line_end_horizontal(terminal_width as usize)
                }
                (KeyCode::Char('J'), KeyModifiers::SHIFT) => app.move_context(10),
                (KeyCode::Char('K'), KeyModifiers::SHIFT) => app.move_context(-10),
                (KeyCode::Char('d'), KeyModifiers::CONTROL) => app.move_context(half_page),
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => app.move_context(-half_page),
                (KeyCode::Char('f'), KeyModifiers::CONTROL) | (KeyCode::PageDown, _) => {
                    app.move_context(page)
                }
                (KeyCode::Char('b'), KeyModifiers::CONTROL) | (KeyCode::PageUp, _) => {
                    app.move_context(-page)
                }
                (KeyCode::Char('o'), KeyModifiers::CONTROL) => app.context_history_back(),
                (KeyCode::Char('i'), KeyModifiers::CONTROL) | (KeyCode::Tab, _) => {
                    app.context_history_forward()
                }
                (KeyCode::Char('g'), KeyModifiers::NONE) => app.handle_context_g(),
                (KeyCode::Char('d'), KeyModifiers::NONE) if app.pending_g => {
                    app.search_context_symbol(RepositorySearchKind::Definition)
                }
                (KeyCode::Char('r'), KeyModifiers::NONE) if app.pending_g => {
                    app.search_context_symbol(RepositorySearchKind::References)
                }
                (KeyCode::Char('w'), KeyModifiers::NONE) => app.move_context_symbol(1),
                (KeyCode::Char('W'), KeyModifiers::SHIFT) => app.move_context_symbol(-1),
                (KeyCode::Char('I'), KeyModifiers::SHIFT) => {
                    app.search_context_symbol(RepositorySearchKind::Impact)
                }
                (KeyCode::Char('G'), KeyModifiers::SHIFT) | (KeyCode::End, _) => {
                    app.select_context_end()
                }
                (KeyCode::Home, _) => {
                    app.context_cursor = 0;
                    app.context_symbol_column = 0;
                    app.pending_g = false;
                    app.context_jump_direction = None;
                }
                (KeyCode::Char(']'), KeyModifiers::NONE) => app.begin_context_jump(1),
                (KeyCode::Char('['), KeyModifiers::NONE) => app.begin_context_jump(-1),
                (KeyCode::Char('r'), KeyModifiers::NONE)
                    if app.context_jump_direction.is_some() =>
                {
                    app.complete_context_reading_jump()
                }
                (KeyCode::Char('c'), KeyModifiers::NONE)
                    if app.context_jump_direction.is_some() =>
                {
                    app.complete_context_change_jump()
                }
                (KeyCode::Char('m'), KeyModifiers::NONE)
                    if app.context_jump_direction.is_some() =>
                {
                    app.complete_context_bookmark_jump()
                }
                (KeyCode::Char('m'), KeyModifiers::NONE) => app.toggle_context_bookmark(),
                (KeyCode::Char('Q'), KeyModifiers::SHIFT) => app.start_understanding_question(),
                (KeyCode::Char('R'), KeyModifiers::SHIFT) => app.request_guided_reading(),
                (KeyCode::Char('T'), KeyModifiers::SHIFT) => app.toggle_context_reading_trail(),
                (KeyCode::Char('V'), KeyModifiers::SHIFT) => app.toggle_context_range(),
                (KeyCode::Char('s'), KeyModifiers::NONE) => app.open_sessions(),
                (KeyCode::Char('/'), KeyModifiers::NONE) => app.start_search(),
                (KeyCode::Char('*'), _) => app.start_repository_search(),
                (KeyCode::Char('n'), KeyModifiers::NONE) => app.repeat_search(1),
                (KeyCode::Char('N'), KeyModifiers::SHIFT) => app.repeat_search(-1),
                (KeyCode::Char('?'), _) => app.open_help(),
                _ => {
                    app.pending_g = false;
                    app.context_jump_direction = None;
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
                (KeyCode::Esc, KeyModifiers::NONE) if app.cancel_agent_dispatch() => {}
                (KeyCode::Esc, KeyModifiers::NONE) if app.cancel_range_selection() => {}
                (KeyCode::Esc | KeyCode::Char('q'), KeyModifiers::NONE) => app.close_diff(),
                (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                    if app.range_start.is_some() {
                        app.move_diff(1);
                    } else {
                        app.move_diff_cursor(1);
                    }
                }
                (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                    if app.range_start.is_some() {
                        app.move_diff(-1);
                    } else {
                        app.move_diff_cursor(-1);
                    }
                }
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
                    app.diff_thread_selected = None;
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
                (KeyCode::Char('i'), KeyModifiers::NONE) if app.diff_jump_direction.is_some() => {
                    app.complete_diff_jump(DiffTarget::Intent)
                }
                (KeyCode::Char('i'), KeyModifiers::NONE) => app.toggle_agent_intents(),
                (KeyCode::Char('}'), _) => app.jump_diff(DiffTarget::Change, 1),
                (KeyCode::Char('{'), _) => app.jump_diff(DiffTarget::Change, -1),
                (KeyCode::Tab, _) => app.jump_diff(DiffTarget::File, 1),
                (KeyCode::BackTab, _) => app.jump_diff(DiffTarget::File, -1),
                (KeyCode::Char('a'), KeyModifiers::NONE) => app.start_comment(),
                (KeyCode::Char('V'), KeyModifiers::SHIFT) => app.toggle_range_selection(),
                (KeyCode::Char('F'), KeyModifiers::SHIFT) => app.start_file_comment(),
                (KeyCode::Char('C'), KeyModifiers::SHIFT) => app.start_commit_comment(),
                (KeyCode::Enter, KeyModifiers::NONE) => app.activate_diff_selection(),
                (KeyCode::Char('v'), KeyModifiers::NONE) => app.open_comments(),
                (KeyCode::Char('t'), KeyModifiers::NONE) => app.open_files(),
                (KeyCode::Char(':'), KeyModifiers::NONE) => app.start_command(),
                (KeyCode::Char('S'), _) => app.request_send_review(),
                (KeyCode::Char('X'), KeyModifiers::SHIFT) => app.view_agent_changes(),
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
            InputMode::Question => unreachable!("questions are only entered in the context view"),
            InputMode::ConfirmSend => unreachable!("send confirmation is handled first"),
            InputMode::Search => unreachable!("search input is handled first"),
            InputMode::RepositorySearch => {
                unreachable!("repository search input is handled first")
            }
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
