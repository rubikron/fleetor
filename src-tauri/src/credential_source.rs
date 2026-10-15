//! **Which harnesses this machine has a readable operator login for** (D-118).
//!
//! Every worker seat runs on the operator's own login, so this is the whole of
//! what the gate asks before a start. It holds names and no credential.

/// The harness names from `Host::operator_logins` — the same list `place_worker`
/// consults, so what the gate promises and what the spawn path refuses are one
/// answer.
#[derive(Debug, Clone, Default)]
pub struct PlanOffer {
    readable: Vec<String>,
}

impl PlanOffer {
    /// The offer as this machine has it.
    pub fn new(readable: Vec<String>) -> Self {
        Self { readable }
    }

    /// The harness names this machine has a readable operator login for.
    pub fn readable(&self) -> &[String] {
        &self.readable
    }

    /// Whether a seat on this harness has a login this machine can read.
    pub fn honours_the_plan(&self, harness: &str) -> bool {
        self.readable.iter().any(|n| n == harness)
    }

    /// Why a worker seat on this harness cannot start, or `None` when it can.
    pub fn gap_for(&self, harness: &str) -> Option<Gap> {
        (!self.honours_the_plan(harness)).then_some(Gap::NoLogin)
    }
}

/// What a worker seat needed and this machine does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    /// This machine has no readable login for that harness.
    NoLogin,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_harness_with_no_readable_login_is_a_gap() {
        let offer = PlanOffer::new(vec!["claude-code".into()]);

        assert_eq!(offer.gap_for("claude-code"), None);
        assert_eq!(offer.gap_for("codex"), Some(Gap::NoLogin));
        assert_eq!(PlanOffer::default().gap_for("claude-code"), Some(Gap::NoLogin));
    }
}
