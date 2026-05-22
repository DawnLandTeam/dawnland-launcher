<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import CustomTitleBar from "./components/CustomTitleBar.vue";

const systemInfo = ref<string | null>(null);
const isLoading = ref(false);
const error = ref<string | null>(null);

async function fetchSystemInfo(): Promise<void> {
  isLoading.value = true;
  error.value = null;
  systemInfo.value = null;
  try {
    systemInfo.value = await invoke<string>("get_system_info");
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
  } finally {
    isLoading.value = false;
  }
}
</script>

<template>
  <div class="flex h-full flex-col bg-neutral-950 text-neutral-100">
    <CustomTitleBar />

    <main class="flex flex-1 flex-col items-center justify-center gap-6 px-8">
      <h1 class="text-3xl font-bold tracking-tight text-white">
        Dawnland Launcher
      </h1>
      <p class="text-sm text-neutral-500">Phase 1 — Skeleton &amp; IPC Test</p>

      <button
        class="rounded-lg bg-indigo-600 px-5 py-2.5 text-sm font-medium text-white transition-colors hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="isLoading"
        @click="fetchSystemInfo"
      >
        {{ isLoading ? "Loading..." : "Get System Info" }}
      </button>

      <div v-if="systemInfo" class="rounded-lg bg-neutral-800 px-6 py-3">
        <p class="text-sm text-emerald-400">{{ systemInfo }}</p>
      </div>

      <div v-if="error" class="rounded-lg bg-red-900/40 px-6 py-3">
        <p class="text-sm text-red-400">{{ error }}</p>
      </div>
    </main>
  </div>
</template>
