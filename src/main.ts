import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import i18n from "./i18n";
import { safeHtml } from "./directives/safeHtml";
import "./style.css";

// Disable right-click context menu in production
if (import.meta.env.PROD) {
  document.addEventListener("contextmenu", (e) => e.preventDefault());
}

const app = createApp(App);

app.use(router);
app.use(i18n);
app.directive('safe-html', safeHtml);
app.mount("#app");