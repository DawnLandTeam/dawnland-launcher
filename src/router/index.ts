import { createRouter, createWebHistory } from "vue-router";
import HomeView from "../views/HomeView.vue";
import InstancesView from "../views/InstancesView.vue";
import ServersView from "../views/ServersView.vue";
import DownloadsView from "../views/DownloadsView.vue";
import SettingsView from "../views/SettingsView.vue";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: "/",
      name: "home",
      component: HomeView,
    },
    {
      path: "/instances",
      name: "instances",
      component: InstancesView,
    },
    {
      path: "/servers",
      name: "servers",
      component: ServersView,
    },
    {
      path: "/downloads",
      name: "downloads",
      component: DownloadsView,
    },
    {
      path: "/settings",
      name: "settings",
      component: SettingsView,
    },
  ],
});

export default router;