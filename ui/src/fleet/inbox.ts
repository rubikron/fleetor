// The operator's side of the record (WP-07): what was addressed to the human,
// and who they would be answering if they hit Reply.
//
// Both are **folds over the message stream the app already has** — there is no
// second source and no extra invoke. That matters for more than tidiness: the
// hub keeps its own `last_inbound_from` map so `fleet reply` works for the
// operator identity, and if this file derived the same answer from anywhere
// else the composer could show one name while the reply went to another.
// Folding the same events the hub folded is what keeps them agreeing.

import { hasPty, OPERATOR, type MessageEvent, type PaneId } from "./types";

/// Messages addressed to the human, newest first.
///
/// A broadcast leg is not one of these: nothing in the fleet fans out to the
/// operator (`Hub::roster` adds them to the *listing*, never to the broadcast
/// targets), so anything here was addressed by name. That is the whole reason
/// this list is worth showing without a count, a badge or a chime — it is short
/// because it can only contain things somebody meant for a person.
export function inbox(messages: MessageEvent[]): MessageEvent[] {
  return messages.filter((m) => !hasPty(m.to));
}

/// Who the operator's `reply` resolves to: whoever last got a message *through*
/// to them. `null` when nobody has written yet, which is exactly when the hub
/// would answer "nobody has messaged operator yet" — so the composer can leave
/// the option out rather than offering one that errors.
///
/// `messages` is newest-first, so the first match is the latest.
export function replyTarget(messages: MessageEvent[]): PaneId | null {
  return inbox(messages)[0]?.from ?? null;
}

/// The outcome word for one message row — the fleet's vocabulary, in the one
/// place the UI decides it (Tier 1.5).
///
///  - `accepted`: the bytes reached a live pty. Never "delivered": nothing on
///    this side knows whether the agent read them (L3).
///  - `recorded`: it entered the log, and no pty exists to have taken it. The
///    addressee is the operator; `accepted: false` on such a row is the literal
///    truth of that field and not a failure.
///  - `undelivered`: a real terminal was asked and did not take it. The send
///    happened; the arrival did not.
export type Outcome = "accepted" | "recorded" | "undelivered";

export function outcomeOf(to: PaneId, accepted: boolean): Outcome {
  if (accepted) return "accepted";
  return hasPty(to) ? "undelivered" : "recorded";
}

/// True when this message is the human speaking. Used only to mark a row —
/// never to reorder or filter one.
export function isFromOperator(message: MessageEvent): boolean {
  return message.from === OPERATOR;
}
