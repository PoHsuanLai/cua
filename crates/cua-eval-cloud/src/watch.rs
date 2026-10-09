//! What the eval keeps count of while it runs on a hosted model: tokens, turns, and whether
//! inferd said the spend cap was reached. Money is inferd's ledger; these are for the report and
//! for stopping.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Counters shared by the provider and the run loop.
#[derive(Debug, Default)]
pub struct Watch {
    spend_refused: AtomicBool,
    turns: AtomicU64,
    input: AtomicU64,
    output: AtomicU64,
}

/// Tokens and turns so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    /// Chat turns answered.
    pub turns: u64,
    /// Prompt tokens.
    pub input: u64,
    /// Reply tokens (reasoning included).
    pub output: u64,
}

impl Watch {
    /// Inferd refused a turn because a spend cap was reached.
    pub fn refuse_spend(&self) {
        self.spend_refused.store(true, Ordering::SeqCst);
    }

    /// Whether inferd has refused on a spend cap.
    pub fn spend_refused(&self) -> bool {
        self.spend_refused.load(Ordering::SeqCst)
    }

    /// One answered turn.
    pub fn answered(&self, input: u64, output: u64) {
        self.turns.fetch_add(1, Ordering::SeqCst);
        self.input.fetch_add(input, Ordering::SeqCst);
        self.output.fetch_add(output, Ordering::SeqCst);
    }

    /// What has been counted.
    pub fn totals(&self) -> Totals {
        Totals {
            turns: self.turns.load(Ordering::SeqCst),
            input: self.input.load(Ordering::SeqCst),
            output: self.output.load(Ordering::SeqCst),
        }
    }
}

/// The price of a model per million tokens, in micro-dollars (the catalogue's unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    /// Per million prompt tokens.
    pub input: u64,
    /// Per million reply tokens.
    pub output: u64,
}

impl Price {
    /// What `totals` cost at this price, in micro-dollars.
    pub fn of(self, totals: Totals) -> u64 {
        (totals.input * self.input + totals.output * self.output) / 1_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_price_is_per_million_tokens() {
        let price = Price {
            input: 2_000_000,
            output: 10_000_000,
        };
        let totals = Totals {
            turns: 1,
            input: 500_000,
            output: 100_000,
        };
        assert_eq!(price.of(totals), 2_000_000);
    }

    #[test]
    fn a_spend_refusal_stays() {
        let watch = Watch::default();
        assert!(!watch.spend_refused());
        watch.refuse_spend();
        assert!(watch.spend_refused());
    }
}
