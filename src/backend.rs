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

/// The narrow surface the UI needs. The TDLib adapter will implement this
/// with channels and async commands; the demo backend is synchronous.
pub trait Backend {
    fn chats(&self) -> &[Chat];
    fn messages(&self, chat: ChatId) -> &[Message];
    fn send_text(&mut self, chat: ChatId, text: String) -> Message;
    fn mark_read(&mut self, chat: ChatId);
    fn tick(&mut self) -> Option<Message>;
}
