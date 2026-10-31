# Gramterm — план разработки

**Рабочее название:** Gramterm
**Дата:** 31.10.2026
**Статус:** draft for approval
**Связанный документ:** [tuigram-feature-audit.md](tuigram-feature-audit.md)

---

## 0. Название

### Шортлист

| Название | crates.io | Плюсы | Минусы |
|---|---|---|---|
| **Gramterm** | свободно | Понятно: Telegram + terminal. Легко искать, легко печатать, нет прямого конфликта | Суховато, звучит как «grammar term» |
| **Gramline** | свободно | Мягче, брендовее, «line» = terminal/command line | Есть компания GramLine; слово «line» слишком общее |
| **Telui** | свободно | Прямо говорит «Telegram UI» | Уже используется в docs проекта trusty-mpm; выглядит как «tel-ui» |
| **Strizh** | свободно | Уникально, быстрый стриж, красивый логотип | Поиск забит военной техникой; непонятно иностранцам |
| **Aerogram** | свободно | Красиво, «лёгкое письмо» | Уже есть чат-приложение Aerogram |
| **Siskin** | свободно | Милая птица, социальная | Есть Siskin IM — XMPP-мессенджер |
| **Pipit** | занято | — | Крейт занят аудиокодеком |
| **TTYgram** | занято | — | Уже есть Telegram-бот ttygram |

### Рекомендация

**Gramterm** — рабочее имя.

- `gramterm` свободен на crates.io;
- сразу понятно, что это Telegram-клиент для терминала;
- хорошо выглядит как имя бинарника: `gramterm`;
- не привязано к vim или к конкретному TUI-фреймворку;
- легко переименовать до первого релиза, если передумаем.

**Tagline:** *Telegram в терминале — быстро и по-человечески.*

**Позиционирование:** не «ещё один vim-клиент», а **Telegram TUI для людей**, которые живут в терминале, но не хотят учить `hjkl` ради переписки.

---

## 1. Цели и не-цели

### Цели v1

1. **Быстрый и приятный текстовый мессенджер** для Windows Terminal, Linux и macOS.
2. **Низкий порог входа:** Simple-режим по умолчанию, мышь, русский язык, понятный онбординг.
3. **Vim-режим** для опытных — как альтернатива, не как обязательный.
4. **Приватность по умолчанию:** скрытый текст уведомлений, passcode, шифрование локальной БД.
5. **Надёжность:** черновики, retry, восстановление после reconnect, без потери сообщений.
6. **Полноценный текст + базовые медиа:** фото, файлы, голосовые на прослушивание, стикеры.
7. **Один бинарник**, без телеметрии и промежуточных серверов.

### Не-цели v1

- звонки;
- stories;
- комментарии под постами;
- платежи, mini apps, игры;
- полноценное администрирование групп/каналов;
- собственный MTProto с нуля;
- мобильные платформы;
- плагины и скриптовый API.

Это осознанный выбор: сначала UX, потом глубина.

---

## 2. Ключевые решения

| Решение | Выбор | Почему |
|---|---|---|
| Backend | **TDLib через `tdlib-rs`** | Быстрее всего до полного клиента: локальная БД, файлы, updates, секретные чаты, звонки-сигналинг |
| Свой MTProto | **не в v1** | Месяцы работы; TDLib закрывает 95% бэкенда |
| UI | **ratatui 0.30 + crossterm 0.29** | Зрелый стек, мышь, image-протоколы, тесты через `TestBackend` |
| Async | **tokio** | TDLib-adapter, команды, таймеры |
| Язык | **Rust 1.88+**, edition 2024 | Требование ratatui 0.30 |
| Платформа v1 | **Windows-first**, Linux/macOS поддерживаются | Основной пользователь — Windows Terminal + WSL |
| Режимы | **Simple (default) + Vim (opt-in)** | Массовый пользователь не должен учить modal editing |
| Мышь | **полная поддержка** | Дифференциатор против tuigram |
| Языки | **RU + EN** | Русскоязычная аудитория — сильный дифференциатор |
| Приватность | **passcode + ключ TDLib + скрытие текста уведомлений** | tuigram этого не даёт |
| Аккаунты | **multi-account в v1.1** | Сильный дифференциатор, но не блокирует v1 |
| Лицензия | **MIT** | Совместимо с ratatui/crossterm/tdlib-rs; проще для контрибьюторов |
| Телеметрия | **нет** | Принцип проекта |

### Почему TDLib, а не свой MTProto

- `tdlib-rs` 1.4.0 активно поддерживается и умеет скачивать готовый `tdjson` под Windows/Linux/macOS.
- TDLib уже умеет: диалоги, историю, файлы, updates, секретные чаты, папки, топики, реакции, опросы, ботов.
- Свой MTProto — отдельный проект на 2–4 месяца; для UX-дифференциации это плохой старт.
- `Backend` trait остаётся: если позже захотим свой MTProto, UI не переписывается.

---

## 3. Архитектура

### 3.1 Схема

```
┌──────────────────────────────────────────────┐
│ UI: ratatui, main thread                     │
│ App state · update() · draw() · hitboxes     │
└──────────▲──────────────────────────┬────────┘
           │ AppEvent                 │ Cmd
           │                          ▼
┌──────────┴──────────────────────────┐
│ App core / Backend trait            │
│ domain types, commands, events      │
└──────────▲──────────────────────────┘
           │ TgEvent
┌──────────┴──────────────────────────┐
│ TDLib adapter (tdlib-rs)            │
│ receive thread · auth · mapping     │
└──────────▲──────────────────────────┘
           │ JSON / FFI
┌──────────┴──────────────────────────┐
│ libtdjson (prebuilt / static)       │
└─────────────────────────────────────┘
```

### 3.2 Модули

```
src/
  main.rs              — запуск, terminal init/restore, panic hook
  app/                 — состояние, update(), команды, режимы
  ui/                  — layout, widgets, theme, hitboxes, popups
  tg/                  — Backend trait, TDLib adapter, domain mapping
  input/               — keymap, Simple/Vim, mouse
  i18n/                — строки RU/EN
  config/              — settings.toml, env, keyring
  platform/            — clipboard, notifications, open, paths
  demo/                — MockBackend и фейковые данные
tests/                 — интеграционные тесты, snapshots
locales/               — en.ftl, ru.ftl
```

Начать можно одним крейтом; если TDLib-зависимость начнёт мешать тестам — вынести `tg` в отдельный `gramterm-td`.

### 3.3 Потоки

- **Main thread:** terminal, event loop, draw.
- **TDLib receive thread:** блокирующий `receive()`, отправляет `TgEvent` в канал.
- **Tokio runtime:** команды, запросы, таймеры, загрузки.
- **Один writer состояния:** только `App`; ничего не мутирует состояние из других потоков.
- **Все ответы тегируются** `request_id`/`generation`, чтобы отбрасывать устаревшие.

### 3.4 Event loop

```rust
loop {
    tokio::select! {
        Some(ev) = term_events.next() => app.on_event(ev),
        Some(ev) = tg_rx.recv()       => app.on_tg(ev),
        _ = tick.tick()               => app.on_tick(),
    }
    if app.dirty {
        terminal.draw(|f| ui::draw(f, &mut app))?;
        app.dirty = false;
    }
}
```

### 3.5 Backend trait

```rust
#[async_trait]
pub trait Backend: Send + Sync {
    async fn login_qr(&self) -> Result<LoginToken>;
    async fn login_phone(&self, phone: &str) -> Result<LoginToken>;
    async fn submit_code(&self, code: &str) -> Result<AuthState>;
    async fn submit_password(&self, password: &str) -> Result<AuthState>;

    async fn chats(&self) -> Result<Vec<Chat>>;
    async fn history(&self, chat: ChatId, before: Option<MsgId>, limit: usize) -> Result<Page>;
    async fn send_text(&self, chat: ChatId, text: &str, reply: Option<MsgId>) -> Result<MsgId>;
    async fn edit(&self, chat: ChatId, msg: MsgId, text: &str) -> Result<()>;
    async fn delete(&self, chat: ChatId, msg: MsgId) -> Result<()>;
    async fn forward(&self, from: ChatId, msg: MsgId, to: ChatId) -> Result<MsgId>;
    async fn mark_read(&self, chat: ChatId, up_to: MsgId) -> Result<()>;

    fn events(&self) -> broadcast::Receiver<TgEvent>;
}
```

Доменные типы (`ChatId`, `MsgId`, `Chat`, `Message`, `Entity`, `Media`) не должны протекать из TDLib в UI.

---

## 4. Scope

### P0 — v1.0

| Блок | Функции |
|---|---|
| Вход | QR, телефон + код, 2FA, сессия, logout |
| Чаты | список, папки, архив, unread, pin, mute, поиск по названию |
| Сообщения | текст, Markdown, reply, edit, delete, forward, albums, drafts, retry, read receipts |
| Медиа | фото, файлы, голосовые на прослушивание, стикеры, просмотр фото |
| Поиск | в чате: `from:`, `has:`, `before:`, `after:`; переходы `n/N` |
| Уведомления | OSC 9/777/99 + bell, mute, звук, скрытие текста |
| Приватность | passcode, шифрование TDLib DB, focus-aware read receipts |
| UX | Simple + Vim, мышь, RU/EN, онбординг, `?`, command palette |
| Инфраструктура | прокси, темы, настройки, логи, CI, Windows zip |

### P1 — v1.1

- мультиаккаунт;
- глобальный поиск по всем чатам;
- batch-действия;
- ручные read/unread;
- mute на время;
- запись голосовых;
- отправка видео как видео;
- GIF-поиск;
- профиль и контакты;
- кастомные клавиши;
- native notifications;
- управление кэшем;
- high-contrast тема.

### P2 — после v1.1

- управление папками и топиками;
- создание групп/каналов, админка;
- комментарии;
- stories;
- звонки;
- перевод;
- отложенные и тихие сообщения;
- inline-боты;
- платежи и mini apps.

---

## 5. UX-контракт

### 5.1 Раскладка

```
┌ Chats ─────────────┬─ Rust Community ─────────────────┐
│ # rust             │ 10:42 Alice: привет               │
│ @ news             │ 10:43 Bob: как дела?              │
│ …                  │                                   │
├────────────────────┴───────────────────────────────────┤
│ > сообщение…                                  [Enter]  │
└────────────────────────────────────────────────────────┘
```

- Слева — чаты; справа — история; снизу — композер; сверху/снизу — статус.
- Попапы: help, command palette, поиск, инфо, реакции, вложения.
- В Simple-режиме всё кликабельно; в Vim-режиме — те же действия с клавиш.

### 5.2 Режимы

| Режим | Для кого | Поведение |
|---|---|---|
| **Simple** (default) | Обычный пользователь | Стрелки/Tab/Enter, мышь, `/` поиск, `:` команды, `?` помощь |
| **Vim** (opt-in) | Опытный пользователь | `hjkl`, `gg/G`, `Ctrl-d/u`, `i`/`Esc`, `:` команды |

Оба режима используют одни и те же команды; отличается только маппинг клавиш.

### 5.3 Мышь

- клик по чату — открыть;
- клик по сообщению — выбрать; double-click — открыть медиа/выделить слово;
- колесо — скролл списка/истории/попапа;
- клик по композеру — поставить курсор;
- drag разделителя — изменить ширину панелей;
- right-click — контекстное меню;
- `F2` — отпустить мышь для нативного выделения текста терминалом;
- подсказка «Shift+drag — выделение» в статус-баре.

### 5.4 Приватность

- Текст уведомлений **скрыт по умолчанию**: «New message».
- Опция «Показывать текст» — явная.
- Read receipts уходят только когда окно и чат в фокусе.
- Online/typing — только при активности.
- Passcode: Argon2id + ключ TDLib; при старте — экран разблокировки.
- Session/DB не хранятся в git и не логируются.

### 5.5 Онбординг

1. Выбор языка (RU/EN).
2. Ввод/подтверждение API ID и API Hash (или env).
3. QR или телефон.
4. Код, 2FA.
5. Короткий тур: «`?` — помощь, `:` — команды, мышь работает».
6. Демо-режим `--demo` для знакомства без аккаунта.

---

## 6. Этапы

| Этап | Срок | Задачи | Критерий готовности |
|---|---|---|---|
| **0. Foundation** | 1 неделя | Репозиторий, MIT, CI, каркас TUI, темы, i18n, MockBackend, demo | `cargo run --demo` показывает чаты; тесты и clippy зелёные |
| **1. TDLib bridge + auth** | 2 недели | `tdlib-rs`, receive loop, login QR/phone/2FA, сессия, logout | Вход на Windows и Linux; сессия переживает рестарт |
| **2. Chats + messages** | 2 недели | Список чатов, папки, архив, история, отправка, reply/edit/delete/forward, drafts, retry | Можно переписываться текстом в TUI |
| **3. Media + search** | 1–2 недели | Фото, файлы, голосовые, стикеры, просмотр, in-chat поиск | Фото и файлы открываются; поиск с фильтрами работает |
| **4. UX-дифференциаторы** | 2 недели | Simple/Vim, мышь, RU/EN, passcode, notification privacy, онбординг | Пользователь без vim проходит 10 базовых задач без документации |
| **5. Reliability + release** | 1–2 недели | reconnect, flood wait, outbox, soak, Windows zip, Linux tarball, docs | 24–72 ч soak; установка и запуск на чистой машине |
| **6. v1.1** | 2–3 недели | multi-account, global search, batch, voice recording, native notifications | v1.1-фичи по списку P1 |

**Итого до v1.0:** 8–10 недель плотной работы. Буфер уже включён в этапы 5–6.

---

## 7. Первый спринт

Цель: через 5–7 дней иметь запускаемый каркас, в который можно вставлять TDLib.

1. `cargo init`, workspace/модули, `.gitignore`, MIT, README.
2. `rustfmt`, `clippy`, CI (Linux + Windows + macOS).
3. Доменные типы: `ChatId`, `MsgId`, `Chat`, `Message`, `Entity`, `Media`.
4. `Backend` trait + `MockBackend` + demo-данные.
5. TUI shell: layout, header, chat list, history, composer, status bar.
6. Тема: палитра, rounded corners, truecolor + fallback.
7. i18n: `en.ftl`, `ru.ftl`, переключатель языка.
8. Simple-режим: стрелки/Tab/Enter, `?`, `:`, `/`.
9. Hitboxes + mouse: клик по чату, колесо, клик по композеру.
10. `--demo`: фейковые чаты, сообщения, реакции, фото-заглушка.

**Демо:** запускается в Windows Terminal через WSL, выглядит как клиент, реагирует на мышь и клавиатуру.

---

## 8. Тестирование

| Уровень | Что проверяем |
|---|---|
| Unit | доменные типы, парсинг Markdown, i18n, настройки, hit-testing |
| Snapshot | `TestBackend` + `insta`: layout, темы, попапы, ошибки |
| Mock integration | `MockBackend`: полный сценарий login → chat → send → update |
| TDLib integration | тестовый аккаунт, ручной чек-лист; не в CI |
| Soak | 24–72 ч, reconnect, flood wait, память |
| Security | passcode, логи, буфер обмена, опасные файлы/ссылки |
| Platform | Windows Terminal, WezTerm, Linux, macOS; WSL |

**Правило:** ни один PR не считается готовым без теста или ручного сценария.

---

## 9. CI/CD и поставка

### CI

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- сборка Linux/Windows/macOS
- кэш cargo и TDLib
- артефакты: `gramterm-linux-x86_64.tar.gz`, `gramterm-windows-x86_64.zip`, `gramterm-macos-*.tar.gz`

### Поставка

- **Windows:** zip + `gramterm.exe`; позже MSI/winget/scoop.
- **Linux:** tarball; позже AUR/Flatpak.
- **macOS:** tarball; позже Homebrew.
- **Обновления:** ручные в v1; автообновление позже.
- **Код-сигнинг:** позже, после стабилизации.

---

## 10. Риски

| Риск | Вероятность | Митигация |
|---|---|---|
| TDLib `tdlib-rs` не соберётся на Windows | средняя | `download-tdlib` + `static`; проверить в первую неделю |
| Смена ключа TDLib DB / passcode | средняя | Исследовать API; если смена не поддерживается — задокументировать сценарий |
| TDLib updates приходят пачками | высокая | Батчинг, bounded-канал, отбрасывание устаревших запросов |
| Уведомления в Windows Terminal | средняя | OSC 777 + `allowOSC777`; native toast позже |
| Картинки в Windows Terminal — halfblock | высокая | Честно сказать в docs; рекомендовать WezTerm |
| Русский ввод/IME | средняя | Проверить в первую неделю; bracketed paste |
| Размер TDLib и старт | средняя | Ленивая инициализация, кэш, прогресс на первом запуске |
| Scope creep | высокая | P0/P1/P2, v1 без stories/calls |
| Локализация | средняя | Строки только через i18n; plural rules |
| Аккаунт-риски | низкая | Тестовый аккаунт, flood wait, без автоматизации |

---

## 11. Метрики

| Метрика | Цель |
|---|---|
| Time to first message | < 2 минут |
| Task success без документации | ≥ 90% для 10 базовых задач |
| Input latency | < 50 мс |
| Startup | < 500 мс до интерактивного UI |
| Поиск сообщения в чате | < 10 секунд |
| Failed send recovery | 1 действие |
| Crash-free sessions | > 99% |
| Локализация | 100% строк RU/EN |
| Телеметрия | 0 |
| Soak | 72 ч без потери сообщений |

---

## 12. Открытые вопросы

1. **Название:** утверждаем Gramterm или берём Gramline?
2. **API-ключи:** пользователь вводит свои при первом запуске или release-сборка несёт встроенный ключ?
3. **v1.0 scope:** multi-account в v1.0 или v1.1?
4. **Windows-поставка:** zip или сразу MSI?
5. **Минимальная версия Windows:** Windows 10 1809+ или Windows 11 only?
6. **Стиль:** ближе к tuigram (плотный, тёмный, bubbles) или более минималистичный?
7. **Тестовый аккаунт:** есть ли номер, который не жалко?

---

## 13. Приложение: команды

```bash
cargo run -- --demo          # демо без аккаунта
cargo run                    # реальный клиент
cargo test                   # тесты
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Запуск в Windows Terminal через WSL:

```powershell
wt.exe -w new wsl.exe -d Ubuntu-26.04 -- bash -lc "cd /mnt/f/TDUI && cargo run"
```

---

## 14. Итог

**Gramterm** — это не «ещё один vim-клиент». Это Telegram TUI, который:

- выглядит и ведёт себя как современный клиент;
- не требует учить vim;
- работает мышью;
- говорит по-русски;
- не светит текстом уведомлений;
- закрывает сессию passcode;
- не теряет сообщения;
- остаётся быстрым.

Если этот документ утверждён, следующий шаг — **первый спринт**: каркас, MockBackend, demo, Simple-режим и мышь. TDLib подключаем вторым этапом, когда UI уже можно смотреть и трогать.
