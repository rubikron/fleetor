import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import { warmTheme } from "./theme";

// Raw pty bytes arrive base64-encoded so escape sequences and multibyte UTF-8
// never split across a chunk boundary. Decode to bytes; xterm handles the
// incremental UTF-8 decode across writes.
function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

async function main(): Promise<void> {
  const term = new Terminal({
    theme: warmTheme,
    fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
    fontSize: 13,
    lineHeight: 1.2,
    cursorBlink: true,
    scrollback: 10000,
    allowProposedApi: true,
    macOptionIsMeta: true,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);

  const host = document.getElementById("terminal")!;
  term.open(host);
  fit.fit();
  term.focus();

  // pty output -> screen.
  await listen<string>("pty://output", (e) => {
    term.write(decodeBase64(e.payload));
  });
  await listen("pty://exit", () => {
    term.write("\r\n\x1b[38;2;138;134;124m[claude exited — close the window]\x1b[0m\r\n");
  });

  // keystrokes / paste -> pty.
  term.onData((data) => {
    void invoke("pty_write", { data });
  });

  // fitted grid changes -> pty size.
  term.onResize(({ rows, cols }) => {
    void invoke("pty_resize", { rows, cols });
  });

  // Refit on any container size change (window resize, devtools, etc.).
  const refit = () => {
    try {
      fit.fit();
    } catch {
      /* terminal not yet measurable; ignore */
    }
  };
  new ResizeObserver(refit).observe(host);
  window.addEventListener("resize", refit);

  // Spawn with the initial fitted size.
  await invoke("pty_spawn", { rows: term.rows, cols: term.cols });
}

void main();
