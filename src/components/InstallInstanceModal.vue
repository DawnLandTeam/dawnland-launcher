<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Download, Loader2, Check, AlertCircle, RefreshCw, Box, Puzzle } from "@lucide/vue";
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

// Props & Emits - Using explicit props for compatibility
const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "installed-success"): void;
}>();

// State
const currentStep = ref<number>(1); // 1: Base version, 2: Mod loader (optional), 3: Name & Install
const versions = ref<VanillaVersion[]>([]);
const selectedVersion = ref<string>("");
const installModLoader = ref<boolean>(false);
const stableFabricLoaders = ref<string[]>([]);
const unstableFabricLoaders = ref<string[]>([]);
const selectedFabricLoader = ref<string>("");
const customInstanceName = ref<string>("");
const isLoadingVersions = ref(false);
const isLoadingFabric = ref(false);
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

const latestFabricLoader = computed(() => {
  return stableFabricLoaders.value.length > 0 ? stableFabricLoaders.value[0] : 
         (unstableFabricLoaders.value.length > 0 ? unstableFabricLoaders.value[0] : "");
});

const canProceedToStep2 = computed(() => {
  return selectedVersion.value !== "";
});

const canProceedToStep3 = computed(() => {
  if (!installModLoader.value) return true;
  return selectedFabricLoader.value !== "";
});

const downloadProgressPercent = computed(() => {
  if (!installProgress.value?.totalTasks) return 0;
  const completedTasks = installProgress.value.completedTasks ?? 0;
  const totalTasks = installProgress.value.totalTasks;
  const percent = Math.floor((completedTasks / totalTasks) * 100);
  if (percent === 0 && completedTasks > 0) return 1;
  return percent;
});

// Generate default instance name based on selections
function generateInstanceName(): string {
  if (!selectedVersion.value) return "";
  
  if (installModLoader.value && selectedFabricLoader.value) {
    return `Fabric-${selectedVersion.value}-${selectedFabricLoader.value}`;
  }
  return selectedVersion.value;
}

// When dialog opens, load versions if not yet loaded
watch(() => props.open, (isOpen) => {
  if (isOpen) {
    // Reset state
    currentStep.value = 1;
    selectedVersion.value = "";
    installModLoader.value = false;
    stableFabricLoaders.value = [];
    unstableFabricLoaders.value = [];
    selectedFabricLoader.value = "";
    customInstanceName.value = "";
    error.value = null;
    installProgress.value = null;
    downloadProgress.value.clear();
    
    // Load versions
    if (versions.value.length === 0) {
      loadVersions();
    }
  }
});

// Watch for mod loader toggle to load fabric loaders
watch(installModLoader, async (enabled) => {
  if (enabled && selectedVersion.value && stableFabricLoaders.value.length === 0 && unstableFabricLoaders.value.length === 0) {
    await loadFabricLoaders();
  }
});

// Watch version changes to reset mod loader if version changes
watch(selectedVersion, () => {
  installModLoader.value = false;
  stableFabricLoaders.value = [];
  unstableFabricLoaders.value = [];
  selectedFabricLoader.value = "";
  customInstanceName.value = "";
});

// Combined watch: load Fabric loaders when either version OR mod loader toggle changes
// This ensures the list refreshes when switching versions with toggle already enabled
watch([selectedVersion, installModLoader], async () => {
  const newVersion = selectedVersion.value;
  const newLoaderEnabled = installModLoader.value;
  
  if (newLoaderEnabled && newVersion) {
    // Version or toggle changed - reload Fabric loaders
    await loadFabricLoaders();
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

// Load Fabric loaders for selected Minecraft version
async function loadFabricLoaders(): Promise<void> {
  if (!selectedVersion.value) return;
  
  isLoadingFabric.value = true;
  error.value = null;
  stableFabricLoaders.value = [];
  unstableFabricLoaders.value = [];
  selectedFabricLoader.value = "";

  try {
    interface FabricLoaderList {
      stable: string[];
      unstable: string[];
    }
    
    const loaders = await invoke<FabricLoaderList>("get_fabric_loaders", {
      mcVersion: selectedVersion.value,
    });
    
    stableFabricLoaders.value = loaders.stable;
    unstableFabricLoaders.value = loaders.unstable;
    
    // Auto-select latest STABLE loader (if available), otherwise latest unstable
    if (loaders.stable.length > 0) {
      selectedFabricLoader.value = loaders.stable[0];
    } else if (loaders.unstable.length > 0) {
      selectedFabricLoader.value = loaders.unstable[0];
    }
    
    if (selectedFabricLoader.value) {
      customInstanceName.value = generateInstanceName();
    }
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    console.error("Failed to load Fabric loaders:", err);
  } finally {
    isLoadingFabric.value = false;
  }
}

// Navigation
function goToStep2() {
  if (canProceedToStep2.value) {
    currentStep.value = 2;
    error.value = null;
  }
}

function goToStep3() {
  if (canProceedToStep3.value) {
    currentStep.value = 3;
    customInstanceName.value = generateInstanceName();
    error.value = null;
  }
}

function goBackToStep1() {
  currentStep.value = 1;
  error.value = null;
}

function goBackToStep2() {
  currentStep.value = 2;
  error.value = null;
}

// Install selected version
async function installVersion(): Promise<void> {
  if (!selectedVersion.value) {
    error.value = "Please select a Minecraft version";
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
    // Step 1: Install base vanilla version
    await invoke("install_vanilla_version", {
      versionId: selectedVersion.value,
      versionJsonUrl: version.url,
    });

    // Step 2: If mod loader is selected, install Fabric on top
    if (installModLoader.value && selectedFabricLoader.value) {
      // Update progress
      installProgress.value = {
        ...installProgress.value,
        phase: "resolving_libraries",
        versionId: customInstanceName.value,
      };

      await invoke("install_fabric_instance", {
        mcVersion: selectedVersion.value,
        loaderVersion: selectedFabricLoader.value,
        customInstanceName: customInstanceName.value,
      });
    }
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

// Handle installation complete
function handleInstallationComplete() {
  isInstalling.value = false;
  // Emit success event to refresh parent list
  emit("installed-success");
}

// Register event listeners once on mount
onMounted(async () => {
  const un1 = await listen<InstallProgress>("install-progress", (event) => {
    installProgress.value = event.payload;

    if (event.payload.phase === "complete") {
      handleInstallationComplete();
    } else if (event.payload.phase === "error") {
      isInstalling.value = false;
    }
  });

  const un2 = await listen<DownloadProgress>("download-progress", (event) => {
    const progress = event.payload;

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
    // Batch complete
  });

  unlisteners.push(un1, un2, un3);
});

onUnmounted(() => {
  unlisteners.forEach((un) => un());
});
</script>

<template>
  <DialogContent :open="props.open" @update:open="(val) => emit('update:open', val)" class="max-w-2xl">
    <DialogTitle>Install New Instance</DialogTitle>
    <DialogDescription>
      Progressive installation wizard
    </DialogDescription>

    <!-- Step Indicator -->
    <div class="flex items-center justify-center gap-2 py-4">
      <div 
        :class="[
          'flex items-center justify-center w-8 h-8 rounded-full text-sm font-medium',
          currentStep >= 1 ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'
        ]"
      >
        1
      </div>
      <div :class="['h-0.5 w-8', currentStep >= 2 ? 'bg-primary' : 'bg-muted']" />
      <div 
        :class="[
          'flex items-center justify-center w-8 h-8 rounded-full text-sm font-medium',
          currentStep >= 2 ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'
        ]"
      >
        2
      </div>
      <div :class="['h-0.5 w-8', currentStep >= 3 ? 'bg-primary' : 'bg-muted']" />
      <div 
        :class="[
          'flex items-center justify-center w-8 h-8 rounded-full text-sm font-medium',
          currentStep >= 3 ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'
        ]"
      >
        3
      </div>
    </div>

    <!-- Step 1: Base Minecraft Version -->
    <div v-if="currentStep === 1" class="space-y-4">
      <div class="flex items-center gap-2 text-lg font-medium">
        <Box class="w-5 h-5" />
        Select Base Minecraft Version
      </div>
      
      <div class="space-y-3">
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
            v-if="versions.length > releaseVersions.length + snapshotVersions.length"
            label="Other"
          >
            <option
              v-for="v in sortedVersions.filter(
                (v) => !['release', 'snapshot'].includes(v.versionType)
              )"
              :key="v.id"
              :value="v.id"
            >
              {{ v.id }} ({{ v.versionType }})
            </option>
          </optgroup>
        </select>
      </div>

      <div class="flex justify-end">
        <button
          @click="goToStep2"
          :disabled="!canProceedToStep2"
          class="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
        >
          Next
          <span class="text-lg">→</span>
        </button>
      </div>
    </div>

    <!-- Step 2: Optional Mod Loader -->
    <div v-if="currentStep === 2" class="space-y-4">
      <div class="flex items-center gap-2 text-lg font-medium">
        <Puzzle class="w-5 h-5" />
        Install Mod Loader (Optional)
      </div>

      <!-- Toggle for mod loader -->
      <div class="flex items-center gap-3 p-4 border rounded-lg">
        <input
          type="checkbox"
          id="installModLoader"
          v-model="installModLoader"
          :disabled="isInstalling"
          class="w-5 h-5 rounded border-gray-300 text-primary focus:ring-primary"
        />
        <label for="installModLoader" class="flex-1">
          <span class="font-medium">Install Fabric Mod Loader</span>
          <p class="text-sm text-muted-foreground">
            Enable this to install Fabric loader on top of the base version
          </p>
        </label>
      </div>

      <!-- Fabric Loader Selector (only when toggled) -->
      <div v-if="installModLoader" class="space-y-3 animate-in fade-in slide-in-from-top-2">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">Fabric Loader Version</label>
          <button
            @click="loadFabricLoaders"
            :disabled="isLoadingFabric || !selectedVersion"
            class="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
          >
            <RefreshCw
              v-if="isLoadingFabric"
              class="w-3.5 h-3.5 animate-spin"
            />
            <RefreshCw v-else class="w-3.5 h-3.5" />
            Refresh
          </button>
        </div>

        <select
          v-model="selectedFabricLoader"
          :disabled="isLoadingFabric || !selectedVersion"
          class="w-full px-3 py-2 bg-background border rounded-md text-sm disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring"
        >
          <option value="" disabled>Select a loader version...</option>
          <optgroup v-if="stableFabricLoaders.length" label="Stable">
            <option
              v-for="loader in stableFabricLoaders"
              :key="loader"
              :value="loader"
            >
              {{ loader }}
            </option>
          </optgroup>
          <optgroup v-if="unstableFabricLoaders.length" label="Unstable (Testing)">
            <option
              v-for="loader in unstableFabricLoaders"
              :key="loader"
              :value="loader"
            >
              {{ loader }}
            </option>
          </optgroup>
        </select>

        <p v-if="stableFabricLoaders.length > 0 || unstableFabricLoaders.length > 0" class="text-xs text-muted-foreground">
          Latest: {{ latestFabricLoader }}
        </p>
      </div>

      <div class="flex justify-between">
        <button
          @click="goBackToStep1"
          :disabled="isInstalling"
          class="px-4 py-2 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
        >
          ← Back
        </button>
        <button
          @click="goToStep3"
          :disabled="!canProceedToStep3"
          class="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
        >
          Next
          <span class="text-lg">→</span>
        </button>
      </div>
    </div>

<!-- Step 3: Instance Name & Install -->
    <div v-if="currentStep === 3" class="space-y-4">
      <!-- Show summary when installing OR when installation is complete -->
      <div v-if="isInstalling" class="p-4 bg-muted/30 rounded-lg space-y-2">
        <div class="flex justify-between text-sm">
          <span class="text-muted-foreground">Base Version:</span>
          <span class="font-medium">{{ selectedVersion }}</span>
        </div>
        <div v-if="installModLoader" class="flex justify-between text-sm">
          <span class="text-muted-foreground">Mod Loader:</span>
          <span class="font-medium">Fabric {{ selectedFabricLoader }}</span>
        </div>
        <div class="flex justify-between text-sm">
          <span class="text-muted-foreground">Instance Name:</span>
          <span class="font-medium">{{ customInstanceName }}</span>
        </div>
      </div>

      <!-- Show form only when NOT installing AND installation is complete -->
      <template v-else>
        <div class="p-4 bg-muted/30 rounded-lg space-y-2">
          <div class="flex justify-between text-sm">
            <span class="text-muted-foreground">Base Version:</span>
            <span class="font-medium">{{ selectedVersion }}</span>
          </div>
          <div v-if="installModLoader" class="flex justify-between text-sm">
            <span class="text-muted-foreground">Mod Loader:</span>
            <span class="font-medium">Fabric {{ selectedFabricLoader }}</span>
          </div>
        </div>

        <div class="space-y-3">
          <label class="text-sm font-medium">Instance Name</label>
          <input
            v-model="customInstanceName"
            type="text"
            placeholder="e.g., Fabric-1.20.1"
            class="w-full px-3 py-2 bg-background border rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <p class="text-xs text-muted-foreground">
            This name will be used for the instance folder and version ID.
            Each instance has isolated saves, resourcepacks, and options.txt.
          </p>
        </div>

        <!-- Install Button -->
        <div class="flex items-center gap-3">
          <button
            @click="installVersion"
            :disabled="!customInstanceName || isLoadingVersions"
            class="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
          >
            <Download class="w-4 h-4" />
            <span>Install Instance</span>
          </button>
        </div>

        <!-- Back Button -->
        <div class="flex justify-start">
          <button
            @click="goBackToStep2"
            class="px-4 py-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
          >
            ← Back
          </button>
        </div>
      </template>
    </div>

    <!-- Installation Progress (always show during installation) -->
    <div
      v-if="installProgress"
      class="rounded-lg border bg-muted/30 p-4 space-y-3"
    >
      <!-- Phase indicator -->
      <div class="flex items-center gap-2 text-sm">
        <Loader2
          v-if="!['complete', 'error'].includes(installProgress.phase)"
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
          >Instance {{ installProgress.versionId }} installed
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