use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Focus, Hit, Mode, Popup};
use crate::backend::{AuthView, ChatKind};
use crate::i18n;
use crate::theme::Theme;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let theme = app.theme;

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.fg)),
        area,
    );
    app.hits.clear();

    if !app.is_ready() {
        draw_login(frame, app, area, theme);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(frame, app, chunks[0], theme);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(chunks[1]);

    draw_chat_list(frame, app, body[0], theme);
    draw_messages(frame, app, body[1], theme);
    draw_composer(frame, app, chunks[2], theme);
    draw_status(frame, app, chunks[3], theme);

    match app.popup {
        Popup::Help => draw_help(frame, app, area, theme),
        Popup::Command => draw_prompt(frame, app, area, theme, true),
        Popup::Search => draw_prompt(frame, app, area, theme, false),
        Popup::None => {}
    }
}

fn base(theme: Theme) -> Style {
    Style::default().fg(theme.fg).bg(theme.bg)
}

fn draw_login(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let popup = centered_rect(64, 52, area);
    frame.render_widget(Clear, popup);

    let auth = app.backend.auth();
    let mut lines = vec![
        Line::from(Span::styled(
            " Gramterm ",
            Style::default()
                .fg(theme.bg)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            i18n::t(app.lang, "login_title"),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    match auth {
        AuthView::Starting => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_starting"),
                Style::default().fg(theme.dim),
            )));
        }
        AuthView::WaitPhoneNumber => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_phone"),
                Style::default().fg(theme.accent),
            )));
            lines.push(Line::from(format!("> {}▌", app.login_input)));
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_hint_phone"),
                Style::default().fg(theme.dim),
            )));
        }
        AuthView::WaitCode => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_code"),
                Style::default().fg(theme.accent),
            )));
            lines.push(Line::from(format!("> {}▌", app.login_input)));
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_hint_code"),
                Style::default().fg(theme.dim),
            )));
        }
        AuthView::WaitPassword { hint } => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_password"),
                Style::default().fg(theme.accent),
            )));
            let masked = "•".repeat(app.login_input.chars().count());
            lines.push(Line::from(format!("> {masked}▌")));
            if !hint.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("hint: {hint}"),
                    Style::default().fg(theme.warning),
                )));
            }
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_hint_password"),
                Style::default().fg(theme.dim),
            )));
        }
        AuthView::WaitOtherDevice { link } => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_qr"),
                Style::default().fg(theme.accent),
            )));
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_hint_qr"),
                Style::default().fg(theme.dim),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                link,
                Style::default().fg(theme.fg),
            )));
        }
        AuthView::Ready => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "connected"),
                Style::default().fg(theme.success),
            )));
        }
        AuthView::Closed => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_closed"),
                Style::default().fg(theme.error),
            )));
        }
        AuthView::Error(message) => {
            lines.push(Line::from(Span::styled(
                i18n::t(app.lang, "login_error"),
                Style::default().fg(theme.error),
            )));
            lines.push(Line::from(Span::styled(
                message,
                Style::default().fg(theme.error),
            )));
        }
    }

    if !app.status.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            app.status.clone(),
            Style::default().fg(theme.dim),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Ctrl+C — exit",
        Style::default().fg(theme.dim),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            format!(" {} ", i18n::t(app.lang, "app_title")),
            Style::default().fg(theme.accent),
        ))
        .style(base(theme));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(base(theme))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut spans = vec![
        Span::styled(
            " Gramterm ",
            Style::default()
                .fg(theme.bg)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", app.mode.label()),
            Style::default().fg(theme.bg).bg(theme.warning),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", app.lang.label()),
            Style::default().fg(theme.dim).bg(theme.bg),
        ),
        Span::raw(" "),
        Span::styled(
            if app.demo { " DEMO " } else { " NO TDLIB " },
            Style::default().fg(theme.bg).bg(theme.mention),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" mouse: {} ", if app.mouse_enabled { "on" } else { "off" }),
            Style::default().fg(theme.dim).bg(theme.bg),
        ),
    ];
    spans.push(Span::raw(
        " ".repeat(
            usize::from(area.width).saturating_sub(
                spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum::<usize>(),
            ),
        ),
    ));
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line).style(base(theme)), area);
}

fn draw_chat_list(frame: &mut Frame, app: &mut App, area: Rect, theme: Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            format!(" {} ", i18n::t(app.lang, "chats")),
            Style::default().fg(theme.accent),
        ))
        .style(base(theme));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chats = app.backend.chats().to_vec();
    let visible = app.visible_chat_indices();
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                i18n::t(app.lang, "no_chats"),
                Style::default().fg(theme.dim),
            ))
            .style(base(theme)),
            inner,
        );
        return;
    }

    let rows = usize::from(inner.height / 2).max(1);
    if let Some(pos) = visible.iter().position(|i| *i == app.selected_chat) {
        if pos < app.chat_scroll {
            app.chat_scroll = pos;
        } else if pos >= app.chat_scroll + rows {
            app.chat_scroll = pos + 1 - rows;
        }
    }
    app.chat_scroll = app
        .chat_scroll
        .min(visible.len().saturating_sub(rows.min(visible.len())));

    let mut lines = Vec::new();
    for (row, index) in visible.iter().enumerate().skip(app.chat_scroll).take(rows) {
        let chat = &chats[*index];
        let selected = *index == app.selected_chat;
        let bg = if selected { theme.selected } else { theme.bg };
        let title_style = Style::default()
            .fg(theme.fg)
            .bg(bg)
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let dim = Style::default().fg(theme.dim).bg(bg);
        let accent = Style::default().fg(theme.accent).bg(bg);

        let icon = match chat.kind {
            ChatKind::Saved => "★",
            ChatKind::Private => "•",
            ChatKind::Group => "#",
            ChatKind::Channel => "@",
        };
        let unread = if chat.unread > 0 {
            format!(" [{}]", chat.unread)
        } else {
            String::new()
        };
        let pinned = if chat.pinned { " ^" } else { "" };
        let muted = if chat.muted { " m" } else { "" };
        let suffix_width = UnicodeWidthStr::width(unread.as_str())
            + UnicodeWidthStr::width(pinned)
            + UnicodeWidthStr::width(muted);
        let title = fit(
            &chat.title,
            usize::from(inner.width)
                .saturating_sub(2 + suffix_width)
                .max(1),
        );

        let mut spans = vec![
            Span::styled(format!("{icon} "), accent),
            Span::styled(title, title_style),
        ];
        if !unread.is_empty() {
            spans.push(Span::styled(unread, accent));
        }
        if !pinned.is_empty() {
            spans.push(Span::styled(pinned, dim));
        }
        if !muted.is_empty() {
            spans.push(Span::styled(muted, dim));
        }
        let used = spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum::<usize>();
        spans.push(Span::styled(
            " ".repeat(usize::from(inner.width).saturating_sub(used)),
            Style::default().bg(bg),
        ));
        lines.push(Line::from(spans));

        let preview = fit(
            &chat.preview,
            usize::from(inner.width).saturating_sub(2).max(1),
        );
        lines.push(Line::from(vec![
            Span::styled(format!("  {preview}"), dim),
            Span::styled(
                " ".repeat(
                    usize::from(inner.width)
                        .saturating_sub(2 + UnicodeWidthStr::width(preview.as_str())),
                ),
                dim,
            ),
        ]));

        let y = inner.y + (row as u16 * 2);
        if y + 2 <= inner.y + inner.height {
            app.hits
                .push((Rect::new(inner.x, y, inner.width, 2), Hit::Chat(*index)));
        }
    }

    frame.render_widget(Paragraph::new(lines).style(base(theme)), inner);
}

fn draw_messages(frame: &mut Frame, app: &mut App, area: Rect, theme: Theme) {
    let title = app
        .current_chat()
        .map(|chat| chat.title.clone())
        .unwrap_or_else(|| i18n::t(app.lang, "messages").to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(theme.accent),
        ))
        .style(base(theme));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let messages = app.current_messages().to_vec();
    if messages.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                i18n::t(app.lang, "no_messages"),
                Style::default().fg(theme.dim),
            ))
            .alignment(Alignment::Center)
            .style(base(theme)),
            inner,
        );
        return;
    }

    let width = usize::from(inner.width).saturating_sub(4).max(1);
    let mut lines = Vec::new();
    let mut ranges: Vec<(usize, usize, usize)> = Vec::new();

    for (index, message) in messages.iter().enumerate() {
        let start = lines.len();
        let selected = app.selected_message == Some(index);
        let bg = if selected {
            theme.selected
        } else if message.outgoing {
            theme.own
        } else {
            theme.other
        };
        let text_style = Style::default().fg(theme.fg).bg(bg);
        let sender_style = Style::default()
            .fg(if message.outgoing {
                theme.success
            } else {
                theme.accent
            })
            .bg(bg)
            .add_modifier(Modifier::BOLD);
        let dim = Style::default().fg(theme.dim).bg(bg);

        let sender = if message.outgoing {
            i18n::t(app.lang, "you")
        } else {
            message.sender.as_str()
        };
        let mut header = vec![
            Span::styled(sender.to_string(), sender_style),
            Span::raw("  "),
            Span::styled(message.time.clone(), dim),
        ];
        if message.edited {
            header.push(Span::styled("  edited", dim));
        }
        if message.pending {
            header.push(Span::styled("  …", Style::default().fg(theme.error).bg(bg)));
        }
        lines.push(Line::from(header));

        for part in textwrap::wrap(&message.text, width) {
            lines.push(Line::from(Span::styled(format!("  {part}"), text_style)));
        }
        lines.push(Line::from(Span::styled(String::new(), text_style)));
        ranges.push((start, lines.len(), index));
    }

    let total = lines.len() as u16;
    let max_scroll = total.saturating_sub(inner.height);
    if app.follow_bottom {
        app.message_scroll = max_scroll;
    } else {
        app.message_scroll = app.message_scroll.min(max_scroll);
    }
    let scroll = app.message_scroll;
    let paragraph = Paragraph::new(lines).style(base(theme)).scroll((scroll, 0));
    frame.render_widget(paragraph, inner);

    for (start, end, index) in ranges {
        let top = start as i32 - i32::from(scroll);
        let bottom = end as i32 - i32::from(scroll);
        let visible_top = top.max(0);
        let visible_bottom = bottom.min(i32::from(inner.height));
        if visible_bottom <= visible_top {
            continue;
        }
        app.hits.push((
            Rect::new(
                inner.x,
                inner.y + visible_top as u16,
                inner.width,
                (visible_bottom - visible_top) as u16,
            ),
            Hit::Message(index),
        ));
    }
}

fn draw_composer(frame: &mut Frame, app: &mut App, area: Rect, theme: Theme) {
    let focused = app.focus == Focus::Composer;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { theme.accent } else { theme.border }))
        .title(Span::styled(
            format!(" {} ", i18n::t(app.lang, "composer")),
            Style::default().fg(if focused { theme.accent } else { theme.dim }),
        ))
        .style(base(theme));
    app.hits.push((
        Rect::new(area.x, area.y, area.width, area.height),
        Hit::Composer,
    ));

    let inner = block.inner(area);
    let text = if app.input.is_empty() {
        format!("▌{}", i18n::t(app.lang, "type_message"))
    } else {
        format!("{}▌", app.input)
    };
    let style = if app.input.is_empty() {
        Style::default().fg(theme.dim).bg(theme.bg)
    } else {
        Style::default().fg(theme.fg).bg(theme.bg)
    };
    let paragraph = Paragraph::new(Line::from(Span::styled(text, style)))
        .block(block)
        .style(base(theme));
    frame.render_widget(paragraph, area);

    if focused {
        let cursor_x = inner
            .x
            .saturating_add(UnicodeWidthStr::width(app.input.as_str()) as u16)
            .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
        frame.set_cursor_position((cursor_x, inner.y));
    }
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let focus = match app.focus {
        Focus::Chats => i18n::t(app.lang, "chats"),
        Focus::Messages => i18n::t(app.lang, "messages"),
        Focus::Composer => i18n::t(app.lang, "composer"),
    };
    let mouse = if app.mouse_enabled { "M" } else { "m" };
    let left = format!(
        " {} · {} · {} · {} ",
        app.mode.label(),
        focus,
        app.lang.label(),
        mouse
    );
    let hint = if app.status.is_empty() {
        i18n::t(
            app.lang,
            if app.mode == Mode::Simple {
                "status_simple"
            } else {
                "status_vim"
            },
        )
    } else {
        app.status.as_str()
    };
    let right = fit(
        hint,
        usize::from(area.width)
            .saturating_sub(UnicodeWidthStr::width(left.as_str()) + 1)
            .max(1),
    );
    let used = UnicodeWidthStr::width(left.as_str()) + UnicodeWidthStr::width(right.as_str());
    let line = Line::from(vec![
        Span::styled(
            left,
            Style::default()
                .fg(theme.bg)
                .bg(theme.border)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(usize::from(area.width).saturating_sub(used))),
        Span::styled(right, Style::default().fg(theme.dim).bg(theme.bg)),
    ]);
    frame.render_widget(Paragraph::new(line).style(base(theme)), area);
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let popup = centered_rect(72, 72, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            format!(" {} ", i18n::t(app.lang, "help")),
            Style::default().fg(theme.accent),
        ))
        .style(base(theme));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let paragraph = Paragraph::new(i18n::t(app.lang, "help_body"))
        .style(base(theme))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn draw_prompt(frame: &mut Frame, app: &App, area: Rect, theme: Theme, command: bool) {
    if area.height < 4 {
        return;
    }
    let popup = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(3),
        area.width,
        3,
    );
    frame.render_widget(Clear, popup);
    let (title, text) = if command {
        (
            i18n::t(app.lang, "command_hint"),
            format!(":{}", app.command),
        )
    } else {
        (i18n::t(app.lang, "search_hint"), format!("/{}", app.search))
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(theme.accent),
        ))
        .style(base(theme));
    frame.render_widget(
        Paragraph::new(Line::from(text))
            .block(block)
            .style(base(theme)),
        popup,
    );
}

fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let width = (u32::from(area.width) * u32::from(percent_x) / 100) as u16;
    let height = (u32::from(area.height) * u32::from(percent_y) / 100) as u16;
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
