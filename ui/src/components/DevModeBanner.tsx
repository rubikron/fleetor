// The dev-mode band (WP-16): a full-width strip under the topbar, present only
// while the mode is on.
//
// **Why it is this loud.** Dev mode is the posture the evaluator (WP-15) and the
// fence (WP-17) live inside. An operator who is in it without knowing runs a
// real fleet under evaluation rules; an operator who is *not* in it and thinks
// they are waits for a retro that will never happen. Both failures are silent,
// so the mode says itself on every screen rather than on one settings row.
//
// Theme rules it holds to (building.md §7): coral, never blue — coral is the
// "needs or has attention" token and this is the app's one standing attention
// state. Flat: a hairline rule and a 2px coral underline, no gradient, no
// shadow. And status is never colour alone (rule 3) — the dot is paired with the
// words DEV MODE, so it reads identically to an operator who cannot tell coral
// from gold.
//
// Conditionally rendered, unlike the stage views. §7 rule 5 pins *terminals*
// mounted because unmounting an xterm destroys its buffer; this strip holds no
// state worth preserving, and `StartGate` sets the precedent for chrome that
// simply is not there when it does not apply.

export function DevModeBanner() {
  return (
    <div className="devbar" role="status">
      <span className="dot dot--accent" aria-hidden="true" />
      <span className="devbar__label">DEV MODE</span>
      <span className="devbar__note">
        evaluation posture — this fleet is being run for assessment. Settings → Development turns
        it off.
      </span>
    </div>
  );
}
