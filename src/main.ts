import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import i18n from "./i18n";
import { safeHtml } from "./directives/safeHtml";
import "./style.css";

if (import.meta.env.PROD) {
  // Disable right-click context menu globally
  document.addEventListener("contextmenu", (e) => e.preventDefault());

  // Disable common webview shortcuts (DevTools, Search, Print, Reload, etc.)
  document.addEventListener("keydown", (e) => {
    if (e.key === "F12" || e.key === "F5") {
      e.preventDefault();
      return;
    }
    
    if (e.ctrlKey) {
      const key = e.key.toLowerCase();
      if (["r", "f", "p", "u", "s", "g", "+", "-", "0", "="].includes(key)) {
        e.preventDefault();
        return;
      }
      if (e.shiftKey && ["i", "j", "c"].includes(key)) {
        e.preventDefault();
        return;
      }
    }

    if (e.altKey && e.shiftKey && e.key.toLowerCase() === "p") {
      e.preventDefault();
      return;
    }
  });
}

import { trackEvent } from "./utils/analytics";

const app = createApp(App);

app.use(router);
app.use(i18n);
app.directive('safe-html', safeHtml);
app.mount("#app");

trackEvent("App Started").catch(console.error);