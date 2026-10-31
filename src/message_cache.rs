use std::collections::HashMap;

use crate::backend::{ChatId, Message, MsgId};

pub type MessageCache = HashMap<ChatId, Vec<Message>>;

/// Appends a local pending message to its chat.
pub fn push_pending(cache: &mut MessageCache, message: Message) {
    cache.entry(message.chat_id).or_default().push(message);
}

/// Replaces the local pending message with the version returned by the server.
///
/// History loading can replace the cache while a send is in flight, so when the
/// pending entry is gone the sent message is still kept: an already loaded copy
/// is refreshed, otherwise the message is appended.
pub fn replace_pending(
    cache: &mut MessageCache,
    chat_id: ChatId,
    temp_id: MsgId,
    sent: Message,
) -> bool {
    let messages = cache.entry(chat_id).or_default();
    if let Some(index) = pending_index(messages, temp_id) {
        messages[index] = sent;
        return true;
    }
    if let Some(index) = messages.iter().position(|message| message.id == sent.id) {
        messages[index] = sent;
    } else {
        messages.push(sent);
    }
    false
}

/// Removes the local pending message after a failed send and returns it so the
/// UI can put the text back into the composer.
pub fn remove_pending(
    cache: &mut MessageCache,
    chat_id: ChatId,
    temp_id: MsgId,
) -> Option<Message> {
    let messages = cache.get_mut(&chat_id)?;
    let index = pending_index(messages, temp_id)?;
    Some(messages.remove(index))
}

fn pending_index(messages: &[Message], temp_id: MsgId) -> Option<usize> {
    messages
        .iter()
        .position(|message| message.pending && message.id == temp_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: MsgId, text: &str, pending: bool) -> Message {
        Message {
            id,
            chat_id: 7,
            sender: "You".into(),
            text: text.into(),
            outgoing: true,
            time: "12:00".into(),
            pending,
            edited: false,
        }
    }

    #[test]
    fn pending_message_is_appended_to_the_chat() {
        let mut cache = MessageCache::new();
        push_pending(&mut cache, message(1_000, "old", false));
        push_pending(&mut cache, message(-1, "hello", true));

        let messages = &cache[&7];
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].text, "hello");
        assert!(messages[1].pending);
    }

    #[test]
    fn successful_send_replaces_pending_in_place() {
        let mut cache = MessageCache::new();
        push_pending(&mut cache, message(-1, "hello", true));

        let replaced = replace_pending(&mut cache, 7, -1, message(42, "hello", false));

        assert!(replaced);
        assert_eq!(cache[&7].len(), 1);
        assert_eq!(cache[&7][0].id, 42);
        assert!(!cache[&7][0].pending);
    }

    #[test]
    fn successful_send_appends_when_pending_was_reloaded_away() {
        let mut cache = MessageCache::new();
        push_pending(&mut cache, message(1_000, "old", false));

        let replaced = replace_pending(&mut cache, 7, -1, message(42, "hello", false));

        assert!(!replaced);
        assert_eq!(cache[&7].len(), 2);
        assert_eq!(cache[&7][1].id, 42);
    }

    #[test]
    fn successful_send_does_not_duplicate_an_already_loaded_message() {
        let mut cache = MessageCache::new();
        push_pending(&mut cache, message(42, "hello", false));

        let replaced = replace_pending(&mut cache, 7, -1, message(42, "hello!", false));

        assert!(!replaced);
        assert_eq!(cache[&7].len(), 1);
        assert_eq!(cache[&7][0].text, "hello!");
    }

    #[test]
    fn failed_send_removes_pending_and_returns_its_text() {
        let mut cache = MessageCache::new();
        push_pending(&mut cache, message(1_000, "old", false));
        push_pending(&mut cache, message(-1, "hello", true));

        let removed = remove_pending(&mut cache, 7, -1);

        assert_eq!(removed.map(|message| message.text), Some("hello".into()));
        assert_eq!(cache[&7].len(), 1);
        assert_eq!(cache[&7][0].text, "old");
        assert!(remove_pending(&mut cache, 7, -1).is_none());
    }
}
