use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::{App, InputMode};

pub fn draw(frame: &mut Frame, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let viewport_height = areas[0].height.saturating_sub(2) as usize;
    let start = viewport_start(app, viewport_height);
    let end = (start + viewport_height).min(app.history.len());
    let lines = app.history[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            let index = start + offset;
            let style = if app.selected == Some(index) {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if line.commit().is_some() {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::styled(line.display_text(), style)
        })
        .collect::<Vec<_>>();

    let title = if app.loading {
        " atig [loading] "
    } else {
        " atig "
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(title)),
        areas[0],
    );

    frame.render_widget(
        Paragraph::new(app.message.as_str()).style(Style::default().fg(Color::DarkGray)),
        areas[1],
    );

    match app.input_mode {
        InputMode::Normal => {
            frame.render_widget(
                Paragraph::new("j/k move  gg/G ends  Ctrl-d/u page  : command  q quit")
                    .style(Style::default().fg(Color::DarkGray)),
                areas[2],
            );
        }
        InputMode::Command => {
            let prompt = format!(":{}", app.command);
            frame.render_widget(Paragraph::new(prompt.as_str()), areas[2]);
            let cursor_x = areas[2]
                .x
                .saturating_add(1)
                .saturating_add(app.command.chars().count() as u16)
                .min(areas[2].right().saturating_sub(1));
            frame.set_cursor_position((cursor_x, areas[2].y));
        }
    }
}

fn viewport_start(app: &App, height: usize) -> usize {
    if height == 0 || app.history.len() <= height {
        return 0;
    }

    let selected = app.selected.unwrap_or(0);
    let padding = (height / 4).max(1);
    selected
        .saturating_sub(padding)
        .min(app.history.len().saturating_sub(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_follows_selection_without_exceeding_history() {
        let mut app = App::new();
        app.history = (0..20)
            .map(|index| crate::git::HistoryLine::Graph(index.to_string()))
            .collect();
        app.selected = Some(18);

        assert_eq!(viewport_start(&app, 8), 12);
    }
}
