//! Every limit the canister enforces, and the checks on what callers send.

use crate::api::{Character, ChatError};

pub const MAX_REQUEST_ID_BYTES: usize = 64;
/// A user message, in UTF-8 bytes. The LLM canister accepts 10 KiB per request, system prompt and history included.
pub const MAX_MESSAGE_BYTES: usize = 4_000;
/// A reply is cut to this before it is saved (the LLM canister stops at 1000 tokens, well below it).
pub const MAX_REPLY_BYTES: usize = 16_000;
/// Saved turns per conversation (25 exchanges). Older ones are dropped.
pub const MAX_TURNS: usize = 50;

pub const MAX_CHARACTER_NAME_BYTES: usize = 64;
pub const MAX_SYSTEM_PROMPT_BYTES: usize = 2_000;
pub const MAX_CHARACTER_LINES: usize = 8;
pub const MAX_BIO_LINE_BYTES: usize = 200;
pub const MAX_STYLE_LINE_BYTES: usize = 100;
/// The system message of a model request: name, system prompt, bio and style together, as `llm::system_text`
/// composes them. Each field may be at its own limit only as long as the whole fits; `update_character` refuses a
/// character that does not, so none of it is cut from the prompt.
pub const MAX_SYSTEM_MESSAGE_BYTES: usize = 3_000;

/// What an entry and a turn are counted as on top of their text, generously above their encoded size.
pub const ENTRY_OVERHEAD_BYTES: usize = 96;
pub const TURN_OVERHEAD_BYTES: usize = 32;
/// The most one exchange can add to the store, a new entry included.
pub const MAX_EXCHANGE_BYTES: usize =
    ENTRY_OVERHEAD_BYTES + 2 * TURN_OVERHEAD_BYTES + MAX_REQUEST_ID_BYTES + MAX_MESSAGE_BYTES + MAX_REPLY_BYTES;
/// The most a conversation written by this version can hold.
pub const MAX_CONVERSATION_BYTES: usize = ENTRY_OVERHEAD_BYTES + (MAX_TURNS / 2) * MAX_EXCHANGE_BYTES;

/// Counted bytes of all conversations plus what the chats in flight may still add never exceed this: when a new
/// chat would, the least recently used conversations are removed first, and if that is not enough the chat is
/// refused.
#[cfg(not(feature = "small-limits"))]
pub const MAX_STORE_BYTES: usize = 32 * 1024 * 1024;
/// Entries (conversations, including cleared ones that only keep a revision).
#[cfg(not(feature = "small-limits"))]
pub const MAX_ENTRIES: usize = 100_000;
/// Chats being answered at the same time, over all callers.
#[cfg(not(feature = "small-limits"))]
pub const MAX_CONCURRENT_CHATS: usize = 20;
/// Calls to the model started within any 60 seconds, over all callers.
#[cfg(not(feature = "small-limits"))]
pub const MAX_MODEL_CALLS_PER_MINUTE: usize = 60;

// Tiny limits so the PocketIC tests can reach them (`--features small-limits`; never deployed).
#[cfg(feature = "small-limits")]
pub const MAX_STORE_BYTES: usize = 2 * 1024 * 1024;
#[cfg(feature = "small-limits")]
pub const MAX_ENTRIES: usize = 8;
#[cfg(feature = "small-limits")]
pub const MAX_CONCURRENT_CHATS: usize = 2;
#[cfg(feature = "small-limits")]
pub const MAX_MODEL_CALLS_PER_MINUTE: usize = 5;

/// Eviction frees down to this share of the limit that was hit, so it runs once per tenth of the budget.
pub const EVICTION_TARGET_PERCENT: usize = 90;

// Making room for a chat can always succeed: the entries that may not be removed (the caller's and those with a chat
// in flight) plus what the chats in flight may add fit below the eviction target. This holds for conversations of
// at most MAX_CONVERSATION_BYTES, which this version writes and to which state from 0.6.0 is cut when it is loaded.
const _: () = assert!(
    (MAX_CONCURRENT_CHATS + 1) * MAX_CONVERSATION_BYTES + (MAX_CONCURRENT_CHATS + 1) * MAX_EXCHANGE_BYTES
        < MAX_STORE_BYTES / 100 * EVICTION_TARGET_PERCENT
);
const _: () = assert!(MAX_CONCURRENT_CHATS + 1 < MAX_ENTRIES * EVICTION_TARGET_PERCENT / 100);

/// The saved state (one candid blob in stable memory) is refused above this when it is WRITTEN (in `pre_upgrade`).
/// The store budget keeps it far below.
pub const MAX_WRITTEN_STATE_BYTES: usize = 64 * 1024 * 1024;
/// ...and above this when it is READ (in `post_upgrade`): the limit of 0.6.0, which wrote its state without a budget
/// and read back up to this much itself. Anything it could upgrade from is read, then cut to this version's limits.
pub const MAX_READ_STATE_BYTES: usize = 100_000_000;
const _: () = assert!(MAX_WRITTEN_STATE_BYTES <= MAX_READ_STATE_BYTES);

/// The model is given at most this long (the longest bounded-wait call the IC offers).
pub const MODEL_CALL_TIMEOUT_SECS: u32 = 300;
/// A chat older than this is over: its reply is not saved, it no longer counts as pending, and the caller's next
/// request may take its place. Not shorter than the model call's timeout.
pub const STALE_FLIGHT_NS: u64 = 5 * 60 * 1_000_000_000;
const _: () = assert!(STALE_FLIGHT_NS >= MODEL_CALL_TIMEOUT_SECS as u64 * 1_000_000_000);

pub const RATE_WINDOW_NS: u64 = 60 * 1_000_000_000;

/// Largest candid argument accepted at ingress for each kind of method (see `accepts_ingress`).
/// `send_message`'s is about four times what a valid new message needs, because a request saved by 0.6.0 (which had
/// no length limit) replays only through `send_message`, with its message as saved. A message saved before the
/// upgrade that is longer than this (with its request id) cannot be replayed through `send_message` any more;
/// `get_message_status` still reports it.
pub const MAX_SEND_ARG_BYTES: usize = 16 * 1024;
pub const MAX_ADMIN_ARG_BYTES: usize = 16 * 1024;
pub const MAX_SMALL_ARG_BYTES: usize = 256;

/// Whether `inspect_message` lets an ingress call through: first who may call the method at all, then how big its
/// argument may be. `arg_bytes` is only asked once caller and method pass, so a call refused for either is not
/// copied to find its size.
pub fn accepts_ingress(method: &str, signed_in: bool, controller: bool, arg_bytes: impl FnOnce() -> usize) -> bool {
    let max_bytes = match method {
        "send_message" if signed_in => MAX_SEND_ARG_BYTES,
        // Also the queries a client may want answered by the whole subnet rather than one replica.
        "clear_conversation" | "get_conversation" | "get_message_status" if signed_in => MAX_SMALL_ARG_BYTES,
        "update_character" | "set_llm_provider" if controller => MAX_ADMIN_ARG_BYTES,
        #[cfg(feature = "test-hooks")]
        "test_fill" if controller => return true,
        // Everything else is a query (call it as one), a method that does not exist, or a caller who may not.
        _ => return false,
    };
    arg_bytes() <= max_bytes
}

pub fn validate_request_id(request_id: &str) -> Result<(), ChatError> {
    let valid = !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_BYTES
        && request_id.bytes().all(|b| b.is_ascii_graphic());
    if valid {
        Ok(())
    } else {
        Err(ChatError::Invalid("Invalid request id.".to_string()))
    }
}

pub fn validate_message(content: &str) -> Result<(), ChatError> {
    if content.len() > MAX_MESSAGE_BYTES {
        return Err(ChatError::Invalid(format!(
            "Message is too long ({} bytes; the limit is {}).",
            content.len(),
            MAX_MESSAGE_BYTES
        )));
    }
    if content.trim().is_empty() {
        return Err(ChatError::Invalid("Message is empty.".to_string()));
    }
    Ok(())
}

fn check_text(what: &str, text: &str, max_bytes: usize, multiline: bool) -> Result<(), String> {
    if text.len() > max_bytes {
        return Err(format!("{what} is too long ({} bytes; the limit is {max_bytes})", text.len()));
    }
    if text.chars().any(|c| c.is_control() && !(multiline && c == '\n')) {
        return Err(format!("{what} contains control characters"));
    }
    Ok(())
}

pub fn validate_character(character: &Character) -> Result<(), String> {
    if character.name.trim().is_empty() {
        return Err("name is empty".to_string());
    }
    if character.system_prompt.trim().is_empty() {
        return Err("system_prompt is empty".to_string());
    }
    check_text("name", &character.name, MAX_CHARACTER_NAME_BYTES, false)?;
    check_text("system_prompt", &character.system_prompt, MAX_SYSTEM_PROMPT_BYTES, true)?;
    for (what, lines, max_bytes) in
        [("bio", &character.bio, MAX_BIO_LINE_BYTES), ("style", &character.style, MAX_STYLE_LINE_BYTES)]
    {
        if lines.len() > MAX_CHARACTER_LINES {
            return Err(format!("{what} has {} lines; the limit is {MAX_CHARACTER_LINES}", lines.len()));
        }
        for line in lines {
            check_text(&format!("a {what} line"), line, max_bytes, false)?;
        }
    }
    let system = crate::llm::whole_system_text(character).len();
    if system > MAX_SYSTEM_MESSAGE_BYTES {
        return Err(format!(
            "name, system_prompt, bio and style together make a system message that is too long ({system} bytes; \
             the limit is {MAX_SYSTEM_MESSAGE_BYTES})"
        ));
    }
    Ok(())
}

/// The longest prefix of `text` that is at most `max_bytes` long and ends on a character boundary.
pub fn prefix(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_short_printable_ascii() {
        assert!(validate_request_id("0f8c2c1e-4b7a-4d2e-9a51-8f2b7c6d5e4a").is_ok());
        assert!(validate_request_id(&"x".repeat(64)).is_ok());
        for bad in ["", "has space", "tab\t", "ü", "\u{7f}"] {
            assert!(validate_request_id(bad).is_err(), "{bad:?}");
        }
        assert!(validate_request_id(&"x".repeat(65)).is_err());
    }

    #[test]
    fn messages_are_bounded_and_not_blank() {
        assert!(validate_message("hi").is_ok());
        assert!(validate_message(&"x".repeat(MAX_MESSAGE_BYTES)).is_ok());
        assert!(validate_message(&"x".repeat(MAX_MESSAGE_BYTES + 1)).is_err());
        // Bytes, not characters: 1334 three-byte characters are 4002 bytes.
        assert!(validate_message(&"あ".repeat(1333)).is_ok());
        assert!(validate_message(&"あ".repeat(1334)).is_err());
        for blank in ["", " ", "\n\t "] {
            assert!(validate_message(blank).is_err(), "{blank:?}");
        }
    }

    fn character() -> Character {
        Character {
            name: "Coo".into(),
            system_prompt: "You are Coo.\nBe brief.".into(),
            bio: vec!["An agent".into()],
            style: vec!["Friendly".into()],
        }
    }

    #[test]
    fn characters_are_bounded() {
        assert!(validate_character(&character()).is_ok());
        assert!(validate_character(&crate::state::default_character()).is_ok());

        type Edit = fn(&mut Character);
        let edits: [(&str, Edit); 9] = [
            ("empty name", |c| c.name = " ".into()),
            ("long name", |c| c.name = "n".repeat(MAX_CHARACTER_NAME_BYTES + 1)),
            ("newline in name", |c| c.name = "Co\no".into()),
            ("empty prompt", |c| c.system_prompt = "\n".into()),
            ("long prompt", |c| c.system_prompt = "p".repeat(MAX_SYSTEM_PROMPT_BYTES + 1)),
            ("control character in prompt", |c| c.system_prompt = "a\u{0}b".into()),
            ("too many bio lines", |c| c.bio = vec!["b".into(); MAX_CHARACTER_LINES + 1]),
            ("long bio line", |c| c.bio = vec!["b".repeat(MAX_BIO_LINE_BYTES + 1)]),
            ("long style line", |c| c.style = vec!["s".repeat(MAX_STYLE_LINE_BYTES + 1)]),
        ];
        for (what, edit) in edits {
            let mut c = character();
            edit(&mut c);
            assert!(validate_character(&c).is_err(), "{what}");
        }
    }

    #[test]
    fn a_character_must_fit_the_system_message_as_a_whole() {
        // Every field at its own limit: each is accepted alone, together they are refused.
        let largest = Character {
            name: "n".repeat(MAX_CHARACTER_NAME_BYTES),
            system_prompt: "p".repeat(MAX_SYSTEM_PROMPT_BYTES),
            bio: vec!["b".repeat(MAX_BIO_LINE_BYTES); MAX_CHARACTER_LINES],
            style: vec!["s".repeat(MAX_STYLE_LINE_BYTES); MAX_CHARACTER_LINES],
        };
        let refused = validate_character(&largest).unwrap_err();
        assert!(refused.contains("together make a system message that is too long"), "{refused}");

        // A character whose system message is exactly the limit is accepted, and all of it reaches the model.
        let mut fits = Character {
            name: "Coo".into(),
            system_prompt: "p".into(),
            bio: vec!["b".repeat(MAX_BIO_LINE_BYTES); 4],
            style: vec!["s".repeat(MAX_STYLE_LINE_BYTES - 1) + "!"; 4],
        };
        let rest = crate::llm::whole_system_text(&fits).len() - 1;
        fits.system_prompt = "p".repeat(MAX_SYSTEM_MESSAGE_BYTES - rest);
        assert!(fits.system_prompt.len() <= MAX_SYSTEM_PROMPT_BYTES);
        assert_eq!(crate::llm::whole_system_text(&fits).len(), MAX_SYSTEM_MESSAGE_BYTES);
        assert_eq!(validate_character(&fits), Ok(()));
        let sent = crate::llm::system_text(&fits);
        assert_eq!(sent, crate::llm::whole_system_text(&fits), "nothing is cut");
        assert!(sent.ends_with(&fits.style[3]) && sent.contains("Your name is Coo"));
        // One byte more is refused.
        fits.system_prompt.push('p');
        assert!(validate_character(&fits).is_err());
    }

    #[test]
    fn ingress_is_decided_on_caller_and_method_before_the_argument_is_read() {
        let unread = || -> usize { panic!("the argument was read for a call refused anyway") };
        // (method, signed in, controller): refused whatever the argument.
        for (method, signed_in, controller) in [
            ("send_message", false, false),
            ("clear_conversation", false, false),
            ("get_conversation", false, false),
            ("update_character", true, false),
            ("set_llm_provider", true, false),
            ("health", true, true),
            ("get_metrics", true, true),
            ("chat", true, true),
            ("test_fill", true, false),
        ] {
            assert!(!accepts_ingress(method, signed_in, controller, unread), "{method}");
        }
        // Then the size, for the calls that may pass.
        for (method, signed_in, controller, max) in [
            ("send_message", true, false, MAX_SEND_ARG_BYTES),
            ("get_message_status", true, false, MAX_SMALL_ARG_BYTES),
            ("update_character", true, true, MAX_ADMIN_ARG_BYTES),
        ] {
            assert!(accepts_ingress(method, signed_in, controller, || max), "{method}");
            assert!(!accepts_ingress(method, signed_in, controller, || max + 1), "{method}");
        }
    }

    #[test]
    fn prefix_respects_character_boundaries() {
        assert_eq!(prefix("hello", 10), "hello");
        assert_eq!(prefix("hello", 3), "hel");
        assert_eq!(prefix("あいう", 4), "あ");
        assert_eq!(prefix("あいう", 2), "");
    }
}
