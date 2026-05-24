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

// Start dragging using Tauri API - dual insurance with data-tauri-drag-region
async function startDrag(e: MouseEvent): Promise<void> {
  console.log("Mouse down detected on Header!"); // Debug log
  if (e.button === 0) {
    try {
      await appWindow.startDragging();
    } catch (error) {
      console.error("Drag error:", error);
    }
  }
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
    class="region-drag flex h-8 shrink-0 select-none items-center justify-between px-3 w-full"
    style="background-color: #171717; position: relative; z-index: 99999;"
    data-tauri-drag-region
    @mousedown="startDrag"
  >
    <!-- Left: App title - prevent events to allow drag on parent -->
    <div
      class="text-xs font-medium pointer-events-none select-none"
      style="color: #a3a3a3;"
    >
      Dawnland Launcher
    </div>

    <!-- Right: Window controls - enable pointer events for button clicks -->
    <div class="flex items-center pointer-events-auto">
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
