use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use gramterm_tg::{AuthState, ChatInfo, Config, TgClient};

use crate::backend::{AuthView, Backend, Chat, ChatId, ChatKind, Message, MsgId, SendFailure};
use crate::message_cache;

enum Command {
    Phone(String),
    Code(String),
    Password(String),
    Qr,
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
    Notice(String),
    Error(String),
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
    chats_loading: bool,
    message_cache: HashMap<ChatId, Vec<Message>>,
    history_loading: HashSet<ChatId>,
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
            chats_loading: false,
            message_cache: HashMap::new(),
            history_loading: HashSet::new(),
            notice: None,
            next_temp_id: -1,
            send_failure: None,
        }
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
                                        client.send_text(chat_id, &text),
                                    )
                                    .await
                                    {
                                        Ok(Ok(info)) => {
                                            let message = Message {
                                                id: info.id,
                                                chat_id: info.chat_id,
                                                sender: info.sender,
                                                text: info.text,
                                                outgoing: info.outgoing,
                                                time: info.time,
                                                pending: false,
                                                edited: false,
                                            };
                                            let _ = event_tx.send(Event::Sent {
                                                chat_id,
                                                temp_id,
                                                message,
                                            });
                                        }
                                        Ok(Err(error)) => {
                                            let _ = event_tx.send(Event::SendFailed {
                                                chat_id,
                                                temp_id,
                                                error: format!("{error:#}"),
                                            });
                                        }
                                        Err(_) => {
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
            notice: None,
            next_temp_id: -1,
            send_failure: None,
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
