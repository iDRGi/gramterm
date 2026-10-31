use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use gramterm_tg::{AuthState, ChatInfo, Config, TgClient};

use crate::backend::{AuthView, Backend, Chat, ChatId, ChatKind, Message};

enum Command {
    Phone(String),
    Code(String),
    Password(String),
}

enum Event {
    Auth(AuthView),
    Chats(Vec<ChatInfo>),
    Error(String),
}

pub struct TgBackend {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    auth: AuthView,
    chats: Vec<Chat>,
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
        }
    }

    fn start(config: Config) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ =
                        event_tx.send(Event::Error(format!("cannot start tokio runtime: {error}")));
                    return;
                }
            };

            runtime.block_on(async move {
                let (mut client, mut state) = match TgClient::connect(config).await {
                    Ok(pair) => pair,
                    Err(error) => {
                        let _ = event_tx.send(Event::Error(format!("{error:#}")));
                        return;
                    }
                };

                let _ = event_tx.send(Event::Auth(map_auth(state.clone())));
                if state == AuthState::Ready {
                    publish_ready(&client, &event_tx).await;
                }

                loop {
                    loop {
                        match cmd_rx.try_recv() {
                            Ok(Command::Phone(phone)) => {
                                if let Err(error) = client.submit_phone(&phone).await {
                                    let _ = event_tx.send(Event::Error(format!("{error:#}")));
                                }
                            }
                            Ok(Command::Code(code)) => {
                                if let Err(error) = client.submit_code(&code).await {
                                    let _ = event_tx.send(Event::Error(format!("{error:#}")));
                                }
                            }
                            Ok(Command::Password(password)) => {
                                if let Err(error) = client.submit_password(&password).await {
                                    let _ = event_tx.send(Event::Error(format!("{error:#}")));
                                }
                            }
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
        }
    }
}

async fn publish_ready(client: &TgClient, events: &Sender<Event>) {
    match client.chats(30).await {
        Ok(chats) => {
            let _ = events.send(Event::Chats(chats));
        }
        Err(error) => {
            let _ = events.send(Event::Error(format!("{error:#}")));
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

    fn can_send(&self) -> bool {
        false
    }

    fn chats(&self) -> &[Chat] {
        &self.chats
    }

    fn messages(&self, _chat: ChatId) -> &[Message] {
        &[]
    }

    fn send_text(&mut self, chat: ChatId, text: String) -> Message {
        Message {
            id: 0,
            chat_id: chat,
            sender: "You".into(),
            text,
            outgoing: true,
            time: "now".into(),
            pending: true,
            edited: false,
        }
    }

    fn mark_read(&mut self, _chat: ChatId) {}

    fn tick(&mut self) -> Option<Message> {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Auth(auth) => {
                    self.auth = auth;
                }
                Event::Chats(chats) => {
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
