import { createRouter, createWebHistory } from "vue-router";
import HomeView from "../views/HomeView.vue";
import InstancesView from "../views/InstancesView.vue";
import ServersView from "../views/ServersView.vue";
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
      path: "/settings",
      name: "settings",
      component: SettingsView,
    },
  ],
});

export default router;