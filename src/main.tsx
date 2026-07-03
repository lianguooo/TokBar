import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { QuickPanel } from "./pages/QuickPanel";
import { I18nProvider } from "./lib/i18n";
import { ThemeProvider } from "./lib/theme";
import { SubscriptionsProvider } from "./lib/subscriptions";
import "./index.css";

const isQuickPanel = getCurrentWebviewWindow().label === "quick";
const isMac = navigator.userAgent.includes("Mac");

// macOS gets the vibrancy material treatment (transparent windows +
// translucent surface washes, see index.css); other platforms keep the
// solid palette.
if (isMac) {
  document.documentElement.classList.add("mac");
}

if (isQuickPanel) {
  document.documentElement.classList.add("quick-panel");
  // The quick window is only transparent on macOS (see setup_tray); on
  // Windows/Linux it is opaque, so the page must paint its own background.
  if (!isMac) {
    document.documentElement.classList.add("quick-panel-opaque");
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <I18nProvider>
        <SubscriptionsProvider>
          {isQuickPanel ? <QuickPanel /> : <App />}
        </SubscriptionsProvider>
      </I18nProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
