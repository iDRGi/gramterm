//! TDLib-backed primitives for Gramterm.
//!
//! This crate deliberately hides TDLib's generated types behind a small
//! surface so the TUI can eventually depend on it without knowing about
//! `tdlib-rs` details.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use tdlib_rs::enums::{self, Update};
use tdlib_rs::functions;
use tdlib_rs::types;
use tokio::sync::mpsc;

const APPLICATION_NAME: &str = "Gramterm";

#[derive(Debug, Clone)]
pub struct Config {
    pub api_id: i32,
    pub api_hash: String,
    pub data_dir: PathBuf,
}

impl Config {
    pub fn new(api_id: i32, api_hash: impl Into<String>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            api_id,
            api_hash: api_hash.into(),
            data_dir: data_dir.into(),
        }
    }

    fn prepare(&self) -> Result<()> {
        std::fs::create_dir_all(self.data_dir.join("db"))
            .with_context(|| format!("cannot create {}", self.data_dir.join("db").display()))?;
        std::fs::create_dir_all(self.data_dir.join("files"))
            .with_context(|| format!("cannot create {}", self.data_dir.join("files").display()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    WaitPhoneNumber,
    WaitCode,
    WaitPassword { hint: String },
    WaitOtherDevice { link: String },
    Ready,
    Closed,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ChatInfo {
    pub id: i64,
    pub title: String,
    pub preview: String,
    pub unread: u32,
    pub muted: bool,
    pub pinned: bool,
    pub kind: ChatInfoKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatInfoKind {
    Saved,
    Private,
    Group,
    Channel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    Photo,
    Video,
    Animation,
    Audio,
    Voice,
    Sticker,
    File,
    Poll,
    Location,
    Contact,
    Service,
    Other,
}

#[derive(Debug, Clone)]
pub struct MessageInfo {
    pub id: i64,
    pub chat_id: i64,
    pub sender: String,
    pub text: String,
    pub outgoing: bool,
    pub time: String,
    pub kind: MessageKind,
    pub edited: bool,
}

pub struct TgClient {
    client_id: i32,
    config: Config,
    updates: mpsc::UnboundedReceiver<(i32, Update)>,
}

impl TgClient {
    /// Creates the TDLib client, starts its update loop and hands back the
    /// first authorization state. The first call to `next_auth` handles
    /// `waitTdlibParameters` internally.
    pub async fn connect(config: Config) -> Result<(Self, AuthState)> {
        config.prepare()?;
        let (tx, updates) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            loop {
                match tdlib_rs::receive() {
                    Some((update, client_id)) => {
                        if tx.send((client_id, update)).is_err() {
                            break;
                        }
                    }
                    None => {
                        // `receive` times out after a couple of seconds; keep
                        // waiting instead of treating it as end-of-stream.
                    }
                }
            }
        });

        Self::start(config, updates).await
    }

    /// Creates a TDLib client that shares this instance's update loop.
    async fn start(
        config: Config,
        updates: mpsc::UnboundedReceiver<(i32, Update)>,
    ) -> Result<(Self, AuthState)> {
        tracing::info!("tdlib: creating client");
        let client_id = tdlib_rs::create_client();
        let mut client = Self {
            client_id,
            config,
            updates,
        };

        // A new client sends nothing until it gets its first request.
        tracing::info!("tdlib: get_option");
        td(functions::get_option("version".into(), client_id).await)?;
        tracing::info!("tdlib: waiting for auth");
        let state = client.next_auth().await?;
        tracing::info!("tdlib: auth state {state:?}");
        Ok((client, state))
    }

    pub fn client_id(&self) -> i32 {
        self.client_id
    }

    /// Waits for the next authorization state. `waitTdlibParameters` is
    /// answered here, so callers never see it.
    pub async fn next_auth(&mut self) -> Result<AuthState> {
        loop {
            tracing::debug!("tdlib: waiting for update");
            let Some((client_id, update)) = self.updates.recv().await else {
                bail!("TDLib update channel closed");
            };
            tracing::debug!("tdlib: update received for client {client_id}");
            if client_id != self.client_id {
                continue;
            }
            let Update::AuthorizationState(state) = update else {
                tracing::debug!("tdlib: non-auth update");
                continue;
            };
            tracing::info!("tdlib: auth update");
            match state.authorization_state {
                enums::AuthorizationState::WaitTdlibParameters => {
                    tracing::info!("tdlib: setting parameters");
                    self.set_parameters().await?;
                    tracing::info!("tdlib: parameters set");
                }
                other => return Ok(map_auth(other)),
            }
        }
    }

    async fn set_parameters(&self) -> Result<()> {
        let db = self.config.data_dir.join("db");
        let files = self.config.data_dir.join("files");
        td(functions::set_tdlib_parameters(
            false,
            path_string(&db),
            path_string(&files),
            String::new(),
            true,
            true,
            true,
            true,
            self.config.api_id,
            self.config.api_hash.clone(),
            "en".into(),
            APPLICATION_NAME.into(),
            std::env::consts::OS.into(),
            env!("CARGO_PKG_VERSION").into(),
            self.client_id,
        )
        .await)?;
        Ok(())
    }

    pub async fn submit_phone(&self, phone: &str) -> Result<()> {
        td(
            functions::set_authentication_phone_number(phone.to_string(), None, self.client_id)
                .await,
        )?;
        Ok(())
    }

    pub async fn submit_code(&self, code: &str) -> Result<()> {
        td(functions::check_authentication_code(code.to_string(), self.client_id).await)?;
        Ok(())
    }

    pub async fn submit_password(&self, password: &str) -> Result<()> {
        td(functions::check_authentication_password(password.to_string(), self.client_id).await)?;
        Ok(())
    }

    pub async fn request_qr(&self) -> Result<()> {
        td(functions::request_qr_code_authentication(Vec::new(), self.client_id).await)?;
        Ok(())
    }

    /// Logs the current account out. TDLib destroys the local data and emits
    /// `authorizationStateClosed`; call [`Self::wait_after_logout`] and then
    /// [`Self::reconnect`] to get back to a usable client.
    pub async fn log_out(&self) -> Result<()> {
        td(functions::log_out(self.client_id).await)?;
        Ok(())
    }

    /// Waits for the authorization state that follows a successful logout.
    ///
    /// TDLib 1.8 sends `Closed`; older versions may go straight back to
    /// `WaitPhoneNumber`, which is returned as-is.
    pub async fn wait_after_logout(&mut self, timeout: Duration) -> Result<AuthState> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                bail!("timed out waiting for TDLib after logout");
            }
            match tokio::time::timeout(remaining, self.next_auth()).await {
                Ok(Ok(state @ (AuthState::Closed | AuthState::WaitPhoneNumber))) => {
                    return Ok(state);
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => return Err(error),
                Err(_) => bail!("timed out waiting for TDLib after logout"),
            }
        }
    }

    /// Creates a fresh TDLib client after a logout, reusing the existing
    /// update loop so updates for the old client id are not consumed by a
    /// second receiver thread.
    pub async fn reconnect(&mut self) -> Result<AuthState> {
        self.config.prepare()?;
        self.client_id = tdlib_rs::create_client();
        td(functions::get_option("version".into(), self.client_id).await)?;
        self.next_auth().await
    }

    pub async fn me(&self) -> Result<UserInfo> {
        let user = td(functions::get_me(self.client_id).await)?;
        let enums::User::User(user) = user;
        Ok(UserInfo {
            id: user.id,
            name: format!("{} {}", user.first_name, user.last_name)
                .trim()
                .to_string(),
        })
    }

    /// Loads and resolves the first `limit` chats. TDLib returns ids from
    /// `getChats`; titles require a `getChat` per id.
    pub async fn chats(&self, limit: i32) -> Result<Vec<ChatInfo>> {
        let client_id = self.client_id;
        let my_id = match functions::get_me(client_id).await {
            Ok(enums::User::User(user)) => Some(user.id),
            _ => None,
        };
        // TDLib starts loading the main chat list automatically after login.
        // `loadChats` here used to trigger a second, slow sync and could block
        // the first `getChats`; query what is already available instead.
        let chats = match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            functions::get_chats(None, limit, client_id),
        )
        .await
        {
            Ok(result) => td(result)?,
            Err(_) => bail!("getChats timed out"),
        };
        let enums::Chats::Chats(chats) = chats;

        let futures = chats.chat_ids.into_iter().map(|chat_id| async move {
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                functions::get_chat(chat_id, client_id),
            )
            .await
            {
                Ok(Ok(enums::Chat::Chat(chat))) => {
                    let kind = match &chat.r#type {
                        enums::ChatType::Supergroup(group) if group.is_channel => {
                            ChatInfoKind::Channel
                        }
                        enums::ChatType::Supergroup(_) | enums::ChatType::BasicGroup(_) => {
                            ChatInfoKind::Group
                        }
                        enums::ChatType::Private(user) if Some(user.user_id) == my_id => {
                            ChatInfoKind::Saved
                        }
                        _ => ChatInfoKind::Private,
                    };
                    ChatInfo {
                        id: chat.id,
                        title: chat.title,
                        preview: chat
                            .last_message
                            .as_ref()
                            .map(|message| content_preview(&message.content))
                            .unwrap_or_default(),
                        unread: chat.unread_count.max(0) as u32,
                        muted: chat.notification_settings.mute_for > 0,
                        pinned: chat.positions.iter().any(|position| position.is_pinned),
                        kind,
                    }
                }
                _ => ChatInfo {
                    id: chat_id,
                    title: format!("Chat {chat_id}"),
                    preview: String::new(),
                    unread: 0,
                    muted: false,
                    pinned: false,
                    kind: ChatInfoKind::Private,
                },
            }
        });
        Ok(futures::future::join_all(futures).await)
    }

    /// Tells TDLib to load this chat's history from the server. Without it
    /// `getChatHistory` may only return the last cached message.
    pub async fn open_chat(&self, chat_id: i64) -> Result<()> {
        td(functions::open_chat(chat_id, self.client_id).await)?;
        Ok(())
    }

    pub async fn history(&self, chat_id: i64, limit: i32) -> Result<Vec<MessageInfo>> {
        let _ = self.open_chat(chat_id).await;
        // TDLib may answer from its database before the server page for a
        // freshly opened chat arrives.
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let page =
            td(functions::get_chat_history(chat_id, 0, 0, limit, false, self.client_id).await)?;
        let enums::Messages::Messages(page) = page;
        let mut raw: Vec<types::Message> = page.messages.into_iter().flatten().collect();

        // For a chat opened while TDLib was still syncing, the first answer
        // can contain only the last cached message. Retry briefly and ask for
        // the page before that message; this also gives the server loader a
        // chance to fill in the rest.
        for _ in 0..2 {
            if raw.len() > 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            if let Ok(result) =
                functions::get_chat_history(chat_id, 0, 0, limit, false, self.client_id).await
            {
                let enums::Messages::Messages(page) = result;
                raw.extend(page.messages.into_iter().flatten());
            }
            if raw.len() <= 1
                && let Some(last_id) = raw.first().map(|message| message.id)
                && let Ok(result) =
                    functions::get_chat_history(chat_id, last_id, 0, limit, false, self.client_id)
                        .await
            {
                let enums::Messages::Messages(older) = result;
                raw.extend(older.messages.into_iter().flatten());
            }
        }

        raw.sort_by_key(|message| message.id);
        raw.dedup_by_key(|message| message.id);
        if raw.len() > limit as usize {
            raw.drain(..raw.len() - limit as usize);
        }

        let mut senders = HashMap::new();
        let mut messages = Vec::with_capacity(raw.len());
        for message in raw {
            let sender = if message.is_outgoing {
                "You".to_string()
            } else {
                self.sender_name(&message.sender_id, &mut senders).await
            };
            messages.push(map_message(message, sender));
        }
        messages.reverse();
        Ok(messages)
    }

    async fn sender_name(
        &self,
        sender: &enums::MessageSender,
        cache: &mut HashMap<String, String>,
    ) -> String {
        let key = match sender {
            enums::MessageSender::User(user) => format!("u:{}", user.user_id),
            enums::MessageSender::Chat(chat) => format!("c:{}", chat.chat_id),
        };
        if let Some(name) = cache.get(&key) {
            return name.clone();
        }
        let name = match sender {
            enums::MessageSender::User(user) => {
                match functions::get_user(user.user_id, self.client_id).await {
                    Ok(enums::User::User(user)) => {
                        format!("{} {}", user.first_name, user.last_name)
                            .trim()
                            .to_string()
                    }
                    _ => "Unknown".to_string(),
                }
            }
            enums::MessageSender::Chat(chat) => {
                match functions::get_chat(chat.chat_id, self.client_id).await {
                    Ok(enums::Chat::Chat(chat)) => chat.title,
                    _ => "Unknown".to_string(),
                }
            }
        };
        let name = if name.is_empty() {
            "Unknown".to_string()
        } else {
            name
        };
        cache.insert(key, name.clone());
        name
    }

    pub async fn send_text(&self, chat_id: i64, text: &str) -> Result<MessageInfo> {
        let content = enums::InputMessageContent::InputMessageText(types::InputMessageText {
            text: types::FormattedText {
                text: text.to_string(),
                entities: Vec::new(),
            },
            link_preview_options: None,
            clear_draft: true,
        });
        let message =
            td(functions::send_message(chat_id, None, None, None, content, self.client_id).await)?;
        let enums::Message::Message(message) = message;
        Ok(map_message(message, "You".to_string()))
    }

    pub async fn view_messages(&self, chat_id: i64, message_id: i64) -> Result<()> {
        td(functions::view_messages(chat_id, vec![message_id], None, true, self.client_id).await)?;
        Ok(())
    }
}

fn map_message(message: types::Message, sender: String) -> MessageInfo {
    let kind = message_kind(&message.content);
    let text = match &message.content {
        enums::MessageContent::MessageText(content) => content.text.text.clone(),
        enums::MessageContent::MessagePhoto(content) => content.caption.text.clone(),
        enums::MessageContent::MessageVideo(content) => content.caption.text.clone(),
        enums::MessageContent::MessageAnimation(content) => content.caption.text.clone(),
        enums::MessageContent::MessageAudio(content) => content.caption.text.clone(),
        enums::MessageContent::MessageVoiceNote(content) => content.caption.text.clone(),
        enums::MessageContent::MessageDocument(content) => content.caption.text.clone(),
        enums::MessageContent::MessagePoll(content) => content.poll.question.text.clone(),
        _ => String::new(),
    };
    MessageInfo {
        id: message.id,
        chat_id: message.chat_id,
        sender,
        text,
        outgoing: message.is_outgoing,
        time: format_time(message.date),
        kind,
        edited: message.edit_date > 0,
    }
}

fn message_kind(content: &enums::MessageContent) -> MessageKind {
    use enums::MessageContent as C;
    match content {
        C::MessageText(_) => MessageKind::Text,
        C::MessagePhoto(_) => MessageKind::Photo,
        C::MessageVideo(_) => MessageKind::Video,
        C::MessageAnimation(_) => MessageKind::Animation,
        C::MessageAudio(_) => MessageKind::Audio,
        C::MessageVoiceNote(_) => MessageKind::Voice,
        C::MessageSticker(_) => MessageKind::Sticker,
        C::MessageDocument(_) => MessageKind::File,
        C::MessagePoll(_) => MessageKind::Poll,
        C::MessageLocation(_) => MessageKind::Location,
        C::MessageContact(_) => MessageKind::Contact,
        C::MessageBasicGroupChatCreate(_)
        | C::MessageSupergroupChatCreate(_)
        | C::MessageChatAddMembers(_)
        | C::MessageChatChangeTitle(_)
        | C::MessageChatChangePhoto(_)
        | C::MessageChatDeleteMember(_)
        | C::MessageChatDeletePhoto
        | C::MessageChatJoinByLink
        | C::MessageChatJoinByRequest
        | C::MessageChatUpgradeFrom(_)
        | C::MessageChatUpgradeTo(_)
        | C::MessagePinMessage(_)
        | C::MessageScreenshotTaken
        | C::MessageChatSetBackground(_)
        | C::MessageChatSetMessageAutoDeleteTime(_)
        | C::MessageChatSetTheme(_)
        | C::MessageChatBoost(_)
        | C::MessageContactRegistered
        | C::MessageCustomServiceAction(_)
        | C::MessageForumTopicCreated(_)
        | C::MessageForumTopicEdited(_)
        | C::MessageForumTopicIsClosedToggled(_)
        | C::MessageForumTopicIsHiddenToggled(_)
        | C::MessageGameScore(_)
        | C::MessageInviteVideoChatParticipants(_)
        | C::MessageVideoChatEnded(_)
        | C::MessageVideoChatScheduled(_)
        | C::MessageVideoChatStarted(_) => MessageKind::Service,
        _ => MessageKind::Other,
    }
}

fn content_preview(content: &enums::MessageContent) -> String {
    let kind = message_kind(content);
    let label = match kind {
        MessageKind::Text => return preview_text(&content_text(content)),
        MessageKind::Photo => "photo",
        MessageKind::Video => "video",
        MessageKind::Animation => "GIF",
        MessageKind::Audio => "audio",
        MessageKind::Voice => "voice",
        MessageKind::Sticker => "sticker",
        MessageKind::File => "file",
        MessageKind::Poll => "poll",
        MessageKind::Location => "location",
        MessageKind::Contact => "contact",
        MessageKind::Service => "service",
        MessageKind::Other => "message",
    };
    let caption = content_text(content);
    let caption = preview_text(&caption);
    if caption.is_empty() {
        format!("[{label}]")
    } else {
        format!("[{label}] {caption}")
    }
}

fn content_text(content: &enums::MessageContent) -> String {
    use enums::MessageContent as C;
    match content {
        C::MessageText(text) => text.text.text.clone(),
        C::MessagePhoto(photo) => photo.caption.text.clone(),
        C::MessageVideo(video) => video.caption.text.clone(),
        C::MessageAnimation(animation) => animation.caption.text.clone(),
        C::MessageAudio(audio) => audio.caption.text.clone(),
        C::MessageVoiceNote(voice) => voice.caption.text.clone(),
        C::MessageDocument(document) => document.caption.text.clone(),
        C::MessagePoll(poll) => poll.poll.question.text.clone(),
        _ => String::new(),
    }
}

fn preview_text(text: &str) -> String {
    text.replace(['\n', '\t'], " ")
        .chars()
        .take(120)
        .collect::<String>()
}

fn format_time(date: i32) -> String {
    chrono::DateTime::from_timestamp(i64::from(date), 0)
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default()
}

fn map_auth(state: enums::AuthorizationState) -> AuthState {
    match state {
        enums::AuthorizationState::WaitPhoneNumber => AuthState::WaitPhoneNumber,
        enums::AuthorizationState::WaitCode(_) => AuthState::WaitCode,
        enums::AuthorizationState::WaitPassword(password) => AuthState::WaitPassword {
            hint: password.password_hint,
        },
        enums::AuthorizationState::WaitOtherDeviceConfirmation(confirmation) => {
            AuthState::WaitOtherDevice {
                link: confirmation.link,
            }
        }
        enums::AuthorizationState::Ready => AuthState::Ready,
        enums::AuthorizationState::Closed => AuthState::Closed,
        other => AuthState::Other(format!("{other:?}")),
    }
}

fn td<T>(result: Result<T, tdlib_rs::types::Error>) -> Result<T> {
    result.map_err(|error| anyhow!("TDLib error {}: {}", error.code, error.message))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
