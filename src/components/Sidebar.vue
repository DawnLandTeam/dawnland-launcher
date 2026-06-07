<script setup lang="ts">
import { computed } from "vue";
import { RouterLink, useRoute } from "vue-router";
import { useDark, useToggle } from "@vueuse/core";
import { Gamepad2, Library, Server, Users, Settings, Sun, Moon, DownloadCloud } from "@lucide/vue";
import { hasUpdateAvailable } from "../composables/useUpdate";
import { isAppBusy } from "../composables/useAppStatus";
import { useTaskStore } from "../composables/useTaskStore";
import { useI18n } from "vue-i18n";

const route = useRoute();
const isDark = useDark();
const toggleDark = useToggle(isDark);
const { t } = useI18n();
const taskStore = useTaskStore();

const navItems = computed(() => [
  { name: "home", path: "/", label: t("sidebar.home"), icon: Gamepad2 },
  { name: "instances", path: "/instances", label: t("sidebar.instances"), icon: Library },
  { name: "servers", path: "/servers", label: t("sidebar.servers"), icon: Server },
  { name: "accounts", path: "/accounts", label: t("sidebar.accounts"), icon: Users },
  { name: "settings", path: "/settings", label: t("sidebar.settings"), icon: Settings },
]);
</script>

<template>
  <aside class="flex w-16 flex-col border-r border-white/20 bg-white/10 dark:border-white/10 dark:bg-black/20 backdrop-blur-md">
    <nav class="flex flex-1 flex-col items-center gap-1 py-2">
      <RouterLink
        v-for="item in navItems"
        :key="item.name"
        :to="isAppBusy ? '' : item.path"
        class="relative flex h-12 w-12 items-center justify-center rounded-lg text-neutral-800 transition-colors"
        :class="[
          route.path === item.path ? 'bg-black/10 text-black dark:bg-white/10 dark:text-white' : '',
          isAppBusy && route.path !== item.path ? 'opacity-50 cursor-not-allowed pointer-events-none' : 'hover:bg-black/10 hover:text-black dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-white'
        ]"
        :title="item.label"
      >
        <component :is="item.icon" :size="20" />
        <span v-if="item.name === 'settings' && hasUpdateAvailable" class="absolute top-2.5 right-2.5 flex h-2 w-2">
          <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-red-400 opacity-75"></span>
          <span class="relative inline-flex rounded-full h-2 w-2 bg-red-500"></span>
        </span>
      </RouterLink>
    </nav>

    <div class="flex flex-col items-center gap-1 py-2">
      <button
        class="task-center-toggle relative flex h-10 w-10 items-center justify-center rounded-lg transition-colors"
        :class="[
          taskStore.isTaskCenterOpen.value ? 'bg-emerald-500/20 text-emerald-500' : 'text-neutral-800 hover:bg-black/10 hover:text-black dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-white'
        ]"
        title="Task Center"
        @click="taskStore.toggleTaskCenter()"
      >
        <DownloadCloud :size="20" />
        <span 
          v-if="taskStore.hasActiveTasks.value" 
          class="absolute top-2 right-2 flex h-2 w-2"
        >
          <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
          <span class="relative inline-flex rounded-full h-2 w-2 bg-emerald-500"></span>
        </span>
      </button>
    </div>

    <div class="border-t border-white/20 p-2 dark:border-white/10">
      <button
        class="flex h-10 w-10 items-center justify-center rounded-lg text-neutral-800 transition-colors hover:bg-black/10 hover:text-black dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-white"
        :title="isDark ? 'Switch to light mode' : 'Switch to dark mode'"
        @click="toggleDark()"
      >
        <Moon v-if="isDark" :size="18" />
        <Sun v-else :size="18" />
      </button>
    </div>
  </aside>
</template>