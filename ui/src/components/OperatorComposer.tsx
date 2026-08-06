// The operator's composer (WP-07) — the human's `fleet send`.
//
// Three rules it must not break:
//
//  1. **It is the same three verbs the panes have**, not a fourth kind of
//     message: a pane name is a send, `all` is the broadcast, `reply` answers
//     whoever last wrote. What crosses into the backend is an ordinary wire op
//     with `from: operator`.
//  2. **It reports the fleet's own word.** `accepted` means the bytes reached a
//     live terminal and nothing more (L3); a refusal says `undelivered` and
//     names the pane. Nothing here ever says "sent" or "delivered".
//  3. **It logs a message, not keystrokes.** Typing into a terminal stays what
//     it was — raw, unlogged, and none of this file's business.

import { useEffect, useMemo, useState } from "react";
import { sendAsOperator } from "../fleet/api";
import { ORCH, ROSTER, type PaneId } from "../fleet/types";

/// `all` and `reply` are spellings of a verb, not of a participant — the
/// backend resolves them, for the same reason the CLI resolves `self` locally.
const TARGET_ALL = "all";
const TARGET_REPLY = "reply";

type Target = PaneId | typeof TARGET_ALL | typeof TARGET_REPLY;

interface Sent {
  ok: boolean;
  text: string;
}

interface OperatorComposerProps {
  /// The pane the operator would be answering, or `null` when nobody has
  /// written to them yet. Drives the default target so a worker's question can
  /// be answered without noticing which worker asked.
  replyTo: PaneId | null;
}

export function OperatorComposer({ replyTo }: OperatorComposerProps) {
  const [target, setTarget] = useState<Target>(ORCH);
  const [touched, setTouched] = useState(false);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState<Sent | null>(null);

  // A question arriving picks the answer's target — but only until the operator
  // sets one themselves. Overriding a deliberate choice because a message
  // landed mid-sentence would send it somewhere they did not pick, which is the
  // one mistake this surface must not make.
  useEffect(() => {
    if (!touched && replyTo) setTarget(TARGET_REPLY);
  }, [replyTo, touched]);

  const targets = useMemo(
    () => [
      ...(replyTo ? [{ value: TARGET_REPLY, label: `reply → ${replyTo}` }] : []),
      ...ROSTER.map((pane) => ({ value: pane, label: pane })),
      { value: TARGET_ALL, label: "all (broadcast)" },
    ],
    [replyTo],
  );

  async function send() {
    const body = text.trim();
    if (!body || busy) return;
    setBusy(true);
    try {
      const result = await sendAsOperator(target, body);
      setText("");
      setSent(
        result.outcome === "accepted"
          ? { ok: true, text: "accepted — the bytes reached the terminal" }
          : { ok: false, text: `${result.outcome} — ${result.detail ?? "the pane refused"}` },
      );
    } catch (e) {
      // Never swallowed: a message the operator believes they sent and did not
      // is the same failure as a log that lies.
      setSent({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      className="composer"
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      <label className="composer__label" htmlFor="composer-target">
        as <span className="composer__me">operator</span> →
      </label>
      <select
        id="composer-target"
        className="composer__target"
        value={target}
        onChange={(e) => {
          setTouched(true);
          setTarget(e.target.value as Target);
        }}
      >
        {targets.map((t) => (
          <option key={t.value} value={t.value}>
            {t.label}
          </option>
        ))}
      </select>
      <input
        className="composer__input"
        value={text}
        placeholder="say something to the fleet — it lands in the pane and in the log"
        onChange={(e) => setText(e.target.value)}
        // Enter is the gesture for a single-line box; ⌘/Ctrl+Enter is the one a
        // model-shaped habit reaches for. Both do the same thing, so neither is
        // a surprise.
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void send();
          }
        }}
      />
      <button className="composer__send" type="submit" disabled={busy || !text.trim()}>
        {busy ? "sending…" : "send"}
      </button>
      {sent && (
        <span className={`composer__result ${sent.ok ? "" : "composer__result--bad"}`}>
          {sent.text}
        </span>
      )}
    </form>
  );
}
