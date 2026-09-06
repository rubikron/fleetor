//! **Whose usage a worker seat spends** (WP-25 #49; C73, C74, C75, C78) — the
//! operator's own plan, or the key they supplied.
//!
//! **A choice per seat, made on the start gate.** #49 planned a per-seat toggle
//! defaulting to the fleet's key; C75 reversed both halves to one fleet-wide
//! picker defaulting to the plan, on the grounds that a per-seat toggle is an
//! extra step every launch; C78 settled the shape that keeps both — a per-seat
//! dropdown on every worker row, and a bulk control above them that writes all
//! four at once and reports `mixed` when they disagree. The zero-step path
//! survives because the default is still the plan; the per-seat answer exists
//! because a fleet is not obliged to be uniform.
//!
//! **This module holds the type and the offer, and no storage.** C75 put the
//! choice in `~/.fleetor/config.json` beside `dev_mode`, arguing that the spawn
//! path branches on it and so it must be readable without a webview. C78 removed
//! that: the spawn path branches on [`PaneSpec`], which comes from
//! [`FleetSeats`], so the seat *is* the home and a config key beside it would be
//! a second answer to one question — the shape `M15` exists to refuse. What
//! survives a restart is what already survived one: the interface's remembered
//! selection, which pushes the seats back through `fleet_set_seats`.
//!
//! **This is the operator's *pick*. [`CredentialSource`] is the *credential*.**
//! Two types on purpose: a pick is a serialisable preference, and a credential is
//! a live secret that must never be one. They meet in `FleetSeats::spec_for`,
//! which turns a pick into a [`PaneSpec`] flag, and in `place_worker`, which
//! pairs that flag with `Host::operator_login` — the one place a missing login
//! becomes a refusal instead of a silent fallback.
//!
//! [`CredentialSource`]: crate::placement::CredentialSource
//! [`PaneSpec`]: crate::placement::PaneSpec
//! [`FleetSeats`]: crate::fleet::FleetSeats

use serde::{Deserialize, Serialize};

/// **What the operator picked for one seat**, as opposed to what that seat is
/// handed at spawn.
///
/// Two states rather than three: there is no "unset". A seat with no answer is a
/// seat on the plan, which is what makes a first run — and a selection remembered
/// from before this field existed — land on C75's default with no migration.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialChoice {
    /// This seat spends the operator's existing login. **The default**, including
    /// on a payload that carries no `credential` field at all.
    #[default]
    Plan,
    /// This seat spends the key the operator supplied — D-062's original rule,
    /// still one click away.
    FleetKey,
}

impl CredentialChoice {
    /// The operator's own plan, spelled affirmatively.
    #[allow(non_upper_case_globals)]
    pub const OperatorsPlan: Self = Self::Plan;

    /// Whether this seat spends the operator's plan. The one predicate the cost
    /// lines, the seat count, the bulk control and `spec_for` all read, so they
    /// cannot disagree about what a seat is.
    pub fn is_the_operators_plan(self) -> bool {
        matches!(self, CredentialChoice::Plan)
    }
}

/// **What this machine can actually honour**, paired with what a seat asked for
/// (C75, C78) — what the gate needs to say a true sentence about a click.
///
/// **A struct rather than two loose lists**, for C39's reason: they are always
/// read together, and a caller holding one without the other can promise a plan
/// seat on a harness with no login in it.
///
/// `readable` is harness names from `Host::operator_logins` — the same list
/// `place_worker` consults — so what the gate promises and what the spawn path
/// refuses are one answer rather than two that agree by luck. `has_fleet_key` is
/// the `.env` walk's, for the same reason: since C78 a seat on the key refuses
/// individually rather than the whole fleet degrading to orchestrator-only.
#[derive(Debug, Clone, Default)]
pub struct PlanOffer {
    readable: Vec<String>,
    has_fleet_key: bool,
}

impl PlanOffer {
    /// The offer as this machine has it.
    pub fn new(readable: Vec<String>, has_fleet_key: bool) -> Self {
        Self { readable, has_fleet_key }
    }

    /// **A machine with a key and no login**, which is what every test that is not
    /// about this feature wants: seats on the fleet's key place, seats on the plan
    /// refuse. Also what [`Default`] would produce but for the key, so it is named
    /// rather than left implicit.
    pub fn fleet_key() -> Self {
        Self { readable: Vec::new(), has_fleet_key: true }
    }

    /// The harness names this machine has a readable operator login for — what the
    /// gate's picker says when it explains an unavailable option.
    pub fn readable(&self) -> &[String] {
        &self.readable
    }

    /// Whether the `.env` walk found a key.
    pub fn has_fleet_key(&self) -> bool {
        self.has_fleet_key
    }

    /// Whether a seat on this harness, having asked for the plan, would actually
    /// get it — the ask **and** a login this machine can read.
    pub fn honours_the_plan(&self, harness: &str) -> bool {
        self.readable.iter().any(|n| n == harness)
    }

    /// Why this seat cannot start, or `None` when it can (C78).
    ///
    /// **Both directions refuse and neither falls back.** A plan seat with no
    /// login must not quietly spend the metered key, and a key seat with no key
    /// must not quietly spend the plan — the second is the one C78 added, and it
    /// is what retired the all-or-nothing `workers_run`.
    ///
    /// The sentence is assembled by the caller, which holds the harness's own
    /// [`LoginInstruction`](crate::placement::harness::LoginInstruction) and the
    /// path the key was looked for under; this answers only *which* gap it is.
    pub fn gap_for(&self, harness: &str, choice: CredentialChoice) -> Option<Gap> {
        match choice {
            CredentialChoice::Plan if !self.honours_the_plan(harness) => Some(Gap::NoLogin),
            CredentialChoice::FleetKey if !self.has_fleet_key => Some(Gap::NoKey),
            _ => None,
        }
    }
}

/// Which credential a seat asked for and did not get (C78).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    /// Asked for the plan; this machine has no readable login for that harness.
    NoLogin,
    /// Asked for the key; the `.env` walk found none.
    NoKey,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The wire default is the plan, and an absent field is not an error**
    /// (C75, C78). A selection remembered by an interface built before this field
    /// existed must land on the default the operator was offered, not on a parse
    /// failure and not on the fleet's key.
    #[test]
    fn a_seat_with_no_credential_field_reads_as_the_plan() {
        #[derive(serde::Deserialize)]
        struct Seat {
            #[serde(default)]
            credential: CredentialChoice,
        }
        let old: Seat = serde_json::from_str(r#"{"harness":"claude-code","model":null}"#)
            .expect("a payload from before this field existed must still parse");
        assert_eq!(old.credential, CredentialChoice::Plan);

        let named: Seat = serde_json::from_str(r#"{"credential":"fleet_key"}"#).expect("parse");
        assert_eq!(named.credential, CredentialChoice::FleetKey);
    }

    /// **The spelling on the wire is what `ui/src/fleet/types.ts` says it is.**
    /// The field is a bare string; a drift between the two is invisible in both
    /// directions, and an unrecognised value would fail the deserialize rather
    /// than silently pick a side — which is why this pins the exact strings.
    #[test]
    fn the_wire_spellings_are_plan_and_fleet_key() {
        assert_eq!(serde_json::to_string(&CredentialChoice::Plan).unwrap(), "\"plan\"");
        assert_eq!(serde_json::to_string(&CredentialChoice::FleetKey).unwrap(), "\"fleet_key\"");
    }

    /// **A plan seat needs both the ask and a readable login**, and a key seat
    /// needs a key. Both gaps refuse; neither downgrades.
    #[test]
    fn each_credential_has_its_own_gap() {
        let offer = PlanOffer::new(vec!["claude-code".into()], false);

        assert_eq!(offer.gap_for("claude-code", CredentialChoice::Plan), None);
        assert_eq!(offer.gap_for("codex", CredentialChoice::Plan), Some(Gap::NoLogin));
        assert_eq!(offer.gap_for("claude-code", CredentialChoice::FleetKey), Some(Gap::NoKey));

        let keyed = PlanOffer::fleet_key();
        assert_eq!(keyed.gap_for("claude-code", CredentialChoice::FleetKey), None);
        assert_eq!(keyed.gap_for("claude-code", CredentialChoice::Plan), Some(Gap::NoLogin));
    }

    /// **`fleet_key()` must not promise the operator's plan.** Every call site
    /// that has no opinion about this feature gets it, and the wrong answer here
    /// would spend a subscription from a test helper.
    #[test]
    fn the_stock_offer_honours_no_plan() {
        assert!(!PlanOffer::fleet_key().honours_the_plan("claude-code"));
        assert!(PlanOffer::fleet_key().has_fleet_key());
        assert!(!PlanOffer::default().has_fleet_key());
    }
}
