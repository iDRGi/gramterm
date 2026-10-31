use std::collections::HashMap;

use crate::backend::{Backend, Chat, ChatId, ChatKind, Message, MsgId};

#[derive(Debug)]
pub struct DemoBackend {
    chats: Vec<Chat>,
    messages: HashMap<ChatId, Vec<Message>>,
    next_id: MsgId,
    ticks: u64,
}

impl DemoBackend {
    pub fn new() -> Self {
        let chats = vec![
            Chat {
                id: 1,
                title: "Saved Messages".into(),
                preview: "Заметки по Gramterm".into(),
                unread: 0,
                muted: false,
                pinned: true,
                kind: ChatKind::Saved,
            },
            Chat {
                id: 2,
                title: "Rust Community".into(),
                preview: "Alice: TUI уже собирается".into(),
                unread: 3,
                muted: false,
                pinned: true,
                kind: ChatKind::Group,
            },
            Chat {
                id: 3,
                title: "Gramterm Dev".into(),
                preview: "План Phase 0 готов".into(),
                unread: 1,
                muted: false,
                pinned: false,
                kind: ChatKind::Group,
            },
            Chat {
                id: 4,
                title: "Alice".into(),
                preview: "Как продвигается клиент?".into(),
                unread: 2,
                muted: false,
                pinned: false,
                kind: ChatKind::Private,
            },
            Chat {
                id: 5,
                title: "Telegram News".into(),
                preview: "Новые функции Telegram".into(),
                unread: 0,
                muted: true,
                pinned: false,
                kind: ChatKind::Channel,
            },
        ];

        let mut messages = HashMap::new();

        messages.insert(
            1,
            vec![
                Message {
                    id: 101,
                    chat_id: 1,
                    sender: "You".into(),
                    text: "Gramterm: Telegram в терминале, но по-человечески.".into(),
                    outgoing: true,
                    time: "09:12".into(),
                    pending: false,
                    edited: false,
                },
                Message {
                    id: 102,
                    chat_id: 1,
                    sender: "You".into(),
                    text: "Simple по умолчанию, vim по желанию. Мышь, RU/EN, passcode.".into(),
                    outgoing: true,
                    time: "09:13".into(),
                    pending: false,
                    edited: false,
                },
            ],
        );

        messages.insert(
            2,
            vec![
                Message {
                    id: 201,
                    chat_id: 2,
                    sender: "Alice".into(),
                    text: "Привет! Видел твой TUI — уже выглядит живо.".into(),
                    outgoing: false,
                    time: "10:41".into(),
                    pending: false,
                    edited: false,
                },
                Message {
                    id: 202,
                    chat_id: 2,
                    sender: "Bob".into(),
                    text: "Главное — не уходить в vim-only. Мышь и простой режим решают.".into(),
                    outgoing: false,
                    time: "10:42".into(),
                    pending: false,
                    edited: false,
                },
                Message {
                    id: 203,
                    chat_id: 2,
                    sender: "You".into(),
                    text: "Согласен. Начинаем с каркаса и demo-режима.".into(),
                    outgoing: true,
                    time: "10:43".into(),
                    pending: false,
                    edited: false,
                },
                Message {
                    id: 204,
                    chat_id: 2,
                    sender: "Alice".into(),
                    text: "TUI уже собирается. Ждём TDLib.".into(),
                    outgoing: false,
                    time: "10:44".into(),
                    pending: false,
                    edited: false,
                },
            ],
        );

        messages.insert(
            3,
            vec![
                Message {
                    id: 301,
                    chat_id: 3,
                    sender: "You".into(),
                    text: "План Phase 0 готов: каркас, MockBackend, Simple, мышь.".into(),
                    outgoing: true,
                    time: "11:00".into(),
                    pending: false,
                    edited: false,
                },
                Message {
                    id: 302,
                    chat_id: 3,
                    sender: "Dev".into(),
                    text: "TDLib подключаем вторым этапом, когда UI уже можно смотреть.".into(),
                    outgoing: false,
                    time: "11:01".into(),
                    pending: false,
                    edited: false,
                },
            ],
        );

        messages.insert(
            4,
            vec![
                Message {
                    id: 401,
                    chat_id: 4,
                    sender: "Alice".into(),
                    text: "Привет! Как продвигается клиент?".into(),
                    outgoing: false,
                    time: "12:05".into(),
                    pending: false,
                    edited: false,
                },
                Message {
                    id: 402,
                    chat_id: 4,
                    sender: "You".into(),
                    text: "Пишу каркас. Мышь уже работает, осталось TDLib.".into(),
                    outgoing: true,
                    time: "12:06".into(),
                    pending: false,
                    edited: false,
                },
                Message {
                    id: 403,
                    chat_id: 4,
                    sender: "Alice".into(),
                    text: "Круто. Не забудь про русский язык и уведомления.".into(),
                    outgoing: false,
                    time: "12:07".into(),
                    pending: false,
                    edited: false,
                },
            ],
        );

        messages.insert(
            5,
            vec![Message {
                id: 501,
                chat_id: 5,
                sender: "Telegram News".into(),
                text: "Новые функции Telegram: истории, реакции и многое другое.".into(),
                outgoing: false,
                time: "08:30".into(),
                pending: false,
                edited: false,
            }],
        );

        Self {
            chats,
            messages,
            next_id: 1000,
            ticks: 0,
        }
    }
}

impl Default for DemoBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for DemoBackend {
    fn chats(&self) -> &[Chat] {
        &self.chats
    }

    fn messages(&self, chat: ChatId) -> &[Message] {
        self.messages.get(&chat).map(Vec::as_slice).unwrap_or(&[])
    }

    fn send_text(&mut self, chat: ChatId, text: String) -> Message {
        self.next_id += 1;
        let message = Message {
            id: self.next_id,
            chat_id: chat,
            sender: "You".into(),
            text: text.clone(),
            outgoing: true,
            time: "now".into(),
            pending: false,
            edited: false,
        };

        self.messages.entry(chat).or_default().push(message.clone());
        if let Some(chat) = self.chats.iter_mut().find(|c| c.id == chat) {
            chat.preview = text;
            chat.unread = 0;
        }
        message
    }

    fn mark_read(&mut self, chat: ChatId) {
        if let Some(chat) = self.chats.iter_mut().find(|c| c.id == chat) {
            chat.unread = 0;
        }
    }

    fn tick(&mut self) -> Option<Message> {
        self.ticks += 1;
        if !self.ticks.is_multiple_of(40) {
            return None;
        }

        let chat_id = 4;
        self.next_id += 1;
        let message = Message {
            id: self.next_id,
            chat_id,
            sender: "Alice".into(),
            text: "Сообщение из demo-режима: TDLib ещё не подключён.".into(),
            outgoing: false,
            time: "now".into(),
            pending: false,
            edited: false,
        };

        self.messages
            .entry(chat_id)
            .or_default()
            .push(message.clone());
        if let Some(chat) = self.chats.iter_mut().find(|c| c.id == chat_id) {
            chat.preview = message.text.clone();
            chat.unread = chat.unread.saturating_add(1);
        }
        Some(message)
    }
}
