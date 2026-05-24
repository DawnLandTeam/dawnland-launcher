<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

interface DownloadTask {
  id: string;
  url: string;
  destPath: string;
  hash?: string;
}

interface DownloadProgress {
  taskId: string;
  downloaded: number;
  total: number;
  speed: number;
  completed: boolean;
  error?: string;
}

interface TaskDisplay {
  id: string;
  url: string;
  progress: number;
  downloaded: number;
  total: number;
  speed: string;
  completed: boolean;
  error?: string;
}

const tasks = ref<TaskDisplay[]>([]);
const isDownloading = ref(false);
let unlistenProgress: UnlistenFn | null = null;

// Helper: format bytes to human readable
function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

// Helper: format speed to human readable
function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec === 0) return "0 B/s";
  return formatBytes(bytesPerSec) + "/s";
}

// Format URL to show filename only
function getFileName(url: string): string {
  try {
    const urlObj = new URL(url);
    const path = urlObj.pathname;
    return path.split("/").pop() || "unknown";
  } catch {
    return url.split("/").pop() || "unknown";
  }
}

// Start test download
async function startTestDownload(): Promise<void> {
  if (isDownloading.value) return;

  // Get temp directory for downloads
  const tempDir = "C:\\Users\\MaoZa\\AppData\\Local\\Temp\\dawnland-test";

  // Create 3 test download tasks using reliable public URLs
  const testTasks: DownloadTask[] = [
    {
      id: crypto.randomUUID(),
      url: "https://nodejs.org/dist/v22.16.0/node-v22.16.0-x64.msi",
      destPath: `${tempDir}\\node-v22.16.0-x64.msi`,
    },
    {
      id: crypto.randomUUID(),
      url: "https://aka.ms/vs/17/release/vs_community.exe",
      destPath: `${tempDir}\\vs_community.exe`,
    },
    {
      id: crypto.randomUUID(),
      url: "https://github.com/tauri-apps/tauri/releases/download/v2.11.2/tauri-v2.11.2-x64-setup.exe",
      destPath: `${tempDir}\\tauri-setup.exe`,
    },
  ];

  // Initialize task display state
  tasks.value = testTasks.map((t) => ({
    id: t.id,
    url: getFileName(t.url),
    progress: 0,
    downloaded: 0,
    total: 0,
    speed: "0 B/s",
    completed: false,
  }));

  isDownloading.value = true;

  try {
    await invoke("batch_download", { tasks: testTasks });
  } catch (err) {
    console.error("Failed to start download:", err);
    isDownloading.value = false;
  }
}

// Handle progress events
function handleProgress(payload: DownloadProgress): void {
  const task = tasks.value.find((t) => t.id === payload.taskId);
  if (!task) return;

  task.downloaded = payload.downloaded;
  task.total = payload.total;
  task.speed = formatSpeed(payload.speed);

  if (payload.total > 0) {
    task.progress = Math.round((payload.downloaded / payload.total) * 100);
  }

  if (payload.completed) {
    task.completed = true;
    task.progress = 100;
    if (payload.error) {
      task.error = payload.error;
    }

    // Check if all tasks completed
    if (tasks.value.every((t) => t.completed)) {
      isDownloading.value = false;
    }
  }
}

onMounted(async () => {
  unlistenProgress = await listen<DownloadProgress>("download-progress", (event) => {
    handleProgress(event.payload);
  });
});

onUnmounted(() => {
  if (unlistenProgress) {
    unlistenProgress();
  }
});
</script>

<template>
  <div class="flex h-full flex-col gap-4">
    <h1 class="text-2xl font-bold">Downloads</h1>
    <p class="text-sm text-neutral-500">Phase 3 — Download Engine Test</p>

    <button
      class="w-fit rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
      :disabled="isDownloading"
      @click="startTestDownload"
    >
      {{ isDownloading ? "Downloading..." : "Start Test Download" }}
    </button>

    <div v-if="tasks.length > 0" class="flex flex-1 flex-col gap-3 overflow-y-auto">
      <div
        v-for="task in tasks"
        :key="task.id"
        class="rounded-lg border border-neutral-200 bg-white p-4 dark:border-zinc-800 dark:bg-zinc-900"
      >
        <div class="mb-2 flex items-center justify-between">
          <span class="font-medium text-neutral-900 dark:text-white">
            {{ task.url }}
          </span>
          <span
            v-if="task.completed"
            class="rounded bg-emerald-100 px-2 py-0.5 text-xs font-medium text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400"
          >
            {{ task.error ? "Failed" : "Completed" }}
          </span>
        </div>

        <div class="mb-2 h-2 w-full overflow-hidden rounded-full bg-neutral-200 dark:bg-zinc-800">
          <div
            class="h-full bg-indigo-600 transition-all duration-300"
            :style="{ width: `${task.progress}%` }"
          />
        </div>

        <div class="flex items-center justify-between text-xs text-neutral-500">
          <span>{{ formatBytes(task.downloaded) }} / {{ formatBytes(task.total) }}</span>
          <span>{{ task.speed }}</span>
        </div>

        <div v-if="task.error" class="mt-2 text-xs text-red-500">
          {{ task.error }}
        </div>
      </div>
    </div>

    <div v-else class="flex flex-1 items-center justify-center text-neutral-400">
      Click the button above to start a test download
    </div>
  </div>
</template>