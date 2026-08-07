import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { EvaluatorWindow } from "./EvaluatorWindow";
import { ErrorBoundary } from "./dev/ErrorBoundary";
import { installDevErrorReporting } from "./dev/devLog";
import "./styles.css";

// Async throws and rejected promises never reach a React boundary. Installed
// before the first render so a failure during mount is still reported.
installDevErrorReporting();

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

// Which window this is (WP-15). One bundle, two roots: the Rust side creates the
// second window pointed at `index.html?window=evaluator`, and the branch happens
// **here**, before either root mounts, so the evaluator's window never runs
// `fleet_bootstrap`, never subscribes to the event feed and never renders a
// start gate. A second `App` would be a second webview racing the first for one
// fleet.
//
// A query parameter rather than a second Vite entry point: the two roots share
// the stylesheet, the error boundary and `TerminalPane`, and a separate HTML
// entry would buy a build-config split for a difference that is one conditional.
const isEvaluatorWindow = new URLSearchParams(window.location.search).get("window") === "evaluator";

createRoot(root).render(
  <StrictMode>
    <ErrorBoundary>{isEvaluatorWindow ? <EvaluatorWindow /> : <App />}</ErrorBoundary>
  </StrictMode>,
);
