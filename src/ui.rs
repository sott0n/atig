use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{
    AgentIntent, App, ImpactKind, InputMode, RepositorySearchKind, ReviewComment, ReviewThreadRole,
    View, visible_agent_response,
};
use crate::git::{CommitLine, HistoryLine, fit_column, format_refs};

// Follow tig's default terminal palette instead of imposing an RGB theme.
const THEME_TEXT: Color = Color::White;
const THEME_BODY: Color = Color::Reset;
const THEME_MUTED: Color = Color::DarkGray;
const THEME_COOL: Color = Color::Blue;
const THEME_ACCENT: Color = Color::Cyan;
const THEME_POSITIVE: Color = Color::Green;
const THEME_NEGATIVE: Color = Color::Red;
const THEME_WARNING: Color = Color::Yellow;
const THEME_SECONDARY: Color = Color::Magenta;
const THEME_TYPE: Color = Color::Cyan;
const THEME_SELECTION_BACKGROUND: Color = Color::Green;
const THEME_RANGE_BACKGROUND: Color = Color::Blue;
const THEME_STATUS_BACKGROUND: Color = Color::Blue;
const THEME_PROMPT_BACKGROUND: Color = Color::Magenta;
const THEME_DANGER_BACKGROUND: Color = Color::Red;
const THEME_SYMBOL_BACKGROUND: Color = Color::Blue;
const DIFF_ADDED_MARKER: Color = Color::Green;
const DIFF_REMOVED_MARKER: Color = Color::Red;
const ADDED_LINE_BACKGROUND: Color = Color::Rgb(27, 51, 36);
const REMOVED_LINE_BACKGROUND: Color = Color::Rgb(58, 35, 39);

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
    if app.view == View::Context {
        draw_context(frame, app, &areas);
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
    if app.view == View::RepositorySearch {
        draw_repository_search(frame, app, &areas);
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
                        .fg(THEME_TEXT)
                        .bg(THEME_STATUS_BACKGROUND)
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
        InputMode::Question => unreachable!("questions are drawn in the context view"),
        InputMode::ConfirmSend => unreachable!("send confirmation is drawn in the review view"),
        InputMode::Search => draw_search_prompt(frame, app, areas[1]),
        InputMode::RepositorySearch => {
            unreachable!("repository search starts from code context")
        }
    }
}

fn draw_help(frame: &mut Frame, areas: &[ratatui::layout::Rect]) {
    let lines = [
        "atig keys",
        "",
        "j/k, arrows       move",
        "Context h/l       scroll code left / right",
        "Context 0/$       code line start / end",
        "gg / G            first / last",
        "Ctrl-d / Ctrl-u   half page down / up",
        "Ctrl-f / Ctrl-b   full page down / up",
        "PgDn / PgUp       full page down / up",
        "J / K             move 10 lines",
        "Enter             open code context / edit / reply",
        "Esc, q            back",
        "[f / ]f, Tab       previous / next file",
        "[c / ]c, { / }     previous / next change",
        "[i / ]i            previous / next Agent Intent",
        "[r / ]r            previous / next reading point",
        "[c / ]c            previous / next changed line",
        "m, [m / ]m        toggle / visit code bookmarks",
        "w / W             next / previous symbol on line",
        "gd / gr           symbol definitions / references",
        "I                 map selected symbol impact",
        "Ctrl-o / Ctrl-i    older / newer context location",
        "Q                 ask Agent about context line",
        "V, move, Q        ask Agent about a code range",
        "*                 search the repository version",
        "R                 open code and build Agent reading path",
        "T                 expand / collapse reading path",
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
        "i                 toggle Agent Intents",
        "b                 back from code context",
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
                .fg(THEME_TEXT)
                .bg(THEME_STATUS_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn draw_context(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let height = areas[0].height as usize;
    let mut trail_header = if let Some(point) = app.current_reading_point() {
        reading_arrival_lines(app, point, areas[0].width as usize)
    } else if !app.context_reading_path.is_empty() {
        vec![Line::from(vec![
            Span::styled(" ◆ ", Style::default().fg(THEME_SECONDARY)),
            Span::styled(
                format!(
                    "READING TRAIL · {}/{} visited · ]r start · T list",
                    app.context_reading_visited.len(),
                    app.context_reading_path.len(),
                ),
                Style::default()
                    .fg(THEME_SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ])]
    } else {
        Vec::new()
    };
    if app.show_context_reading_trail {
        trail_header.extend(reading_trail_lines(app));
    }
    let source_height = height.saturating_sub(trail_header.len());
    let mut start = app
        .context_cursor
        .saturating_sub((source_height / 4).max(1))
        .min(app.context_lines.len().saturating_sub(source_height));
    let rows_before_selection = trail_header.len()
        + (start..app.context_cursor)
            .map(|index| {
                1 + app
                    .current_reading_annotations(index + 1)
                    .into_iter()
                    .map(|annotation| {
                        temporary_annotation_lines(annotation, areas[0].width as usize).len()
                    })
                    .sum::<usize>()
                    + app
                        .latest_understanding_thread_at_context(index + 1)
                        .map(|(thread, hidden)| {
                            understanding_thread_box(
                                thread,
                                hidden,
                                areas[0].width as usize,
                                app.dispatch_animation_frame,
                            )
                            .len()
                        })
                        .unwrap_or(0)
            })
            .sum::<usize>();
    if rows_before_selection >= height {
        start = app.context_cursor;
    }
    let number_width = app.context_lines.len().max(1).to_string().len();
    let syntax = language_for_path(&app.context_path);
    let mut lines = trail_header;
    for (index, text) in app.context_lines.iter().enumerate().skip(start) {
        if lines.len() >= height {
            break;
        }
        let selection = if index == app.context_cursor {
            SourceSelection::Cursor
        } else if app.context_line_is_selected(index + 1) {
            SourceSelection::Range
        } else {
            SourceSelection::None
        };
        for annotation in app.current_reading_annotations(index + 1) {
            lines.extend(temporary_annotation_lines(
                annotation,
                areas[0].width as usize,
            ));
        }
        lines.push(source_line(
            index + 1,
            text,
            number_width,
            selection,
            SourceGutter {
                changed_side: app.context_change_side(index + 1),
                bookmarked: app.context_line_is_bookmarked(index + 1),
                symbol: (index == app.context_cursor)
                    .then(|| app.current_context_symbol_char_span())
                    .flatten(),
            },
            SourceViewport {
                syntax,
                horizontal_scroll: app.context_horizontal_scroll,
                width: areas[0].width as usize,
            },
        ));
        if let Some((thread, hidden)) = app.latest_understanding_thread_at_context(index + 1) {
            lines.extend(understanding_thread_box(
                thread,
                hidden,
                areas[0].width as usize,
                app.dispatch_animation_frame,
            ));
        }
    }
    lines.truncate(height);
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);

    if app.input_mode == InputMode::Question {
        let prompt = format!("Ask Agent: {}", app.question_buffer);
        frame.render_widget(
            Paragraph::new(prompt).style(
                Style::default()
                    .fg(THEME_TEXT)
                    .bg(THEME_PROMPT_BACKGROUND)
                    .add_modifier(Modifier::BOLD),
            ),
            areas[1],
        );
        let cursor_x = areas[1]
            .x
            .saturating_add("Ask Agent: ".len() as u16)
            .saturating_add(app.question_buffer.chars().count() as u16)
            .min(areas[1].right().saturating_sub(1));
        frame.set_cursor_position((cursor_x, areas[1].y));
        return;
    }
    if app.input_mode == InputMode::Search {
        draw_search_prompt(frame, app, areas[1]);
        return;
    }
    if app.input_mode == InputMode::RepositorySearch {
        draw_repository_search_prompt(frame, app, areas[1]);
        return;
    }

    let position = if app.context_lines.is_empty() {
        0
    } else {
        app.context_cursor + 1
    };
    let left = app.delivery_status().unwrap_or_else(|| {
        if app.context_range_start.is_some() {
            let (start, end) = app.context_selected_range();
            format!("VISUAL LINES {start}-{end} · Q ask range · V/Esc cancel")
        } else {
            format!(
                "{} · {} · line {position}/{} · {} · w/W symbol · gd def · gr refs · I impact · ◆{} · m mark · / file · b diff",
                app.context_path,
                app.context_version,
                app.context_lines.len(),
                app.current_context_symbol()
                    .map(|(symbol, _, _)| symbol)
                    .unwrap_or("no symbol"),
                app.context_bookmark_count()
            )
        }
    });
    let right = if app.context_lines.is_empty() {
        "0%".to_string()
    } else {
        format!("{}%", (position * 100) / app.context_lines.len())
    };
    let gap = (areas[1].width as usize).saturating_sub(left.chars().count() + right.len());
    frame.render_widget(
        Paragraph::new(format!("{left}{}{right}", " ".repeat(gap))).style(
            Style::default()
                .fg(THEME_TEXT)
                .bg(THEME_STATUS_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn understanding_thread_box(
    thread: &crate::app::UnderstandingThread,
    hidden_earlier: usize,
    width: usize,
    animation_frame: usize,
) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(4).max(1);
    let guided = thread.is_guided_reading();
    let mut lines = if guided {
        vec![Line::from(vec![
            Span::styled("  ◆ ", Style::default().fg(THEME_SECONDARY)),
            Span::styled(
                format!("AGENT READING GUIDE · {}", thread.agent),
                Style::default()
                    .fg(THEME_SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ])]
    } else {
        let mut question = vec![Line::from(vec![
            Span::styled("  ? ", Style::default().fg(THEME_ACCENT)),
            Span::styled(
                "QUESTION",
                Style::default()
                    .fg(THEME_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                thread
                    .end_line
                    .filter(|end| *end != thread.line)
                    .map(|end| format!(" · LINES {}-{end}", thread.line))
                    .unwrap_or_default(),
                Style::default().fg(THEME_MUTED),
            ),
        ])];
        for chunk in wrap_comment_body(&thread.question, content_width) {
            question.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(THEME_MUTED)),
                Span::styled(chunk, Style::default().fg(THEME_TEXT)),
            ]));
        }
        question.push(Line::from(vec![
            Span::styled("  ◆ ", Style::default().fg(THEME_SECONDARY)),
            Span::styled(
                format!("LATEST ANSWER · {}", thread.agent),
                Style::default()
                    .fg(THEME_SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        question
    };
    let answer = thread
        .answer
        .clone()
        .unwrap_or_else(|| format!("Thinking{}", ".".repeat(animation_frame % 3 + 1)));
    append_agent_response(&mut lines, &answer, content_width);
    if !thread.references.is_empty() || hidden_earlier > 0 {
        lines.push(Line::from(vec![
            Span::styled("  └ ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                match (thread.references.len(), hidden_earlier) {
                    (0, hidden) => format!("{hidden} earlier response(s) hidden"),
                    (references, 0) => format!(
                        "READING TRAIL ABOVE · {references} location(s) · ]r next · T toggle list"
                    ),
                    (references, hidden) => format!(
                        "READING TRAIL ABOVE · {references} location(s) · {hidden} earlier response(s) hidden"
                    ),
                },
                Style::default().fg(THEME_MUTED),
            ),
        ]));
    }
    lines
}

fn temporary_annotation_lines(
    annotation: &crate::app::AgentCodeAnnotation,
    width: usize,
) -> Vec<Line<'static>> {
    let prefix = format!("       // ↓ L{} · ", annotation.line);
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let continuation = " ".repeat(prefix_width);
    let content_width = width.saturating_sub(prefix_width).max(1);
    wrap_comment_body(&annotation.comment, content_width)
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            Line::from(vec![
                Span::styled(
                    if index == 0 {
                        prefix.clone()
                    } else {
                        continuation.clone()
                    },
                    Style::default().fg(THEME_ACCENT),
                ),
                Span::styled(
                    chunk,
                    Style::default()
                        .fg(THEME_ACCENT)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])
        })
        .collect()
}

fn reading_point_location(point: &crate::app::AgentReadingPoint) -> String {
    if point.symbol.is_empty() {
        format!("{}:{}", point.file, point.line)
    } else {
        format!("{} · {}:{}", point.symbol, point.file, point.line)
    }
}

fn reading_arrival_lines(
    app: &App,
    point: &crate::app::AgentReadingPoint,
    width: usize,
) -> Vec<Line<'static>> {
    let position = app.context_reading_index.unwrap_or(0) + 1;
    let mut lines = vec![Line::from(vec![
        Span::styled(" ◆ ", Style::default().fg(THEME_SECONDARY)),
        Span::styled(
            format!(
                "READING TRAIL · STEP {position}/{}",
                app.context_reading_path.len()
            ),
            Style::default()
                .fg(THEME_SECONDARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", reading_point_location(point)),
            Style::default().fg(THEME_TEXT),
        ),
    ])];
    let reason_width = width.saturating_sub(20).max(1);
    for (index, chunk) in wrap_comment_body(&point.reason, reason_width)
        .into_iter()
        .enumerate()
    {
        lines.push(Line::from(vec![
            Span::styled(" │ ", Style::default().fg(THEME_SECONDARY)),
            Span::styled(
                if index == 0 {
                    "WHY THIS CODE · "
                } else {
                    "                "
                },
                Style::default()
                    .fg(THEME_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(chunk, Style::default().fg(THEME_TEXT)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled(" └ ", Style::default().fg(THEME_SECONDARY)),
        Span::styled(
            "temporary notes point to exact lines · ]r next · [r previous · Ctrl-o back · T full trail",
            Style::default().fg(THEME_MUTED),
        ),
    ]));
    lines
}

fn reading_trail_lines(app: &App) -> Vec<Line<'static>> {
    app.context_reading_path
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let current = app.context_reading_index == Some(index);
            let visited = app.context_reading_visited.contains(&index);
            let (marker, color) = if current {
                ('●', THEME_ACCENT)
            } else if visited {
                ('✓', THEME_POSITIVE)
            } else {
                ('○', THEME_MUTED)
            };
            Line::from(vec![
                Span::styled(format!("   {marker} "), Style::default().fg(color)),
                Span::styled(
                    format!("{} ", index + 1),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    reading_point_location(point),
                    Style::default().fg(THEME_TEXT),
                ),
                Span::styled(
                    format!(" · {}", point.reason),
                    Style::default().fg(THEME_MUTED),
                ),
            ])
        })
        .collect()
}

#[derive(Clone, Copy)]
enum SourceSelection {
    None,
    Cursor,
    Range,
}

#[derive(Clone, Copy)]
struct SourceGutter {
    changed_side: Option<crate::git::DiffSide>,
    bookmarked: bool,
    symbol: Option<(usize, usize)>,
}

#[derive(Clone, Copy)]
struct SourceViewport {
    syntax: Option<SyntaxLanguage>,
    horizontal_scroll: usize,
    width: usize,
}

fn source_line(
    number: usize,
    text: &str,
    number_width: usize,
    selection: SourceSelection,
    gutter: SourceGutter,
    viewport: SourceViewport,
) -> Line<'static> {
    let SourceViewport {
        syntax,
        horizontal_scroll,
        width: viewport_width,
    } = viewport;
    let marker = match gutter.changed_side {
        Some(crate::git::DiffSide::New) => '+',
        Some(crate::git::DiffSide::Old) => '-',
        None => ' ',
    };
    let bookmark_marker = if gutter.bookmarked { '◆' } else { ' ' };
    let prefix = format!("{marker}{bookmark_marker}{number:>number_width$} │ ");
    let code_width = viewport_width.saturating_sub(prefix.chars().count());
    if matches!(selection, SourceSelection::Cursor) {
        let mut spans = vec![Span::styled(
            prefix,
            Style::default()
                .fg(THEME_TEXT)
                .bg(THEME_SELECTION_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        )];
        spans.extend(source_code_spans(
            text,
            horizontal_scroll,
            code_width,
            syntax,
            Style::default().fg(THEME_TEXT),
            Some(THEME_SELECTION_BACKGROUND),
            gutter.symbol,
        ));
        let mut line = Line::from(spans);
        line.push_span(Span::styled(
            " ".repeat(viewport_width.saturating_sub(line.width())),
            Style::default().bg(THEME_SELECTION_BACKGROUND),
        ));
        line
    } else if matches!(selection, SourceSelection::Range) {
        let mut spans = vec![Span::styled(
            prefix,
            Style::default()
                .fg(THEME_TEXT)
                .bg(THEME_RANGE_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        )];
        spans.extend(source_code_spans(
            text,
            horizontal_scroll,
            code_width,
            syntax,
            Style::default().fg(THEME_TEXT),
            Some(THEME_RANGE_BACKGROUND),
            None,
        ));
        let mut line = Line::from(spans);
        line.push_span(Span::styled(
            " ".repeat(viewport_width.saturating_sub(line.width())),
            Style::default().bg(THEME_RANGE_BACKGROUND),
        ));
        line
    } else {
        let marker_color = match gutter.changed_side {
            Some(crate::git::DiffSide::New) => DIFF_ADDED_MARKER,
            Some(crate::git::DiffSide::Old) => DIFF_REMOVED_MARKER,
            None => THEME_MUTED,
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), Style::default().fg(marker_color)),
            Span::styled(
                bookmark_marker.to_string(),
                Style::default().fg(if gutter.bookmarked {
                    THEME_ACCENT
                } else {
                    THEME_MUTED
                }),
            ),
            Span::styled(
                format!("{number:>number_width$} │ "),
                Style::default().fg(THEME_MUTED),
            ),
        ];
        spans.extend(source_code_spans(
            text,
            horizontal_scroll,
            code_width,
            syntax,
            Style::default(),
            None,
            None,
        ));
        let background = match gutter.changed_side {
            Some(crate::git::DiffSide::New) => Some(ADDED_LINE_BACKGROUND),
            Some(crate::git::DiffSide::Old) => Some(REMOVED_LINE_BACKGROUND),
            None => None,
        };
        let mut line = Line::from(spans);
        if let Some(color) = background {
            line = line.style(Style::default().bg(color));
            line.push_span(Span::styled(
                " ".repeat(viewport_width.saturating_sub(line.width())),
                Style::default().bg(color),
            ));
        }
        line
    }
}

fn source_code_spans(
    text: &str,
    offset: usize,
    width: usize,
    syntax: Option<SyntaxLanguage>,
    base: Style,
    background: Option<Color>,
    symbol: Option<(usize, usize)>,
) -> Vec<Span<'static>> {
    let characters = text.chars().collect::<Vec<_>>();
    let offset = offset.min(characters.len());
    let left = usize::from(offset > 0);
    let available_without_left = width.saturating_sub(left);
    let right = usize::from(characters.len().saturating_sub(offset) > available_without_left);
    let body_width = width.saturating_sub(left + right);
    let end = offset.saturating_add(body_width).min(characters.len());
    let with_background = |style: Style| match background {
        Some(color) => style.bg(color),
        None => style,
    };
    let base = with_background(base);
    let selected = Style::default()
        .fg(THEME_TEXT)
        .bg(THEME_SYMBOL_BACKGROUND)
        .add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    if left > 0 {
        spans.push(Span::styled("‹", base));
    }
    let body = characters[offset..end].iter().collect::<String>();
    let mut source_position = offset;
    for token in highlight_code(&body, syntax, base) {
        for character in token.content.chars() {
            let style =
                if symbol.is_some_and(|(start, end)| (start..end).contains(&source_position)) {
                    selected
                } else {
                    with_background(token.style)
                };
            push_styled_character(&mut spans, character, style);
            source_position += 1;
        }
    }
    if right > 0 {
        spans.push(Span::styled("›", base));
    }
    spans
}

fn push_styled_character(spans: &mut Vec<Span<'static>>, character: char, style: Style) {
    if let Some(previous) = spans.last_mut()
        && previous.style == style
    {
        previous.content.to_mut().push(character);
    } else {
        spans.push(Span::styled(character.to_string(), style));
    }
}

#[cfg(test)]
fn visible_source_text(text: &str, offset: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let characters = text.chars().collect::<Vec<_>>();
    let offset = offset.min(characters.len());
    let left = usize::from(offset > 0);
    let available_without_left = width.saturating_sub(left);
    let right = usize::from(characters.len().saturating_sub(offset) > available_without_left);
    let body_width = width.saturating_sub(left + right);
    let mut visible = String::new();
    if left > 0 {
        visible.push('‹');
    }
    visible.extend(characters.iter().skip(offset).take(body_width));
    if right > 0 {
        visible.push('›');
    }
    visible
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
            let changed = if file.agent_changed { "Δ " } else { "  " };
            let intents = if file.intent_count == 0 {
                String::new()
            } else {
                format!("  ◆{}", file.intent_count)
            };
            let reviews = if file.review_count == 0 {
                String::new()
            } else {
                format!("  ●{}", file.review_count)
            };
            let text = format!(" {changed}{}{intents}{reviews}", file.path);
            if index == app.file_selected {
                let mut line = Line::styled(
                    text,
                    Style::default()
                        .fg(THEME_TEXT)
                        .bg(THEME_SELECTION_BACKGROUND)
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
                        THEME_SECONDARY
                    } else {
                        THEME_TEXT
                    }),
                )
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);
    frame.render_widget(
        Paragraph::new("Δ Agent changed  ◆ intents  ● reviews  j/k move  Enter jump  q back")
            .style(
                Style::default()
                    .fg(THEME_TEXT)
                    .bg(THEME_STATUS_BACKGROUND)
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
                        .fg(THEME_TEXT)
                        .bg(THEME_SELECTION_BACKGROUND)
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
                .fg(THEME_TEXT)
                .bg(THEME_STATUS_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn draw_repository_search(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let height = areas[0].height as usize;
    let start = app
        .repository_search_selected
        .saturating_sub((height / 4).max(1))
        .min(app.repository_search_results.len().saturating_sub(height));
    let lines = app
        .repository_search_results
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(index, found)| {
            let mut spans = Vec::new();
            if let Some(impact) = app.repository_result_impact(found) {
                let (label, color) = impact_style(impact);
                spans.push(Span::styled(
                    format!(" {label:<7} "),
                    Style::default()
                        .fg(THEME_TEXT)
                        .bg(color)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" "));
            }
            spans.extend([
                Span::styled(found.path.clone(), Style::default().fg(THEME_ACCENT)),
                Span::styled(
                    format!(":{} ", found.line),
                    Style::default().fg(THEME_WARNING),
                ),
                Span::raw(found.text.trim().to_string()),
            ]);
            let mut line = Line::from(spans);
            if index == app.repository_search_selected {
                line = line.style(
                    Style::default()
                        .fg(THEME_TEXT)
                        .bg(THEME_SELECTION_BACKGROUND)
                        .add_modifier(Modifier::BOLD),
                );
                line.push_span(Span::styled(
                    " ".repeat((areas[0].width as usize).saturating_sub(line.width())),
                    Style::default().bg(THEME_SELECTION_BACKGROUND),
                ));
            }
            line
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), areas[0]);

    let state = if app.repository_search_loading {
        "searching"
    } else if app.repository_search_results.len() == 200 {
        "200 matches (limit)"
    } else {
        "complete"
    };
    let search_label = match app.repository_search_kind {
        RepositorySearchKind::Text => "repo search",
        RepositorySearchKind::Definition => "definitions",
        RepositorySearchKind::References => "references",
        RepositorySearchKind::Impact => "impact map",
    };
    let left = if app.repository_search_kind == RepositorySearchKind::Impact {
        let [changed, definitions, tests, calls, references] = app.impact_counts();
        format!(
            "{search_label} '{}' · Δ{changed} def {definitions} calls {calls} tests {tests} refs {references} · {state}",
            app.repository_search_query
        )
    } else {
        format!(
            "{search_label} '{}' · {} matches · {state}",
            app.repository_search_query,
            app.repository_search_results.len()
        )
    };
    let right = "j/k move  Enter open  q back";
    let gap = (areas[1].width as usize).saturating_sub(left.chars().count() + right.len());
    frame.render_widget(
        Paragraph::new(format!("{left}{}{right}", " ".repeat(gap))).style(
            Style::default()
                .fg(THEME_TEXT)
                .bg(THEME_STATUS_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        ),
        areas[1],
    );
}

fn impact_style(impact: ImpactKind) -> (&'static str, Color) {
    match impact {
        ImpactKind::Changed => ("CHANGED", THEME_SECONDARY),
        ImpactKind::Definition => ("DEF", THEME_ACCENT),
        ImpactKind::Test => ("TEST", THEME_WARNING),
        ImpactKind::Call => ("CALL", THEME_POSITIVE),
        ImpactKind::Reference => ("REF", THEME_MUTED),
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
                "{} [{}] {}",
                comment.scope.label(),
                comment.commit,
                comment.body
            );
            if index == app.comment_selected {
                let mut line = Line::styled(
                    text,
                    Style::default()
                        .fg(THEME_TEXT)
                        .bg(THEME_SELECTION_BACKGROUND)
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
    let status = app.delivery_status().unwrap_or_else(|| {
        format!(
            "Review comments - {} total{} - j/k move  e edit  d delete  q back",
            app.comments.len(),
            agent_target_suffix(app)
        )
    });
    frame.render_widget(
        Paragraph::new(status).style(
            Style::default()
                .fg(THEME_TEXT)
                .bg(THEME_STATUS_BACKGROUND)
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

fn draw_repository_search_prompt(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let prefix = "Repo search: ";
    let prompt = format!("{prefix}{}", app.repository_search_input);
    frame.render_widget(Paragraph::new(prompt), area);
    let cursor_x = area
        .x
        .saturating_add(prefix.len() as u16)
        .saturating_add(app.repository_search_input.chars().count() as u16)
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
                .fg(THEME_TEXT)
                .bg(THEME_DANGER_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn draw_diff(frame: &mut Frame, app: &App, areas: &[ratatui::layout::Rect]) {
    let height = areas[0].height as usize;
    let commenting = app.input_mode == InputMode::Comment;
    let intent_anchors = if app.show_agent_intents {
        app.agent_intent_anchors()
    } else {
        HashMap::new()
    };
    let mut start = diff_viewport_start(app, height);
    let rows_before_selection = (start..app.diff_scroll)
        .map(|index| rendered_diff_row_height(app, index, areas[0].width as usize, &intent_anchors))
        .sum::<usize>();
    let selected_rows = 1 + if commenting { 3 } else { 0 };
    if rows_before_selection + selected_rows > height {
        start = app.diff_scroll;
    }

    let mut lines = Vec::with_capacity(height);
    let mut comment_input_row = None;
    let mut syntax = syntax_at_diff_line(&app.diff, start);
    for (index, line) in app.diff.iter().enumerate().skip(start) {
        if lines.len() >= height {
            break;
        }
        update_diff_syntax(line, &mut syntax);
        lines.push(diff_line(
            line,
            (app.diff_scroll == index && app.diff_comment_selected.is_none())
                || app.range_contains(index),
            areas[0].width as usize,
            syntax,
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
        for intent in intent_anchors.get(&index).into_iter().flatten() {
            lines.extend(agent_intent_box(intent, areas[0].width as usize));
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
                    .fg(THEME_TEXT)
                    .bg(THEME_STATUS_BACKGROUND)
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
    } else if let Some(message) = app.delivery_status() {
        message
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
            "commit {oid} - line {position} of {} - Enter context - {} comments{}{}",
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
                .fg(THEME_TEXT)
                .bg(THEME_STATUS_BACKGROUND)
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

fn rendered_diff_row_height(
    app: &App,
    diff_index: usize,
    width: usize,
    intent_anchors: &HashMap<usize, Vec<AgentIntent>>,
) -> usize {
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
        + intent_anchors
            .get(&diff_index)
            .into_iter()
            .flatten()
            .map(|intent| agent_intent_box(intent, width).len())
            .sum::<usize>()
}

fn agent_intent_box(intent: &AgentIntent, width: usize) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(4).max(1);
    let mut lines = vec![Line::from(vec![
        Span::styled("  ◆ ", Style::default().fg(THEME_SECONDARY)),
        Span::styled(
            "AGENT INTENT",
            Style::default()
                .fg(THEME_SECONDARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}:{}", intent.file, intent.line),
            Style::default().fg(THEME_MUTED),
        ),
    ])];
    for chunk in wrap_comment_body(&intent.intent, content_width) {
        lines.push(Line::from(vec![
            Span::styled("  │ ", Style::default().fg(THEME_MUTED)),
            Span::styled(chunk, Style::default().fg(THEME_TEXT)),
        ]));
    }
    for (index, point) in intent.reading_path.iter().enumerate() {
        lines.push(Line::from(vec![
            Span::styled("  ├ ", Style::default().fg(THEME_SECONDARY)),
            Span::styled(
                format!("READ {}", index + 1),
                Style::default()
                    .fg(THEME_SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" · {}", reading_point_location(point)),
                Style::default().fg(THEME_MUTED),
            ),
        ]));
        for chunk in wrap_comment_body(&point.reason, content_width) {
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(THEME_MUTED)),
                Span::styled(chunk, Style::default().fg(THEME_TEXT)),
            ]));
        }
    }
    lines
}

fn saved_comment_box(
    comment: &ReviewComment,
    number: usize,
    selection: Option<Option<usize>>,
    width: usize,
) -> Vec<Line<'static>> {
    let root_selected = selection == Some(None);
    let root_color = if root_selected {
        THEME_POSITIVE
    } else {
        THEME_WARNING
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
            Style::default().fg(THEME_MUTED),
        ),
    ])];
    let content_width = width.saturating_sub(4).max(1);
    for chunk in wrap_comment_body(&comment.body, content_width) {
        lines.push(Line::from(vec![
            Span::styled("  │ ", Style::default().fg(THEME_MUTED)),
            Span::styled(chunk, Style::default().fg(THEME_TEXT)),
        ]));
    }
    for (thread_index, message) in comment.thread.iter().enumerate() {
        let selected = selection == Some(Some(thread_index));
        let (label, role_color, marker, agent_response) = match &message.role {
            ReviewThreadRole::Human => ("YOU".to_string(), THEME_ACCENT, "  ● ", false),
            ReviewThreadRole::Agent(agent) => {
                (format!("AGENT · {agent}"), THEME_SECONDARY, "  ◆ ", true)
            }
        };
        let color = if selected { THEME_POSITIVE } else { role_color };
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
        let body = visible_agent_response(&message.body);
        if agent_response {
            append_agent_response(&mut lines, body, content_width);
        } else {
            for chunk in wrap_comment_body(body, content_width) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(THEME_MUTED)),
                    Span::styled(chunk, Style::default().fg(THEME_TEXT)),
                ]));
            }
        }
    }
    lines
}

fn append_agent_response(lines: &mut Vec<Line<'static>>, body: &str, width: usize) {
    for mut response_line in agent_response_lines(body, width) {
        response_line
            .spans
            .insert(0, Span::styled("  │ ", Style::default().fg(THEME_MUTED)));
        lines.push(response_line);
    }
}

fn agent_response_lines(body: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut fenced_code = false;
    for logical_line in body.lines() {
        let trimmed = logical_line.trim_start();
        if trimmed.starts_with("```") {
            fenced_code = !fenced_code;
            lines.push(Line::from(Span::styled(
                if fenced_code { "CODE" } else { "END CODE" },
                Style::default()
                    .fg(THEME_MUTED)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if logical_line.is_empty() {
            lines.push(Line::default());
            continue;
        }
        if fenced_code {
            for chunk in wrap_comment_body(logical_line, width) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(THEME_TEXT),
                )));
            }
            continue;
        }
        if let Some(heading) = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| trimmed.strip_prefix("# "))
        {
            for chunk in wrap_comment_body(heading, width) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD),
                )));
            }
            continue;
        }
        if let Some(body) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("• "))
        {
            lines.extend(agent_list_item_lines("• ", body, width));
            continue;
        }
        if let Some((marker, body)) = ordered_list_item(trimmed) {
            lines.extend(agent_list_item_lines(marker, body, width));
            continue;
        }
        for chunk in wrap_comment_body(logical_line, width) {
            lines.push(Line::from(inline_agent_spans(&chunk)));
        }
    }
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn agent_list_item_lines(marker: &str, body: &str, width: usize) -> Vec<Line<'static>> {
    let marker_width = UnicodeWidthStr::width(marker);
    let chunks = wrap_comment_body(body, width.saturating_sub(marker_width).max(1));
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let mut spans = vec![Span::styled(
                if index == 0 {
                    marker.to_string()
                } else {
                    " ".repeat(marker_width)
                },
                Style::default()
                    .fg(THEME_MUTED)
                    .add_modifier(Modifier::BOLD),
            )];
            spans.extend(inline_agent_spans(&chunk));
            Line::from(spans)
        })
        .collect()
}

fn ordered_list_item(line: &str) -> Option<(&str, &str)> {
    let digits = line
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let suffix = &line[digits..];
    if suffix.starts_with(". ") || suffix.starts_with(") ") {
        Some((&line[..digits + 2], &line[digits + 2..]))
    } else {
        None
    }
}

fn inline_agent_spans(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if let Some(code) = remaining.strip_prefix('`')
            && let Some(end) = code.find('`')
        {
            spans.push(Span::styled(
                code[..end].to_string(),
                Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD),
            ));
            remaining = &code[end + 1..];
            continue;
        }
        if let Some(bold) = remaining.strip_prefix("**")
            && let Some(end) = bold.find("**")
        {
            spans.push(Span::styled(
                bold[..end].to_string(),
                Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD),
            ));
            remaining = &bold[end + 2..];
            continue;
        }
        let next_markup = [remaining.find('`'), remaining.find("**")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(remaining.len());
        if next_markup == 0 {
            let character = remaining.chars().next().expect("non-empty response text");
            push_agent_plain_text(&mut spans, &character.to_string());
            remaining = &remaining[character.len_utf8()..];
        } else {
            push_agent_plain_text(&mut spans, &remaining[..next_markup]);
            remaining = &remaining[next_markup..];
        }
    }
    spans
}

fn push_agent_plain_text(spans: &mut Vec<Span<'static>>, text: &str) {
    for token in text.split_inclusive(char::is_whitespace) {
        let visible = token.trim_end_matches(char::is_whitespace);
        let location = visible.rsplit_once(':').is_some_and(|(path, line)| {
            line.trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<usize>()
                .is_ok()
                && (path.contains('/') || path.contains('.'))
        });
        spans.push(Span::styled(
            token.to_string(),
            if location {
                Style::default()
                    .fg(THEME_TEXT)
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                Style::default().fg(THEME_BODY)
            },
        ));
    }
}

fn wrap_comment_body(body: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for logical_line in body.lines() {
        if logical_line.is_empty() {
            wrapped.push(String::new());
        } else {
            let mut chunk = String::new();
            let mut chunk_width = 0;
            for character in logical_line.chars() {
                let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
                if !chunk.is_empty() && chunk_width + character_width > width {
                    wrapped.push(std::mem::take(&mut chunk));
                    chunk_width = 0;
                }
                chunk.push(character);
                chunk_width += character_width;
            }
            if !chunk.is_empty() {
                wrapped.push(chunk);
            }
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
    let accent = Style::default().fg(THEME_POSITIVE);
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
                    .fg(THEME_POSITIVE)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::default().fg(THEME_MUTED)),
            Span::styled(input, Style::default().fg(THEME_TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  └ ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                if editing {
                    "Enter update · Esc cancel"
                } else if followup {
                    "Enter add reply · S sends thread · Esc cancel"
                } else {
                    "Enter save · then S send · Esc cancel"
                },
                Style::default().fg(THEME_MUTED),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyntaxLanguage {
    Rust,
    JavaScript,
    Python,
    Go,
    CLike,
    Ruby,
    Shell,
    Data,
    Sql,
    Web,
}

fn syntax_at_diff_line(diff: &[String], end: usize) -> Option<SyntaxLanguage> {
    for line in diff.iter().take(end).rev() {
        if let Some(path) = line
            .strip_prefix("+++ b/")
            .or_else(|| line.strip_prefix("--- a/"))
        {
            return language_for_path(path);
        }
        if line.starts_with("diff --git ") {
            break;
        }
    }
    None
}

fn update_diff_syntax(line: &str, syntax: &mut Option<SyntaxLanguage>) {
    if line.starts_with("diff --git ") {
        *syntax = None;
    }
    if let Some(path) = line
        .strip_prefix("--- a/")
        .or_else(|| line.strip_prefix("+++ b/"))
    {
        *syntax = language_for_path(path);
    }
}

fn language_for_path(path: &str) -> Option<SyntaxLanguage> {
    let path = path.trim_matches('"');
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map(|(_, extension)| extension)?;
    match extension.to_ascii_lowercase().as_str() {
        "rs" => Some(SyntaxLanguage::Rust),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some(SyntaxLanguage::JavaScript),
        "py" | "pyi" => Some(SyntaxLanguage::Python),
        "go" => Some(SyntaxLanguage::Go),
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "java" | "kt" | "kts" | "cs" | "swift" => {
            Some(SyntaxLanguage::CLike)
        }
        "rb" => Some(SyntaxLanguage::Ruby),
        "sh" | "bash" | "zsh" | "fish" => Some(SyntaxLanguage::Shell),
        "json" | "jsonc" | "yaml" | "yml" | "toml" => Some(SyntaxLanguage::Data),
        "sql" => Some(SyntaxLanguage::Sql),
        "html" | "htm" | "css" | "scss" | "sass" | "vue" | "svelte" => Some(SyntaxLanguage::Web),
        _ => None,
    }
}

fn diff_line(
    line: &str,
    selected: bool,
    viewport_width: usize,
    syntax: Option<SyntaxLanguage>,
) -> Line<'static> {
    let style = diff_line_style(line);
    let mut rendered = if is_diff_code_line(line) {
        highlighted_code_line(line, syntax, style, selected)
    } else {
        Line::styled(line.to_string(), style)
    };
    if selected {
        rendered = rendered.style(
            Style::default()
                .fg(THEME_TEXT)
                .bg(THEME_SELECTION_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        );
        rendered.push_span(Span::raw(
            " ".repeat(viewport_width.saturating_sub(rendered.width())),
        ));
    } else if line.starts_with('+') && !line.starts_with("+++") {
        rendered = rendered.style(Style::default().bg(ADDED_LINE_BACKGROUND));
        rendered.push_span(Span::styled(
            " ".repeat(viewport_width.saturating_sub(rendered.width())),
            Style::default().bg(ADDED_LINE_BACKGROUND),
        ));
    } else if line.starts_with('-') && !line.starts_with("---") {
        rendered = rendered.style(Style::default().bg(REMOVED_LINE_BACKGROUND));
        rendered.push_span(Span::styled(
            " ".repeat(viewport_width.saturating_sub(rendered.width())),
            Style::default().bg(REMOVED_LINE_BACKGROUND),
        ));
    }
    rendered
}

fn diff_line_style(line: &str) -> Style {
    if line.starts_with("diff --git ") {
        Style::default()
            .fg(THEME_WARNING)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        Style::default().fg(THEME_ACCENT)
    } else if line.starts_with("+++") {
        Style::default().fg(DIFF_ADDED_MARKER)
    } else if line.starts_with("---") {
        Style::default().fg(DIFF_REMOVED_MARKER)
    } else if line.starts_with('+') {
        Style::default().fg(DIFF_ADDED_MARKER)
    } else if line.starts_with('-') {
        Style::default().fg(DIFF_REMOVED_MARKER)
    } else if line.starts_with("index ")
        || line.starts_with("new file mode ")
        || line.starts_with("deleted file mode ")
        || line.starts_with("similarity index ")
    {
        Style::default().fg(THEME_COOL)
    } else if line.starts_with("commit ") {
        Style::default()
            .fg(THEME_WARNING)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn is_diff_code_line(line: &str) -> bool {
    (line.starts_with('+') && !line.starts_with("+++"))
        || (line.starts_with('-') && !line.starts_with("---"))
        || line.starts_with(' ')
}

fn highlighted_code_line(
    line: &str,
    language: Option<SyntaxLanguage>,
    base: Style,
    selected: bool,
) -> Line<'static> {
    let mut characters = line.char_indices();
    let Some((_, marker)) = characters.next() else {
        return Line::default();
    };
    let code_start = marker.len_utf8();
    let code_base = if selected {
        Style::default().fg(THEME_TEXT)
    } else {
        Style::default()
    };
    let marker_style = if selected { code_base } else { base };
    let mut spans = vec![Span::styled(marker.to_string(), marker_style)];
    spans.extend(highlight_code(&line[code_start..], language, code_base));
    Line::from(spans)
}

fn highlight_code(code: &str, language: Option<SyntaxLanguage>, base: Style) -> Vec<Span<'static>> {
    let Some(language) = language else {
        return vec![Span::styled(code.to_string(), base)];
    };
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < code.len() {
        let rest = &code[cursor..];
        if starts_line_comment(rest, language) {
            spans.push(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(THEME_MUTED)
                    .add_modifier(Modifier::ITALIC),
            ));
            break;
        }
        if rest.starts_with("/*") || rest.starts_with("<!--") {
            let closing = if rest.starts_with("<!--") {
                "-->"
            } else {
                "*/"
            };
            let length = rest
                .find(closing)
                .map(|index| index + closing.len())
                .unwrap_or(rest.len());
            spans.push(Span::styled(
                rest[..length].to_string(),
                Style::default()
                    .fg(THEME_MUTED)
                    .add_modifier(Modifier::ITALIC),
            ));
            cursor += length;
            continue;
        }
        let character = rest.chars().next().expect("non-empty code remainder");
        if matches!(character, '"' | '\'' | '`') {
            let length = quoted_token_len(rest, character);
            spans.push(Span::styled(
                rest[..length].to_string(),
                Style::default().fg(THEME_WARNING),
            ));
            cursor += length;
            continue;
        }
        if character.is_ascii_digit() {
            let length = rest
                .char_indices()
                .take_while(|(_, value)| {
                    value.is_ascii_alphanumeric() || matches!(value, '.' | '_')
                })
                .map(|(index, value)| index + value.len_utf8())
                .last()
                .unwrap_or(character.len_utf8());
            spans.push(Span::styled(
                rest[..length].to_string(),
                Style::default().fg(THEME_ACCENT),
            ));
            cursor += length;
            continue;
        }
        if is_identifier_start(character) {
            let length = rest
                .char_indices()
                .take_while(|(_, value)| is_identifier_continue(*value))
                .map(|(index, value)| index + value.len_utf8())
                .last()
                .unwrap_or(character.len_utf8());
            let token = &rest[..length];
            let following = rest[length..].trim_start();
            let style = if is_keyword(token, language) {
                Style::default()
                    .fg(THEME_SECONDARY)
                    .add_modifier(Modifier::BOLD)
            } else if is_builtin_type(token, language)
                || token.chars().next().is_some_and(char::is_uppercase)
            {
                Style::default().fg(THEME_TYPE)
            } else if following.starts_with('(') {
                Style::default().fg(THEME_ACCENT)
            } else {
                base
            };
            spans.push(Span::styled(token.to_string(), style));
            cursor += length;
            continue;
        }
        let length = character.len_utf8();
        spans.push(Span::styled(rest[..length].to_string(), base));
        cursor += length;
    }
    spans
}

fn starts_line_comment(text: &str, language: SyntaxLanguage) -> bool {
    text.starts_with("//")
        || matches!(
            language,
            SyntaxLanguage::Python | SyntaxLanguage::Ruby | SyntaxLanguage::Shell
        ) && text.starts_with('#')
        || language == SyntaxLanguage::Data && text.starts_with('#')
        || language == SyntaxLanguage::Sql && text.starts_with("--")
}

fn quoted_token_len(text: &str, quote: char) -> usize {
    let mut escaped = false;
    for (index, character) in text.char_indices().skip(1) {
        if character == quote && !escaped {
            return index + character.len_utf8();
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    text.len()
}

fn is_identifier_start(character: char) -> bool {
    character == '_' || character == '$' || character.is_alphabetic()
}

fn is_identifier_continue(character: char) -> bool {
    is_identifier_start(character) || character.is_ascii_digit()
}

fn is_keyword(token: &str, language: SyntaxLanguage) -> bool {
    let common = matches!(
        token,
        "as" | "async"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "from"
            | "if"
            | "import"
            | "in"
            | "interface"
            | "let"
            | "match"
            | "new"
            | "null"
            | "return"
            | "static"
            | "struct"
            | "switch"
            | "throw"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "var"
            | "where"
            | "while"
            | "yield"
    );
    common
        || match language {
            SyntaxLanguage::Rust => matches!(
                token,
                "crate"
                    | "dyn"
                    | "extern"
                    | "fn"
                    | "impl"
                    | "loop"
                    | "mod"
                    | "move"
                    | "mut"
                    | "pub"
                    | "ref"
                    | "self"
                    | "Self"
                    | "super"
                    | "unsafe"
                    | "use"
            ),
            SyntaxLanguage::JavaScript => matches!(token, "function" | "of" | "this" | "void"),
            SyntaxLanguage::Python => matches!(
                token,
                "and"
                    | "def"
                    | "del"
                    | "elif"
                    | "except"
                    | "global"
                    | "is"
                    | "lambda"
                    | "not"
                    | "or"
                    | "pass"
                    | "raise"
                    | "with"
            ),
            SyntaxLanguage::Go => matches!(
                token,
                "chan"
                    | "defer"
                    | "fallthrough"
                    | "func"
                    | "go"
                    | "map"
                    | "package"
                    | "range"
                    | "select"
            ),
            SyntaxLanguage::CLike => matches!(
                token,
                "auto"
                    | "boolean"
                    | "char"
                    | "double"
                    | "final"
                    | "float"
                    | "int"
                    | "long"
                    | "namespace"
                    | "private"
                    | "protected"
                    | "public"
                    | "short"
                    | "signed"
                    | "sizeof"
                    | "template"
                    | "this"
                    | "unsigned"
                    | "virtual"
                    | "void"
                    | "volatile"
            ),
            SyntaxLanguage::Ruby => matches!(
                token,
                "alias"
                    | "begin"
                    | "def"
                    | "defined"
                    | "end"
                    | "ensure"
                    | "module"
                    | "next"
                    | "nil"
                    | "redo"
                    | "rescue"
                    | "retry"
                    | "then"
                    | "unless"
                    | "until"
            ),
            SyntaxLanguage::Shell => matches!(
                token,
                "case" | "done" | "elif" | "esac" | "fi" | "function" | "then"
            ),
            SyntaxLanguage::Data => false,
            SyntaxLanguage::Sql => matches!(
                token.to_ascii_lowercase().as_str(),
                "alter"
                    | "and"
                    | "as"
                    | "by"
                    | "create"
                    | "delete"
                    | "distinct"
                    | "drop"
                    | "from"
                    | "group"
                    | "having"
                    | "insert"
                    | "into"
                    | "join"
                    | "limit"
                    | "not"
                    | "null"
                    | "on"
                    | "or"
                    | "order"
                    | "select"
                    | "set"
                    | "table"
                    | "union"
                    | "update"
                    | "values"
                    | "where"
            ),
            SyntaxLanguage::Web => matches!(token, "important" | "media" | "var"),
        }
}

fn is_builtin_type(token: &str, language: SyntaxLanguage) -> bool {
    match language {
        SyntaxLanguage::Rust => matches!(
            token,
            "bool"
                | "char"
                | "f32"
                | "f64"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "str"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
        ),
        SyntaxLanguage::JavaScript => matches!(
            token,
            "any" | "bigint" | "boolean" | "never" | "number" | "object" | "string" | "unknown"
        ),
        SyntaxLanguage::Python => matches!(
            token,
            "bool" | "bytes" | "dict" | "float" | "int" | "list" | "set" | "str" | "tuple"
        ),
        SyntaxLanguage::Go => matches!(
            token,
            "bool"
                | "byte"
                | "complex64"
                | "complex128"
                | "error"
                | "float32"
                | "float64"
                | "int"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "rune"
                | "string"
                | "uint"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uintptr"
        ),
        _ => false,
    }
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
                .fg(THEME_TEXT)
                .bg(THEME_SELECTION_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        );
        let padding = viewport_width.saturating_sub(rendered.width());
        rendered.push_span(Span::raw(" ".repeat(padding)));
        return rendered;
    }

    let HistoryLine::Commit(commit) = line else {
        return Line::styled(
            line.display_text(author_width),
            Style::default().fg(THEME_MUTED),
        );
    };

    let mut spans = vec![
        Span::styled(commit.date.clone(), Style::default().fg(THEME_COOL)),
        Span::raw(" "),
        Span::styled(
            fit_column(&commit.author, author_width),
            Style::default().fg(THEME_POSITIVE),
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
        THEME_SECONDARY,
        THEME_WARNING,
        THEME_ACCENT,
        THEME_POSITIVE,
        THEME_TEXT,
        THEME_NEGATIVE,
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
                Style::default().fg(color.unwrap_or(THEME_COOL)),
            )
        })
        .collect()
}

fn ref_color(refs: &str) -> Color {
    if refs
        .split(", ")
        .any(|reference| reference.starts_with("tag: "))
    {
        THEME_SECONDARY
    } else if refs.split(", ").any(|reference| reference.contains('/')) {
        THEME_WARNING
    } else {
        THEME_ACCENT
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

        assert_eq!(rendered.spans[0].style.fg, Some(THEME_COOL));
        assert_eq!(rendered.spans[2].style.fg, Some(THEME_POSITIVE));
        assert!(
            rendered
                .spans
                .iter()
                .any(|span| span.style.fg == Some(THEME_ACCENT))
        );
    }

    #[test]
    fn selected_history_line_matches_tig_cursor_style() {
        let line = parse_history_line(
            "* \u{1e}012345\u{1f}012345\u{1f}parent\u{1f}2026-11-01 10:30 +0900\u{1f}Alice\u{1f}\u{1f}Initial commit",
        );

        let rendered = history_line(&line, 8, true, 100);

        assert_eq!(rendered.style.fg, Some(THEME_TEXT));
        assert_eq!(rendered.style.bg, Some(THEME_SELECTION_BACKGROUND));
        assert!(rendered.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(rendered.width(), 100);
    }

    #[test]
    fn diff_lines_use_semantic_colors() {
        let added = diff_line("+added", false, 80, None);
        let removed = diff_line("-removed", false, 80, None);
        assert_eq!(added.spans[0].style.fg, Some(DIFF_ADDED_MARKER));
        assert_eq!(removed.spans[0].style.fg, Some(DIFF_REMOVED_MARKER));
        assert_eq!(added.spans[1].style.fg, None);
        assert_eq!(removed.spans[1].style.fg, None);
        assert_eq!(added.style.bg, Some(ADDED_LINE_BACKGROUND));
        assert_eq!(removed.style.bg, Some(REMOVED_LINE_BACKGROUND));
        assert_eq!(added.width(), 80);
        assert_eq!(removed.width(), 80);
        assert_eq!(
            diff_line("@@ -1 +1 @@", false, 80, None).style.fg,
            Some(THEME_ACCENT)
        );
    }

    #[test]
    fn selected_diff_line_fills_the_viewport() {
        let rendered = diff_line("+added", true, 80, None);

        assert_eq!(rendered.style.bg, Some(THEME_SELECTION_BACKGROUND));
        assert_eq!(rendered.width(), 80);
    }

    #[test]
    fn diff_code_uses_language_syntax_colors() {
        let rendered = diff_line(
            "+pub fn answer() -> usize { 42 // result",
            false,
            80,
            Some(SyntaxLanguage::Rust),
        );
        let styled = rendered
            .spans
            .iter()
            .map(|span| (span.content.as_ref(), span.style.fg))
            .collect::<Vec<_>>();

        assert!(styled.contains(&("pub", Some(THEME_SECONDARY))));
        assert!(styled.contains(&("fn", Some(THEME_SECONDARY))));
        assert!(styled.contains(&("answer", Some(THEME_ACCENT))));
        assert!(styled.contains(&("usize", Some(THEME_TYPE))));
        assert!(styled.contains(&("42", Some(THEME_ACCENT))));
        assert!(styled.contains(&("// result", Some(THEME_MUTED))));
    }

    #[test]
    fn diff_language_follows_the_current_file_header() {
        let diff = vec![
            "diff --git a/src/main.rs b/src/main.rs".into(),
            "--- a/src/main.rs".into(),
            "+++ b/src/main.rs".into(),
            "@@ -1 +1 @@".into(),
        ];

        assert_eq!(
            syntax_at_diff_line(&diff, diff.len()),
            Some(SyntaxLanguage::Rust)
        );
        assert_eq!(
            language_for_path("app.tsx"),
            Some(SyntaxLanguage::JavaScript)
        );
        assert_eq!(language_for_path("image.png"), None);
    }

    #[test]
    fn selected_source_line_includes_a_line_number_and_fills_the_viewport() {
        let rendered = source_line(
            12,
            "let answer = 42;",
            3,
            SourceSelection::Cursor,
            SourceGutter {
                changed_side: None,
                bookmarked: false,
                symbol: Some((4, 10)),
            },
            SourceViewport {
                syntax: Some(SyntaxLanguage::Rust),
                horizontal_scroll: 0,
                width: 40,
            },
        );

        assert!(rendered.to_string().contains("  12 │ let answer = 42;"));
        assert!(
            rendered
                .spans
                .iter()
                .any(|span| span.style.bg == Some(THEME_SYMBOL_BACKGROUND))
        );
        assert_eq!(rendered.width(), 40);
    }

    #[test]
    fn changed_source_line_has_a_colored_gutter_marker() {
        let rendered = source_line(
            12,
            "let answer = 42;",
            3,
            SourceSelection::None,
            SourceGutter {
                changed_side: Some(crate::git::DiffSide::New),
                bookmarked: false,
                symbol: None,
            },
            SourceViewport {
                syntax: Some(SyntaxLanguage::Rust),
                horizontal_scroll: 0,
                width: 40,
            },
        );

        assert!(rendered.to_string().starts_with("+  12 │"));
        assert_eq!(rendered.spans[0].style.fg, Some(DIFF_ADDED_MARKER));
        assert_eq!(rendered.style.bg, Some(ADDED_LINE_BACKGROUND));
        assert_eq!(rendered.width(), 40);
        let identifier = rendered
            .spans
            .iter()
            .find(|span| span.content.contains("answer"))
            .expect("identifier should remain in a neutral syntax span");
        assert_eq!(identifier.style.fg, None);
    }

    #[test]
    fn bookmarked_source_line_has_a_cyan_gutter_marker() {
        let rendered = source_line(
            12,
            "let answer = 42;",
            3,
            SourceSelection::None,
            SourceGutter {
                changed_side: None,
                bookmarked: true,
                symbol: None,
            },
            SourceViewport {
                syntax: Some(SyntaxLanguage::Rust),
                horizontal_scroll: 0,
                width: 40,
            },
        );

        assert!(rendered.to_string().starts_with(" ◆ 12 │"));
        assert_eq!(rendered.spans[1].style.fg, Some(THEME_ACCENT));
    }

    #[test]
    fn visually_selected_source_range_fills_the_viewport() {
        let rendered = source_line(
            12,
            "let answer = 42;",
            3,
            SourceSelection::Range,
            SourceGutter {
                changed_side: None,
                bookmarked: false,
                symbol: None,
            },
            SourceViewport {
                syntax: Some(SyntaxLanguage::Rust),
                horizontal_scroll: 0,
                width: 40,
            },
        );

        assert_eq!(rendered.style.bg, None);
        assert_eq!(rendered.spans[0].style.bg, Some(THEME_RANGE_BACKGROUND));
        assert_eq!(rendered.width(), 40);
    }

    #[test]
    fn context_source_keeps_syntax_colors_under_cursor_background() {
        let rendered = source_line(
            12,
            "let answer = 42;",
            3,
            SourceSelection::Cursor,
            SourceGutter {
                changed_side: None,
                bookmarked: false,
                symbol: None,
            },
            SourceViewport {
                syntax: Some(SyntaxLanguage::Rust),
                horizontal_scroll: 0,
                width: 40,
            },
        );

        let keyword = rendered
            .spans
            .iter()
            .find(|span| span.content == "let")
            .expect("Rust keyword should be a separate highlighted span");
        assert_eq!(keyword.style.fg, Some(THEME_SECONDARY));
        assert_eq!(keyword.style.bg, Some(THEME_SELECTION_BACKGROUND));
    }

    #[test]
    fn horizontally_scrolled_source_shows_hidden_text_indicators() {
        assert_eq!(visible_source_text("0123456789", 0, 6), "01234›");
        assert_eq!(visible_source_text("0123456789", 4, 6), "‹4567›");
        assert_eq!(visible_source_text("0123456789", 7, 6), "‹789");
    }

    #[test]
    fn reading_trail_distinguishes_current_visited_and_unread_locations() {
        let mut app = App::new();
        app.context_reading_path = (1..=3)
            .map(|line| crate::app::AgentReadingPoint {
                file: "src/main.rs".into(),
                line,
                symbol: format!("step_{line}"),
                reason: format!("Read step {line}"),
                annotations: Vec::new(),
            })
            .collect();
        app.context_reading_index = Some(1);
        app.context_reading_visited.extend([0, 1]);

        let rendered = reading_trail_lines(&app);

        assert!(rendered[0].to_string().contains('✓'));
        assert!(rendered[1].to_string().contains('●'));
        assert!(rendered[2].to_string().contains('○'));
    }

    #[test]
    fn understanding_thread_shows_the_question_and_agent_answer() {
        let thread = crate::app::UnderstandingThread {
            source: "WORKTREE".into(),
            version: "WORKTREE".into(),
            file: "src/main.rs".into(),
            line: 12,
            end_line: None,
            question: "Why retry here?".into(),
            agent: "codex:session-1".into(),
            answer: Some("This is the shared request boundary.".into()),
            references: vec![crate::app::AgentReadingPoint {
                file: "src/client.rs".into(),
                line: 42,
                symbol: "Client::send".into(),
                reason: "Shows the request boundary".into(),
                annotations: Vec::new(),
            }],
        };
        let text = understanding_thread_box(&thread, 1, 60, 0)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("QUESTION"));
        assert!(text.contains("Why retry here?"));
        assert!(text.contains("LATEST ANSWER · codex:session-1"));
        assert!(text.contains("shared request boundary"));
        assert!(text.contains("READING TRAIL ABOVE · 1 location(s)"));
        assert!(text.contains("1 earlier response(s) hidden"));
        assert!(!text.contains("READ 1"));
    }

    #[test]
    fn pending_understanding_thread_animates_thinking_dots() {
        let thread = crate::app::UnderstandingThread {
            source: "WORKTREE".into(),
            version: "WORKTREE".into(),
            file: "src/main.rs".into(),
            line: 12,
            end_line: None,
            question: "Why?".into(),
            agent: "codex:session-1".into(),
            answer: None,
            references: Vec::new(),
        };

        let first = understanding_thread_box(&thread, 0, 60, 0)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let third = understanding_thread_box(&thread, 0, 60, 2)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(first.contains("Thinking."));
        assert!(third.contains("Thinking..."));
    }

    #[test]
    fn reading_arrival_explains_why_the_destination_matters() {
        let mut app = App::new();
        app.context_reading_path = vec![crate::app::AgentReadingPoint {
            file: "src/client.rs".into(),
            line: 42,
            symbol: "Client::send".into(),
            reason: "This is the shared request boundary".into(),
            annotations: Vec::new(),
        }];
        app.context_reading_index = Some(0);

        let text = reading_arrival_lines(&app, &app.context_reading_path[0], 80)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("READING TRAIL · STEP 1/1"));
        assert!(text.contains("WHY THIS CODE"));
        assert!(text.contains("shared request boundary"));
        assert!(text.contains("Ctrl-o back"));
    }

    #[test]
    fn temporary_reading_annotation_looks_like_a_code_comment() {
        let annotation = crate::app::AgentCodeAnnotation {
            line: 42,
            comment: "Retries only the recoverable transport error".into(),
        };

        let text = temporary_annotation_lines(&annotation, 80)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("// ↓ L42 ·"));
        assert!(text.contains("Retries only the recoverable transport error"));
        assert!(!text.contains("TEMP"));
    }

    #[test]
    fn comment_wrapping_uses_terminal_display_width() {
        assert_eq!(wrap_comment_body("あいうえ", 4), ["あい", "うえ"]);
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
    fn agent_response_uses_visual_hierarchy() {
        let rendered = agent_response_lines(
            "# Result\n- Call `parse` at src/parser.rs:12\n1. Verify behavior\n```rust\nlet value = 42;\n```",
            60,
        );
        let spans = rendered
            .iter()
            .flat_map(|line| &line.spans)
            .collect::<Vec<_>>();

        assert!(spans.iter().any(|span| {
            span.content == "Result"
                && span.style.fg == Some(THEME_TEXT)
                && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        assert!(
            spans
                .iter()
                .any(|span| { span.content == "• " && span.style.fg == Some(THEME_MUTED) })
        );
        assert!(spans.iter().any(|span| {
            span.content == "parse"
                && span.style.fg == Some(THEME_TEXT)
                && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        assert!(spans.iter().any(|span| {
            span.content.contains("src/parser.rs:12")
                && span.style.fg == Some(THEME_TEXT)
                && span.style.add_modifier.contains(Modifier::UNDERLINED)
        }));
        assert!(spans.iter().any(|span| {
            span.content == "let value = 42;" && span.style.fg == Some(THEME_TEXT)
        }));
    }

    #[test]
    fn agent_intent_uses_a_compact_card_and_manifest_stays_hidden() {
        let intent = AgentIntent {
            file: "src/main.rs".into(),
            line: 12,
            side: crate::app::AgentIntentSide::New,
            intent: "Keep parsing separate from rendering".into(),
            reading_path: vec![crate::app::AgentReadingPoint {
                file: "src/parser.rs".into(),
                line: 8,
                symbol: "parse".into(),
                reason: "Follow the parsing entry point".into(),
                annotations: Vec::new(),
            }],
        };
        let rendered = agent_intent_box(&intent, 60);
        let text = rendered
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("◆ AGENT INTENT · src/main.rs:12"));
        assert!(text.contains("Keep parsing separate from rendering"));
        assert!(text.contains("READ 1 · parse · src/parser.rs:8"));
        assert!(text.contains("Follow the parsing entry point"));
        assert_eq!(
            visible_agent_response(
                "Implemented.\n<atig-intents>\n[{\"file\":\"src/main.rs\"}]\n</atig-intents>"
            ),
            "Implemented."
        );
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
