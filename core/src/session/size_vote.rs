//! Which attached client decides a session's box (protocol §6).
//!
//! Pure bookkeeping: who is attached, what box each asked for, and who the policy
//! currently honours. The caller owns the session box and applies the result.

use std::collections::HashMap;

use super::size_policy::SizePolicy;

/// The box one attached client needs for its own grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subscriber {
    pub cols: u16,
    pub rows: u16,
}

/// The size policy of one session plus every client that votes in it.
#[derive(Debug)]
pub struct SizeVote {
    policy: SizePolicy,
    /// The client that decided `policy`; the only one `locked` listens to.
    owner: u32,
    /// The client `latest` currently honours.
    active: u32,
    /// Every attached client and the box it asked for.
    clients: HashMap<u32, Subscriber>,
}

impl Default for SizeVote {
    fn default() -> Self {
        Self {
            policy: SizePolicy::Latest,
            owner: 0,
            active: 0,
            clients: HashMap::new(),
        }
    }
}

impl SizeVote {
    pub fn policy(&self) -> SizePolicy {
        self.policy
    }

    /// How many clients vote (the `size_state.clients` count).
    pub fn clients(&self) -> usize {
        self.clients.len()
    }

    pub fn contains(&self, client: u32) -> bool {
        self.clients.contains_key(&client)
    }

    /// `attach`: the first client decides the policy, later ones merely join. A newcomer
    /// starts out asking for the box the session already has, so joining alone moves nothing.
    pub fn join(&mut self, client: u32, policy: SizePolicy, current: (u16, u16)) {
        if self.clients.is_empty() {
            self.policy = policy;
            self.owner = client;
            self.active = client;
        }
        self.clients.insert(
            client,
            Subscriber {
                cols: current.0,
                rows: current.1,
            },
        );
        if self.policy == SizePolicy::Locked {
            self.active = self.owner;
        }
    }

    /// `resize_pane`: record the box `client` wants. Under `locked` only the owner may take
    /// the size; otherwise the reporting client becomes the one `latest` follows.
    pub fn report(&mut self, client: u32, wanted: (u16, u16)) {
        if let Some(subscriber) = self.clients.get_mut(&client) {
            subscriber.cols = wanted.0;
            subscriber.rows = wanted.1;
        }
        if self.policy != SizePolicy::Locked || client == self.owner {
            self.active = client;
        }
    }

    /// `focus`: focusing is what makes a window the one that owns the size. Returns whether
    /// the vote took it into account.
    pub fn focus(&mut self, client: u32) -> bool {
        if self.policy == SizePolicy::Locked || !self.clients.contains_key(&client) {
            return false;
        }
        self.active = client;
        true
    }

    /// Typing or pasting under `latest`. Returns whether `client` just took over the size.
    pub fn activity(&mut self, client: u32) -> bool {
        if self.policy != SizePolicy::Latest
            || self.active == client
            || !self.clients.contains_key(&client)
        {
            return false;
        }
        self.active = client;
        true
    }

    /// A closed connection stops voting, or `smallest` would stay pinned to a box nobody
    /// can see any more. Returns whether `client` was a voter at all.
    pub fn leave(&mut self, client: u32) -> bool {
        if self.clients.remove(&client).is_none() {
            return false;
        }
        if self.active == client {
            self.active = self.clients.keys().copied().min().unwrap_or(0);
        }
        true
    }

    /// The box the policy asks for, or `None` when nobody votes for one; `current` is the
    /// answer when the honoured client is gone.
    pub fn negotiated(&self, current: (u16, u16)) -> Option<(u16, u16)> {
        let wanted = match self.policy {
            SizePolicy::Smallest => self
                .clients
                .values()
                .fold((u16::MAX, u16::MAX), |(cols, rows), client| {
                    (cols.min(client.cols), rows.min(client.rows))
                }),
            // `locked` only lets the owner report, and `active` is pinned to it on join.
            SizePolicy::Latest | SizePolicy::Locked => self
                .clients
                .get(&self.active)
                .map(|client| (client.cols, client.rows))
                .unwrap_or(current),
        };
        (wanted.0 != u16::MAX).then_some(wanted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: (u16, u16) = (80, 24);

    #[test]
    fn smallest_takes_the_minimum_of_every_client() {
        let mut vote = SizeVote::default();
        vote.join(1, SizePolicy::Smallest, START);
        vote.join(2, SizePolicy::Latest, START);
        assert_eq!(vote.policy(), SizePolicy::Smallest, "the first attach decides");
        vote.report(1, (120, 20));
        vote.report(2, (100, 40));
        assert_eq!(vote.negotiated(START), Some((100, 20)));
        assert!(vote.leave(2));
        assert_eq!(vote.negotiated(START), Some((120, 20)));
    }

    #[test]
    fn smallest_with_nobody_attached_asks_for_nothing() {
        let mut vote = SizeVote::default();
        vote.join(1, SizePolicy::Smallest, START);
        assert!(vote.leave(1));
        assert_eq!(vote.negotiated(START), None);
        assert!(!vote.leave(1), "leaving twice is a no-op");
    }

    #[test]
    fn latest_follows_whoever_typed_last() {
        let mut vote = SizeVote::default();
        vote.join(1, SizePolicy::Latest, START);
        vote.join(2, SizePolicy::Latest, START);
        vote.report(2, (90, 30));
        vote.report(1, (70, 20));
        assert_eq!(vote.negotiated(START), Some((70, 20)));
        assert!(vote.activity(2));
        assert_eq!(vote.negotiated(START), Some((90, 30)));
        assert!(!vote.activity(2), "already active");
        assert!(!vote.activity(9), "not attached");
    }

    #[test]
    fn locked_ignores_everyone_but_the_owner() {
        let mut vote = SizeVote::default();
        vote.join(1, SizePolicy::Locked, START);
        vote.join(2, SizePolicy::Smallest, START);
        vote.report(2, (40, 10));
        assert_eq!(vote.negotiated(START), Some(START));
        assert!(!vote.focus(2));
        assert!(!vote.activity(2));
        vote.report(1, (100, 30));
        assert_eq!(vote.negotiated(START), Some((100, 30)));
    }

    #[test]
    fn the_active_client_leaving_hands_the_size_to_the_lowest_id() {
        let mut vote = SizeVote::default();
        vote.join(3, SizePolicy::Latest, START);
        vote.join(5, SizePolicy::Latest, START);
        vote.report(3, (60, 20));
        vote.report(5, (90, 30));
        assert!(vote.leave(5));
        assert_eq!(vote.negotiated(START), Some((60, 20)));
        assert!(vote.leave(3));
        assert_eq!(vote.negotiated((61, 21)), Some((61, 21)), "nobody left: keep the box");
    }
}
