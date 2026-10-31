use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use gramterm_tg::{AuthState, ChatInfo, Config, MessageInfo, TgClient};

use crate::backend::{AuthView, Backend, Chat, ChatId, ChatKind, Message};

/// How many recent messages `open_chat` loads.
const HISTORY_LIMIT: i32 = 50;

enum Command {
    Phone(String),
    Code(String),
    Password(String),
    Qr,
    History(ChatId),
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
}

pub struct TgBackend {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    auth: AuthView,
    chats: Vec<Chat>,
    chats_loading: bool,
    message_cache: HashMap<ChatId, Vec<Message>>,
    history_loading: HashSet<ChatId>,
    history_loaded: HashSet<ChatId>,
    notice: Option<String>,
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
            chats_loading: false,
            message_cache: HashMap::new(),
            history_loading: HashSet::new(),
            history_loaded: HashSet::new(),
            notice: None,
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
            chats_loading: false,
            message_cache: HashMap::new(),
            history_loading: HashSet::new(),
            history_loaded: HashSet::new(),
            notice: None,
        };
        (backend, events, commands)
    }

    fn start(config: Config) -> Self {
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

                let _ = event_tx.send(Event::Auth(map_auth(state.clone())));
                if state == AuthState::Ready {
                    publish_ready(&client, &event_tx).await;
                }

                loop {
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
                            Ok(Command::History(chat_id)) => {
                                load_history(&client, &event_tx, chat_id).await;
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
                                publish_ready(&client, &event_tx).await;
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
            chats: Vec::new(),
            chats_loading: false,
            message_cache: HashMap::new(),
            history_loading: HashSet::new(),
            history_loaded: HashSet::new(),
            notice: None,
        }
    }
}

async fn publish_ready(client: &TgClient, events: &Sender<Event>) {
    let _ = events.send(Event::ChatsLoading);
    tracing::info!("loading chat list");
    match client.chats(30).await {
        Ok(chats) => {
            tracing::info!("chat list loaded: {} chats", chats.len());
            let _ = events.send(Event::Chats(chats));
        }
        Err(error) => {
            tracing::error!("chat list failed: {error:#}");
            let _ = events.send(Event::Notice(format!("{error:#}")));
            let _ = events.send(Event::ChatsFailed);
        }
    }
}

async fn load_history(client: &TgClient, events: &Sender<Event>, chat_id: ChatId) {
    tracing::info!("loading history for chat {chat_id}");
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        client.history(chat_id, HISTORY_LIMIT),
    )
    .await;
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
        edited: false,
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

    fn open_chat(&mut self, chat: ChatId) {
        if self.history_loaded.contains(&chat) || !self.history_loading.insert(chat) {
            return;
        }
        tracing::debug!("requesting history for chat {chat}");
        if self.tx.send(Command::History(chat)).is_err() {
            self.history_loading.remove(&chat);
        }
    }

    fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    fn chats_loading(&self) -> bool {
        self.chats_loading
    }

    fn can_send(&self) -> bool {
        false
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
        let message = Message {
            id: 0,
            chat_id: chat,
            sender: "You".into(),
            text,
            outgoing: true,
            time: "now".into(),
            pending: true,
            edited: false,
        };
        self.message_cache
            .entry(chat)
            .or_default()
            .push(message.clone());
        message
    }

    fn mark_read(&mut self, _chat: ChatId) {}

    fn tick(&mut self) -> Option<Message> {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Auth(auth) => {
                    self.auth = auth;
                }
                Event::Chats(chats) => {
                    self.chats_loading = false;
                    self.chats = chats
                        .into_iter()
                        .map(|chat| Chat {
                            id: chat.id,
                            title: chat.title,
                            preview: String::new(),
                            unread: 0,
                            muted: false,
                            pinned: false,
                            kind: ChatKind::Private,
                        })
                        .collect();
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
            }
        }
        None
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
