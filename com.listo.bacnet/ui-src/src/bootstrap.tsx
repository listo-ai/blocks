import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./main.css";
import Panel from "./Panel";

// Standalone dev harness — only runs when the block is opened directly
// (e.g. `pnpm dev`). Studio loads Panel via MF, not this entry.
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Panel />
    </StrictMode>,
  );
}
