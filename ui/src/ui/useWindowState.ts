// Window geometry memory: the app reopens where and how big you left it.
//
// This is the one piece of state memory that is not localStorage-only — the
// size and position live with the OS, so restoring them needs the two window
// *writes* (`core:window:allow-set-size` / `allow-set-position`) that had to be
// added to src-tauri/capabilities/default.json. `core:default` grants the
// matching reads (`outer-size`, `outer-position`, `available-monitors`) but
// not those, so without that capability edit every restore here rejects.
//
// **A saved position is not automatically a valid one.** Unplug the external
// display the window was on and the stored coordinates now point into dead
// space — restore them blind and the app launches somewhere the operator
// cannot reach, with no visible failure to explain it. So a position is only
// re-applied if it still overlaps a currently-connected monitor by enough to
// grab; otherwise the size is restored and placement is left to the OS.
//
// Everything here is best-effort. A rejected Tauri call, a corrupt stored
// value, or a missing capability must degrade to "the OS picks" — never to a
// crashed shell.

import { useEffect, useRef } from "react";
import { availableMonitors, getCurrentWindow, type Monitor } from "@tauri-apps/api/window";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import type { UnlistenFn } from "@tauri-apps/api/event";

const STORAGE_KEY = "fleetor:window-state";

/// Resize and move events fire continuously through a drag; only the value
/// the operator settles on is worth a write.
const PERSIST_DEBOUNCE_MS = 400;

/// Sanity floor in physical pixels. Anything smaller is a corrupt read, not a
/// window someone deliberately left that size.
const MIN_PHYSICAL_PX = 200;

/// How much of the window must overlap a live monitor for its saved position
/// to count as reachable — roughly "enough title bar to grab".
const MIN_VISIBLE_PX = 80;

/// How much of the monitor a first launch claims. Not 1.0 — a window pinned to
/// the screen edges is indistinguishable from a maximized one, and the operator
/// loses the handles to resize it.
const FIRST_RUN_SCREEN_FRACTION = 0.85;

/// Physical pixels, as reported by `outerSize` / `outerPosition`. `x` and `y`
/// are signed: a monitor arranged left of the primary has negative origins.
interface WindowGeometry {
  width: number;
  height: number;
  x: number;
  y: number;
  /// Whether the window filled the screen when it was last recorded.
  ///
  /// Stored as a flag rather than baked into the width and height, because
  /// those two facts want different lifetimes: the geometry should remember the
  /// size the operator sized the window *to*, and this should remember that
  /// they then filled the screen. Collapsing them loses the first one forever
  /// the moment anything zooms.
  maximized: boolean;
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function parseGeometry(raw: string | null): WindowGeometry | null {
  if (raw === null) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    const { width, height, x, y, maximized } = parsed as Record<string, unknown>;
    if (!isFiniteNumber(width) || !isFiniteNumber(height)) return null;
    if (!isFiniteNumber(x) || !isFiniteNumber(y)) return null;
    if (width < MIN_PHYSICAL_PX || height < MIN_PHYSICAL_PX) return null;
    // Absent on anything written before the flag existed — treat as not
    // maximized rather than discarding an otherwise valid geometry.
    return { width, height, x, y, maximized: maximized === true };
  } catch {
    // Corrupt JSON or a storage-access error reads exactly like "nothing
    // saved" — the OS places the window and the app carries on.
    return null;
  }
}

/// True when enough of `geometry` lands on some connected monitor to be
/// grabbable. Guards against restoring onto a display that is gone.
function isReachable(geometry: WindowGeometry, monitors: Monitor[]): boolean {
  return monitors.some((monitor) => {
    const left = monitor.position.x;
    const top = monitor.position.y;
    const right = left + monitor.size.width;
    const bottom = top + monitor.size.height;
    const overlapX = Math.min(geometry.x + geometry.width, right) - Math.max(geometry.x, left);
    const overlapY = Math.min(geometry.y + geometry.height, bottom) - Math.max(geometry.y, top);
    return overlapX >= MIN_VISIBLE_PX && overlapY >= MIN_VISIBLE_PX;
  });
}

/// First-run sizing, used only when nothing has been saved yet.
///
/// tauri.conf.json's 1024x720 is the wrong shape of default for this app: it is
/// a *fixed* number, and no fixed number is right for both a laptop panel and a
/// 5K display, where it lands at roughly 40% of the width and reads as a window
/// that failed to open properly. Five terminals want room. So the first launch
/// is sized to the monitor it actually opened on, and centred there; every
/// launch after that restores whatever the operator chose instead.
async function sizeToScreen(win: ReturnType<typeof getCurrentWindow>): Promise<void> {
  const monitors = await availableMonitors();
  if (monitors.length === 0) return;

  // Pick the monitor the window opened on rather than assuming the primary —
  // on a multi-display setup those are frequently not the same one.
  const origin = await win.outerPosition();
  const host =
    monitors.find((m) => {
      const left = m.position.x;
      const top = m.position.y;
      return (
        origin.x >= left &&
        origin.x < left + m.size.width &&
        origin.y >= top &&
        origin.y < top + m.size.height
      );
    }) ?? monitors[0];

  const width = Math.round(host.size.width * FIRST_RUN_SCREEN_FRACTION);
  const height = Math.round(host.size.height * FIRST_RUN_SCREEN_FRACTION);
  await win.setSize(new PhysicalSize(width, height));
  await win.setPosition(
    new PhysicalPosition(
      Math.round(host.position.x + (host.size.width - width) / 2),
      Math.round(host.position.y + (host.size.height - height) / 2),
    ),
  );
}

async function persistGeometry(win: ReturnType<typeof getCurrentWindow>): Promise<void> {
  try {
    // On macOS this is `NSWindow.isZoomed`, which compares the frame against
    // the screen's standard frame — a *geometry test*, not a state flag. So it
    // reports true for a window zoomed with the green button AND for one a
    // third-party window manager (Rectangle, Magnet, yabai) merely sized to
    // fill the screen. Those two cases are indistinguishable from here, and in
    // the second one filling the screen is exactly what the operator chose.
    //
    // An earlier version refused to save at all while this was true, to stop a
    // screen-sized frame overwriting the real one. That also meant a window
    // deliberately snapped to full screen was never remembered.
    const maximized = await win.isMaximized();
    const stored = parseGeometry(window.localStorage.getItem(STORAGE_KEY));

    // The flag is always current; the geometry only advances while the window
    // is not filling the screen, so the size the operator actually sized it to
    // survives a zoom. With nothing stored yet there is no earlier size to
    // protect, so take the current one even if it is screen-sized.
    let geometry = stored;
    if (!maximized || stored === null) {
      const size = await win.outerSize();
      const position = await win.outerPosition();
      if (size.width < MIN_PHYSICAL_PX || size.height < MIN_PHYSICAL_PX) return;
      geometry = {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized,
      };
    }
    if (geometry === null) return;
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...geometry, maximized }));
  } catch {
    /* best-effort: the window still works, it just will not be remembered */
  }
}

/// Restores the saved window geometry once on mount, then keeps it current.
/// Returns nothing — this hook is entirely a side effect on the OS window.
export function useWindowState(): void {
  // React 18 StrictMode double-invokes effects in dev; restoring twice is
  // harmless but re-applying geometry over a window the operator has already
  // moved is not, so the restore is latched.
  const hasRestored = useRef(false);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const unlisteners: UnlistenFn[] = [];

    const setup = async () => {
      const win = getCurrentWindow();

      if (!hasRestored.current) {
        hasRestored.current = true;
        const saved = parseGeometry(window.localStorage.getItem(STORAGE_KEY));
        try {
          if (saved && !cancelled) {
            await win.setSize(new PhysicalSize(saved.width, saved.height));
            const monitors = await availableMonitors();
            if (isReachable(saved, monitors)) {
              await win.setPosition(new PhysicalPosition(saved.x, saved.y));
            }
            // Re-zoom last, so the un-zoomed geometry above is what the window
            // returns to when the operator un-maximizes.
            if (saved.maximized) await win.maximize();
            // Unreachable: the display it was on is no longer connected. Size
            // is restored, placement deliberately left to the OS.
          } else if (!cancelled) {
            await sizeToScreen(win);
          }
        } catch {
          /* missing capability or a rejected call — keep the default window */
        }
      }

      const schedulePersist = () => {
        window.clearTimeout(timer);
        timer = window.setTimeout(() => void persistGeometry(win), PERSIST_DEBOUNCE_MS);
      };

      try {
        const stopResize = await win.onResized(schedulePersist);
        const stopMove = await win.onMoved(schedulePersist);
        if (cancelled) {
          stopResize();
          stopMove();
          return;
        }
        unlisteners.push(stopResize, stopMove);
      } catch {
        /* no geometry events available; the app simply will not remember */
      }
    };

    void setup();

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      unlisteners.forEach((stop) => stop());
    };
  }, []);
}
