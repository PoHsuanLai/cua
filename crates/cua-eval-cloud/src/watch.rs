//! What the eval keeps count of while it runs on a hosted model: tokens, turns, the charges the
//! provider reported, and whether inferd said the spend cap was reached. These are for the
//! report and for stopping.

use model_provider::Money;
use porter_infer::ChargeTotal;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Counters shared by the provider and the run loop.
#[derive(Debug, Default)]
pub struct Watch {
    spend_refused: AtomicBool,
    turns: AtomicU64,
    input: AtomicU64,
    output: AtomicU64,
    charges: Mutex<ChargeTotal>,
}

/// Tokens, turns and reported charges so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    /// Chat turns answered.
    pub turns: u64,
    /// Prompt tokens.
    pub input: u64,
    /// Reply tokens (reasoning included).
    pub output: u64,
    /// The charges the provider reported, and the turns that reported none.
    pub charges: ChargeTotal,
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

    /// One answered turn, with the charge the provider reported for it, if it did.
    pub fn answered(&self, input: u64, output: u64, charge: Option<Money>) {
        self.turns.fetch_add(1, Ordering::SeqCst);
        self.input.fetch_add(input, Ordering::SeqCst);
        self.output.fetch_add(output, Ordering::SeqCst);
        let mut charges = self.charges.lock().unwrap_or_else(PoisonError::into_inner);
        *charges = charges.counting(charge);
    }

    /// What has been counted.
    pub fn totals(&self) -> Totals {
        Totals {
            turns: self.turns.load(Ordering::SeqCst),
            input: self.input.load(Ordering::SeqCst),
            output: self.output.load(Ordering::SeqCst),
            charges: *self.charges.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn a_spend_refusal_stays() {
        let watch = Watch::default();
        assert!(!watch.spend_refused());
        watch.refuse_spend();
        assert!(watch.spend_refused());
    }

    #[test]
    fn only_reported_charges_are_summed_and_the_rest_are_counted() {
        let watch = Watch::default();
        watch.answered(10, 5, Some(Money::usd(Decimal::new(2, 3))));
        watch.answered(10, 5, None);
        watch.answered(10, 5, Some(Money::usd(Decimal::new(3, 3))));
        let totals = watch.totals();
        assert_eq!((totals.turns, totals.input, totals.output), (3, 30, 15));
        assert_eq!(totals.charges.reported, Money::usd(Decimal::new(5, 3)));
        assert_eq!(totals.charges.unpriced.0, 1);
    }
}
