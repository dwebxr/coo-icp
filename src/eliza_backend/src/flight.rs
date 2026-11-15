//! The chats being answered: one per caller, a cap on how many run at once, and a cap on model calls per minute.
//! Heap only on purpose — an upgrade runs with the canister stopped, when no call is open.

use crate::api::ChatError;
use crate::limits::{MAX_MODEL_CALLS_PER_MINUTE, RATE_WINDOW_NS, STALE_FLIGHT_NS};
use candid::Principal;
use std::collections::{HashMap, VecDeque};

struct Flight {
    token: u64,
    request_id: String,
    started_at: u64,
    cleared: bool,
}

impl Flight {
    /// False once the chat is too old to be saved; it then counts for nothing any more.
    fn live(&self, now: u64) -> bool {
        now.saturating_sub(self.started_at) < STALE_FLIGHT_NS
    }
}

/// What became of a chat while the model was answering.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Still the caller's chat: the reply may be saved.
    Current,
    /// The caller cleared the conversation meanwhile.
    Cleared,
    /// It took too long, or another request of the caller took its place.
    Over,
}

#[derive(Default)]
pub struct Flights {
    by_caller: HashMap<Principal, Flight>,
    next_token: u64,
    /// Start times of the model calls in the current rate window, oldest first.
    model_calls: VecDeque<u64>,
}

impl Flights {
    /// Refuses a caller that already has a live chat: `InProgress` for the same request id, else `Busy`.
    pub fn check(&self, caller: &Principal, request_id: &str, now: u64) -> Result<(), ChatError> {
        match self.by_caller.get(caller) {
            Some(flight) if flight.live(now) => {
                Err(if flight.request_id == request_id { ChatError::InProgress } else { ChatError::Busy })
            }
            _ => Ok(()),
        }
    }

    /// Chats that can still be saved.
    pub fn live(&self, now: u64) -> usize {
        self.by_caller.values().filter(|f| f.live(now)).count()
    }

    /// True while the caller has a chat whose call has not returned; its entry must not be evicted.
    pub fn holds(&self, caller: &Principal) -> bool {
        self.by_caller.contains_key(caller)
    }

    pub fn model_call_allowed(&mut self, now: u64) -> bool {
        while self.model_calls.front().is_some_and(|&t| now.saturating_sub(t) >= RATE_WINDOW_NS) {
            self.model_calls.pop_front();
        }
        self.model_calls.len() < MAX_MODEL_CALLS_PER_MINUTE
    }

    /// Starts the caller's chat (taking the place of one that is no longer live) and returns its token.
    pub fn begin(&mut self, caller: Principal, request_id: &str, now: u64, calls_model: bool) -> u64 {
        self.next_token += 1;
        let token = self.next_token;
        self.by_caller
            .insert(caller, Flight { token, request_id: request_id.to_owned(), started_at: now, cleared: false });
        if calls_model {
            self.model_calls.push_back(now);
        }
        token
    }

    pub fn verdict(&self, caller: &Principal, token: u64, now: u64) -> Verdict {
        match self.by_caller.get(caller) {
            Some(flight) if flight.token == token && flight.cleared => Verdict::Cleared,
            Some(flight) if flight.token == token && flight.live(now) => Verdict::Current,
            _ => Verdict::Over,
        }
    }

    /// Ends the chat with this token; a chat that took its place is left alone.
    pub fn end(&mut self, caller: &Principal, token: u64) {
        if self.by_caller.get(caller).is_some_and(|f| f.token == token) {
            self.by_caller.remove(caller);
        }
    }

    pub fn mark_cleared(&mut self, caller: &Principal) {
        if let Some(flight) = self.by_caller.get_mut(caller) {
            flight.cleared = true;
        }
    }

    /// True while this request id is being answered and its reply can still be saved.
    pub fn pending(&self, caller: &Principal, request_id: &str, now: u64) -> bool {
        self.by_caller.get(caller).is_some_and(|f| f.request_id == request_id && !f.cleared && f.live(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(n: u8) -> Principal {
        Principal::from_slice(&[n; 29])
    }

    #[test]
    fn one_chat_per_caller() {
        let (alice, bob) = (who(1), who(2));
        let mut flights = Flights::default();
        assert_eq!(flights.check(&alice, "r1", 100), Ok(()));
        let token = flights.begin(alice, "r1", 100, false);
        assert_eq!(flights.check(&alice, "r1", 101), Err(ChatError::InProgress));
        assert_eq!(flights.check(&alice, "r2", 101), Err(ChatError::Busy));
        assert_eq!(flights.check(&bob, "r1", 101), Ok(()));
        let other = flights.begin(bob, "r1", 101, false);
        assert!(flights.pending(&alice, "r1", 102) && !flights.pending(&alice, "r2", 102));
        assert_eq!(flights.verdict(&alice, token, 102), Verdict::Current);
        assert_eq!(flights.live(102), 2);

        flights.end(&alice, token);
        assert!(!flights.pending(&alice, "r1", 103) && !flights.holds(&alice));
        assert_eq!(flights.check(&alice, "r2", 104), Ok(()));
        assert_eq!(flights.verdict(&alice, token, 104), Verdict::Over);
        flights.end(&bob, other);
        assert_eq!(flights.live(105), 0);
    }

    #[test]
    fn clearing_cancels_the_save_but_not_the_chat() {
        let alice = who(3);
        let mut flights = Flights::default();
        let token = flights.begin(alice, "r1", 0, false);
        flights.mark_cleared(&alice);
        assert_eq!(flights.verdict(&alice, token, 1), Verdict::Cleared);
        assert!(!flights.pending(&alice, "r1", 1));
        // Still busy until the cleared chat's call returns.
        assert_eq!(flights.check(&alice, "r2", 1), Err(ChatError::Busy));
        flights.end(&alice, token);
        assert_eq!(flights.check(&alice, "r2", 2), Ok(()));
    }

    #[test]
    fn a_chat_that_took_too_long_is_over() {
        let alice = who(4);
        let mut flights = Flights::default();
        let stuck = flights.begin(alice, "r1", 0, false);
        assert_eq!(flights.check(&alice, "r2", STALE_FLIGHT_NS - 1), Err(ChatError::Busy));
        assert_eq!(flights.verdict(&alice, stuck, STALE_FLIGHT_NS - 1), Verdict::Current);
        // From the limit on: not pending, not saved, not counted, and no longer in the way —
        assert!(!flights.pending(&alice, "r1", STALE_FLIGHT_NS));
        assert_eq!(flights.verdict(&alice, stuck, STALE_FLIGHT_NS), Verdict::Over);
        assert_eq!(flights.live(STALE_FLIGHT_NS), 0);
        assert_eq!(flights.check(&alice, "r2", STALE_FLIGHT_NS), Ok(()));
        // but its entry stays protected until its call returns.
        assert!(flights.holds(&alice));

        let fresh = flights.begin(alice, "r2", STALE_FLIGHT_NS, false);
        assert_eq!(flights.verdict(&alice, stuck, STALE_FLIGHT_NS + 1), Verdict::Over);
        // The old call returning does not end the new chat.
        flights.end(&alice, stuck);
        assert_eq!(flights.verdict(&alice, fresh, STALE_FLIGHT_NS + 1), Verdict::Current);
        assert!(flights.pending(&alice, "r2", STALE_FLIGHT_NS + 1));
    }

    #[test]
    fn model_calls_are_limited_per_minute() {
        let mut flights = Flights::default();
        for i in 0..MAX_MODEL_CALLS_PER_MINUTE {
            assert!(flights.model_call_allowed(i as u64));
            let token = flights.begin(who(i as u8), "r", i as u64, true);
            flights.end(&who(i as u8), token);
        }
        assert!(!flights.model_call_allowed(RATE_WINDOW_NS - 1));
        // The oldest call leaves the window one minute after it started.
        assert!(flights.model_call_allowed(RATE_WINDOW_NS));
        // Chats that do not call the model are not counted.
        let mut flights = Flights::default();
        for i in 0..2 * MAX_MODEL_CALLS_PER_MINUTE {
            let token = flights.begin(who(1), "r", i as u64, false);
            flights.end(&who(1), token);
        }
        assert!(flights.model_call_allowed(0));
    }
}
