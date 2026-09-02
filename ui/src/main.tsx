import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ErrorBoundary } from "./dev/ErrorBoundary";
import { installDevErrorReporting } from "./dev/devLog";
import "./styles.css";

// Async throws and rejected promises never reach a React boundary. Installed
// before the first render so a failure during mount is still reported.
installDevErrorReporting();

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

// **One window, one root, no branch** (D-073). WP-15 read a query parameter
// here and mounted a second root for the evaluator's own window, on the grounds
// that a second `App` would be a second webview racing the first for one fleet.
// That was a true thing about the *root* and never an argument for the window:
// the evaluator is now a view inside this one `App`, which races nothing because
// there is nothing to race.
createRoot(root).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
