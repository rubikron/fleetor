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

createRoot(root).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
