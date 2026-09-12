// Stopping a Tauri event listener, without turning teardown into an unhandled
// rejection.
//
// **`UnlistenFn` is typed `() => void` and is in fact `async`.** It calls the
// injected `unregisterListener` and then awaits `plugin:event|unlisten`. Every
// caller here is a React cleanup, which cannot await — so a throw inside that
// function became a floating rejected promise, and `installDevErrorReporting`
// reported one per listener: "unhandled promise rejection", with a stack ending
// in `unregisterListener@user-script` and no message at all. A reopen produced
// twelve at once, which is what five panes with two listeners each plus the feed
// and the evaluator's wake come to.
//
// **The command behind it cannot fail** — tauri 2.11.5's `unlisten`
// (`src/event/plugin.rs`) returns `Ok(())` unconditionally — so what throws is
// the injected bookkeeping, over a listener the webview no longer has. There is
// nothing to do about that at teardown: the listener is gone either way. It is
// still logged rather than swallowed, because a listener that failed to stop
// while the webview was alive would show up here first, and that one matters.
import { devLog } from "../dev/devLog";

/// Reported **once per session**, not once per listener. Twelve identical lines
/// on every reopen is the noise this function exists to remove; one line still
/// says the failure exists, which is all the first one ever told anyone.
let told = false;

export function stopListening(stop: (() => void) | undefined, where: string): void {
  if (!stop) return;
  const failed = (e: unknown) => {
    if (told) return;
    told = true;
    devLog("warn", `could not stop listening: ${where} (silenced for the rest of this session)`, e);
  };
  try {
    const done: unknown = stop();
    if (done && typeof (done as PromiseLike<unknown>).then === "function") {
      void (done as Promise<unknown>).catch(failed);
    }
  } catch (e) {
    failed(e);
  }
}
