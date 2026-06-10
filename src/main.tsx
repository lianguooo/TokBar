import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { QuickPanel } from "./pages/QuickPanel";
import { I18nProvider } from "./lib/i18n";
import { ThemeProvider } from "./lib/theme";
import "./index.css";

const isQuickPanel = getCurrentWebviewWindow().label === "quick";

if (isQuickPanel) {
  document.documentElement.classList.add("quick-panel");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <I18nProvider>{isQuickPanel ? <QuickPanel /> : <App />}</I18nProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
