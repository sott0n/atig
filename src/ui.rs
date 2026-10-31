use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::app::{App, InputMode, View};
use crate::git::{CommitLine, HistoryLine, fit_column, format_refs};

pub fn draw(frame: &mut Frame, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    if app.view == View::Diff {
        draw_diff(frame, app, &areas);
        return;
    }
    if app.view == View::Comments {
        draw_comments(frame, app, &areas);
        return;
    }

    let viewport_height = areas[0].height as usize;
    let start = viewport_start(app, viewport_height);
    let end = (start + viewport_height).min(app.history.len());
    let author_width = app
        .history
        .iter()
        .filter_map(|line| line.commit())
        .map(|commit| commit.author.chars().count())
        .max()
        .unwrap_or(0)
        .min(20);
    let lines = app.history[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            let index = start + offset;
            history_line(
                line,
                author_width,
                app.selected == Some(index),
                areas[0].width as usize,
            )
        })
        .collect::<Vec<_>>();

    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);

    match app.input_mode {
        InputMode::Normal => {
            let oid = app
                .selected
                .and_then(|selected| app.history[selected].commit())
                .map(|commit| commit.oid.as_str())
                .unwrap_or("-");
            let position = app.selected_commit_position().unwrap_or(0);
            let total = app.commit_count();
            let left = if total == 0 && !app.loading {
                app.message.clone()
            } else {
                format!(
                    "[{}] {oid} - commit {position} of {total}",
                    app.current_branch()
                )
            };
            let right = if app.loading { "loading" } else { "100%" };
            let available = areas[1].width as usize;
            let gap = available.saturating_sub(left.chars().count() + right.len());
            let status = format!("{left}{}{right}", " ".repeat(gap));
            frame.render_widget(
                Paragraph::new(status).style(
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                areas[1],
            );
        }
        InputMode::Command => {
            let prompt = format!(":{}", app.command);
            frame.render_widget(Paragraph::new(prompt.as_str()), areas[1]);
            let cursor_x = areas[1]
                .x
                .saturating_add(1)
                .saturating_add(app.command.chars().count() as u16)
                .min(areas[1].right().saturating_sub(1));
            frame.set_cursor_position((cursor_x, areas[1].y));
        }
        InputMode::Comment => unreachable!("comments are drawn in the diff view"),
    }
}

fn draw_comments(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let height = areas[0].height as usize;
    let start = app
        .comment_selected
        .saturating_sub((height / 4).max(1))
        .min(app.comments.len().saturating_sub(height));
    let lines = app
        .comments
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(index, comment)| {
            let text = format!(
                "{}:{} [{}] {}",
                comment.file, comment.line, comment.commit, comment.body
            );
            if index == app.comment_selected {
                let mut line = Line::styled(
                    text,
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                );
                line.push_span(Span::raw(
                    " ".repeat((areas[0].width as usize).saturating_sub(line.width())),
                ));
                line
            } else {
                Line::raw(text)
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);

    if app.input_mode == InputMode::Comment {
        draw_comment_prompt(frame, app, areas[1]);
        return;
    }
    let status = format!(
        "Review comments - {} total - j/k move  e edit  d delete  q back",
        app.comments.len()
    );
    frame.render_widget(
        Paragraph::new(status).style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn draw_comment_prompt(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let prompt = format!("Comment: {}", app.comment_buffer);
    frame.render_widget(Paragraph::new(prompt), area);
    let cursor_x = area
        .x
        .saturating_add("Comment: ".len() as u16)
        .saturating_add(app.comment_buffer.chars().count() as u16)
        .min(area.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, area.y));
}

fn draw_command_prompt(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let prompt = format!(":{}", app.command);
    frame.render_widget(Paragraph::new(prompt), area);
    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(app.command.chars().count() as u16)
        .min(area.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, area.y));
}

fn draw_diff(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let height = areas[0].height as usize;
    let start = diff_viewport_start(app, height);
    let lines = app
        .diff
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(index, line)| diff_line(line, app.diff_scroll == index, areas[0].width as usize))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);

    if app.input_mode == InputMode::Command {
        draw_command_prompt(frame, app, areas[1]);
        return;
    }
    if app.input_mode == InputMode::Comment {
        draw_comment_prompt(frame, app, areas[1]);
        return;
    }

    let oid = app.diff_oid.as_deref().unwrap_or("-");
    let position = if app.diff.is_empty() {
        0
    } else {
        app.diff_scroll + 1
    };
    let left = format!(
        "commit {oid} - line {position} of {} - {} comments",
        app.diff.len(),
        app.comments.len()
    );
    let right = if app.diff_loading {
        "loading".to_string()
    } else if app.diff.is_empty() {
        "0%".to_string()
    } else {
        format!("{}%", (position * 100) / app.diff.len())
    };
    let gap = (areas[1].width as usize).saturating_sub(left.chars().count() + right.len());
    frame.render_widget(
        Paragraph::new(format!("{left}{}{right}", " ".repeat(gap))).style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn diff_viewport_start(app: &App, height: usize) -> usize {
    if height == 0 || app.diff.len() <= height {
        return 0;
    }
    let padding = (height / 4).max(1);
    app.diff_scroll
        .saturating_sub(padding)
        .min(app.diff.len().saturating_sub(height))
}

fn diff_line(line: &str, selected: bool, viewport_width: usize) -> Line<'static> {
    if selected {
        let mut rendered = Line::styled(
            line.to_string(),
            Style::default()
                .fg(Color::White)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );
        rendered.push_span(Span::raw(
            " ".repeat(viewport_width.saturating_sub(rendered.width())),
        ));
        return rendered;
    }

    let style = if line.starts_with("diff --git ") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        Style::default().fg(Color::Cyan)
    } else if line.starts_with("+++") {
        Style::default().fg(Color::Green)
    } else if line.starts_with("---") {
        Style::default().fg(Color::Red)
    } else if line.starts_with('+') {
        Style::default().fg(Color::Green)
    } else if line.starts_with('-') {
        Style::default().fg(Color::Red)
    } else if line.starts_with("index ")
        || line.starts_with("new file mode ")
        || line.starts_with("deleted file mode ")
        || line.starts_with("similarity index ")
    {
        Style::default().fg(Color::Blue)
    } else if line.starts_with("commit ") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::styled(line.to_string(), style)
}

fn history_line(
    line: &HistoryLine,
    author_width: usize,
    selected: bool,
    viewport_width: usize,
) -> Line<'static> {
    if selected {
        let mut rendered = Line::styled(
            line.display_text(author_width),
            Style::default()
                .fg(Color::White)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );
        let padding = viewport_width.saturating_sub(rendered.width());
        rendered.push_span(Span::raw(" ".repeat(padding)));
        return rendered;
    }

    let HistoryLine::Commit(commit) = line else {
        return Line::styled(
            line.display_text(author_width),
            Style::default().fg(Color::DarkGray),
        );
    };

    let mut spans = vec![
        Span::styled(commit.date.clone(), Style::default().fg(Color::Blue)),
        Span::raw(" "),
        Span::styled(
            fit_column(&commit.author, author_width),
            Style::default().fg(Color::Green),
        ),
        Span::raw(" "),
    ];
    spans.extend(graph_spans(commit));

    let refs = format_refs(&commit.refs);
    if !refs.is_empty() {
        let modifier = if commit.refs.contains("HEAD -> ") {
            Modifier::BOLD
        } else {
            Modifier::empty()
        };
        spans.push(Span::styled(
            refs,
            Style::default()
                .fg(ref_color(&commit.refs))
                .add_modifier(modifier),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::raw(commit.subject.clone()));
    Line::from(spans)
}

fn graph_spans(commit: &CommitLine) -> Vec<Span<'static>> {
    const LANE_COLORS: [Color; 6] = [
        Color::Magenta,
        Color::Yellow,
        Color::Cyan,
        Color::Green,
        Color::White,
        Color::Red,
    ];

    commit
        .tig_graph()
        .chars()
        .enumerate()
        .map(|(index, character)| {
            let color = if matches!(character, 'M' | 'o' | 'I') {
                None
            } else if character == ' ' {
                return Span::raw(" ");
            } else {
                let lane = if matches!(character, '─' | '┐' | '┘') {
                    (index / 2).max(1)
                } else {
                    index / 2
                };
                Some(LANE_COLORS[lane % LANE_COLORS.len()])
            };
            Span::styled(
                character.to_string(),
                Style::default().fg(color.unwrap_or(Color::Blue)),
            )
        })
        .collect()
}

fn ref_color(refs: &str) -> Color {
    if refs
        .split(", ")
        .any(|reference| reference.starts_with("tag: "))
    {
        Color::Magenta
    } else if refs.split(", ").any(|reference| reference.contains('/')) {
        Color::Yellow
    } else {
        Color::Cyan
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
    use crate::git::parse_history_line;

    #[test]
    fn viewport_follows_selection_without_exceeding_history() {
        let mut app = App::new();
        app.history = (0..20)
            .map(|index| crate::git::HistoryLine::Graph(index.to_string()))
            .collect();
        app.selected = Some(18);

        assert_eq!(viewport_start(&app, 8), 12);
    }

    #[test]
    fn unselected_history_line_uses_tig_column_colors() {
        let line = parse_history_line(
            "* \u{1e}012345\u{1f}012345\u{1f}parent\u{1f}2026-11-01 10:30 +0900\u{1f}Alice\u{1f}HEAD -> main\u{1f}Initial commit",
        );

        let rendered = history_line(&line, 8, false, 100);

        assert_eq!(rendered.spans[0].style.fg, Some(Color::Blue));
        assert_eq!(rendered.spans[2].style.fg, Some(Color::Green));
        assert!(
            rendered
                .spans
                .iter()
                .any(|span| span.style.fg == Some(Color::Cyan))
        );
    }

    #[test]
    fn selected_history_line_matches_tig_cursor_style() {
        let line = parse_history_line(
            "* \u{1e}012345\u{1f}012345\u{1f}parent\u{1f}2026-11-01 10:30 +0900\u{1f}Alice\u{1f}\u{1f}Initial commit",
        );

        let rendered = history_line(&line, 8, true, 100);

        assert_eq!(rendered.style.fg, Some(Color::White));
        assert_eq!(rendered.style.bg, Some(Color::Green));
        assert!(rendered.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(rendered.width(), 100);
    }

    #[test]
    fn diff_lines_use_semantic_colors() {
        assert_eq!(diff_line("+added", false, 80).style.fg, Some(Color::Green));
        assert_eq!(diff_line("-removed", false, 80).style.fg, Some(Color::Red));
        assert_eq!(
            diff_line("@@ -1 +1 @@", false, 80).style.fg,
            Some(Color::Cyan)
        );
    }

    #[test]
    fn selected_diff_line_fills_the_viewport() {
        let rendered = diff_line("+added", true, 80);

        assert_eq!(rendered.style.bg, Some(Color::Green));
        assert_eq!(rendered.width(), 80);
    }
}
