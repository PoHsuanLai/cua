//! What a model reply cost.

use porter_core::{Locality, MicroUsd};
use porter_infer::ServedBy;

/// The least a reply from `served` can have cost. inferd reports no price with the token usage,
/// so a reply from a provider's servers counts one micro-dollar: enough to stop a run whose
/// spend cap is zero ("local models only"), not a measure for a larger cap. A reply from this
/// computer, a machine of the person's own, or a model not yet routed costs nothing.
pub(super) fn reply_cost(served: Option<&ServedBy>) -> MicroUsd {
    match served.map(|s| &s.locality) {
        Some(Locality::Cloud { .. }) => MicroUsd(1),
        Some(Locality::OnDevice | Locality::LocalNetwork) | None => MicroUsd(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_core::{AccountId, ModelId};

    fn served(locality: Locality) -> ServedBy {
        ServedBy {
            account: AccountId::parse("acct-1").expect("an account id"),
            model: ModelId::parse("model-1").expect("a model id"),
            locality,
        }
    }

    #[test]
    fn only_a_providers_servers_cost_anything() {
        let cases = [
            (None, MicroUsd(0)),
            (Some(served(Locality::OnDevice)), MicroUsd(0)),
            (Some(served(Locality::LocalNetwork)), MicroUsd(0)),
            (Some(served(Locality::Cloud { region: None })), MicroUsd(1)),
        ];
        for (served, want) in cases {
            assert_eq!(reply_cost(served.as_ref()), want);
        }
    }
}
