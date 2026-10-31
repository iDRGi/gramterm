use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ru,
    En,
}

impl Default for Lang {
    fn default() -> Self {
        Self::detect()
    }
}

impl Lang {
    pub fn detect() -> Self {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(key)
                && value.to_ascii_lowercase().starts_with("ru")
            {
                return Self::Ru;
            }
        }
        Self::En
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ru" | "russian" | "рус" => Some(Self::Ru),
            "en" | "english" | "eng" => Some(Self::En),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Ru => "ru",
            Self::En => "en",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ru => "RU",
            Self::En => "EN",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Ru => Self::En,
            Self::En => Self::Ru,
        }
    }
}

/// Tiny Phase 0 dictionary. The real i18n layer will move to Fluent and
/// locale files; keeping it behind `t()` means call sites will not change.
pub fn t(lang: Lang, key: &'static str) -> &'static str {
    match (lang, key) {
        (Lang::Ru, "app_title") => "Gramterm — Telegram в терминале",
        (Lang::En, "app_title") => "Gramterm — Telegram in your terminal",

        (Lang::Ru, "chats") => "Чаты",
        (Lang::En, "chats") => "Chats",
        (Lang::Ru, "messages") => "Сообщения",
        (Lang::En, "messages") => "Messages",
        (Lang::Ru, "composer") => "Сообщение",
        (Lang::En, "composer") => "Message",
        (Lang::Ru, "type_message") => "Введите сообщение…",
        (Lang::En, "type_message") => "Type a message…",
        (Lang::Ru, "send") => "Отправить",
        (Lang::En, "send") => "Send",
        (Lang::Ru, "help") => "Помощь",
        (Lang::En, "help") => "Help",
        (Lang::Ru, "close") => "Закрыть",
        (Lang::En, "close") => "Close",
        (Lang::Ru, "demo") => "демо",
        (Lang::En, "demo") => "demo",
        (Lang::Ru, "simple") => "Простой",
        (Lang::En, "simple") => "Simple",
        (Lang::Ru, "vim") => "Vim",
        (Lang::En, "vim") => "Vim",
        (Lang::Ru, "quit") => "Выход",
        (Lang::En, "quit") => "Quit",
        (Lang::Ru, "unknown_command") => "Неизвестная команда",
        (Lang::En, "unknown_command") => "Unknown command",
        (Lang::Ru, "sent") => "Отправлено",
        (Lang::En, "sent") => "Sent",
        (Lang::Ru, "no_chats") => "Нет чатов",
        (Lang::En, "no_chats") => "No chats",
        (Lang::Ru, "no_messages") => "Пока нет сообщений",
        (Lang::En, "no_messages") => "No messages yet",
        (Lang::Ru, "loading_messages") => "Загружаем сообщения…",
        (Lang::En, "loading_messages") => "Loading messages…",
        (Lang::Ru, "saved") => "Избранное",
        (Lang::En, "saved") => "Saved",
        (Lang::Ru, "private") => "Личный",
        (Lang::En, "private") => "Private",
        (Lang::Ru, "group") => "Группа",
        (Lang::En, "group") => "Group",
        (Lang::Ru, "channel") => "Канал",
        (Lang::En, "channel") => "Channel",
        (Lang::Ru, "you") => "Вы",
        (Lang::En, "you") => "You",
        (Lang::Ru, "now") => "сейчас",
        (Lang::En, "now") => "now",
        (Lang::Ru, "typing") => "печатает…",
        (Lang::En, "typing") => "typing…",
        (Lang::Ru, "mouse_on") => "мышь захвачена",
        (Lang::En, "mouse_on") => "mouse captured",
        (Lang::Ru, "mouse_off") => "мышь отпущена",
        (Lang::En, "mouse_off") => "mouse released",
        (Lang::Ru, "search") => "Поиск",
        (Lang::En, "search") => "Search",
        (Lang::Ru, "search_hint") => "фильтр по названию, Esc — сброс",
        (Lang::En, "search_hint") => "filter by title, Esc to clear",
        (Lang::Ru, "command_hint") => "команда: demo, lang, theme, simple, vim, help, quit",
        (Lang::En, "command_hint") => "command: demo, lang, theme, simple, vim, help, quit",
        (Lang::Ru, "status_simple") => {
            "↑/↓ — чаты · Tab — фокус · Enter — открыть/отправить · мышь · ? — помощь · : — команды"
        }
        (Lang::En, "status_simple") => {
            "↑/↓ chats · Tab focus · Enter open/send · mouse · ? help · : commands"
        }
        (Lang::Ru, "status_vim") => {
            "j/k — движение · h/l — фокус · i — ввод · Esc — назад · ? — помощь · : — команды"
        }
        (Lang::En, "status_vim") => {
            "j/k move · h/l focus · i insert · Esc back · ? help · : commands"
        }
        (Lang::Ru, "not_wired") => "Отправка ещё не подключена к TDLib",
        (Lang::En, "not_wired") => "Sending is not wired to TDLib yet",
        (Lang::Ru, "connected") => "Подключено",
        (Lang::En, "connected") => "Connected",
        (Lang::Ru, "phone_submitted") => "Телефон отправлен, ждём код",
        (Lang::En, "phone_submitted") => "Phone submitted, waiting for the code",
        (Lang::Ru, "code_submitted") => "Код отправлен",
        (Lang::En, "code_submitted") => "Code submitted",
        (Lang::Ru, "password_submitted") => "Пароль отправлен",
        (Lang::En, "password_submitted") => "Password submitted",
        (Lang::Ru, "login_title") => "Вход в Telegram",
        (Lang::En, "login_title") => "Sign in to Telegram",
        (Lang::Ru, "login_starting") => "Подключаемся к Telegram…",
        (Lang::En, "login_starting") => "Connecting to Telegram…",
        (Lang::Ru, "login_phone") => "Номер телефона",
        (Lang::En, "login_phone") => "Phone number",
        (Lang::Ru, "login_code") => "Код из Telegram",
        (Lang::En, "login_code") => "Login code",
        (Lang::Ru, "login_password") => "Пароль 2FA",
        (Lang::En, "login_password") => "2FA password",
        (Lang::Ru, "login_qr") => "Вход по QR",
        (Lang::En, "login_qr") => "QR login",
        (Lang::Ru, "login_closed") => "TDLib закрыт",
        (Lang::En, "login_closed") => "TDLib closed",
        (Lang::Ru, "login_error") => "Ошибка входа",
        (Lang::En, "login_error") => "Login error",
        (Lang::Ru, "login_hint_phone") => "Введите номер в международном формате и нажмите Enter",
        (Lang::En, "login_hint_phone") => {
            "Enter the number in international format and press Enter"
        }
        (Lang::Ru, "login_hint_code") => "Введите код из сообщения Telegram",
        (Lang::En, "login_hint_code") => "Enter the code from Telegram",
        (Lang::Ru, "login_hint_password") => "Введите облачный пароль",
        (Lang::En, "login_hint_password") => "Enter your cloud password",
        (Lang::Ru, "login_hint_qr") => "Открой ссылку или отсканируй QR в Telegram",
        (Lang::En, "login_hint_qr") => "Open the link or scan the QR code in Telegram",
        (Lang::Ru, "phone_required") => "Введите номер телефона",
        (Lang::En, "phone_required") => "Enter a phone number",
        (Lang::Ru, "code_required") => "Введите код из Telegram",
        (Lang::En, "code_required") => "Enter the code from Telegram",
        (Lang::Ru, "password_required") => "Введите пароль 2FA",
        (Lang::En, "password_required") => "Enter the 2FA password",
        (Lang::Ru, "qr_requested") => "Запрашиваем QR-код…",
        (Lang::En, "qr_requested") => "Requesting a QR code…",
        (Lang::Ru, "show_password") => "показать",
        (Lang::En, "show_password") => "show",
        (Lang::Ru, "hide_password") => "скрыть",
        (Lang::En, "hide_password") => "hide",
        (Lang::Ru, "continue") => "Продолжить",
        (Lang::En, "continue") => "Continue",
        (Lang::Ru, "sign_in") => "Войти",
        (Lang::En, "sign_in") => "Sign in",
        (Lang::Ru, "use_qr") => "Войти по QR",
        (Lang::En, "use_qr") => "Use QR code",
        (Lang::Ru, "please_wait") => "Подождите…",
        (Lang::En, "please_wait") => "Please wait…",
        (Lang::Ru, "loading_chats") => "Загружаем чаты…",
        (Lang::En, "loading_chats") => "Loading chats…",
        (Lang::Ru, "help_body") => {
            "↑/↓ или j/k — двигаться\n\
             Tab / Shift+Tab — менять фокус\n\
             Enter — открыть чат или отправить\n\
             i — писать\n\
             Esc — назад / закрыть\n\
             / — поиск по чатам\n\
             : — команда\n\
             F2 — отпустить мышь для выделения\n\
             ? — эта справка\n\
             q — выход"
        }
        (Lang::En, "help_body") => {
            "↑/↓ or j/k — move\n\
             Tab / Shift+Tab — change focus\n\
             Enter — open chat or send\n\
             i — write\n\
             Esc — back / close\n\
             / — search chats\n\
             : — command\n\
             F2 — release mouse for text selection\n\
             ? — this help\n\
             q — quit"
        }
        _ => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_parsing() {
        assert_eq!(Lang::parse("ru"), Some(Lang::Ru));
        assert_eq!(Lang::parse("EN"), Some(Lang::En));
        assert_eq!(Lang::parse("de"), None);
    }

    #[test]
    fn russian_and_english_differ() {
        assert_ne!(t(Lang::Ru, "chats"), t(Lang::En, "chats"));
        assert_eq!(t(Lang::Ru, "chats"), "Чаты");
        assert_ne!(
            t(Lang::Ru, "loading_messages"),
            t(Lang::En, "loading_messages")
        );
    }
}
