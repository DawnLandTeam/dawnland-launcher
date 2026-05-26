<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Loader2, Download, Coffee } from "@lucide/vue";

interface SystemMemoryInfo {
  totalMb: number;
  recommendedMaxMb: number;
}

interface JavaInfo {
  path: string;
  majorVersion: number;
  versionString: string;
  vendor: string;
  is64Bit: boolean;
}

interface DownloadProgress {
  taskId: string;
  downloaded: number;
  total: number;
}

// Global settings
const systemMemory = ref<SystemMemoryInfo>({ totalMb: 8192, recommendedMaxMb: 4096 });
const defaultMaxMemory = ref(4096);

// Java management state
const installedJavas = ref<JavaInfo[]>([]);
const isScanningJava = ref(false);
const isDownloadingJava = ref(false);
const downloadingVersion = ref<number | null>(null);
const javaDownloadProgress = ref(0);

async function loadSystemMemory(): Promise<void> {
  try {
    systemMemory.value = await invoke<SystemMemoryInfo>("get_system_memory");
    defaultMaxMemory.value = systemMemory.value.recommendedMaxMb;
  } catch (err) {
    console.error("Failed to load system memory:", err);
  }
}

// Java management functions
async function scanLocalJavas(): Promise<void> {
  isScanningJava.value = true;
  try {
    installedJavas.value = await invoke<JavaInfo[]>("scan_local_javas");
  } catch (err) {
    console.error("Failed to scan Java installations:", err);
  } finally {
    isScanningJava.value = false;
  }
}

async function downloadJava(majorVersion: number): Promise<void> {
  isDownloadingJava.value = true;
  downloadingVersion.value = majorVersion;
  javaDownloadProgress.value = 0;

  try {
    // Listen for download progress
    const unlisten = await listen<DownloadProgress>("download-progress", (event) => {
      const payload = event.payload;
      if (payload.total > 0) {
        javaDownloadProgress.value = Math.round((payload.downloaded / payload.total) * 100);
      }
    });

    const javaInfo = await invoke<JavaInfo>("download_java", { majorVersion });
    installedJavas.value.unshift(javaInfo);
    
    unlisten();
  } catch (err) {
    console.error("Failed to download Java:", err);
    alert(`Failed to download Java ${majorVersion}: ${err}`);
  } finally {
    isDownloadingJava.value = false;
    downloadingVersion.value = null;
    javaDownloadProgress.value = 0;
  }
}

onMounted(() => {
  loadSystemMemory();
  scanLocalJavas();
});
</script>

<template>
  <div class="flex h-full flex-col p-6 gap-6 overflow-y-auto">
    <div>
      <h1 class="text-2xl font-bold">Settings</h1>
      <p class="text-sm text-neutral-500 mt-1">Manage application settings</p>
    </div>

    <!-- Global Memory Settings -->
    <div class="rounded-lg border border-neutral-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900">
      <h2 class="mb-4 text-lg font-semibold">Default Memory Settings</h2>
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">Default Max Memory</label>
          <span class="text-sm font-mono text-primary">{{ defaultMaxMemory }} MB</span>
        </div>
        <input
          v-model.number="defaultMaxMemory"
          type="range"
          min="512"
          :max="systemMemory.totalMb"
          step="512"
          class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-blue-500"
        />
        <div class="flex justify-between text-xs text-muted-foreground">
          <span>512 MB</span>
          <span>System: {{ systemMemory.totalMb }} MB</span>
        </div>
        <p class="text-xs text-muted-foreground">
          This will be used as the default memory for new instances. Recommended: {{ systemMemory.recommendedMaxMb }} MB (1/3 of system RAM)
        </p>
      </div>
    </div>

    <!-- Java Management -->
    <div class="rounded-lg border border-neutral-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">Java Management</h2>
        <button
          class="flex items-center gap-2 rounded-lg bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          :disabled="isScanningJava"
          @click="scanLocalJavas"
        >
          <Loader2 v-if="isScanningJava" :size="14" class="animate-spin" />
          <Coffee v-else :size="14" />
          {{ isScanningJava ? 'Scanning...' : 'Refresh' }}
        </button>
      </div>

      <!-- Download Java Section -->
      <div class="mb-4 p-3 bg-muted/30 rounded-lg">
        <p class="text-sm text-muted-foreground mb-3">Download Java from Adoptium:</p>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="version in [8, 11, 17, 21]"
            :key="version"
            class="flex items-center gap-1.5 px-3 py-1.5 text-sm border rounded-lg hover:bg-primary/10 hover:border-primary/50 transition-colors disabled:opacity-50"
            :disabled="isDownloadingJava"
            @click="downloadJava(version)"
          >
            <Download :size="14" />
            Java {{ version }}
          </button>
        </div>
        <!-- Download progress -->
        <div v-if="isDownloadingJava" class="mt-3">
          <div class="flex items-center justify-between text-sm mb-1">
            <span>Downloading Java {{ downloadingVersion }}...</span>
            <span>{{ javaDownloadProgress }}%</span>
          </div>
          <div class="w-full h-2 bg-gray-200 rounded-full dark:bg-zinc-700">
            <div
              class="h-full bg-blue-500 rounded-full transition-all"
              :style="{ width: `${javaDownloadProgress}%` }"
            ></div>
          </div>
        </div>
      </div>

      <!-- Installed Java List -->
      <div class="space-y-2">
        <p class="text-sm font-medium">Installed Java ({{ installedJavas.length }})</p>
        <div v-if="installedJavas.length === 0" class="text-sm text-muted-foreground py-2">
          No Java installations found. Click a version above to download.
        </div>
        <div
          v-for="java in installedJavas"
          :key="java.path"
          class="flex items-center justify-between p-3 border rounded-lg bg-muted/20"
        >
          <div class="flex items-center gap-3">
            <Coffee class="h-5 w-5 text-orange-500" />
            <div>
              <p class="text-sm font-medium">Java {{ java.majorVersion }} ({{ java.vendor }})</p>
              <p class="text-xs text-muted-foreground truncate max-w-xs">{{ java.path }}</p>
            </div>
          </div>
          <span class="text-xs text-muted-foreground">{{ java.versionString }}</span>
        </div>
      </div>
    </div>
  </div>
</template>