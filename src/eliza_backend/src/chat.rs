//! Sending a message, asking for its status, and clearing the conversation.
//!
//! A message is sent as the next exchange after the revision the client last saw, under a request id. A (request
//! id, revision) pair is saved at most once: repeating it returns the saved reply, and once the conversation has
//! moved on the request is a `Conflict` for good. The steps are split in two synchronous halves around the model
//! call (`begin`, `finish`) that only touch the state passed in, so every outcome is covered by unit tests.

use crate::api::{ChatError, Conversation, LlmProvider, Message, MessageStatus, Sent};
use crate::flight::Verdict;
use crate::limits::{
    validate_message, validate_request_id, ENTRY_OVERHEAD_BYTES, MAX_CONCURRENT_CHATS, MAX_EXCHANGE_BYTES,
};
use crate::llm::{self, PromptMessage};
use crate::state::{self, State};
use candid::Principal;

pub enum Begun {
    /// The request was already answered: here is the saved reply. The model is not asked again.
    Replay(Sent),
    Run(Run),
}

/// A chat that is now in flight.
pub struct Run {
    pub token: u64,
    /// The conversation's revision, which the request continues.
    pub revision: u64,
    pub provider: LlmProvider,
    pub prompt: Vec<PromptMessage>,
}

/// Everything before the model is asked. The order of the checks is part of the protocol:
/// a repeated request is recognised before anything can refuse it; what is wrong with the request itself comes
/// before what is only wrong right now; and a request for an outdated revision is a `Conflict` even while the
/// canister is at a limit, so no room is made for a request that can never run.
pub fn begin(
    state: &mut State,
    caller: Principal,
    request_id: &str,
    base_revision: u64,
    content: &str,
    now: u64,
) -> Result<Begun, ChatError> {
    // Anonymous callers would all share one conversation.
    if caller == Principal::anonymous() {
        return Err(ChatError::Invalid("Sign in to chat.".to_string()));
    }
    validate_request_id(request_id)?;

    let State { store, flights, character, provider } = state;
    // Compared as saved, byte for byte, and before the length check: a message accepted by an older version still
    // replays.
    if let Some((sent, reply)) = store.saved_exchange(&caller, request_id) {
        if sent != content {
            return Err(ChatError::Invalid("This request id was used for another message.".to_string()));
        }
        return Ok(Begun::Replay(Sent { reply: reply.to_string(), revision: store.revision_of(&caller) }));
    }
    validate_message(content)?;

    // One chat per caller: InProgress for the same request id, else Busy.
    flights.check(&caller, request_id, now)?;

    let revision = store.revision_of(&caller);
    if base_revision != revision {
        return Err(ChatError::Conflict);
    }

    // Limits over all callers. Refusals are `Busy`: nothing was saved and the same request may be sent again.
    let live = flights.live(now);
    if live >= MAX_CONCURRENT_CHATS {
        return Err(ChatError::Busy);
    }
    let calls_model = *provider == LlmProvider::OnChain;
    if calls_model && !flights.model_call_allowed(now) {
        return Err(ChatError::Busy);
    }
    // Room for this exchange and for those of the chats already in flight, now, so that saving a reply never has
    // to make room (or fail for lack of it).
    let new_entry = !store.contains(&caller);
    let protected = |who: &Principal| *who == caller || flights.holds(who);
    if !store.make_room((live + 1) * MAX_EXCHANGE_BYTES, new_entry, protected) {
        return Err(ChatError::Busy);
    }

    // The entry exists from here on, at the revision that was just checked (making room may have raised the
    // floor), and is protected from eviction while the chat is in flight.
    store.pin(caller, revision, now);
    let token = flights.begin(caller, request_id, now, calls_model);
    let prompt = llm::build_prompt(character, store.turns(&caller), content);
    Ok(Begun::Run(Run { token, revision, provider: *provider, prompt }))
}

/// Everything after the model answered (or failed to).
pub fn finish(
    state: &mut State,
    caller: Principal,
    run: (u64, u64),
    request_id: String,
    content: String,
    answer: Result<String, String>,
    now: u64,
) -> Result<Sent, ChatError> {
    let (token, revision) = run;
    let reply = answer.map_err(ChatError::Failed)?;
    match state.flights.verdict(&caller, token, now) {
        Verdict::Current => {}
        Verdict::Cleared => return Err(ChatError::Cleared),
        Verdict::Over => return Err(ChatError::Failed("The reply took too long and was not saved.".to_string())),
    }
    // Only a clear changes the revision of a caller with a chat in flight, and that is the `Cleared` verdict.
    if state.store.revision_of(&caller) != revision {
        return Err(ChatError::Failed("The conversation changed while the reply was generated.".to_string()));
    }
    let revision = state.store.append_exchange(caller, content, reply.clone(), request_id, now);
    Ok(Sent { reply, revision })
}

/// Ends the caller's chat when `send_message` ends — also after a trap in the reply callback, where ic-cdk drops
/// the future during cleanup. So this only touches the state, never the system API.
struct FlightGuard {
    caller: Principal,
    token: u64,
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        state::try_mutate(|s| s.flights.end(&self.caller, self.token));
    }
}

pub async fn send_message(request_id: String, base_revision: u64, content: String) -> Result<Sent, ChatError> {
    let caller = ic_cdk::api::msg_caller();
    let begun = state::mutate(|s| begin(s, caller, &request_id, base_revision, &content, ic_cdk::api::time()))?;
    let run = match begun {
        Begun::Replay(sent) => return Ok(sent),
        Begun::Run(run) => run,
    };
    let _guard = FlightGuard { caller, token: run.token };

    let answer = llm::ask(run.provider, run.prompt).await;

    #[cfg(feature = "test-hooks")]
    if content == "trap after reply" {
        ic_cdk::trap("test: trap in the reply callback");
    }

    state::mutate(|s| finish(s, caller, (run.token, run.revision), request_id, content, answer, ic_cdk::api::time()))
}

pub fn status(state: &State, caller: &Principal, request_id: &str, now: u64) -> MessageStatus {
    if let Some((_, reply)) = state.store.saved_exchange(caller, request_id) {
        MessageStatus::Done(Sent { reply: reply.to_string(), revision: state.store.revision_of(caller) })
    } else if state.flights.pending(caller, request_id, now) {
        MessageStatus::Pending
    } else {
        MessageStatus::Unknown
    }
}

pub fn conversation(state: &State, caller: &Principal) -> Conversation {
    let messages = state
        .store
        .turns(caller)
        .iter()
        .map(|t| Message {
            role: t.role.as_str().to_string(),
            content: t.content.clone(),
            request_id: t.request_id.clone(),
        })
        .collect();
    Conversation { messages, revision: state.store.revision_of(caller) }
}

/// Empties the caller's conversation, moves its revision on, and cancels saving a reply that is still being
/// generated. Returns the new revision. Fails only when the caller has no entry and none can be freed for it.
pub fn clear(state: &mut State, caller: Principal, now: u64) -> Result<u64, String> {
    let State { store, flights, .. } = state;
    if !store.contains(&caller) {
        let reserved = ENTRY_OVERHEAD_BYTES + flights.live(now) * MAX_EXCHANGE_BYTES;
        if !store.make_room(reserved, true, |who| *who == caller || flights.holds(who)) {
            return Err("Coo is at capacity. Try again later.".to_string());
        }
    }
    flights.mark_cleared(&caller);
    Ok(store.clear(caller, now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{MAX_MESSAGE_BYTES, MAX_MODEL_CALLS_PER_MINUTE, STALE_FLIGHT_NS};
    use crate::llm::PromptRole;
    use crate::store::{Store, StoreLimits};

    fn who(n: u8) -> Principal {
        Principal::from_slice(&[n; 29])
    }

    fn fallback() -> State {
        State { provider: LlmProvider::Fallback, ..State::default() }
    }

    fn start(state: &mut State, n: u8, id: &str, base: u64, text: &str, now: u64) -> Result<Run, ChatError> {
        match begin(state, who(n), id, base, text, now)? {
            Begun::Run(run) => Ok(run),
            Begun::Replay(sent) => panic!("unexpected replay: {sent:?}"),
        }
    }

    fn end(
        state: &mut State,
        n: u8,
        run: &Run,
        id: &str,
        text: &str,
        reply: &str,
        now: u64,
    ) -> Result<Sent, ChatError> {
        let result = finish(state, who(n), (run.token, run.revision), id.into(), text.into(), Ok(reply.into()), now);
        state.flights.end(&who(n), run.token);
        result
    }

    /// A whole exchange.
    fn chat(state: &mut State, n: u8, id: &str, base: u64, text: &str, now: u64) -> Result<Sent, ChatError> {
        let run = start(state, n, id, base, text, now)?;
        end(state, n, &run, id, text, &format!("re: {text}"), now)
    }

    fn err(result: Result<Begun, ChatError>) -> ChatError {
        match result {
            Err(e) => e,
            Ok(_) => panic!("expected a refusal"),
        }
    }

    #[test]
    fn an_exchange_is_saved_once_and_replays() {
        let mut state = fallback();
        assert_eq!(chat(&mut state, 1, "r1", 0, "hello", 10), Ok(Sent { reply: "re: hello".into(), revision: 1 }));
        // The same request again: the saved reply, with the revision as it is now, and no new chat.
        chat(&mut state, 1, "r2", 1, "more", 11).unwrap();
        match begin(&mut state, who(1), "r1", 0, "hello", 12) {
            Ok(Begun::Replay(sent)) => assert_eq!(sent, Sent { reply: "re: hello".into(), revision: 2 }),
            _ => panic!("expected a replay"),
        }
        assert!(!state.flights.holds(&who(1)));
        // The id with another text is refused, not replayed.
        assert!(matches!(err(begin(&mut state, who(1), "r1", 0, "other", 12)), ChatError::Invalid(_)));
        assert_eq!(conversation(&state, &who(1)).messages.len(), 4);
        assert_eq!(
            status(&state, &who(1), "r1", 12),
            MessageStatus::Done(Sent { reply: "re: hello".into(), revision: 2 })
        );
        // Another caller's conversation is its own.
        assert_eq!(status(&state, &who(2), "r1", 12), MessageStatus::Unknown);
        assert_eq!(conversation(&state, &who(2)), Conversation { messages: vec![], revision: 0 });
    }

    #[test]
    fn refusals_come_in_the_order_the_client_relies_on() {
        let mut state = State::default(); // OnChain: model calls are rate limited
        let long = "x".repeat(MAX_MESSAGE_BYTES + 1);

        // 1. Who is asking, and with what id.
        assert!(matches!(err(begin(&mut state, Principal::anonymous(), "r", 0, "hi", 0)), ChatError::Invalid(_)));
        assert!(matches!(err(begin(&mut state, who(1), "bad id", 0, "hi", 0)), ChatError::Invalid(_)));

        // 2. A saved request replays even if its text would be refused today and the canister is at every limit.
        state.store.append_exchange(who(1), long.clone(), "saved".into(), "old".into(), 0);
        let held: Vec<Run> =
            (0..MAX_CONCURRENT_CHATS as u8).map(|n| start(&mut state, 100 + n, "h", 0, "hold", 1).unwrap()).collect();
        assert!(matches!(begin(&mut state, who(1), "old", 0, &long, 2), Ok(Begun::Replay(_))));

        // 3. The message itself.
        assert!(matches!(err(begin(&mut state, who(1), "r", 1, &long, 2)), ChatError::Invalid(_)));
        assert!(matches!(err(begin(&mut state, who(1), "r", 1, " ", 2)), ChatError::Invalid(_)));

        // 4. The caller's own chat in flight: InProgress for the same id, Busy for another — before Conflict.
        assert_eq!(err(begin(&mut state, who(100), "h", 7, "hold", 2)), ChatError::InProgress);
        assert_eq!(err(begin(&mut state, who(100), "other", 7, "hi", 2)), ChatError::Busy);

        // 5. An outdated revision is a Conflict even while every slot is taken...
        assert_eq!(err(begin(&mut state, who(1), "r", 0, "hi", 2)), ChatError::Conflict);
        // 6. ...and a request that could run is Busy.
        assert_eq!(err(begin(&mut state, who(1), "r", 1, "hi", 2)), ChatError::Busy);
        assert!(!state.flights.holds(&who(1)), "a refused request leaves nothing behind");

        // Slots free up when chats end.
        for (n, run) in held.iter().enumerate() {
            state.flights.end(&who(100 + n as u8), run.token);
        }
        assert!(start(&mut state, 1, "r", 1, "hi", 3).is_ok());
    }

    #[test]
    fn model_calls_per_minute_are_limited_but_the_fallback_is_not() {
        let mut state = State::default();
        for i in 0..MAX_MODEL_CALLS_PER_MINUTE as u64 {
            chat(&mut state, 1, &format!("r{i}"), i, "hi", i).unwrap();
        }
        let next = MAX_MODEL_CALLS_PER_MINUTE as u64;
        assert_eq!(err(begin(&mut state, who(1), "more", next, "hi", next)), ChatError::Busy);
        assert_eq!(err(begin(&mut state, who(2), "more", 0, "hi", next)), ChatError::Busy);
        // A minute after the first call there is room again.
        assert!(chat(&mut state, 1, "more", next, "hi", crate::limits::RATE_WINDOW_NS).is_ok());

        let mut local = fallback();
        for i in 0..3 * MAX_MODEL_CALLS_PER_MINUTE as u64 {
            chat(&mut local, 1, &format!("r{i}"), i, "hi", 0).unwrap();
        }
    }

    #[test]
    fn a_failed_model_call_saves_nothing_and_leaves_the_request_usable() {
        let mut state = fallback();
        let run = start(&mut state, 1, "r1", 0, "hello", 0).unwrap();
        let failed =
            finish(&mut state, who(1), (run.token, run.revision), "r1".into(), "hello".into(), Err("no".into()), 1);
        state.flights.end(&who(1), run.token);
        assert_eq!(failed, Err(ChatError::Failed("no".into())));
        assert_eq!(conversation(&state, &who(1)), Conversation { messages: vec![], revision: 0 });
        assert_eq!(status(&state, &who(1), "r1", 2), MessageStatus::Unknown);
        assert_eq!(chat(&mut state, 1, "r1", 0, "hello", 3).map(|s| s.revision), Ok(1));
    }

    #[test]
    fn clearing_wins_over_a_reply_in_flight() {
        let mut state = fallback();
        chat(&mut state, 1, "r0", 0, "first", 0).unwrap();
        let run = start(&mut state, 1, "r1", 1, "hello", 1).unwrap();
        assert_eq!(status(&state, &who(1), "r1", 2), MessageStatus::Pending);
        assert_eq!(clear(&mut state, who(1), 2), Ok(2));
        assert_eq!(status(&state, &who(1), "r1", 2), MessageStatus::Unknown);
        // Still one chat per caller until the cleared one returns.
        assert_eq!(err(begin(&mut state, who(1), "r2", 2, "next", 3)), ChatError::Busy);
        assert_eq!(end(&mut state, 1, &run, "r1", "hello", "late", 4), Err(ChatError::Cleared));
        assert_eq!(conversation(&state, &who(1)), Conversation { messages: vec![], revision: 2 });
        // Nothing sent before the clear can run after it, including a first message never seen by the canister.
        assert_eq!(err(begin(&mut state, who(1), "r1", 1, "hello", 5)), ChatError::Conflict);
        assert_eq!(clear(&mut state, who(2), 5), Ok(1));
        assert_eq!(err(begin(&mut state, who(2), "late", 0, "sent before the clear", 6)), ChatError::Conflict);
        assert_eq!(chat(&mut state, 1, "r2", 2, "next", 7).map(|s| s.revision), Ok(3));
    }

    #[test]
    fn a_reply_that_comes_too_late_is_not_saved() {
        let mut state = fallback();
        let slow = start(&mut state, 1, "x", 0, "first try", 0).unwrap();
        assert_eq!(status(&state, &who(1), "x", STALE_FLIGHT_NS), MessageStatus::Unknown);
        // "Unknown" is the truth from then on: the reply is not saved even if nobody took the chat's place...
        let late = end(&mut state, 1, &slow, "x", "first try", "late", STALE_FLIGHT_NS);
        assert!(matches!(late, Err(ChatError::Failed(_))));
        assert_eq!(conversation(&state, &who(1)).messages.len(), 0);

        // ...and when another request did, only that one is saved, in whichever order the replies arrive.
        let stuck = start(&mut state, 1, "y", 0, "second try", STALE_FLIGHT_NS).unwrap();
        let fresh = start(&mut state, 1, "z", 0, "third try", 2 * STALE_FLIGHT_NS).unwrap();
        assert!(matches!(
            finish(
                &mut state,
                who(1),
                (stuck.token, stuck.revision),
                "y".into(),
                "second try".into(),
                Ok("late".into()),
                2 * STALE_FLIGHT_NS + 1
            ),
            Err(ChatError::Failed(_))
        ));
        state.flights.end(&who(1), stuck.token);
        assert_eq!(status(&state, &who(1), "z", 2 * STALE_FLIGHT_NS + 1), MessageStatus::Pending);
        assert_eq!(
            end(&mut state, 1, &fresh, "z", "third try", "ok", 2 * STALE_FLIGHT_NS + 2).map(|s| s.revision),
            Ok(1)
        );
        assert_eq!(err(begin(&mut state, who(1), "y", 0, "second try", 2 * STALE_FLIGHT_NS + 3)), ChatError::Conflict);
    }

    #[test]
    fn the_prompt_is_the_conversation_as_it_will_be_saved() {
        let mut state = fallback();
        chat(&mut state, 1, "r0", 0, "first", 0).unwrap();
        let run = start(&mut state, 1, "r1", 1, "second", 1).unwrap();
        let texts: Vec<(PromptRole, &str)> = run.prompt.iter().map(|m| (m.role, m.content.as_str())).collect();
        assert_eq!(texts[0].0, PromptRole::System);
        assert_eq!(
            &texts[1..],
            [(PromptRole::User, "first"), (PromptRole::Assistant, "re: first"), (PromptRole::User, "second")]
        );
        // A new character is used from the next message on, for every conversation.
        state.character.system_prompt = "You are Zoe.".into();
        end(&mut state, 1, &run, "r1", "second", "ok", 2).unwrap();
        let run = start(&mut state, 1, "r2", 2, "third", 3).unwrap();
        assert!(run.prompt[0].content.starts_with("You are Zoe."));
    }

    /// A state whose store holds at most `entries` entries.
    fn small(entries: usize) -> State {
        State { store: Store::with_limits(StoreLimits { max_bytes: usize::MAX, max_entries: entries }), ..fallback() }
    }

    #[test]
    fn a_newcomer_gets_in_when_room_has_to_be_made_and_keeps_its_revision() {
        let mut state = small(3);
        for n in 1..=3u8 {
            for i in 0..n as u64 {
                chat(&mut state, n, &format!("r{i}"), i, "hi", n as u64).unwrap();
            }
        }
        // The store is full. Caller 4 read revision 0 (the floor) and sends its first message: the oldest entry
        // is removed, which raises the floor — but the request was checked against 0 and is answered in one go.
        let run = start(&mut state, 4, "first", 0, "hello", 10).unwrap();
        assert!(!state.store.contains(&who(1)));
        assert_eq!(state.store.revision_floor(), 2);
        assert_eq!(run.revision, 0);
        // An eviction by someone else while the model answers does not touch the chat in flight either.
        let other = start(&mut state, 5, "first", 2, "hello", 11).unwrap();
        assert!(state.store.contains(&who(4)), "an entry with a chat in flight is never evicted");
        assert_eq!(
            end(&mut state, 4, &run, "first", "hello", "hi 4", 12),
            Ok(Sent { reply: "hi 4".into(), revision: 1 })
        );
        assert_eq!(end(&mut state, 5, &other, "first", "hello", "hi 5", 13).map(|s| s.revision), Ok(3));

        // The removed caller comes back: its old requests can never match, and it starts from the floor.
        let floor = state.store.revision_floor();
        assert!(floor >= 2);
        assert_eq!(conversation(&state, &who(1)), Conversation { messages: vec![], revision: floor });
        assert_eq!(err(begin(&mut state, who(1), "r0", 0, "hi", 20)), ChatError::Conflict);
        assert_eq!(err(begin(&mut state, who(1), "new", 1, "hi", 20)), ChatError::Conflict);
        assert!(chat(&mut state, 1, "new", floor, "hi", 21).is_ok());
    }

    #[test]
    fn when_no_room_can_be_made_the_answer_is_busy_and_clearing_fails() {
        let mut state = small(2);
        // Both entries belong to chats in flight, so neither may be removed.
        let a = start(&mut state, 1, "a", 0, "hi", 0).unwrap();
        let b = start(&mut state, 2, "b", 0, "hi", 0).unwrap();
        assert_eq!(err(begin(&mut state, who(3), "c", 0, "hi", 1)), ChatError::Busy);
        assert!(clear(&mut state, who(3), 1).is_err());
        assert_eq!(state.store.revision_floor(), 0, "nothing was removed");
        // A caller that has an entry can always clear it.
        assert_eq!(clear(&mut state, who(1), 1), Ok(1));
        assert_eq!(end(&mut state, 1, &a, "a", "hi", "x", 2), Err(ChatError::Cleared));
        // Once a chat has ended, its entry can make way.
        end(&mut state, 2, &b, "b", "hi", "x", 3).unwrap();
        assert!(chat(&mut state, 3, "c", 0, "hi", 4).is_ok());
    }
}
