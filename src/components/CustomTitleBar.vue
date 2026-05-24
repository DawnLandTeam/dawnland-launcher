<script setup lang="ts">
import { ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X, Copy } from "@lucide/vue";

const appWindow = getCurrentWindow();
const isMaximized = ref(false);

async function checkMaximized(): Promise<void> {
  isMaximized.value = await appWindow.isMaximized();
}

async function minimize(): Promise<void> {
  await appWindow.minimize();
}

async function toggleMaximize(): Promise<void> {
  await appWindow.toggleMaximize();
  await checkMaximized();
}

async function close(): Promise<void> {
  await appWindow.close();
}

// Listen for resize events to update maximized state
appWindow.onResized(async () => {
  await checkMaximized();
});

// Initialize state
checkMaximized();
</script>

<template>
  <header
    class="flex h-8 shrink-0 select-none items-center justify-between px-3"
    style="background-color: #171717; position: relative; z-index: 9999;"
    data-tauri-drag-region
  >
    <!-- Left: App title -->
    <div
      class="text-xs font-medium"
      style="color: #a3a3a3;"
    >
      Dawnland Launcher
    </div>

    <!-- Right: Window controls (no drag region to prevent interference) -->
    <div class="flex items-center">
      <button
        class="inline-flex h-8 w-10 items-center justify-center transition-colors hover:bg-neutral-700 hover:text-neutral-200"
        style="color: #a3a3a3;"
        @click="minimize"
      >
        <Minus :size="14" />
      </button>
      <button
        class="inline-flex h-8 w-10 items-center justify-center transition-colors hover:bg-neutral-700 hover:text-neutral-200"
        style="color: #a3a3a3;"
        @click="toggleMaximize"
      >
        <Copy v-if="isMaximized" :size="12" />
        <Square v-else :size="12" />
      </button>
      <button
        class="inline-flex h-8 w-10 items-center justify-center transition-colors hover:bg-red-600 hover:text-white"
        style="color: #a3a3a3;"
        @click="close"
      >
        <X :size="14" />
      </button>
    </div>
  </header>
</template>
