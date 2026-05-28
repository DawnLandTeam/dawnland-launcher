<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useI18n } from "vue-i18n";
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

interface FabricLoaderList {
  stable: string[];
  unstable: string[];
}

interface LoaderVersion {
  version: string;
  mcVersion: string;
  installerUrl: string;
}

interface LoaderVersionList {
  versions: LoaderVersion[];
}

// Props & Emits - Using explicit props for compatibility
const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "installed-success"): void;
}>();

const { t } = useI18n();

// State
const currentStep = ref<number>(1); // 1: Base version, 2: Mod loader (optional), 3: Name & Install
const versions = ref<VanillaVersion[]>([]);
const selectedVersion = ref<string>("");
const installModLoader = ref<boolean>(false);
const selectedLoaderType = ref<"fabric" | "forge" | "neoforge">("fabric");
const stableFabricLoaders = ref<string[]>([]);
const unstableFabricLoaders = ref<string[]>([]);
const selectedFabricLoader = ref<string>("");
const forgeLoaders = ref<LoaderVersion[]>([]);
const selectedForgeLoader = ref<string>("");
const neoForgeLoaders = ref<LoaderVersion[]>([]);
const selectedNeoForgeLoader = ref<string>("");
const isLoadingFabric = ref(false);
const isLoadingForge = ref(false);
const isLoadingNeoForge = ref(false);
const customInstanceName = ref<string>("");
const isLoadingVersions = ref(false);
const isInstalling = ref(false);
const installProgress = ref<InstallProgress | null>(null);
const downloadProgress = ref<Map<string, DownloadProgress>>(new Map());
const error = ref<string | null>(null);
// Track if we have more installation steps after current one
const hasMoreSteps = ref<boolean>(false);

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
  
  if (selectedLoaderType.value === "fabric") {
    return selectedFabricLoader.value !== "";
  } else if (selectedLoaderType.value === "forge") {
    return selectedForgeLoader.value !== "";
  } else if (selectedLoaderType.value === "neoforge") {
    return selectedNeoForgeLoader.value !== "";
  }
  return false;
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
  
  if (installModLoader.value) {
    if (selectedLoaderType.value === "fabric" && selectedFabricLoader.value) {
      return `Fabric-${selectedVersion.value}-${selectedFabricLoader.value}`;
    } else if (selectedLoaderType.value === "forge" && selectedForgeLoader.value) {
      return `Forge-${selectedVersion.value}-${selectedForgeLoader.value}`;
    } else if (selectedLoaderType.value === "neoforge" && selectedNeoForgeLoader.value) {
      return `NeoForge-${selectedVersion.value}-${selectedNeoForgeLoader.value}`;
    }
  }
  return selectedVersion.value;
}

// When dialog opens, load versions if not yet loaded
watch(() => props.open, (isOpen) => {
  if (isOpen) {
    // Reset ALL state - critical for proper reopen
    currentStep.value = 1;
    selectedVersion.value = "";
    installModLoader.value = false;
    selectedLoaderType.value = "fabric";
    stableFabricLoaders.value = [];
    unstableFabricLoaders.value = [];
    selectedFabricLoader.value = "";
    forgeLoaders.value = [];
    selectedForgeLoader.value = "";
    neoForgeLoaders.value = [];
    selectedNeoForgeLoader.value = "";
    customInstanceName.value = "";
    error.value = null;
    installProgress.value = null;
    downloadProgress.value.clear();
    isInstalling.value = false;  // Reset installation state too
    hasMoreSteps.value = false;  // Reset steps tracking
    
    // Load versions
    if (versions.value.length === 0) {
      loadVersions();
    }
  }
});

// Prevent closing dialog during installation
function handleOpenChange(open: boolean) {
  // If trying to close during installation, prevent it
  if (!open && isInstalling.value) {
    return; // Prevent closing
  }
  emit('update:open', open);
}

// Watch version changes to reset mod loader if version changes
watch(selectedVersion, () => {
  installModLoader.value = false;
  stableFabricLoaders.value = [];
  unstableFabricLoaders.value = [];
  selectedFabricLoader.value = "";
  forgeLoaders.value = [];
  selectedForgeLoader.value = "";
  neoForgeLoaders.value = [];
  selectedNeoForgeLoader.value = "";
  customInstanceName.value = "";
});

// Watch loader type changes to reload appropriate loaders
watch([selectedVersion, installModLoader, selectedLoaderType], async () => {
  const newVersion = selectedVersion.value;
  const newLoaderEnabled = installModLoader.value;
  const loaderType = selectedLoaderType.value;
  
  if (newLoaderEnabled && newVersion) {
    if (loaderType === "fabric") {
      await loadFabricLoaders();
    } else if (loaderType === "forge") {
      await loadForgeLoaders();
    } else if (loaderType === "neoforge") {
      await loadNeoForgeLoaders();
    }
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

// Load Forge loaders for selected Minecraft version
async function loadForgeLoaders(): Promise<void> {
  if (!selectedVersion.value) return;
  
  isLoadingForge.value = true;
  error.value = null;
  forgeLoaders.value = [];
  selectedForgeLoader.value = "";

  try {
    const loaders = await invoke<LoaderVersionList>("get_forge_loaders", {
      mcVersion: selectedVersion.value,
    });
    
    forgeLoaders.value = loaders.versions;
    
    // Auto-select first version
    if (loaders.versions.length > 0) {
      selectedForgeLoader.value = loaders.versions[0].version;
    }
    
    if (selectedForgeLoader.value) {
      customInstanceName.value = generateInstanceName();
    }
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    console.error("Failed to load Forge loaders:", err);
  } finally {
    isLoadingForge.value = false;
  }
}

// Load NeoForge loaders for selected Minecraft version
async function loadNeoForgeLoaders(): Promise<void> {
  if (!selectedVersion.value) return;
  
  isLoadingNeoForge.value = true;
  error.value = null;
  neoForgeLoaders.value = [];
  selectedNeoForgeLoader.value = "";

  try {
    const loaders = await invoke<LoaderVersionList>("get_neoforge_loaders", {
      mcVersion: selectedVersion.value,
    });
    
    neoForgeLoaders.value = loaders.versions;
    
    // Auto-select first version
    if (loaders.versions.length > 0) {
      selectedNeoForgeLoader.value = loaders.versions[0].version;
    }
    
    if (selectedNeoForgeLoader.value) {
      customInstanceName.value = generateInstanceName();
    }
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    console.error("Failed to load NeoForge loaders:", err);
  } finally {
    isLoadingNeoForge.value = false;
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

  // Determine if there are more steps after vanilla
  // (if mod loader is selected, we have more steps after vanilla completes)
  hasMoreSteps.value = installModLoader.value;

  try {
    // Step 1: Install base vanilla version
    await invoke("install_vanilla_version", {
      versionId: selectedVersion.value,
      versionJsonUrl: version.url,
    });

    // Vanilla is done, but we may still have mod loader to install
    // Update hasMoreSteps: if we have mod loader, we still have more steps
    // But the "complete" event from vanilla should NOT finish the whole process
    
    // Step 2: If mod loader is selected, install the appropriate loader on top
    // Note: Don't set isInstalling = false here, let the final "complete" event do it
    if (installModLoader.value) {
      // Clear the hasMoreSteps flag since we're about to do the final step
      // The next "complete" event should finish everything
      hasMoreSteps.value = false;
      
      // Update progress
      installProgress.value = {
        ...installProgress.value,
        phase: "resolving_libraries",
        versionId: customInstanceName.value,
      };

      if (selectedLoaderType.value === "fabric" && selectedFabricLoader.value) {
        await invoke("install_fabric_instance", {
          mcVersion: selectedVersion.value,
          loaderVersion: selectedFabricLoader.value,
          customInstanceName: customInstanceName.value,
        });
      } else if (selectedLoaderType.value === "forge" && selectedForgeLoader.value) {
        await invoke("install_forge_instance", {
          mcVersion: selectedVersion.value,
          loaderVersion: selectedForgeLoader.value,
          loaderType: "forge",
          customInstanceName: customInstanceName.value,
        });
      } else if (selectedLoaderType.value === "neoforge" && selectedNeoForgeLoader.value) {
        await invoke("install_forge_instance", {
          mcVersion: selectedVersion.value,
          loaderVersion: selectedNeoForgeLoader.value,
          loaderType: "neoforge",
          customInstanceName: customInstanceName.value,
        });
      }
    }
    // Note: isInstalling will be set to false when "complete" event is received
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    isInstalling.value = false;
  }
}

// Format phase label
function formatPhase(phase: string): string {
  const labels: Record<string, string> = {
    resolving_version: t("install.fetchingVersion"),
    resolving_libraries: t("install.filteringLibraries"),
    resolving_assets: t("install.preparingAssets"),
    downloading: t("install.downloadingFiles"),
    complete: t("install.installComplete"),
    error: t("install.installFailed"),
  };
  return labels[phase] || phase;
}

// Format backend progress file strings
function formatCurrentFile(file: string): string {
  const backendMessages: Record<string, string> = {
    "Fetching Minecraft version manifest...": t("install.status.fetchingManifest"),
    "Installing base Minecraft...": t("install.status.installingBase"),
    "Downloading Forge installer...": t("install.status.downloadingForge"),
    "Running Forge processors (this may take a while)...": t("install.status.runningProcessors"),
  };
  return backendMessages[file] || file;
}

// Format speed for display
function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
}

// Handle installation complete
function handleInstallationComplete() {
  // Only finish if there are no more steps (e.g., vanilla done but Forge still pending)
  if (!hasMoreSteps.value) {
    isInstalling.value = false;
    // Emit success event to refresh parent list
    emit("installed-success");
  }
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
  <DialogContent :open="props.open" @update:open="handleOpenChange" class="max-w-2xl">
    <DialogTitle>{{ t("install.title") }}</DialogTitle>
    <DialogDescription>
      {{ t("install.subtitle") }}
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
      <div class="flex items-center gap-2 text-lg font-medium text-neutral-900 dark:text-white">
        <Box class="w-5 h-5" />
        {{ t("install.step1Title") }}
      </div>
      
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">{{ t("install.mcVersion") }}</label>
          <button
            @click="loadVersions"
            :disabled="isLoadingVersions"
            class="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
          >
            <RefreshCw
              v-if="isLoadingVersions"
              class="w-3.5 h-3.5 animate-spin"
            />
            <RefreshCw v-else class="w-3.5 h-3.5" /> {{ t("install.refresh") }} </button>
        </div>

        <select
          v-model="selectedVersion"
          :disabled="isLoadingVersions || isInstalling"
          class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white disabled:opacity-50"
        >
          <option value="" disabled>{{ t("install.selectVersion") }}</option>

          <optgroup v-if="releaseVersions.length" :label="t('install.releases')">
            <option
              v-for="v in releaseVersions"
              :key="v.id"
              :value="v.id"
            >
              {{ v.id }}
            </option>
          </optgroup>

          <optgroup v-if="snapshotVersions.length" :label="t('install.snapshots')">
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
            :label="t('install.other')"
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
          {{ t("install.next") }}
          <span class="text-lg">→</span>
        </button>
      </div>
    </div>

    <!-- Step 2: Optional Mod Loader -->
    <div v-if="currentStep === 2" class="space-y-4">
      <div class="flex items-center gap-2 text-lg font-medium text-neutral-900 dark:text-white">
        <Puzzle class="w-5 h-5" />
        {{ t("install.step2Title") }}
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
          <span class="font-medium">{{ t("install.installModLoader") }}</span>
          <p class="text-sm text-muted-foreground">
            {{ t("install.installModLoaderDesc") }}
          </p>
        </label>
      </div>

      <!-- Loader Type Selector (only when toggled) -->
      <div v-if="installModLoader" class="space-y-3 animate-in fade-in slide-in-from-top-2">
        <label class="text-sm font-medium">{{ t("install.modLoaderType") }}</label>
        <div class="flex gap-2">
          <button
            @click="selectedLoaderType = 'fabric'; loadFabricLoaders()"
            :disabled="isInstalling"
            :class="[
              'flex-1 py-2 px-3 rounded-md text-sm font-medium border transition-colors',
              selectedLoaderType === 'fabric' 
                ? 'bg-blue-100 dark:bg-blue-900/30 border-blue-300 dark:border-blue-700 text-blue-700 dark:text-blue-300' 
                : 'bg-white dark:bg-zinc-800 border-neutral-300 dark:border-zinc-700 text-neutral-600 dark:text-neutral-400 hover:bg-muted'
            ]"
          >
            Fabric
          </button>
          <button
            @click="selectedLoaderType = 'forge'; loadForgeLoaders()"
            :disabled="isInstalling"
            :class="[
              'flex-1 py-2 px-3 rounded-md text-sm font-medium border transition-colors',
              selectedLoaderType === 'forge' 
                ? 'bg-orange-100 dark:bg-orange-900/30 border-orange-300 dark:border-orange-700 text-orange-700 dark:text-orange-300' 
                : 'bg-white dark:bg-zinc-800 border-neutral-300 dark:border-zinc-700 text-neutral-600 dark:text-neutral-400 hover:bg-muted'
            ]"
          >
            Forge
          </button>
          <button
            @click="selectedLoaderType = 'neoforge'; loadNeoForgeLoaders()"
            :disabled="isInstalling"
            :class="[
              'flex-1 py-2 px-3 rounded-md text-sm font-medium border transition-colors',
              selectedLoaderType === 'neoforge' 
                ? 'bg-green-100 dark:bg-green-900/30 border-green-300 dark:border-green-700 text-green-700 dark:text-green-300' 
                : 'bg-white dark:bg-zinc-800 border-neutral-300 dark:border-zinc-700 text-neutral-600 dark:text-neutral-400 hover:bg-muted'
            ]"
          >
            NeoForge
          </button>
        </div>
      </div>

      <!-- Fabric Loader Selector -->
      <div v-if="installModLoader && selectedLoaderType === 'fabric'" class="space-y-3 animate-in fade-in slide-in-from-top-2">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">{{ t("install.fabricLoader") }}</label>
          <button
            @click="loadFabricLoaders"
            :disabled="isLoadingFabric || !selectedVersion"
            class="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
          >
            <RefreshCw
              v-if="isLoadingFabric"
              class="w-3.5 h-3.5 animate-spin"
            />
            <RefreshCw v-else class="w-3.5 h-3.5" /> {{ t("install.refresh") }} </button>
        </div>

        <select
          v-model="selectedFabricLoader"
          :disabled="isLoadingFabric || !selectedVersion"
          class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white disabled:opacity-50"
        >
          <option value="" disabled>{{ t("install.selectLoader") }}</option>
          <optgroup v-if="stableFabricLoaders.length" :label="t('install.stable')">
            <option
              v-for="loader in stableFabricLoaders"
              :key="loader"
              :value="loader"
            >
              {{ loader }}
            </option>
          </optgroup>
          <optgroup v-if="unstableFabricLoaders.length" :label="t('install.unstable')">
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
          {{ t("install.latest") }}: {{ latestFabricLoader }}
        </p>
      </div>

      <!-- Forge Loader Selector -->
      <div v-if="installModLoader && selectedLoaderType === 'forge'" class="space-y-3 animate-in fade-in slide-in-from-top-2">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">{{ t("install.forgeVersion") }}</label>
          <button
            @click="loadForgeLoaders"
            :disabled="isLoadingForge || !selectedVersion"
            class="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
          >
            <RefreshCw
              v-if="isLoadingForge"
              class="w-3.5 h-3.5 animate-spin"
            />
            <RefreshCw v-else class="w-3.5 h-3.5" /> {{ t("install.refresh") }} </button>
        </div>

        <select
          v-model="selectedForgeLoader"
          :disabled="isLoadingForge || !selectedVersion"
          class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white disabled:opacity-50"
        >
          <option value="" disabled>{{ t("install.selectForge") }}</option>
          <option
            v-for="loader in forgeLoaders"
            :key="loader.version"
            :value="loader.version"
          >
            {{ loader.version }}
          </option>
        </select>

        <p v-if="forgeLoaders.length > 0" class="text-xs text-muted-foreground">
          {{ t("install.availableVersions") }}: {{ forgeLoaders.length }}
        </p>
      </div>

      <!-- NeoForge Loader Selector -->
      <div v-if="installModLoader && selectedLoaderType === 'neoforge'" class="space-y-3 animate-in fade-in slide-in-from-top-2">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">{{ t("install.neoforgeVersion") }}</label>
          <button
            @click="loadNeoForgeLoaders"
            :disabled="isLoadingNeoForge || !selectedVersion"
            class="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
          >
            <RefreshCw
              v-if="isLoadingNeoForge"
              class="w-3.5 h-3.5 animate-spin"
            />
            <RefreshCw v-else class="w-3.5 h-3.5" /> {{ t("install.refresh") }} </button>
        </div>

        <select
          v-model="selectedNeoForgeLoader"
          :disabled="isLoadingNeoForge || !selectedVersion"
          class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white disabled:opacity-50"
        >
          <option value="" disabled>{{ t("install.selectNeoforge") }}</option>
          <option
            v-for="loader in neoForgeLoaders"
            :key="loader.version"
            :value="loader.version"
          >
            {{ loader.version }}
          </option>
        </select>

        <p v-if="neoForgeLoaders.length > 0" class="text-xs text-muted-foreground">
          {{ t("install.availableVersions") }}: {{ neoForgeLoaders.length }}
        </p>
      </div>

      <div class="flex justify-between">
        <button
          @click="goBackToStep1"
          :disabled="isInstalling"
          class="px-4 py-2 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
        >
          ← {{ t("install.back") }}
        </button>
        <button
          @click="goToStep3"
          :disabled="!canProceedToStep3"
          class="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
        >
          {{ t("install.next") }}
          <span class="text-lg">→</span>
        </button>
      </div>
    </div>

<!-- Step 3: Instance Name & Install -->
    <div v-if="currentStep === 3" class="space-y-4">
      <!-- Show summary when installing OR when installation is complete -->
      <div v-if="isInstalling" class="p-4 bg-muted/30 rounded-lg space-y-2">
        <div class="flex justify-between text-sm">
          <span class="text-muted-foreground">{{ t("install.baseVersion") }}:</span>
          <span class="font-medium">{{ selectedVersion }}</span>
        </div>
        <div v-if="installModLoader" class="flex justify-between text-sm">
          <span class="text-muted-foreground">{{ t("install.modLoader") }}:</span>
          <span class="font-medium">
            <template v-if="selectedLoaderType === 'fabric'">Fabric {{ selectedFabricLoader }}</template>
            <template v-else-if="selectedLoaderType === 'forge'">Forge {{ selectedForgeLoader }}</template>
            <template v-else-if="selectedLoaderType === 'neoforge'">NeoForge {{ selectedNeoForgeLoader }}</template>
          </span>
        </div>
        <div class="flex justify-between text-sm">
          <span class="text-muted-foreground">{{ t("install.instanceName") }}:</span>
          <span class="font-medium">{{ customInstanceName }}</span>
        </div>
      </div>

      <!-- Show form only when NOT installing AND installation is complete -->
      <template v-else>
        <div class="p-4 bg-muted/30 rounded-lg space-y-2">
          <div class="flex justify-between text-sm">
            <span class="text-muted-foreground">{{ t("install.baseVersion") }}:</span>
            <span class="font-medium">{{ selectedVersion }}</span>
          </div>
          <div v-if="installModLoader" class="flex justify-between text-sm">
            <span class="text-muted-foreground">{{ t("install.modLoader") }}:</span>
            <span class="font-medium">
              <template v-if="selectedLoaderType === 'fabric'">Fabric {{ selectedFabricLoader }}</template>
              <template v-else-if="selectedLoaderType === 'forge'">Forge {{ selectedForgeLoader }}</template>
              <template v-else-if="selectedLoaderType === 'neoforge'">NeoForge {{ selectedNeoForgeLoader }}</template>
            </span>
          </div>
        </div>

        <div class="space-y-3">
          <label class="text-sm font-medium">{{ t("install.instanceName") }}</label>
          <input
            v-model="customInstanceName"
            type="text"
            :placeholder="t('install.instanceNamePlaceholder')"
            class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500"
          />
          <p class="text-xs text-muted-foreground">
            {{ t("install.instanceNameDesc") }}
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
            <span>
            {{ t("install.installInstance") }}
          </span>
          </button>
        </div>

        <!-- Back Button -->
        <div class="flex justify-start">
          <button
            @click="goBackToStep2"
            class="px-4 py-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
          >
            ← {{ t("install.back") }}
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
            {{ installProgress.totalTasks }} {{ t("install.files") }}
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
        {{ t("install.current") }}: {{ formatCurrentFile(installProgress.currentFile) }}
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

    <!-- {{ t("install.activeDownloads") }} -->
    <div
      v-if="downloadProgress.size > 0"
      class="rounded-lg border bg-muted/30 p-3 space-y-2"
    >
      <h4 class="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
        {{ t("install.activeDownloads") }}
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