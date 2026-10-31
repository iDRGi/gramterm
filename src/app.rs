use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::backend::{Backend, Chat, ChatId};
use crate::demo::DemoBackend;
use crate::i18n::{self, Lang};
use crate::theme::{Theme, ThemeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Chats,
    Messages,
    Composer,
}

impl Focus {
    fn next(self) -> Self {
        match self {
            Self::Chats => Self::Messages,
            Self::Messages => Self::Composer,
            Self::Composer => Self::Chats,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Chats => Self::Composer,
            Self::Messages => Self::Chats,
            Self::Composer => Self::Messages,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Simple,
    Vim,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Simple => "SIMPLE",
            Self::Vim => "VIM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    None,
    Help,
    Command,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Chat(usize),
    Message(usize),
    Composer,
}

pub struct App {
    pub backend: DemoBackend,
    pub lang: Lang,
    pub theme_kind: ThemeKind,
    pub theme: Theme,
    pub focus: Focus,
    pub mode: Mode,
    pub selected_chat: usize,
    pub selected_message: Option<usize>,
    pub chat_scroll: usize,
    pub message_scroll: u16,
    pub input: String,
    pub command: String,
    pub search: String,
    pub popup: Popup,
    pub status: String,
    pub should_quit: bool,
    pub dirty: bool,
    pub hits: Vec<(Rect, Hit)>,
    pub mouse_enabled: bool,
    mouse_toggle: Option<bool>,
    pub follow_bottom: bool,
    pub frame: u64,
    pub demo: bool,
}

impl App {
    pub fn new(lang: Lang, theme_kind: ThemeKind, mouse_enabled: bool) -> Self {
        Self {
            backend: DemoBackend::new(),
            lang,
            theme_kind,
            theme: Theme::new(theme_kind),
            focus: Focus::Chats,
            mode: Mode::Simple,
            selected_chat: 0,
            selected_message: None,
            chat_scroll: 0,
            message_scroll: 0,
            input: String::new(),
            command: String::new(),
            search: String::new(),
            popup: Popup::None,
            status: String::new(),
            should_quit: false,
            dirty: true,
            hits: Vec::new(),
            mouse_enabled,
            mouse_toggle: None,
            follow_bottom: true,
            frame: 0,
            demo: true,
        }
    }

    pub fn current_chat(&self) -> Option<&Chat> {
        self.backend.chats().get(self.selected_chat)
    }

    pub fn current_chat_id(&self) -> Option<ChatId> {
        self.current_chat().map(|chat| chat.id)
    }

    pub fn current_messages(&self) -> &[crate::backend::Message] {
        self.current_chat_id()
            .map(|id| self.backend.messages(id))
            .unwrap_or(&[])
    }

    pub fn visible_chat_indices(&self) -> Vec<usize> {
        let needle = self.search.trim().to_lowercase();
        self.backend
            .chats()
            .iter()
            .enumerate()
            .filter(|(_, chat)| needle.is_empty() || chat.title.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn open_selected_chat(&mut self) {
        let Some(chat) = self.current_chat() else {
            return;
        };
        let id = chat.id;
        self.backend.mark_read(id);
        self.focus = Focus::Messages;
        self.selected_message = self.current_messages().len().checked_sub(1);
        self.message_scroll = 0;
        self.follow_bottom = true;
        self.status.clear();
        self.dirty = true;
    }

    pub fn send(&mut self) {
        let Some(chat_id) = self.current_chat_id() else {
            return;
        };
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.backend.send_text(chat_id, text);
        self.input.clear();
        self.selected_message = self.current_messages().len().checked_sub(1);
        self.follow_bottom = true;
        self.status = i18n::t(self.lang, "sent").to_string();
        self.dirty = true;
    }

    pub fn move_chat(&mut self, delta: isize) {
        let visible = self.visible_chat_indices();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|i| *i == self.selected_chat)
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, visible.len() as isize - 1) as usize;
        self.selected_chat = visible[next];
        self.selected_message = None;
        self.dirty = true;
    }

    pub fn move_message(&mut self, delta: isize) {
        let len = self.current_messages().len();
        if len == 0 {
            self.selected_message = None;
            return;
        }
        let current = self.selected_message.unwrap_or(len - 1) as isize;
        let next = (current + delta).clamp(0, len as isize - 1) as usize;
        self.selected_message = Some(next);
        self.follow_bottom = next + 1 == len;
        self.dirty = true;
    }

    pub fn scroll_messages(&mut self, delta: isize) {
        let next = (self.message_scroll as isize + delta).max(0) as u16;
        self.message_scroll = next;
        self.follow_bottom = delta > 0;
        self.dirty = true;
    }

    pub fn toggle_mouse(&mut self) {
        self.mouse_enabled = !self.mouse_enabled;
        self.mouse_toggle = Some(self.mouse_enabled);
        self.status = i18n::t(
            self.lang,
            if self.mouse_enabled {
                "mouse_on"
            } else {
                "mouse_off"
            },
        )
        .to_string();
        self.dirty = true;
    }

    pub fn take_mouse_toggle(&mut self) -> Option<bool> {
        self.mouse_toggle.take()
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        self.status = format!("lang: {}", lang.code());
        self.dirty = true;
    }

    pub fn set_theme(&mut self, kind: ThemeKind) {
        self.theme_kind = kind;
        self.theme = Theme::new(kind);
        self.status = format!("theme: {}", kind.name());
        self.dirty = true;
    }

    pub fn on_tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        if let Some(message) = self.backend.tick() {
            if self.current_chat_id() == Some(message.chat_id) && self.follow_bottom {
                self.selected_message = self.current_messages().len().checked_sub(1);
            }
            self.status = format!("new: {}", message.sender);
            self.dirty = true;
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            self.should_quit = true;
            return;
        }

        if key.code == KeyCode::F(2) {
            self.toggle_mouse();
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('t') | KeyCode::Char('T'))
        {
            let kind = self.theme_kind.toggled();
            self.set_theme(kind);
            return;
        }

        if self.mode == Mode::Vim {
            match key.code {
                KeyCode::Char('g') => {
                    match self.focus {
                        Focus::Chats => {
                            if let Some(first) = self.visible_chat_indices().first() {
                                self.selected_chat = *first;
                            }
                        }
                        Focus::Messages => {
                            self.message_scroll = 0;
                            self.follow_bottom = false;
                        }
                        Focus::Composer => {}
                    }
                    self.dirty = true;
                    return;
                }
                KeyCode::Char('G') => {
                    match self.focus {
                        Focus::Chats => {
                            if let Some(last) = self.visible_chat_indices().last() {
                                self.selected_chat = *last;
                            }
                        }
                        Focus::Messages => {
                            self.selected_message = self.current_messages().len().checked_sub(1);
                            self.follow_bottom = true;
                        }
                        Focus::Composer => {}
                    }
                    self.dirty = true;
                    return;
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_messages(10);
                    return;
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_messages(-10);
                    return;
                }
                _ => {}
            }
        }

        match self.popup {
            Popup::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter
                ) {
                    self.popup = Popup::None;
                }
                self.dirty = true;
                return;
            }
            Popup::Command => {
                self.on_command_key(key);
                return;
            }
            Popup::Search => {
                self.on_search_key(key);
                return;
            }
            Popup::None => {}
        }

        if self.focus == Focus::Composer {
            self.on_composer_key(key);
            return;
        }

        match key.code {
            KeyCode::Char(':') => {
                self.popup = Popup::Command;
                self.command.clear();
                self.dirty = true;
            }
            KeyCode::Char('/') => {
                self.popup = Popup::Search;
                self.search.clear();
                self.dirty = true;
            }
            KeyCode::Char('?') => {
                self.popup = Popup::Help;
                self.dirty = true;
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Char('i') => {
                self.focus = Focus::Composer;
                self.dirty = true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if key.code == KeyCode::Tab {
                    self.focus.next()
                } else {
                    self.focus.prev()
                };
                self.dirty = true;
            }
            KeyCode::Esc => {
                self.focus = match self.focus {
                    Focus::Messages => Focus::Chats,
                    Focus::Composer => Focus::Messages,
                    Focus::Chats => Focus::Chats,
                };
                self.dirty = true;
            }
            KeyCode::Up | KeyCode::Char('k') => match self.focus {
                Focus::Chats => self.move_chat(-1),
                Focus::Messages => self.move_message(-1),
                Focus::Composer => {}
            },
            KeyCode::Down | KeyCode::Char('j') => match self.focus {
                Focus::Chats => self.move_chat(1),
                Focus::Messages => self.move_message(1),
                Focus::Composer => {}
            },
            KeyCode::Left | KeyCode::Char('h') => {
                if self.focus == Focus::Messages {
                    self.focus = Focus::Chats;
                    self.dirty = true;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.focus == Focus::Chats {
                    self.open_selected_chat();
                } else if self.focus == Focus::Messages {
                    self.focus = Focus::Composer;
                    self.dirty = true;
                }
            }
            KeyCode::Enter => match self.focus {
                Focus::Chats => self.open_selected_chat(),
                Focus::Messages => {
                    self.focus = Focus::Composer;
                    self.dirty = true;
                }
                Focus::Composer => self.send(),
            },
            KeyCode::PageUp => self.scroll_messages(-5),
            KeyCode::PageDown => self.scroll_messages(5),
            _ => {}
        }
    }

    fn on_composer_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::Messages;
                self.dirty = true;
            }
            KeyCode::Enter => self.send(),
            KeyCode::Backspace => {
                self.input.pop();
                self.dirty = true;
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.input.push(c);
                    self.dirty = true;
                }
            }
            KeyCode::Tab => {
                self.focus = Focus::Chats;
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn on_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.popup = Popup::None;
                self.dirty = true;
            }
            KeyCode::Enter => {
                let command = self.command.trim().to_string();
                self.popup = Popup::None;
                self.execute_command(&command);
            }
            KeyCode::Backspace => {
                self.command.pop();
                self.dirty = true;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.command.clear();
                self.dirty = true;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.command.push(c);
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.popup = Popup::None;
                self.search.clear();
                self.dirty = true;
            }
            KeyCode::Enter => {
                self.popup = Popup::None;
                self.open_selected_chat();
            }
            KeyCode::Up => self.move_chat(-1),
            KeyCode::Down => self.move_chat(1),
            KeyCode::Backspace => {
                self.search.pop();
                self.dirty = true;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.push(c);
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, command: &str) {
        let mut parts = command.split_whitespace();
        let Some(name) = parts.next() else {
            return;
        };
        let arg = parts.next();

        match name {
            "quit" | "q" => self.should_quit = true,
            "help" | "h" => {
                self.popup = Popup::Help;
                self.dirty = true;
            }
            "search" | "s" => {
                self.popup = Popup::Search;
                self.search.clear();
                self.dirty = true;
            }
            "demo" => {
                self.status = i18n::t(self.lang, "demo").to_string();
                self.dirty = true;
            }
            "simple" => {
                self.mode = Mode::Simple;
                self.dirty = true;
            }
            "vim" => {
                self.mode = Mode::Vim;
                self.dirty = true;
            }
            "lang" => match arg.and_then(Lang::parse) {
                Some(lang) => self.set_lang(lang),
                None => {
                    let lang = self.lang.toggled();
                    self.set_lang(lang);
                }
            },
            "theme" => match arg.and_then(ThemeKind::parse) {
                Some(kind) => self.set_theme(kind),
                None => {
                    self.status = "theme: mocha | latte".into();
                    self.dirty = true;
                }
            },
            "clear" => {
                self.status.clear();
                self.dirty = true;
            }
            "version" => {
                self.status = format!("gramterm {}", env!("CARGO_PKG_VERSION"));
                self.dirty = true;
            }
            other => {
                self.status = format!("{}: {other}", i18n::t(self.lang, "unknown_command"));
                self.dirty = true;
            }
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        if self.popup != Popup::None {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.popup = Popup::None;
                self.dirty = true;
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(hit) = self.hit_at(mouse.column, mouse.row) else {
                    return;
                };
                match hit {
                    Hit::Chat(index) => {
                        self.selected_chat = index;
                        self.open_selected_chat();
                    }
                    Hit::Message(index) => {
                        self.selected_message = Some(index);
                        self.focus = Focus::Messages;
                        self.dirty = true;
                    }
                    Hit::Composer => {
                        self.focus = Focus::Composer;
                        self.dirty = true;
                    }
                }
            }
            MouseEventKind::ScrollDown => match self.hit_at(mouse.column, mouse.row) {
                Some(Hit::Chat(_)) => self.move_chat(1),
                Some(Hit::Composer) => {}
                Some(Hit::Message(_)) | None => self.scroll_messages(3),
            },
            MouseEventKind::ScrollUp => match self.hit_at(mouse.column, mouse.row) {
                Some(Hit::Chat(_)) => self.move_chat(-1),
                Some(Hit::Composer) => {}
                Some(Hit::Message(_)) | None => self.scroll_messages(-3),
            },
            _ => {}
        }
    }

    fn hit_at(&self, column: u16, row: u16) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(rect, _)| inside(*rect, column, row))
            .map(|(_, hit)| *hit)
    }
}

fn inside(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn app() -> App {
        App::new(Lang::Ru, ThemeKind::Mocha, true)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn opens_the_selected_chat_and_marks_it_read() {
        let mut app = app();
        app.selected_chat = 1;
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.focus, Focus::Messages);
        assert_eq!(app.backend.chats()[1].unread, 0);
    }

    #[test]
    fn sends_from_the_composer() {
        let mut app = app();
        app.focus = Focus::Composer;
        for c in "hello".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
        app.on_key(key(KeyCode::Enter));
        let messages = app.current_messages();
        assert_eq!(messages.last().unwrap().text, "hello");
        assert!(messages.last().unwrap().outgoing);
        assert!(app.input.is_empty());
    }

    #[test]
    fn mouse_click_selects_a_chat() {
        let mut app = app();
        app.hits = vec![(Rect::new(0, 0, 10, 2), Hit::Chat(2))];
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.selected_chat, 2);
        assert_eq!(app.focus, Focus::Messages);
    }

    #[test]
    fn f2_toggles_mouse() {
        let mut app = app();
        app.on_key(key(KeyCode::F(2)));
        assert_eq!(app.take_mouse_toggle(), Some(false));
    }

    #[test]
    fn command_changes_language() {
        let mut app = app();
        app.execute_command("lang en");
        assert_eq!(app.lang, Lang::En);
    }
}
