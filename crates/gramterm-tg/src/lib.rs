//! TDLib-backed primitives for Gramterm.
//!
//! This crate deliberately hides TDLib's generated types behind a small
//! surface so the TUI can eventually depend on it without knowing about
//! `tdlib-rs` details.

use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Clone)]
pub struct MessageInfo {
    pub id: i64,
    pub chat_id: i64,
    pub sender: String,
    pub text: String,
    pub outgoing: bool,
    pub time: String,
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
        tracing::info!("tdlib: creating client");
        let client_id = tdlib_rs::create_client();
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
                std::time::Duration::from_secs(5),
                functions::get_chat(chat_id, client_id),
            )
            .await
            {
                Ok(Ok(enums::Chat::Chat(chat))) => ChatInfo {
                    id: chat.id,
                    title: chat.title,
                },
                _ => ChatInfo {
                    id: chat_id,
                    title: format!("Chat {chat_id}"),
                },
            }
        });
        Ok(futures::future::join_all(futures).await)
    }

    /// Returns the newest `limit` messages in chronological order.
    pub async fn history(&self, chat_id: i64, limit: i32) -> Result<Vec<MessageInfo>> {
        let page =
            td(functions::get_chat_history(chat_id, 0, 0, limit, false, self.client_id).await)?;
        let enums::Messages::Messages(page) = page;
        let mut messages: Vec<MessageInfo> = page
            .messages
            .into_iter()
            .flatten()
            .map(|message| map_message(message))
            .collect();
        messages.reverse();
        Ok(messages)
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
        Ok(map_message(message))
    }
}

fn map_message(message: types::Message) -> MessageInfo {
    let text = match message.content {
        enums::MessageContent::MessageText(content) => content.text.text,
        enums::MessageContent::MessagePhoto(content) => content.caption.text,
        _ => String::new(),
    };
    MessageInfo {
        id: message.id,
        chat_id: message.chat_id,
        sender: if message.is_outgoing {
            "You".into()
        } else {
            "Them".into()
        },
        text,
        outgoing: message.is_outgoing,
        time: format_time(message.date),
    }
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
