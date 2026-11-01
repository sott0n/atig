use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::app::{App, InputMode, ReviewComment, ReviewThreadRole, View};
use crate::git::{CommitLine, HistoryLine, fit_column, format_refs};

pub fn draw(frame: &mut Frame, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    if app.view == View::Help {
        draw_help(frame, &areas);
        return;
    }
    if app.view == View::Diff {
        draw_diff(frame, app, &areas);
        return;
    }
    if app.view == View::Comments {
        draw_comments(frame, app, &areas);
        return;
    }
    if app.view == View::Files {
        draw_files(frame, app, &areas);
        return;
    }
    if app.view == View::Sessions {
        draw_sessions(frame, app, &areas);
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
                    "[{}] {oid} - commit {position} of {total}{}",
                    app.current_branch(),
                    agent_target_suffix(app)
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
        InputMode::ConfirmSend => unreachable!("send confirmation is drawn in the review view"),
        InputMode::Search => draw_search_prompt(frame, app, areas[1]),
    }
}

fn draw_help(frame: &mut Frame, areas: &[ratatui::layout::Rect]) {
    let lines = [
        "atig keys",
        "",
        "j/k, arrows       move",
        "gg / G            first / last",
        "Ctrl-d / Ctrl-u   half page down / up",
        "Ctrl-f / Ctrl-b   full page down / up",
        "PgDn / PgUp       full page down / up",
        "J / K             move 10 lines",
        "Enter             open / edit / reply",
        "Esc, q            back",
        "[f / ]f, Tab       previous / next file",
        "[c / ]c, { / }     previous / next change",
        ":goto <line>       jump to a diff line",
        "/, n, N           search / next / previous",
        "r                 reload",
        "a                 add review comment",
        "V, move, a        review a line range",
        "F / C             review file / commit",
        "v                 view review comments",
        "t                 changed file navigator",
        "s                 select an agent session",
        "S                 send review comments",
        "X                 view Agent changes",
        ":                 command",
        "?                 toggle this help",
    ]
    .into_iter()
    .map(Line::raw)
    .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);
    frame.render_widget(
        Paragraph::new("Help - q, Esc, or ? to return").style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn draw_files(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let files = app.diff_files();
    let height = areas[0].height as usize;
    let start = app
        .file_selected
        .saturating_sub((height / 4).max(1))
        .min(files.len().saturating_sub(height));
    let lines = files
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(index, file)| {
            let marker = if file.agent_changed { "◆" } else { " " };
            let reviews = if file.review_count == 0 {
                String::new()
            } else {
                format!("  ●{}", file.review_count)
            };
            let text = format!(" {marker} {}{reviews}", file.path);
            if index == app.file_selected {
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
                Line::styled(
                    text,
                    Style::default().fg(if file.agent_changed {
                        Color::Magenta
                    } else {
                        Color::White
                    }),
                )
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);
    frame.render_widget(
        Paragraph::new("◆ Agent changed  ● reviews  j/k move  Enter jump  q back").style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn draw_sessions(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let height = areas[0].height as usize;
    let start = app
        .session_selected
        .saturating_sub((height / 4).max(1))
        .min(app.sessions.len().saturating_sub(height));
    let lines = app
        .sessions
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(index, session)| {
            let active = app.agent_target.as_ref() == Some(&session.target);
            let text = format!(
                "{} {} [{}] {}",
                if active { "*" } else { " " },
                session.target.label(),
                session.status,
                session.title
            );
            if index == app.session_selected {
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

    let left = app
        .sessions
        .get(app.session_selected)
        .map(|session| session.cwd.as_str())
        .unwrap_or(if app.sessions_loading {
            "Discovering sessions..."
        } else {
            "No sessions found for this repository"
        });
    let right = "j/k move  Enter select  r reload  q back";
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
                "{} [{}] {}",
                comment.scope.label(),
                comment.commit,
                comment.body
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

    if app.input_mode == InputMode::ConfirmSend {
        draw_send_confirmation(frame, app, areas[1]);
        return;
    }
    if app.input_mode == InputMode::Comment {
        draw_comment_prompt(frame, app, areas[1]);
        return;
    }
    let status = app.delivery_message.clone().unwrap_or_else(|| {
        format!(
            "Review comments - {} total{} - j/k move  e edit  d delete  q back",
            app.comments.len(),
            agent_target_suffix(app)
        )
    });
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

fn draw_search_prompt(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let prompt = format!("/{}", app.search_input);
    frame.render_widget(Paragraph::new(prompt), area);
    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(app.search_input.chars().count() as u16)
        .min(area.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, area.y));
}

fn draw_send_confirmation(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let target = app
        .agent_target
        .as_ref()
        .map(|target| target.label())
        .unwrap_or_else(|| "no target".into());
    let prompt = if let Some(index) = app.pending_send_comment_index() {
        format!("Send Review #{} to {target}? [y/N]", index + 1)
    } else {
        format!(
            "Send all {} comments to {target}? [y/N]",
            app.comments.len()
        )
    };
    frame.render_widget(
        Paragraph::new(prompt).style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn draw_diff(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let height = areas[0].height as usize;
    let commenting = app.input_mode == InputMode::Comment;
    let mut start = diff_viewport_start(app, height);
    let rows_before_selection = (start..app.diff_scroll)
        .map(|index| rendered_diff_row_height(app, index, areas[0].width as usize))
        .sum::<usize>();
    let selected_rows = 1 + if commenting { 3 } else { 0 };
    if rows_before_selection + selected_rows > height {
        start = app.diff_scroll;
    }

    let mut lines = Vec::with_capacity(height);
    let mut comment_input_row = None;
    for (index, line) in app.diff.iter().enumerate().skip(start) {
        if lines.len() >= height {
            break;
        }
        lines.push(diff_line(
            line,
            (app.diff_scroll == index && app.diff_comment_selected.is_none())
                || app.range_contains(index),
            areas[0].width as usize,
        ));
        if commenting && app.diff_scroll == index && app.followup_comment_index().is_none() {
            comment_input_row = Some(lines.len() + 1);
            lines.extend(inline_comment_box(
                &app.comment_buffer,
                app.editing_comment_index().is_some(),
                false,
                app.comment_scope_label().as_deref(),
                areas[0].width as usize,
            ));
        }
        for (comment_index, comment) in comments_for_diff_line(app, index) {
            lines.extend(saved_comment_box(
                comment,
                comment_index + 1,
                (app.diff_comment_selected == Some(comment_index))
                    .then_some(app.diff_thread_selected),
                areas[0].width as usize,
            ));
            if app.followup_comment_index() == Some(comment_index) {
                comment_input_row = Some(lines.len() + 1);
                lines.extend(inline_comment_box(
                    &app.comment_buffer,
                    false,
                    true,
                    app.comment_scope_label().as_deref(),
                    areas[0].width as usize,
                ));
            }
        }
    }
    lines.truncate(height);
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);

    if app.input_mode == InputMode::ConfirmSend {
        draw_send_confirmation(frame, app, areas[1]);
        return;
    }
    if app.input_mode == InputMode::Search {
        draw_search_prompt(frame, app, areas[1]);
        return;
    }
    if app.input_mode == InputMode::Command {
        draw_command_prompt(frame, app, areas[1]);
        return;
    }
    if app.input_mode == InputMode::Comment {
        let target = app
            .comment_scope_label()
            .unwrap_or_else(|| format!("LINE {}", app.diff_scroll + 1));
        let status = format!("Writing {target} review{}", agent_target_suffix(app));
        frame.render_widget(
            Paragraph::new(status).style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
            areas[1],
        );
        let input_y = areas[0]
            .y
            .saturating_add(comment_input_row.unwrap_or(1) as u16)
            .min(areas[0].bottom().saturating_sub(1));
        let cursor_x = areas[0]
            .x
            .saturating_add(4)
            .saturating_add(
                visible_comment_input(&app.comment_buffer, areas[0].width as usize)
                    .chars()
                    .count() as u16,
            )
            .min(areas[0].right().saturating_sub(1));
        frame.set_cursor_position((cursor_x, input_y));
        return;
    }

    let oid = app.diff_oid.as_deref().unwrap_or("-");
    let position = if app.diff.is_empty() {
        0
    } else {
        app.diff_scroll + 1
    };
    let left = if let Some(start) = app.range_start {
        format!(
            "RANGE {}-{} - move j/k, a comment, Esc cancel",
            start.min(app.diff_scroll) + 1,
            start.max(app.diff_scroll) + 1
        )
    } else if let Some(message) = &app.delivery_message {
        message.clone()
    } else if let Some(comment_index) = app.diff_comment_selected {
        let action = if app.diff_thread_selected.is_some() {
            "Enter reply"
        } else {
            "Enter edit  S send"
        };
        format!(
            "Review #{:02} - {action} - X changes - {} comments{}{}",
            comment_index + 1,
            app.comments.len(),
            if app.dispatching { " - sending" } else { "" },
            agent_target_suffix(app)
        )
    } else {
        format!(
            "commit {oid} - line {position} of {} - {} comments{}{}",
            app.diff.len(),
            app.comments.len(),
            if app.dispatching { " - sending" } else { "" },
            agent_target_suffix(app)
        )
    };
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

fn comments_for_diff_line(
    app: &App,
    diff_index: usize,
) -> impl Iterator<Item = (usize, &ReviewComment)> {
    app.comments
        .iter()
        .enumerate()
        .filter(move |(index, comment)| {
            comment.diff_index == diff_index
                && app.diff_oid.as_deref() == Some(comment.commit.as_str())
                && app.editing_comment_index() != Some(*index)
        })
}

fn rendered_diff_row_height(app: &App, diff_index: usize, width: usize) -> usize {
    1 + comments_for_diff_line(app, diff_index)
        .map(|(comment_index, comment)| {
            saved_comment_box(
                comment,
                comment_index + 1,
                (app.diff_comment_selected == Some(comment_index))
                    .then_some(app.diff_thread_selected),
                width,
            )
            .len()
        })
        .sum::<usize>()
}

fn saved_comment_box(
    comment: &ReviewComment,
    number: usize,
    selection: Option<Option<usize>>,
    width: usize,
) -> Vec<Line<'static>> {
    let root_selected = selection == Some(None);
    let root_color = if root_selected {
        Color::Green
    } else {
        Color::Yellow
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            if root_selected { "  ▶ " } else { "  ● " },
            Style::default().fg(root_color),
        ),
        Span::styled(
            format!("REVIEW {number:02}"),
            Style::default().fg(root_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", comment.scope.label()),
            Style::default().fg(Color::DarkGray),
        ),
    ])];
    let content_width = width.saturating_sub(4).max(1);
    for chunk in wrap_comment_body(&comment.body, content_width) {
        lines.push(Line::from(vec![
            Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(chunk, Style::default().fg(Color::White)),
        ]));
    }
    for (thread_index, message) in comment.thread.iter().enumerate() {
        let selected = selection == Some(Some(thread_index));
        let (label, role_color, marker) = match &message.role {
            ReviewThreadRole::Human => ("YOU".to_string(), Color::Cyan, "  ● "),
            ReviewThreadRole::Agent(agent) => (format!("AGENT · {agent}"), Color::Magenta, "  ◆ "),
        };
        let color = if selected { Color::Green } else { role_color };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "  ▶ " } else { marker },
                Style::default().fg(color),
            ),
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
        for chunk in wrap_comment_body(&message.body, content_width) {
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(chunk, Style::default().fg(Color::White)),
            ]));
        }
    }
    lines
}

fn wrap_comment_body(body: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for logical_line in body.lines() {
        let characters = logical_line.chars().collect::<Vec<_>>();
        if characters.is_empty() {
            wrapped.push(String::new());
        } else {
            wrapped.extend(
                characters
                    .chunks(width)
                    .map(|chunk| chunk.iter().collect::<String>()),
            );
        }
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

fn inline_comment_box(
    buffer: &str,
    editing: bool,
    followup: bool,
    scope: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    let accent = Style::default().fg(Color::Green);
    let input = visible_comment_input(buffer, width);

    vec![
        Line::from(vec![
            Span::styled("  ▶ ", accent),
            Span::styled(
                format!(
                    "{}{}",
                    if followup {
                        "REPLY TO AGENT"
                    } else if editing {
                        "EDIT REVIEW"
                    } else {
                        "NEW REVIEW"
                    },
                    scope.map(|scope| format!(" · {scope}")).unwrap_or_default()
                ),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(input, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  └ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if editing {
                    "Enter update · Esc cancel"
                } else if followup {
                    "Enter add reply · S sends thread · Esc cancel"
                } else {
                    "Enter save · then S send · Esc cancel"
                },
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ]
}

fn visible_comment_input(buffer: &str, width: usize) -> String {
    let available = width.saturating_sub(4).max(1);
    let mut visible = buffer.chars().rev().take(available).collect::<Vec<_>>();
    visible.reverse();
    visible.into_iter().collect()
}

fn agent_target_suffix(app: &App) -> String {
    app.agent_target
        .as_ref()
        .map(|target| format!(" - agent session {}", target.short_label()))
        .unwrap_or_default()
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

    #[test]
    fn inline_comment_editor_uses_compact_accent_lines() {
        let rendered = inline_comment_box(
            "Please simplify this",
            false,
            false,
            Some("LINE · src/main.rs:12"),
            60,
        );

        assert_eq!(rendered.len(), 3);
        assert!(
            rendered
                .iter()
                .all(|line| line.to_string().starts_with("  "))
        );
        assert!(
            rendered[0]
                .to_string()
                .contains("NEW REVIEW · LINE · src/main.rs:12")
        );
        assert!(rendered[1].to_string().contains("Please simplify this"));
        assert!(
            rendered[2]
                .to_string()
                .contains("Enter save · then S send · Esc cancel")
        );
    }

    #[test]
    fn saved_comment_card_wraps_with_location_header() {
        let comment = ReviewComment {
            commit: "abc123".into(),
            scope: crate::app::ReviewScope::Line {
                file: "src/main.rs".into(),
                line: 12,
            },
            diff_index: 5,
            body: "This comment is long enough to wrap".into(),
            context: Vec::new(),
            thread: Vec::new(),
        };

        let rendered = saved_comment_box(&comment, 1, None, 24);

        assert_eq!(rendered.len(), 3);
        assert!(
            rendered
                .iter()
                .all(|line| line.to_string().starts_with("  "))
        );
        assert!(
            rendered[0]
                .to_string()
                .contains("REVIEW 01 · LINE · src/main.rs:12")
        );
    }

    #[test]
    fn comment_input_keeps_the_visible_tail() {
        assert_eq!(visible_comment_input("abcdefghijk", 10), "fghijk");
    }

    #[test]
    fn agent_reply_is_rendered_below_its_comment() {
        let comment = ReviewComment {
            commit: "abc123".into(),
            scope: crate::app::ReviewScope::Line {
                file: "src/main.rs".into(),
                line: 12,
            },
            diff_index: 5,
            body: "Please simplify".into(),
            context: Vec::new(),
            thread: vec![crate::app::ReviewThreadMessage {
                role: ReviewThreadRole::Agent("codex:session-1".into()),
                body: "Implemented.\nTests pass.".into(),
            }],
        };

        let rendered = saved_comment_box(&comment, 1, Some(Some(0)), 60);
        let text = rendered
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("AGENT · codex:session-1"));
        assert!(text.contains("Implemented."));
        assert!(text.contains("Tests pass."));
    }

    #[test]
    fn agent_follow_up_editor_names_the_thread_action() {
        let rendered = inline_comment_box(
            "One more change",
            false,
            true,
            Some("LINE · src/main.rs:12"),
            60,
        );
        let text = rendered
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("REPLY TO AGENT"));
        assert!(text.contains("S sends thread"));
    }
}
