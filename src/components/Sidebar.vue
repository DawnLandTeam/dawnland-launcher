<script setup lang="ts">
import { RouterLink, useRoute } from "vue-router";
import { useDark, useToggle } from "@vueuse/core";
import { Gamepad2, Library, Download, Settings, Sun, Moon } from "@lucide/vue";

const route = useRoute();
const isDark = useDark();
const toggleDark = useToggle(isDark);

const navItems = [
  { name: "home", path: "/", label: "Home", icon: Gamepad2 },
  { name: "instances", path: "/instances", label: "Instances", icon: Library },
  { name: "downloads", path: "/downloads", label: "Downloads", icon: Download },
  { name: "settings", path: "/settings", label: "Settings", icon: Settings },
];
</script>

<template>
  <aside class="flex w-16 flex-col border-r border-neutral-200 bg-white dark:border-zinc-800 dark:bg-zinc-950">
    <nav class="flex flex-1 flex-col items-center gap-1 py-2">
      <RouterLink
        v-for="item in navItems"
        :key="item.name"
        :to="item.path"
        class="flex h-12 w-12 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-900 dark:text-zinc-500 dark:hover:bg-zinc-800 dark:hover:text-zinc-100"
        :class="{ 'bg-neutral-100 text-neutral-900 dark:bg-zinc-800 dark:text-zinc-100': route.path === item.path }"
        :title="item.label"
      >
        <component :is="item.icon" :size="20" />
      </RouterLink>
    </nav>

    <div class="border-t border-neutral-200 p-2 dark:border-zinc-800">
      <button
        class="flex h-10 w-10 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-900 dark:text-zinc-500 dark:hover:bg-zinc-800 dark:hover:text-zinc-100"
        :title="isDark ? 'Switch to light mode' : 'Switch to dark mode'"
        @click="toggleDark()"
      >
        <Moon v-if="isDark" :size="18" />
        <Sun v-else :size="18" />
      </button>
    </div>
  </aside>
</template>