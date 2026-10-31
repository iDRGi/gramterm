//! TDLib-backed primitives for Gramterm.
//!
//! This crate deliberately hides TDLib's generated types behind a small
//! surface so the TUI can eventually depend on it without knowing about
//! `tdlib-rs` details.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use tdlib_rs::enums::{self, Update};
use tdlib_rs::functions;
use tokio::sync::mpsc;

const APPLICATION_NAME: &str = "Gramterm";

unsafe extern "C" {
    fn td_execute(request: *const c_char) -> *const c_char;
}

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
        init_logging(&config)?;
        let client_id = tdlib_rs::create_client();
        let (tx, updates) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            while let Some((update, client_id)) = tdlib_rs::receive() {
                if tx.send((client_id, update)).is_err() {
                    break;
                }
            }
        });

        let mut client = Self {
            client_id,
            config,
            updates,
        };

        // A new client sends nothing until it gets its first request.
        td(functions::get_option("version".into(), client_id).await)?;
        let state = client.next_auth().await?;
        Ok((client, state))
    }

    pub fn client_id(&self) -> i32 {
        self.client_id
    }

    /// Waits for the next authorization state. `waitTdlibParameters` is
    /// answered here, so callers never see it.
    pub async fn next_auth(&mut self) -> Result<AuthState> {
        loop {
            let Some((client_id, update)) = self.updates.recv().await else {
                bail!("TDLib update channel closed");
            };
            if client_id != self.client_id {
                continue;
            }
            let Update::AuthorizationState(state) = update else {
                continue;
            };
            match state.authorization_state {
                enums::AuthorizationState::WaitTdlibParameters => {
                    self.set_parameters().await?;
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
        // 404 means there are no more chats; the ids already loaded are fine.
        let _ = td(functions::load_chats(None, limit, self.client_id).await);
        let chats = td(functions::get_chats(None, limit, self.client_id).await)?;
        let enums::Chats::Chats(chats) = chats;

        let mut out = Vec::with_capacity(chats.chat_ids.len());
        for chat_id in chats.chat_ids {
            let chat = td(functions::get_chat(chat_id, self.client_id).await)?;
            let enums::Chat::Chat(chat) = chat;
            out.push(ChatInfo {
                id: chat.id,
                title: chat.title,
            });
        }
        Ok(out)
    }
}

fn init_logging(config: &Config) -> Result<()> {
    let path = config
        .data_dir
        .join("tdlib.log")
        .to_string_lossy()
        .replace('\\', "/");
    let stream = format!(
        r#"{{"@type":"setLogStream","log_stream":{{"@type":"logStreamFile","path":"{path}","max_file_size":10485760,"redirect_stderr":false}}}}"#
    );
    execute(&stream)?;
    execute(r#"{"@type":"setLogVerbosityLevel","new_verbosity_level":1}"#)?;
    Ok(())
}

fn execute(request: &str) -> Result<String> {
    let request = CString::new(request).context("TDLib request contains NUL")?;
    let response = unsafe { td_execute(request.as_ptr()) };
    if response.is_null() {
        return Ok(String::new());
    }
    let response = unsafe { CStr::from_ptr(response) };
    Ok(response.to_string_lossy().into_owned())
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
