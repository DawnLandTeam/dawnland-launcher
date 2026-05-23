<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Download, Loader2, Check, AlertCircle, RefreshCw } from "@lucide/vue";
import {
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "./ui/dialog";

// Types
interface VanillaVersion {
  id: string;
  versionType: string;
  url: string;
}

interface InstallProgress {
  phase: string;
  versionId?: string;
  totalTasks?: number;
  completedTasks?: number;
  currentFile?: string;
  errors?: number;
}

interface DownloadProgress {
  taskId: string;
  downloaded: number;
  total: number;
  speed: number;
  completed: boolean;
  error?: string;
}

// Props
const open = defineModel<boolean>("open", { required: true });

// State
const versions = ref<VanillaVersion[]>([]);
const selectedVersion = ref<string>("");
const isLoadingVersions = ref(false);
const isInstalling = ref(false);
const installProgress = ref<InstallProgress | null>(null);
const downloadProgress = ref<Map<string, DownloadProgress>>(new Map());
const error = ref<string | null>(null);

// Event unlisteners
const unlisteners: UnlistenFn[] = [];

// Computed
const releaseVersions = computed(() =>
  versions.value.filter((v) => v.versionType === "release")
);

const snapshotVersions = computed(() =>
  versions.value.filter((v) => v.versionType === "snapshot")
);

const sortedVersions = computed(() =>
  [...versions.value].sort((a, b) => {
    const order: Record<string, number> = {
      release: 0,
      snapshot: 1,
      old_beta: 2,
      old_alpha: 3,
    };
    const orderA = order[a.versionType] ?? 4;
    const orderB = order[b.versionType] ?? 4;
    if (orderA !== orderB) return orderA - orderB;
    return b.id.localeCompare(a.id);
  })
);

const downloadProgressPercent = computed(() => {
  if (!installProgress.value?.totalTasks) return 0;
  const completedTasks = installProgress.value.completedTasks ?? 0;
  const totalTasks = installProgress.value.totalTasks;
  // 用 Math.floor 避免四舍五入导致提前显示 100%
  // 只有当真正完成所有文件时才显示 100%
  const percent = Math.floor((completedTasks / totalTasks) * 100);
  // 如果不是 100%，确保至少有 1% 显示（用户体验）
  if (percent === 0 && completedTasks > 0) return 1;
  return percent;
});

// When dialog opens, load versions if not yet loaded
watch(open, (isOpen) => {
  if (isOpen && versions.value.length === 0) {
    loadVersions();
  }
});

// Load version list from backend
async function loadVersions(): Promise<void> {
  isLoadingVersions.value = true;
  error.value = null;

  try {
    versions.value = await invoke<VanillaVersion[]>("get_vanilla_versions");

    const latestRelease = releaseVersions.value[0];
    if (latestRelease) {
      selectedVersion.value = latestRelease.id;
    }
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    console.error("Failed to load versions:", err);
  } finally {
    isLoadingVersions.value = false;
  }
}

// Install selected version
async function installVersion(): Promise<void> {
  if (!selectedVersion.value) {
    error.value = "Please select a version";
    return;
  }

  const version = versions.value.find((v) => v.id === selectedVersion.value);
  if (!version) {
    error.value = "Version not found";
    return;
  }

  isInstalling.value = true;
  error.value = null;
  installProgress.value = { phase: "resolving_version" };
  downloadProgress.value.clear();

  try {
    await invoke("install_vanilla_version", {
      versionId: selectedVersion.value,
      versionJsonUrl: version.url,
    });
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    isInstalling.value = false;
  }
}

// Format phase label
function formatPhase(phase: string): string {
  const labels: Record<string, string> = {
    resolving_version: "Fetching version metadata...",
    resolving_libraries: "Filtering libraries for your system...",
    resolving_assets: "Preparing game assets...",
    downloading: "Downloading files...",
    complete: "Installation complete!",
    error: "Installation failed",
  };
  return labels[phase] || phase;
}

// Format speed for display
function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
}

// Register event listeners once on mount
onMounted(async () => {
  const un1 = await listen<InstallProgress>("install-progress", (event) => {
    installProgress.value = event.payload;

    if (event.payload.phase === "complete" || event.payload.phase === "error") {
      isInstalling.value = false;
    }
  });

const un2 = await listen<DownloadProgress>("download-progress", (event) => {
    const progress = event.payload;

    // 手动跟踪下载进度：因为 Rust 没有发送 install-progress 事件
    // 只在 totalTasks 已经设置后才计数（避免初始化时的错误计数）
    if (progress.completed && installProgress.value && installProgress.value.totalTasks) {
      const current = installProgress.value.completedTasks || 0;
      installProgress.value = {
        ...installProgress.value,
        completedTasks: current + 1,
        phase: "downloading"
      };
    }

    if (progress.completed) {
      downloadProgress.value.delete(progress.taskId);
    } else {
      downloadProgress.value.set(progress.taskId, progress);
    }
  });

  const un3 = await listen("download-batch-complete", () => {
    // Batch complete — can be used for post-processing later
  });

  unlisteners.push(un1, un2, un3);
});

onUnmounted(() => {
  unlisteners.forEach((un) => un());
});
</script>

<template>
  <DialogContent v-model:open="open" class="max-w-2xl">
    <DialogTitle>Install New Instance</DialogTitle>
    <DialogDescription>
      Select a Minecraft version to download and install.
    </DialogDescription>

      <!-- Version Selector -->
      <div class="space-y-3 pt-2">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">Minecraft Version</label>
          <button
            @click="loadVersions"
            :disabled="isLoadingVersions"
            class="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
          >
            <RefreshCw
              v-if="isLoadingVersions"
              class="w-3.5 h-3.5 animate-spin"
            />
            <RefreshCw v-else class="w-3.5 h-3.5" />
            Refresh
          </button>
        </div>

        <select
          v-model="selectedVersion"
          :disabled="isLoadingVersions || isInstalling"
          class="w-full px-3 py-2 bg-background border rounded-md text-sm disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring"
        >
          <option value="" disabled>Select a version...</option>

          <optgroup v-if="releaseVersions.length" label="Releases">
            <option
              v-for="v in releaseVersions"
              :key="v.id"
              :value="v.id"
            >
              {{ v.id }}
            </option>
          </optgroup>

          <optgroup v-if="snapshotVersions.length" label="Snapshots">
            <option
              v-for="v in snapshotVersions"
              :key="v.id"
              :value="v.id"
            >
              {{ v.id }}
            </option>
          </optgroup>

          <optgroup
            v-if="
              versions.length >
              releaseVersions.length + snapshotVersions.length
            "
            label="Other"
          >
            <option
              v-for="v in sortedVersions.filter(
                (v) =>
                  !['release', 'snapshot'].includes(v.versionType)
              )"
              :key="v.id"
              :value="v.id"
            >
              {{ v.id }} ({{ v.versionType }})
            </option>
          </optgroup>
        </select>
      </div>

      <!-- Install Button -->
      <div class="flex items-center gap-3">
        <button
          @click="installVersion"
          :disabled="!selectedVersion || isLoadingVersions || isInstalling"
          class="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
        >
          <Download class="w-4 h-4" />
          <span v-if="isInstalling">Installing...</span>
          <span v-else>Install Version</span>
        </button>

        <span
          v-if="isInstalling && !installProgress?.totalTasks"
          class="text-sm text-muted-foreground animate-pulse"
        >
          Preparing download queue...
        </span>
      </div>

      <!-- Installation Progress -->
      <div
        v-if="installProgress"
        class="rounded-lg border bg-muted/30 p-4 space-y-3"
      >
        <!-- Phase indicator -->
        <div class="flex items-center gap-2 text-sm">
          <Loader2
            v-if="
              !['complete', 'error'].includes(installProgress.phase)
            "
            class="w-4 h-4 animate-spin text-primary"
          />
          <Check
            v-else-if="installProgress.phase === 'complete'"
            class="w-4 h-4 text-green-500"
          />
          <AlertCircle v-else class="w-4 h-4 text-red-500" />
          <span>{{ formatPhase(installProgress.phase) }}</span>
        </div>

        <!-- Progress bar -->
        <div v-if="installProgress.totalTasks" class="space-y-1.5">
          <div class="flex justify-between text-xs text-muted-foreground">
            <span>
              {{ installProgress.completedTasks || 0 }} /
              {{ installProgress.totalTasks }} files
            </span>
            <span>{{ downloadProgressPercent }}%</span>
          </div>
          <div class="h-2 bg-muted rounded-full overflow-hidden">
            <div
              class="h-full bg-primary transition-all duration-300 rounded-full"
              :style="{ width: `${downloadProgressPercent}%` }"
            />
          </div>
        </div>

        <!-- Current file -->
        <div
          v-if="installProgress.currentFile"
          class="text-xs text-muted-foreground truncate"
        >
          Current: {{ installProgress.currentFile }}
        </div>

        <!-- Error -->
        <div
          v-if="error"
          class="flex items-center gap-2 text-red-500 text-sm"
        >
          <AlertCircle class="w-4 h-4 shrink-0" />
          <span>{{ error }}</span>
        </div>

        <!-- Success -->
        <div
          v-if="installProgress.phase === 'complete'"
          class="flex items-center gap-2 text-green-500 text-sm"
        >
          <Check class="w-4 h-4 shrink-0" />
          <span
            >Version {{ installProgress.versionId }} installed
            successfully!</span
          >
        </div>
      </div>

      <!-- Active Downloads -->
      <div
        v-if="downloadProgress.size > 0"
        class="rounded-lg border bg-muted/30 p-3 space-y-2"
      >
        <h4 class="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
          Active Downloads
        </h4>
        <div class="space-y-1.5 max-h-32 overflow-y-auto">
          <div
            v-for="[taskId, progress] of downloadProgress"
            :key="taskId"
            class="flex items-center gap-2 text-xs"
          >
            <div class="flex-1 min-w-0">
              <div class="truncate text-muted-foreground">
                {{ progress.taskId }}
              </div>
              <div class="h-1 bg-muted rounded-full overflow-hidden mt-0.5">
                <div
                  class="h-full bg-primary/70 rounded-full transition-all duration-200"
                  :style="{
                    width: `${progress.total > 0 ? (progress.downloaded / progress.total) * 100 : 0}%`,
                  }"
                />
              </div>
            </div>
            <span class="text-muted-foreground tabular-nums whitespace-nowrap">
              {{ formatSpeed(progress.speed) }}
            </span>
          </div>
        </div>
      </div>
    </DialogContent>
</template>
