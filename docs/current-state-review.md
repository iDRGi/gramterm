# Gramterm: текущее состояние и ревью

Дата: 2026-11-01  
Ветка: `main`  
Проверяемая функциональность: auth, список чатов, история сообщений, отправка текста.

## 1. Резюме

Сейчас Gramterm — это **рабочий прототип**, а не v1.0:

- вход через TDLib работает: телефон, код, 2FA, QR-запрос, сессия;
- список чатов загружается и кешируется между запусками;
- открывается история последних 50 сообщений;
- текст отправляется через TDLib с optimistic-сообщением;
- есть RU/EN, Simple-режим, мышь, темы.

До бизнес-цели «Telegram TUI for humans» не хватает главного:

- **живых обновлений** (новые сообщения, редактирование, удаление, read receipts);
- **полной истории** (пагинация старше 50);
- **медиа и сервисных сообщений** (сейчас только текст/подписи и заглушки);
- **папок, архива, unread, typing, drafts** в списке чатов;
- **passcode и шифрования локальной БД**;
- **поиска, уведомлений, настроек**.

Оценка: текущий код закрывает примерно **Stage 0 + 60% Stage 1 + 25% Stage 2** из `docs/development-plan.md`. Для ежедневного использования он ещё не готов.

## 2. Бизнес-задачи

Ключевые требования из плана:

1. Быстрый и удобный Telegram-клиент для обычного пользователя, не только для vim-пользователя.
2. Simple-режим по умолчанию: стрелки, Tab, Enter, мышь, понятные экраны.
3. RU/EN, аккуратная типографика, темы.
4. Приватность: passcode, шифрование TDLib DB, осторожные read receipts.
5. Полноценные чаты и сообщения: список, папки, unread, история, отправка, reply/edit/delete, медиа.
6. Поиск и уведомления.
7. Надёжность: reconnect, flood wait, outbox, понятные ошибки.

Проверка ниже — относительно этих задач, а не только «компилируется ли».

## 3. Как приложение работает сейчас

### 3.1. Запуск и авторизация

```
main
  -> App::with_backend(TgBackend::from_env)
  -> TgBackend::start
       -> read chats.json
       -> spawn OS thread
            -> multi-thread tokio runtime
            -> TgClient::connect
                 -> create TDLib client
                 -> receive thread
                 -> setTdlibParameters
                 -> next_auth
  -> UI: login screen until AuthView::Ready
  -> после Ready: cached chats + publish_ready()
```

- `TG_API_ID`, `TG_API_HASH` читаются из окружения; `.env` подхватывается launcher-скриптом.
- TDLib logs сейчас пишутся в stderr и перенаправляются скриптом запуска; программной настройки `tdlib.log` в коде нет.
- `next_auth` **поглощает все не-auth updates** и выбрасывает их. Это ключевая архитектурная проблема: новые сообщения и изменения чатов не доходят до UI.
- После `logOut` создаётся новый TDLib client и переиспользуется старый receive loop.

### 3.2. Загрузка списка чатов

```
TgBackend::start -> load chats.json (если есть)
Auth Ready
  -> publish_ready
       -> Event::ChatsLoading
       -> TgClient::chats(30)
            -> getChats(None, 30)
            -> getChat для каждого id (30 параллельно, timeout 30s)
       -> Event::Chats
            -> map ChatInfo -> domain Chat
            -> merge с предыдущими названиями
            -> save chats.json
```

Что уже есть:

- кеш `~/.local/share/gramterm/chats.json`;
- при повторном запуске список появляется сразу после `Ready`, не ожидая `getChats`;
- названия из кеша не затираются заглушками `Chat <id>`;
- в `Chat` теперь есть `preview`, `unread`, `muted`, `pinned`, `kind`;
- `ChatKind` определяется по `ChatType`: private, secret, basic group, supergroup, channel;
- Saved Messages определяется сравнением private user id с `getMe`.

Ограничения:

- загружаются только **30** чатов, без `loadChats` и без пагинации;
- нет папок, архива, порядка pinned/unread, typing, drafts;
- `getChat` выполняется для каждого id отдельно и может ждать до 30 секунд; первый запуск всё ещё медленный;
- `chats.json` — снимок, а не event-sourced состояние; при внешнем изменении чата он обновится только после нового `getChats`;
- `chats.json` не шифруется и хранит названия/unread/pinned;
- live updates не применяются, поэтому unread/preview в UI не меняются без перезапуска/перезагрузки.

### 3.3. Открытие чата и загрузка истории

```
App::open_selected_chat
  -> mark_read(chat)             // сейчас no-op
  -> open_chat(chat)             // Backend::open_chat
       -> Command::History
            -> TgClient::history
                 -> openChat
                 -> sleep 700ms
                 -> getChatHistory(0, 0, 50, only_local=false)
                 -> если <= 1: sleep 1.5s и повторить
                 -> если <= 1: getChatHistory(last_id, 0, 50)
                 -> dedupe/sort/limit
                 -> resolve sender names
            -> Event::Messages
                 -> history cache
                 -> UI render
```

Что уже есть:

- 50 последних сообщений;
- retry для случая, когда TDLib ещё синхронизирует чат и отдаёт только одно сообщение;
- fallback-запрос страницы «до последнего сообщения»;
- sender names через `getUser`/`getChat`;
- кеш сообщений в памяти;
- состояние загрузки и пустого чата;
- скролл колесом/PageUp/PageDown;
- `openChat` вызывается перед историей.

Ограничения:

- **нет пагинации старше 50**;
- история не персистентна: после рестарта грузится заново;
- `sleep 700ms` и два retry — эвристика, а не нормальное ожидание синхронизации;
- `load_history` имеет общий timeout 30 секунд; при отмене части запросов TDLib получает «unaccessible request» warning;
- sender resolution последовательный: до 50 `getUser`/`getChat`;
- service/media сообщения отображаются заглушками, без содержимого;
- нет reply/forwarded/album/reaction/poll UI;
- `mark_read` всё ещё no-op: read receipts не отправляются, unread не очищается;
- нет `closeChat` при уходе из чата.

### 3.4. Отправка сообщения

```
App::send
  -> Backend::send_text
       -> новый pending Message (temp_id < 0)
       -> Command::Send
  -> UI сразу видит pending
  -> worker: TgClient::send_text
       -> sendMessage
  -> Event::Sent
       -> replace pending на server message
  -> при ошибке: Event::SendFailed
       -> pending удаляется
       -> текст возвращается в composer
       -> статус
```

Что работает:

- реальный `sendMessage`;
- optimistic UI;
- замена pending на server id;
- failure cleanup;
- composer restoration;
- `can_send` только в `Ready`.

Ограничения:

- `sendMessage` может вернуть сообщение с `sending_state = pending`, но `map_message` ставит `pending: false`; UI может показать «отправлено» раньше фактического ack;
- нет обработки `updateMessageSendSucceeded`/`updateMessageSendFailed`, поэтому сообщения, отправленные с другого устройства, и поздние статусы не обновляются;
- нет retry-кнопки;
- нет reply/edit/delete/forward;
- нет outbox на диск: незавершённая отправка теряется при падении.

### 3.5. Что видно в чатах

| TDLib content | Сейчас |
|---|---|
| text | текст |
| photo | подпись; без подписи `[фото]` |
| video | подпись; без подписи `[видео]` |
| animation/GIF | подпись; без подписи `[GIF]` |
| audio | подпись; без подписи `[аудио]` |
| voice | подпись; без подписи `[голосовое]` |
| sticker | `[стикер]` |
| document | подпись; без подписи `[файл]` |
| poll | вопрос |
| location/contact | заглушка |
| service messages | `[служебное сообщение]` |
| unsupported/other | `[сообщение]` |

То есть **пустых баблов больше нет**, но это ещё не отображение медиа: нет фото, файлов, голосовых, стикеров, опросов, альбомов, reply, forwarded, реакций.

Список чатов показывает:

- название;
- kind-иконку (private/group/channel/saved);
- preview последнего сообщения (метка для медиа);
- unread count;
- muted/pinned.

Не показывает: отправителя последнего сообщения, время, typing, draft, папки, архив.

## 4. Соответствие P0-плану

| Блок | Статус | Комментарий |
|---|---|---|
| Вход | ⚠️ частично | телефон/код/2FA/QR-запрос и logout есть; QR-рендер и logout не проверены live; прокси нет |
| Чаты | ⚠️ частично | 30 чатов, title/preview/unread/pin/mute/kind; нет папок/архива/пагинации/live |
| Сообщения | ⚠️ частично | 50 последних, текст, отправка; нет пагинации/reply/edit/delete/forward/read receipts |
| Медиа | ❌ | только текстовые заглушки, без файлов/фото/голоса |
| Поиск | ❌ | нет |
| Уведомления | ❌ | нет |
| Приватность | ❌ | passcode и шифрование DB отсутствуют |
| UX | ⚠️ частично | Simple, мышь, RU/EN, темы, скролл; нет онбординга, splash, date separators, группировки |
| Инфраструктура | ⚠️ частично | CI, кеш чатов, кроссплатформенная сборка; нет прокси, пакетов, reconnect |

## 5. Найденные проблемы

### P0 — блокирует ежедневное использование

1. **Не-auth updates выбрасываются.** `TgClient::next_auth` забирает `Update` из канала и отбрасывает всё, кроме `AuthorizationState`. Без переработки не будет новых сообщений, edits, deletes, read receipts, typing, изменения списка чатов.
2. **`mark_read` — no-op.** Чаты не помечаются прочитанными; read receipts не уходят; unread в списке не уменьшается.
3. **Нет пагинации истории.** Старше 50 сообщений чат выглядит обрезанным.
4. **Медиа и сервисные сообщения не отображаются.** Пользователь видит заглушки вместо фото/файлов/голосовых.
5. **TDLib DB не шифруется.** `database_encryption_key` пустой; passcode отсутствует.

### P1 — сильно ухудшает UX

1. `getChats` + 30 отдельных `getChat` — хрупкий и медленный первый запуск; `getChat` timeout 30s.
2. `history` использует sleep/retry вместо ожидания готовности TDLib.
3. `getUser`/`getChat` для sender names выполняются последовательно.
4. Нет `closeChat`; открытые чаты накапливаются.
5. Нет live-обновления списка чатов; preview/unread быстро устаревают.
6. Нет выбора/прокрутки к выбранному сообщению при keyboard-навигации.
7. Нет date separators, группировки сообщений и reply-контекста.
8. `sendMessage` optimistic-статус не отражает TDLib `sending_state`.
9. Нет понятной обработки flood wait/timeout в UI.
10. `chats.json` не инвалидируется при внешнем logout/смене аккаунта.

### P2 — полировка

- сырые TDLib errors показываются пользователю;
- `message_scroll` и выбранное сообщение живут раздельно;
- кеш чатов не содержит времени последнего сообщения;
- сервисные сообщения показываются как обычные баблы;
- no `NO_COLOR`/reduced motion для новых элементов;
- TDLib log не настраивается программно;
- нет тестов на реальные TDLib updates (только unit/demo).

## 6. Что уже отшлифовано в этом изменении

1. **Список чатов стал содержательным:** preview, unread, muted, pinned, kind; кеш сохраняет эти поля.
2. **Правильные kind-иконки:** private/group/channel/saved.
3. **Медиа-заглушки вместо пустых сообщений:** типы контента сохранены в `MessageKind`, UI показывает локализованную метку.
4. **Sender names** резолвятся через TDLib; `Them` больше не показывается для нормальных сообщений.
5. **Кеш чатов** не затирает реальные названия заглушками.
6. **Первые 50 сообщений** загружаются надёжнее: `openChat`, retry, fallback-страница, dedupe.
7. **Скролл** проверен на 50 сообщениях.

## 7. План следующей полировки

### Этап A — live-обновления и read receipts

- ввести `TgEvent`/`TgClient::next_event`, не выбрасывать non-auth updates;
- обрабатывать `updateNewMessage`, `updateMessageSendSucceeded/Failed`, `updateMessageContent/Edited/DeleteMessages`, `updateChatReadInbox/Outbox`, `updateChatPosition`, `updateNewChat/Title`;
- `mark_read` → `viewMessages` для последнего сообщения;
- обновлять unread/preview в списке без перезагрузки;
- тест: сообщение с телефона появляется в TUI; отправка с TUI обновляет список.

### Этап B — полная история и медиа

- `history_before(chat_id, from_id, limit)` + загрузка при скролле вверх;
- сохранять позицию скролла при prepend;
- медиа-строки: фото/видео/файл/голос/стикер, размеры и file id;
- reply/forwarded/альбомы;
- даты и группировка.

### Этап C — приватность и надёжность

- passcode + `database_encryption_key`;
- outbox на диск;
- обработка `flood wait` и reconnect;
- прокси (MTProto/SOCKS5);
- splash на первом входе (уже в `TODO.md`).

## 8. Критерии готовности текущего среза

- [x] `cargo test --all-targets` — 22 passed;
- [x] `cargo clippy --all-targets -- -D warnings` — чисто;
- [x] `cargo clippy --features tdlib --all-targets -- -D warnings` — чисто;
- [x] в чате видно 50 сообщений, sender names, скролл;
- [x] список чатов показывает preview/unread/pin/mute/kind;
- [x] кеш чатов переживает рестарт;
- [ ] live-обновления;
- [ ] read receipts;
- [ ] пагинация;
- [ ] медиа;
- [ ] passcode/encryption.

## 9. Вывод

Текущий срез можно показывать как **демо**: «вход, список чатов, история, отправка текста».  
Как ежедневный клиент он пока не готов: нет live-обновлений, read receipts, полной истории, медиа и шифрования.

Следующий инженерный приоритет — не косметика, а **event pipeline**: без него любые улучшения списка чатов и сообщений останутся снимком на момент открытия.
