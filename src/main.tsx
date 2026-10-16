import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { SettingsView } from "./SettingsView";
import "./styles.css";

// The settings window loads index.html#settings — render the settings view
// there; the main widget window renders the normal App.
const isSettings = window.location.hash.replace(/^#/, "") === "settings";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isSettings ? <SettingsView /> : <App />}</React.StrictMode>
);
