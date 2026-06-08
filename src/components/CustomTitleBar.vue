<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, X, Bell } from "@lucide/vue";
import { notificationStore } from "../composables/useNotificationStore";

const appWindow = getCurrentWindow();

async function minimize(): Promise<void> {
  await appWindow.minimize();
}

async function close(): Promise<void> {
  await appWindow.close();
}

let isMouseDown = false;

function onMouseDown(e: MouseEvent) {
  if (e.button === 0) {
    isMouseDown = true;
  }
}

function onMouseUp() {
  isMouseDown = false;
}

function onMouseLeave() {
  isMouseDown = false;
}

async function onMouseMove(): Promise<void> {
  if (isMouseDown) {
    isMouseDown = false; // Prevent multiple calls
    try {
      await appWindow.startDragging();
    } catch (error) {
      console.error("Drag error:", error);
    }
  }
}
</script>

<template>
  <header
    class="region-drag flex h-8 shrink-0 select-none items-center justify-between px-3 w-full bg-white/10 dark:bg-black/20 backdrop-blur-md border-b border-white/10"
    style="position: relative; z-index: 99999;"
    @mousedown="onMouseDown"
    @mouseup="onMouseUp"
    @mouseleave="onMouseLeave"
    @mousemove="onMouseMove"
    @dblclick.prevent
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
        class="notification-toggle relative inline-flex h-8 w-10 items-center justify-center transition-colors hover:bg-neutral-700 hover:text-neutral-200"
        :class="notificationStore.isCenterOpen.value ? 'bg-neutral-700 text-neutral-200' : ''"
        style="color: #a3a3a3;"
        @click="notificationStore.toggleCenter()"
      >
        <Bell :size="14" />
        <span 
          v-if="notificationStore.unreadCount.value > 0" 
          class="absolute top-2 right-2.5 flex h-1.5 w-1.5"
        >
          <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-red-400 opacity-75"></span>
          <span class="relative inline-flex rounded-full h-1.5 w-1.5 bg-red-500"></span>
        </span>
      </button>
      <button
        class="inline-flex h-8 w-10 items-center justify-center transition-colors hover:bg-neutral-700 hover:text-neutral-200"
        style="color: #a3a3a3;"
        @click="minimize"
      >
        <Minus :size="14" />
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
