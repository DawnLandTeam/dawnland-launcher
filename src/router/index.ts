import { createRouter, createWebHistory } from "vue-router";
import HomeView from "../views/HomeView.vue";
import InstancesView from "../views/InstancesView.vue";
import ServersView from "../views/ServersView.vue";
import AccountsView from "../views/AccountsView.vue";
import SettingsView from "../views/SettingsView.vue";

import UITestView from "../views/UITestView.vue";
import DownloadsView from "../views/DownloadsView.vue";
import InstanceManagementView from "../views/InstanceManagementView.vue";

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
      path: "/instances/:id",
      name: "instance-management",
      component: InstanceManagementView,
    },
    {
      path: "/servers",
      name: "servers",
      component: ServersView,
    },
    {
      path: "/accounts",
      name: "accounts",
      component: AccountsView,
    },
    {
      path: "/settings",
      name: "settings",
      component: SettingsView,
    },

    {
      path: "/downloads",
      name: "downloads",
      component: DownloadsView,
    },
    {
      path: "/ui-test",
      name: "ui-test",
      component: UITestView,
    },
  ],
});

export default router;