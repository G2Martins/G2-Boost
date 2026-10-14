import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// Desktop app: no browser context menu / text drag outside inputs.
document.addEventListener("contextmenu", (e) => {
  const el = e.target as HTMLElement | null;
  if (!el?.closest("input, textarea")) e.preventDefault();
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
