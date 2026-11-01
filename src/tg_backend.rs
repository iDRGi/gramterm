use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use tokio::sync::Semaphore;

use gramterm_tg::{
    AuthState, ChatInfo, ChatInfoKind, Config, MessageInfo, MessageKind as TgMessageKind, TdUpdate,
    TgApi, TgClient,
};

use crate::backend::{
    AuthView, Backend, Chat, ChatId, ChatKind, Message, MessageKind, MsgId, SendFailure,
};
use crate::message_cache;

/// How many recent messages `open_chat` loads.
const HISTORY_LIMIT: i32 = 50;

enum Command {
    Phone(String),
    Code(String),
    Password(String),
    Qr,
    Logout,
    History(ChatId),
    Prefetch(ChatId),
    MarkRead {
        chat_id: ChatId,
        message_id: MsgId,
    },
    Send {
        chat_id: ChatId,
        text: String,
        temp_id: MsgId,
    },
}

enum Event {
    Auth(AuthView),
    Chats(Vec<ChatInfo>),
    ChatsLoading,
    ChatsFailed,
    Messages {
        chat_id: ChatId,
        messages: Vec<MessageInfo>,
    },
    HistoryFailed(ChatId),
    Notice(String),
    Error(String),
    Incoming(Message),
    MessageSent {
        old_id: MsgId,
        message: Message,
    },
    MessageSendFailed {
        old_id: MsgId,
        error: String,
    },
    ReadInbox {
        chat_id: ChatId,
        unread: u32,
    },
    ChatPreview {
        chat_id: ChatId,
        preview: String,
    },
    ChatTitle {
        chat_id: ChatId,
        title: String,
    },
    Sent {
        chat_id: ChatId,
        temp_id: MsgId,
        message: Message,
    },
    SendFailed {
        chat_id: ChatId,
        temp_id: MsgId,
        error: String,
    },
}

pub struct TgBackend {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    auth: AuthView,
    chats: Vec<Chat>,
    chats_cache: Option<PathBuf>,
    chats_loading: bool,
    message_cache: HashMap<ChatId, Vec<Message>>,
    messages_cache: Option<PathBuf>,
    history_loading: HashSet<ChatId>,
    history_loaded: HashSet<ChatId>,
    last_read: HashMap<ChatId, MsgId>,
    notice: Option<String>,
    next_temp_id: MsgId,
    send_failure: Option<SendFailure>,
}

impl TgBackend {
    pub fn from_env() -> Self {
        match config_from_env() {
            Ok(config) => Self::start(config),
            Err(error) => Self::from_error(format!("{error:#}")),
        }
    }

    fn from_error(error: String) -> Self {
        let (tx, _rx) = mpsc::channel();
        let (_event_tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            auth: AuthView::Error(error),
            chats: Vec::new(),
            chats_cache: None,
            chats_loading: false,
            message_cache: HashMap::new(),
            messages_cache: None,
            history_loading: HashSet::new(),
            history_loaded: HashSet::new(),
            last_read: HashMap::new(),
            notice: None,
            next_temp_id: -1,
            send_failure: None,
        }
    }

    #[cfg(test)]
    fn for_test() -> (Self, Sender<Event>, Receiver<Command>) {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let backend = Self {
            tx,
            rx,
            auth: AuthView::Ready,
            chats: Vec::new(),
            chats_cache: None,
            chats_loading: false,
            message_cache: HashMap::new(),
            messages_cache: None,
            history_loading: HashSet::new(),
            history_loaded: HashSet::new(),
            last_read: HashMap::new(),
            notice: None,
            next_temp_id: -1,
            send_failure: None,
        };
        (backend, events, commands)
    }

    fn start(config: Config) -> Self {
        let chats_cache = config.data_dir.join("chats.json");
        let cached_chats = load_chat_cache(&chats_cache);
        let messages_cache = config.data_dir.join("messages.json");
        let cached_messages = load_message_cache(&messages_cache);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        tracing::info!("starting TDLib backend");
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::error!("cannot build tokio runtime: {error}");
                    let _ =
                        event_tx.send(Event::Error(format!("cannot start tokio runtime: {error}")));
                    return;
                }
            };
            tracing::info!("tokio runtime ready");

            runtime.block_on(async move {
                tracing::info!("connecting TDLib");
                let (mut client, mut state) = match TgClient::connect(config).await {
                    Ok(pair) => pair,
                    Err(error) => {
                        tracing::error!("TDLib connect failed: {error:#}");
                        let _ = event_tx.send(Event::Error(format!("{error:#}")));
                        return;
                    }
                };
                tracing::info!("TDLib connected: {state:?}");
                let mut api = client.api();
                let history_slots = Arc::new(Semaphore::new(1));
                let prefetch_slots = Arc::new(Semaphore::new(1));

                let _ = event_tx.send(Event::Auth(map_auth(state.clone())));
                if state == AuthState::Ready {
                    publish_ready(&api, &event_tx).await;
                }

                loop {
                    for update in client.take_updates() {
                        handle_update(&api, &event_tx, update).await;
                    }
                    loop {
                        match cmd_rx.try_recv() {
                            Ok(Command::Phone(phone)) if state == AuthState::WaitPhoneNumber => {
                                if let Err(error) = client.submit_phone(&phone).await {
                                    let _ = event_tx.send(Event::Notice(format!("{error:#}")));
                                }
                            }
                            Ok(Command::Code(code)) if state == AuthState::WaitCode => {
                                if let Err(error) = client.submit_code(&code).await {
                                    let _ = event_tx.send(Event::Notice(format!("{error:#}")));
                                }
                            }
                            Ok(Command::Password(password))
                                if matches!(state, AuthState::WaitPassword { .. }) =>
                            {
                                if let Err(error) = client.submit_password(&password).await {
                                    let _ = event_tx.send(Event::Notice(format!("{error:#}")));
                                }
                            }
                            Ok(Command::Qr) if state == AuthState::WaitPhoneNumber => {
                                if let Err(error) = client.request_qr().await {
                                    let _ = event_tx.send(Event::Notice(format!("{error:#}")));
                                }
                            }
                            Ok(Command::Logout) if state == AuthState::Ready => {
                                tracing::info!("logout requested from the UI");
                                match client.log_out().await {
                                    Ok(()) => {
                                        match client
                                            .wait_after_logout(Duration::from_secs(20))
                                            .await
                                        {
                                            Ok(AuthState::Closed) => {
                                                match client.reconnect().await {
                                                    Ok(next) => {
                                                        state = next;
                                                        api = client.api();
                                                        let _ = event_tx
                                                            .send(Event::Auth(map_auth(state.clone())));
                                                    }
                                                    Err(error) => {
                                                        tracing::error!(
                                                            "TDLib reconnect after logout failed: {error:#}"
                                                        );
                                                        let _ = event_tx.send(Event::Error(format!(
                                                            "{error:#}"
                                                        )));
                                                        return;
                                                    }
                                                }
                                            }
                                            Ok(next) => {
                                                state = next;
                                                let _ =
                                                    event_tx.send(Event::Auth(map_auth(state.clone())));
                                            }
                                            Err(error) => {
                                                tracing::error!(
                                                    "TDLib logout failed: {error:#}"
                                                );
                                                let _ = event_tx
                                                    .send(Event::Error(format!("{error:#}")));
                                                return;
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        tracing::error!("TDLib log_out failed: {error:#}");
                                        let _ = event_tx.send(Event::Auth(map_auth(state.clone())));
                                        let _ = event_tx
                                            .send(Event::Notice(format!("logout failed: {error:#}")));
                                    }
                                }
                            }
                            Ok(Command::History(chat_id)) => {
                                let api = api.clone();
                                let events = event_tx.clone();
                                let slots = Arc::clone(&history_slots);
                                tokio::spawn(async move {
                                    let Ok(_permit) = slots.acquire_owned().await else {
                                        return;
                                    };
                                    load_history(&api, &events, chat_id).await;
                                });
                            }
                            Ok(Command::Prefetch(chat_id)) => {
                                let api = api.clone();
                                let events = event_tx.clone();
                                let slots = Arc::clone(&prefetch_slots);
                                tokio::spawn(async move {
                                    let Ok(_permit) = slots.acquire_owned().await else {
                                        return;
                                    };
                                    load_history(&api, &events, chat_id).await;
                                });
                            }
                            Ok(Command::MarkRead {
                                chat_id,
                                message_id,
                            }) if state == AuthState::Ready => {
                                if let Err(error) = api.view_messages(chat_id, message_id).await {
                                    let _ = event_tx.send(Event::Notice(format!("{error:#}")));
                                }
                            }
                            Ok(Command::Send {
                                chat_id,
                                text,
                                temp_id,
                            }) => {
                                if state != AuthState::Ready {
                                    let _ = event_tx.send(Event::SendFailed {
                                        chat_id,
                                        temp_id,
                                        error: "TDLib is not ready".into(),
                                    });
                                } else if text.trim().is_empty() {
                                    let _ = event_tx.send(Event::SendFailed {
                                        chat_id,
                                        temp_id,
                                        error: "message is empty".into(),
                                    });
                                } else {
                                    match tokio::time::timeout(
                                        Duration::from_secs(30),
                                        api.send_text(chat_id, &text),
                                    )
                                    .await
                                    {
                                        Ok(Ok(info)) => {
                                            tracing::info!(
                                                "sent message {} to chat {chat_id}",
                                                info.id
                                            );
                                            let message = Message {
                                                id: info.id,
                                                chat_id: info.chat_id,
                                                sender: info.sender,
                                                text: info.text,
                                                outgoing: info.outgoing,
                                                time: info.time,
                                                kind: map_message_kind(info.kind),
                                                pending: false,
                                                edited: info.edited,
                                            };
                                            let _ = event_tx.send(Event::Sent {
                                                chat_id,
                                                temp_id,
                                                message,
                                            });
                                        }
                                        Ok(Err(error)) => {
                                            tracing::error!(
                                                "send to chat {chat_id} failed: {error:#}"
                                            );
                                            let _ = event_tx.send(Event::SendFailed {
                                                chat_id,
                                                temp_id,
                                                error: format!("{error:#}"),
                                            });
                                        }
                                        Err(_) => {
                                            tracing::error!("send to chat {chat_id} timed out");
                                            let _ = event_tx.send(Event::SendFailed {
                                                chat_id,
                                                temp_id,
                                                error: "send timed out".into(),
                                            });
                                        }
                                    }
                                }
                            }
                            Ok(_) => {}
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return,
                        }
                    }

                    match tokio::time::timeout(Duration::from_millis(100), client.next_auth()).await
                    {
                        Ok(Ok(next)) => {
                            state = next;
                            let _ = event_tx.send(Event::Auth(map_auth(state.clone())));
                            if state == AuthState::Ready {
                                publish_ready(&api, &event_tx).await;
                            }
                            if state == AuthState::Closed {
                                return;
                            }
                        }
                        Ok(Err(error)) => {
                            let _ = event_tx.send(Event::Error(format!("{error:#}")));
                            return;
                        }
                        Err(_) => {}
                    }
                }
            });
        });

        Self {
            tx: cmd_tx,
            rx: event_rx,
            auth: AuthView::Starting,
            chats: cached_chats,
            chats_cache: Some(chats_cache),
            chats_loading: false,
            message_cache: cached_messages,
            messages_cache: Some(messages_cache),
            history_loading: HashSet::new(),
            history_loaded: HashSet::new(),
            last_read: HashMap::new(),
            notice: None,
            next_temp_id: -1,
            send_failure: None,
        }
    }

    fn request_history(&mut self, chat: ChatId, prefetch: bool) {
        let sparse = self
            .message_cache
            .get(&chat)
            .is_none_or(|messages| messages.len() <= 1);
        if prefetch && !sparse {
            return;
        }
        if (!sparse && self.history_loaded.contains(&chat)) || !self.history_loading.insert(chat) {
            return;
        }
        if sparse {
            self.history_loaded.remove(&chat);
        }
        tracing::debug!("requesting history for chat {chat} (prefetch={prefetch})");
        let command = if prefetch {
            Command::Prefetch(chat)
        } else {
            Command::History(chat)
        };
        if self.tx.send(command).is_err() {
            self.history_loading.remove(&chat);
        }
    }
}

async fn publish_ready(api: &TgApi, events: &Sender<Event>) {
    let _ = events.send(Event::ChatsLoading);
    tracing::info!("loading chat list");
    for limit in [30, 100] {
        match api.chats(limit).await {
            Ok(chats) => {
                tracing::info!("chat list loaded: {} chats", chats.len());
                let _ = events.send(Event::Chats(chats));
            }
            Err(error) => {
                tracing::error!("chat list failed: {error:#}");
                let _ = events.send(Event::Notice(format!("{error:#}")));
                if limit == 30 {
                    let _ = events.send(Event::ChatsFailed);
                }
            }
        }
    }
}

async fn handle_update(api: &TgApi, events: &Sender<Event>, update: TdUpdate) {
    match update {
        TdUpdate::NewMessage(update) => {
            let message = api.message_info(&update.message).await;
            let _ = events.send(Event::Incoming(map_message_info(message)));
        }
        TdUpdate::MessageSendSucceeded(update) => {
            let message = api.message_info(&update.message).await;
            let _ = events.send(Event::MessageSent {
                old_id: update.old_message_id,
                message: map_message_info(message),
            });
        }
        TdUpdate::MessageSendFailed(update) => {
            let _ = events.send(Event::MessageSendFailed {
                old_id: update.old_message_id,
                error: update.error.message,
            });
        }
        TdUpdate::ChatReadInbox(update) => {
            let _ = events.send(Event::ReadInbox {
                chat_id: update.chat_id,
                unread: update.unread_count.max(0) as u32,
            });
        }
        TdUpdate::ChatTitle(update) => {
            let _ = events.send(Event::ChatTitle {
                chat_id: update.chat_id,
                title: update.title,
            });
        }
        TdUpdate::ChatLastMessage(update) => {
            if let Some(last) = update.last_message.as_ref() {
                let message = api.message_info(last).await;
                let preview = if message.text.is_empty() {
                    "[message]".to_string()
                } else {
                    message.text
                };
                let _ = events.send(Event::ChatPreview {
                    chat_id: update.chat_id,
                    preview,
                });
            }
        }
        TdUpdate::NewChat(update) => {
            let _ = events.send(Event::ChatTitle {
                chat_id: update.chat.id,
                title: update.chat.title,
            });
        }
        _ => {}
    }
}

async fn load_history(api: &TgApi, events: &Sender<Event>, chat_id: ChatId) {
    tracing::info!("loading history for chat {chat_id}");
    let result =
        tokio::time::timeout(Duration::from_secs(30), api.history(chat_id, HISTORY_LIMIT)).await;
    match result {
        Ok(Ok(messages)) => {
            tracing::info!(
                "history loaded for chat {chat_id}: {} messages",
                messages.len()
            );
            let _ = events.send(Event::Messages { chat_id, messages });
        }
        Ok(Err(error)) => {
            tracing::error!("history failed for chat {chat_id}: {error:#}");
            let _ = events.send(Event::Notice(format!("{error:#}")));
            let _ = events.send(Event::HistoryFailed(chat_id));
        }
        Err(_) => {
            tracing::error!("history timed out for chat {chat_id}");
            let _ = events.send(Event::Notice(format!(
                "loading messages timed out (chat {chat_id})"
            )));
            let _ = events.send(Event::HistoryFailed(chat_id));
        }
    }
}

fn map_message_info(info: MessageInfo) -> Message {
    Message {
        id: info.id,
        chat_id: info.chat_id,
        sender: info.sender,
        text: info.text,
        outgoing: info.outgoing,
        time: info.time,
        pending: false,
        edited: info.edited,
        kind: map_message_kind(info.kind),
    }
}

fn map_chat_kind(kind: ChatInfoKind) -> ChatKind {
    match kind {
        ChatInfoKind::Saved => ChatKind::Saved,
        ChatInfoKind::Private => ChatKind::Private,
        ChatInfoKind::Group => ChatKind::Group,
        ChatInfoKind::Channel => ChatKind::Channel,
    }
}

fn map_message_kind(kind: TgMessageKind) -> MessageKind {
    match kind {
        TgMessageKind::Text => MessageKind::Text,
        TgMessageKind::Photo => MessageKind::Photo,
        TgMessageKind::Video => MessageKind::Video,
        TgMessageKind::Animation => MessageKind::Animation,
        TgMessageKind::Audio => MessageKind::Audio,
        TgMessageKind::Voice => MessageKind::Voice,
        TgMessageKind::Sticker => MessageKind::Sticker,
        TgMessageKind::File => MessageKind::File,
        TgMessageKind::Poll => MessageKind::Poll,
        TgMessageKind::Location => MessageKind::Location,
        TgMessageKind::Contact => MessageKind::Contact,
        TgMessageKind::Service => MessageKind::Service,
        TgMessageKind::Other => MessageKind::Other,
    }
}

/// Replaces the cached history with the freshly loaded page while keeping
/// local-only messages (for example an optimistic pending send) at the end.
fn merge_history(cache: &mut Vec<Message>, incoming: Vec<Message>) {
    let known: HashSet<_> = incoming.iter().map(|message| message.id).collect();
    let mut merged = incoming;
    merged.extend(
        cache
            .drain(..)
            .filter(|message| !known.contains(&message.id)),
    );
    *cache = merged;
}

fn load_chat_cache(path: &Path) -> Vec<Chat> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<Vec<Chat>>(&bytes) {
        Ok(chats) => chats,
        Err(error) => {
            tracing::warn!("cannot read chat cache {}: {error}", path.display());
            Vec::new()
        }
    }
}

fn save_chat_cache(path: &Path, chats: &[Chat]) {
    if chats
        .iter()
        .any(|chat| chat.title == format!("Chat {}", chat.id))
    {
        return;
    }
    match serde_json::to_vec(chats) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(path, bytes) {
                tracing::warn!("cannot write chat cache {}: {error}", path.display());
            }
        }
        Err(error) => tracing::warn!("cannot serialize chat cache: {error}"),
    }
}

fn load_message_cache(path: &Path) -> HashMap<ChatId, Vec<Message>> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashMap::new();
    };
    let Ok(mut cache) = serde_json::from_slice::<HashMap<ChatId, Vec<Message>>>(&bytes) else {
        tracing::warn!("cannot read message cache {}", path.display());
        return HashMap::new();
    };
    for messages in cache.values_mut() {
        messages.retain(|message| message.id > 0 && !message.pending);
        if messages.len() > 100 {
            messages.drain(..messages.len() - 100);
        }
    }
    cache
}

fn save_message_cache(path: &Path, cache: &HashMap<ChatId, Vec<Message>>) {
    match serde_json::to_vec(cache) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(path, bytes) {
                tracing::warn!("cannot write message cache {}: {error}", path.display());
            }
        }
        Err(error) => tracing::warn!("cannot serialize message cache: {error}"),
    }
}

fn map_auth(state: AuthState) -> AuthView {
    match state {
        AuthState::WaitPhoneNumber => AuthView::WaitPhoneNumber,
        AuthState::WaitCode => AuthView::WaitCode,
        AuthState::WaitPassword { hint } => AuthView::WaitPassword { hint },
        AuthState::WaitOtherDevice { link } => AuthView::WaitOtherDevice { link },
        AuthState::Ready => AuthView::Ready,
        AuthState::Closed => AuthView::Closed,
        AuthState::Other(state) => AuthView::Error(state),
    }
}

impl Backend for TgBackend {
    fn auth(&self) -> AuthView {
        self.auth.clone()
    }

    fn submit_phone(&mut self, phone: String) {
        let _ = self.tx.send(Command::Phone(phone));
    }

    fn submit_code(&mut self, code: String) {
        let _ = self.tx.send(Command::Code(code));
    }

    fn submit_password(&mut self, password: String) {
        let _ = self.tx.send(Command::Password(password));
    }

    fn request_qr(&mut self) {
        let _ = self.tx.send(Command::Qr);
    }

    fn logout(&mut self) {
        self.chats.clear();
        self.message_cache.clear();
        self.history_loading.clear();
        self.history_loaded.clear();
        self.chats_loading = false;
        self.notice = None;
        if let Some(path) = &self.chats_cache {
            let _ = std::fs::remove_file(path);
        }
        if let Some(path) = &self.messages_cache {
            let _ = std::fs::remove_file(path);
        }
        if self.tx.send(Command::Logout).is_err() {
            self.auth = AuthView::Error("TDLib worker is not running".into());
        } else {
            self.auth = AuthView::Starting;
        }
    }

    fn open_chat(&mut self, chat: ChatId) {
        self.request_history(chat, false);
    }

    fn prefetch_chat(&mut self, chat: ChatId) {
        self.request_history(chat, true);
    }

    fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    fn take_send_failure(&mut self) -> Option<SendFailure> {
        self.send_failure.take()
    }

    fn chats_loading(&self) -> bool {
        self.chats_loading
    }

    fn can_send(&self) -> bool {
        self.auth == AuthView::Ready
    }

    fn chats(&self) -> &[Chat] {
        &self.chats
    }

    fn history_loading(&self, chat: ChatId) -> bool {
        self.history_loading.contains(&chat)
    }

    fn messages(&self, chat: ChatId) -> &[Message] {
        self.message_cache
            .get(&chat)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn send_text(&mut self, chat: ChatId, text: String) -> Message {
        let temp_id = self.next_temp_id;
        self.next_temp_id = self.next_temp_id.wrapping_sub(1);
        let message = Message {
            id: temp_id,
            chat_id: chat,
            sender: "You".into(),
            text: text.clone(),
            outgoing: true,
            time: "now".into(),
            kind: MessageKind::Text,
            pending: true,
            edited: false,
        };

        if text.trim().is_empty() {
            return message;
        }
        if self.auth != AuthView::Ready {
            self.send_failure = Some(SendFailure {
                chat_id: chat,
                text,
                error: "TDLib is not ready".into(),
            });
            return message;
        }

        message_cache::push_pending(&mut self.message_cache, message.clone());
        if let Err(error) = self.tx.send(Command::Send {
            chat_id: chat,
            text,
            temp_id,
        }) {
            message_cache::remove_pending(&mut self.message_cache, chat, temp_id);
            self.send_failure = Some(SendFailure {
                chat_id: chat,
                text: message.text.clone(),
                error: format!("TDLib worker is not running: {error}"),
            });
        }
        message
    }

    fn mark_read(&mut self, chat: ChatId) {
        let Some(message_id) = self
            .message_cache
            .get(&chat)
            .and_then(|messages| messages.last())
            .map(|message| message.id)
            .filter(|message_id| *message_id > 0)
        else {
            return;
        };
        if self
            .last_read
            .get(&chat)
            .is_some_and(|last| *last >= message_id)
        {
            return;
        }
        if self
            .tx
            .send(Command::MarkRead {
                chat_id: chat,
                message_id,
            })
            .is_ok()
        {
            self.last_read.insert(chat, message_id);
            if let Some(cached) = self.chats.iter_mut().find(|cached| cached.id == chat) {
                cached.unread = 0;
            }
            if let Some(path) = &self.chats_cache {
                save_chat_cache(path, &self.chats);
            }
        }
    }

    fn tick(&mut self) -> Option<Message> {
        let mut arrived = None;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Auth(auth) => {
                    self.auth = auth;
                }
                Event::Chats(chats) => {
                    self.chats_loading = false;
                    let previous: HashMap<_, _> = self
                        .chats
                        .iter()
                        .map(|chat| (chat.id, chat.title.clone()))
                        .collect();
                    let mut next: Vec<Chat> = chats
                        .into_iter()
                        .map(|chat| Chat {
                            id: chat.id,
                            title: chat.title,
                            preview: chat.preview,
                            unread: chat.unread,
                            muted: chat.muted,
                            pinned: chat.pinned,
                            kind: map_chat_kind(chat.kind),
                        })
                        .collect();
                    for chat in &mut next {
                        if chat.title == format!("Chat {}", chat.id)
                            && let Some(title) = previous.get(&chat.id)
                        {
                            chat.title.clone_from(title);
                        }
                    }
                    self.chats = next;
                    if let Some(path) = &self.chats_cache {
                        save_chat_cache(path, &self.chats);
                    }
                    let prefetch: Vec<ChatId> =
                        self.chats.iter().take(8).map(|chat| chat.id).collect();
                    for chat_id in prefetch {
                        self.request_history(chat_id, true);
                    }
                }
                Event::ChatsLoading => {
                    self.chats_loading = true;
                }
                Event::ChatsFailed => {
                    self.chats_loading = false;
                }
                Event::Messages { chat_id, messages } => {
                    self.history_loading.remove(&chat_id);
                    self.history_loaded.insert(chat_id);
                    let incoming = messages.into_iter().map(map_message_info).collect();
                    merge_history(self.message_cache.entry(chat_id).or_default(), incoming);
                    if let Some(path) = &self.messages_cache {
                        save_message_cache(path, &self.message_cache);
                    }
                }
                Event::HistoryFailed(chat_id) => {
                    self.history_loading.remove(&chat_id);
                }
                Event::Notice(notice) => {
                    self.notice = Some(notice);
                }
                Event::Error(error) => {
                    self.auth = AuthView::Error(error);
                }
                Event::Sent {
                    chat_id,
                    temp_id,
                    message,
                } => {
                    message_cache::replace_pending(
                        &mut self.message_cache,
                        chat_id,
                        temp_id,
                        message,
                    );
                    if let Some(path) = &self.messages_cache {
                        save_message_cache(path, &self.message_cache);
                    }
                }
                Event::SendFailed {
                    chat_id,
                    temp_id,
                    error,
                } => {
                    let pending =
                        message_cache::remove_pending(&mut self.message_cache, chat_id, temp_id);
                    self.send_failure = Some(SendFailure {
                        chat_id,
                        text: pending.map(|message| message.text).unwrap_or_default(),
                        error,
                    });
                }
                Event::Incoming(message) => {
                    let messages = self.message_cache.entry(message.chat_id).or_default();
                    if !messages.iter().any(|cached| cached.id == message.id) {
                        messages.push(message.clone());
                        messages.sort_by_key(|cached| cached.id);
                    }
                    if let Some(chat) = self
                        .chats
                        .iter_mut()
                        .find(|chat| chat.id == message.chat_id)
                    {
                        chat.preview = if message.text.is_empty() {
                            "[message]".to_string()
                        } else {
                            message.text.clone()
                        };
                        if !message.outgoing {
                            chat.unread = chat.unread.saturating_add(1);
                        }
                    }
                    if let Some(path) = &self.messages_cache {
                        save_message_cache(path, &self.message_cache);
                    }
                    arrived = Some(message);
                }
                Event::MessageSent { old_id, message } => {
                    message_cache::replace_pending(
                        &mut self.message_cache,
                        message.chat_id,
                        old_id,
                        message.clone(),
                    );
                    if let Some(path) = &self.messages_cache {
                        save_message_cache(path, &self.message_cache);
                    }
                    arrived = Some(message);
                }
                Event::MessageSendFailed { old_id, error } => {
                    let pending = self.message_cache.values_mut().find_map(|messages| {
                        messages
                            .iter()
                            .position(|message| message.id == old_id)
                            .map(|index| messages.remove(index))
                    });
                    if let Some(pending) = pending {
                        self.send_failure = Some(SendFailure {
                            chat_id: pending.chat_id,
                            text: pending.text,
                            error,
                        });
                    }
                }
                Event::ReadInbox { chat_id, unread } => {
                    if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                        chat.unread = unread;
                    }
                    if let Some(path) = &self.chats_cache {
                        save_chat_cache(path, &self.chats);
                    }
                }
                Event::ChatPreview { chat_id, preview } => {
                    if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                        chat.preview = preview;
                    }
                    if let Some(path) = &self.chats_cache {
                        save_chat_cache(path, &self.chats);
                    }
                }
                Event::ChatTitle { chat_id, title } => {
                    if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                        chat.title = title;
                    }
                    if let Some(path) = &self.chats_cache {
                        save_chat_cache(path, &self.chats);
                    }
                }
            }
        }
        arrived
    }
}

fn config_from_env() -> anyhow::Result<Config> {
    let api_id = std::env::var("TG_API_ID")
        .map_err(|_| anyhow::anyhow!("TG_API_ID is not set"))?
        .parse::<i32>()
        .map_err(|_| anyhow::anyhow!("TG_API_ID must be a number"))?;
    let api_hash =
        std::env::var("TG_API_HASH").map_err(|_| anyhow::anyhow!("TG_API_HASH is not set"))?;
    let data_dir = match std::env::var("TG_DATA_DIR") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("HOME is not set; set TG_DATA_DIR"))?;
            std::path::PathBuf::from(home).join(".local/share/gramterm")
        }
    };
    Ok(Config::new(api_id, api_hash, data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: i64, chat_id: i64, text: &str) -> MessageInfo {
        MessageInfo {
            id,
            chat_id,
            sender: "Them".into(),
            text: text.into(),
            outgoing: false,
            time: "12:00".into(),
            kind: TgMessageKind::Text,
            edited: false,
        }
    }

    fn message(id: i64, text: &str, pending: bool) -> Message {
        Message {
            id,
            chat_id: 7,
            sender: "You".into(),
            text: text.into(),
            outgoing: true,
            time: "now".into(),
            kind: MessageKind::Text,
            pending,
            edited: false,
        }
    }

    fn backend() -> (TgBackend, Sender<Event>, Receiver<Command>) {
        TgBackend::for_test()
    }

    #[test]
    fn open_chat_requests_history_once() {
        let (mut backend, _events, commands) = backend();
        backend.open_chat(7);
        assert!(backend.history_loading(7));
        assert!(matches!(commands.try_recv(), Ok(Command::History(7))));

        backend.open_chat(7);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn prefetch_uses_the_low_priority_command_once() {
        let (mut backend, _events, commands) = backend();
        backend.prefetch_chat(7);
        assert!(backend.history_loading(7));
        assert!(matches!(commands.try_recv(), Ok(Command::Prefetch(7))));

        backend.prefetch_chat(7);
        assert!(commands.try_recv().is_err());

        backend.open_chat(7);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn mark_read_sends_only_when_the_last_message_changes() {
        let (mut backend, _events, commands) = backend();
        backend
            .message_cache
            .insert(7, vec![message(42, "hello", false)]);
        backend.mark_read(7);
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::MarkRead {
                chat_id: 7,
                message_id: 42
            })
        ));
        backend.mark_read(7);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn incoming_event_populates_cache() {
        let (mut backend, events, _commands) = backend();
        events
            .send(Event::Incoming(message(43, "incoming", false)))
            .unwrap();
        assert!(backend.tick().is_some());
        assert_eq!(backend.messages(7).len(), 1);
        assert_eq!(backend.messages(7)[0].text, "incoming");
    }

    #[test]
    fn message_cache_round_trips_through_disk() {
        let path = std::env::temp_dir().join(format!(
            "gramterm-messages-test-{}.json",
            std::process::id()
        ));
        let mut cache = HashMap::new();
        cache.insert(7, vec![message(1, "hello", false)]);
        save_message_cache(&path, &cache);
        let loaded = load_message_cache(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded[&7].len(), 1);
        assert_eq!(loaded[&7][0].text, "hello");
        assert!(!loaded[&7][0].pending);
    }

    #[test]
    fn history_is_not_requested_again_after_loading() {
        let (mut backend, events, commands) = backend();
        backend.open_chat(7);
        events
            .send(Event::Messages {
                chat_id: 7,
                messages: Vec::new(),
            })
            .unwrap();
        assert!(backend.tick().is_none());
        assert!(!backend.history_loading(7));

        backend.open_chat(7);
        assert!(commands.try_recv().is_ok());
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn messages_event_fills_cache_and_clears_loading() {
        let (mut backend, events, _commands) = backend();
        backend.open_chat(7);
        events
            .send(Event::Messages {
                chat_id: 7,
                messages: vec![info(1, 7, "hi")],
            })
            .unwrap();
        assert!(backend.tick().is_none());

        assert!(!backend.history_loading(7));
        let messages = backend.messages(7);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, 1);
        assert_eq!(messages[0].text, "hi");
        assert_eq!(messages[0].sender, "Them");
        assert!(!messages[0].pending);
        assert!(!messages[0].edited);
    }

    #[test]
    fn history_failure_clears_loading_and_allows_retry() {
        let (mut backend, events, commands) = backend();
        backend.open_chat(7);
        events.send(Event::HistoryFailed(7)).unwrap();
        assert!(backend.tick().is_none());
        assert!(!backend.history_loading(7));

        backend.open_chat(7);
        assert!(matches!(commands.try_recv(), Ok(Command::History(7))));
    }

    #[test]
    fn history_merge_dedupes_loaded_messages_and_keeps_local_tail() {
        let mut cache = vec![message(1, "stale copy", false), message(9, "pending", true)];
        merge_history(&mut cache, vec![map_message_info(info(1, 7, "fresh copy"))]);

        assert_eq!(cache.iter().map(|m| m.id).collect::<Vec<_>>(), vec![1, 9]);
        assert_eq!(cache[0].text, "fresh copy");
        assert!(cache[1].pending);
    }
}
