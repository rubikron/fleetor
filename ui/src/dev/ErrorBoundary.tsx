// The one thing that has to stay on screen.
//
// React 18 unmounts the ENTIRE root when a render throws and nothing catches
// it. The result is a blank window with no DOM and no explanation, which is
// indistinguishable from "the app never started" — an ambiguity that cost a
// full debugging session to resolve. This boundary exists so those two states
// never look alike again.
//
// It is deliberately almost nothing: one line saying the shell failed, and
// where to look. The detail goes to the terminal via devLog (dev), or the
// console (production, where there is no dev server to post to). An elaborate
// on-screen dump was the previous design and it was the wrong place for it.
//
// Inline styles on purpose — if the failure is CSS, a fallback that depends on
// styles.css is hidden by the very thing it is reporting.

import { Component, type ErrorInfo, type ReactNode } from "react";
import { devLog } from "./devLog";

interface State {
  failed: boolean;
}

export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    const detail = `${error.stack ?? error.message}\n\ncomponent stack:${info.componentStack ?? " (none)"}`;
    devLog("error", `render threw: ${error.message}`, detail);
    if (!import.meta.env.DEV) {
      // No dev server to post to; the console is all there is.
      console.error("[fleetor] render threw:", error, info.componentStack);
    }
  }

  render(): ReactNode {
    if (!this.state.failed) return this.props.children;
    return (
      <div
        style={{
          padding: "24px",
          color: "#e8e6e1",
          background: "#1f1e1b",
          font: "13px ui-monospace, Menlo, monospace",
          height: "100%",
        }}
      >
        <span style={{ color: "#d97757" }}>the shell failed to render</span>
        {" — details in the terminal running the dev server"}
      </div>
    );
  }
}
