pub type ChatId = i64;
pub type MsgId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    Saved,
    Private,
    Group,
    Channel,
}

#[derive(Debug, Clone)]
pub struct Chat {
    pub id: ChatId,
    pub title: String,
    pub preview: String,
    pub unread: u32,
    pub muted: bool,
    pub pinned: bool,
    pub kind: ChatKind,
}

#[derive(Debug, Clone)]
pub struct Message {
    /// Reserved for TDLib updates and deduplication.
    #[allow(dead_code)]
    pub id: MsgId,
    pub chat_id: ChatId,
    pub sender: String,
    pub text: String,
    pub outgoing: bool,
    pub time: String,
    pub pending: bool,
    pub edited: bool,
}

#[allow(dead_code)] // variants are constructed by the feature-gated TDLib backend
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthView {
    Starting,
    WaitPhoneNumber,
    WaitCode,
    WaitPassword { hint: String },
    WaitOtherDevice { link: String },
    Ready,
    Closed,
    Error(String),
}

/// The narrow surface the UI needs. The TDLib adapter will implement this
/// with channels and async commands; the demo backend is synchronous.
pub trait Backend {
    fn auth(&self) -> AuthView;
    fn submit_phone(&mut self, phone: String);
    fn submit_code(&mut self, code: String);
    fn submit_password(&mut self, password: String);
    fn request_qr(&mut self) {}
    fn take_notice(&mut self) -> Option<String> {
        None
    }
    fn chats_loading(&self) -> bool {
        false
    }
    fn can_send(&self) -> bool {
        true
    }

    fn chats(&self) -> &[Chat];
    fn messages(&self, chat: ChatId) -> &[Message];
    fn send_text(&mut self, chat: ChatId, text: String) -> Message;
    fn mark_read(&mut self, chat: ChatId);
    fn tick(&mut self) -> Option<Message>;
}
