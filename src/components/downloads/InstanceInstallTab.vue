<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, reactive } from "vue";
import { onClickOutside } from "@vueuse/core";
import { useI18n } from "vue-i18n";
import type { SelectOption } from "../ui/DSelect.vue";
import { invoke } from "@tauri-apps/api/core";
import { trackEvent, getErrorType } from "../../utils/analytics";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Download, Loader2, Check, Puzzle,  } from "@lucide/vue";
import DInput from "../ui/DInput.vue";
import DSelect from "../ui/DSelect.vue";
import InstallCard from "../InstallCard.vue";
import TaskDetailView from "../TaskDetailView.vue";
import { useTaskStore } from "../../composables/useTaskStore";
import { getErrorMessage } from "../../utils/error";

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
  initialVersion?: string;
  initialLoader?: string;
}>();


const taskStore = useTaskStore();
const currentTaskId = ref<string | null>(null);
const currentTask = computed(() => {
  if (!currentTaskId.value) return null;
  return taskStore.tasks.value.find((t: any) => t.id === currentTaskId.value) || null;
});

const { t } = useI18n();

const searchQuery = ref('');
const selectedTypeFilter = ref('all');

const typeFilterOptions = computed<SelectOption[]>(() => [
  { label: t('install.all'), value: 'all' },
  { label: t('install.releases'), value: 'release' },
  { label: t('install.snapshots'), value: 'snapshot' },
  { label: t('install.other'), value: 'other' }
]);

const filteredVersions = computed(() => {
  return sortedVersions.value.filter(v => {
    if (selectedTypeFilter.value !== 'all') {
      if (selectedTypeFilter.value === 'other') {
         if (['release', 'snapshot'].includes(v.versionType)) return false;
      } else if (v.versionType !== selectedTypeFilter.value) {
        return false;
      }
    }
    if (searchQuery.value && !v.id.toLowerCase().includes(searchQuery.value.toLowerCase())) {
      return false;
    }
    return true;
  });
});

function selectVersionAndNext(id: string) {
  if (selectedVersion.value !== id) {
    selectedVersion.value = id;
    
    // Clear mod loader cache
    forgeLoaders.value = [];
    neoForgeLoaders.value = [];
    stableFabricLoaders.value = [];
    unstableFabricLoaders.value = [];
    
    // Clear selected components
    selectedComponents.Forge = null;
    selectedComponents.Fabric = null;
    selectedComponents.NeoForge = null;
    selectedComponents.OptiFine = null;
    selectedComponents['Fabric API'] = null;
    
    // Clear configuring state
    activeConfiguringComponent.value = null;
  }
  goToStep2();
}

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

const forgeLoaderOptions = computed<SelectOption[]>(() => forgeLoaders.value.map(loader => ({
  label: loader.version,
  value: loader.version
})));

const fabricLoaderOptions = computed<SelectOption[]>(() => stableFabricLoaders.value.map(v => ({
  label: v,
  value: v
})));

const neoForgeLoaderOptions = computed<SelectOption[]>(() => neoForgeLoaders.value.map(loader => ({
  label: loader.version,
  value: loader.version
})));

const customInstanceName = ref<string>("");
const isLoadingVersions = ref(false);
const isInstalling = ref(false);
const installProgress = ref<InstallProgress | null>(null);
const downloadProgress = ref<Map<string, DownloadProgress>>(new Map());
const error = ref<string | null>(null);

const CONFLICT_MATRIX: Record<string, string[]> = {
  'Forge': ['Fabric', 'NeoForge', 'Quilt', 'Fabric API', 'QSL/QFAPI'],
  'Fabric': ['Forge', 'NeoForge', 'Quilt', 'OptiFine'],
  'NeoForge': ['Forge', 'Fabric', 'Quilt', 'OptiFine'],
  'OptiFine': ['Fabric', 'NeoForge', 'Quilt'],
  'Fabric API': ['Forge', 'NeoForge', 'Quilt'],
  'QSL/QFAPI': ['Forge', 'NeoForge', 'Quilt'],
};

const DEPENDENCY_MATRIX: Record<string, string[]> = {
  'Fabric API': ['Fabric'],
  'QSL/QFAPI': ['Quilt'],
};

// Selected Components Matrix
const selectedComponents = reactive<Record<string, string | null>>({
  Forge: null,
  Fabric: null,
  NeoForge: null,
  OptiFine: null,
  'Fabric API': null,
});

const activeConfiguringComponent = ref<string | null>(null);
const gridRef = ref<HTMLElement | null>(null);

onClickOutside(gridRef, () => {
  activeConfiguringComponent.value = null;
});

function getConflictReason(target: string): string | undefined {
  // Check exclusions
  for (const [installedKey, installedValue] of Object.entries(selectedComponents)) {
    if (installedValue !== null && CONFLICT_MATRIX[installedKey]?.includes(target)) {
      return t('install.conflict', { component: installedKey });
    }
  }

  // Check requirements
  const reqs = DEPENDENCY_MATRIX[target];
  if (reqs) {
    const hasAnyReq = reqs.some(req => selectedComponents[req as keyof typeof selectedComponents] !== null);
    if (!hasAnyReq) {
      return t('install.requirement', { component: reqs.join(' / ') });
    }
  }

  return undefined;
}

function getCardStatus(target: string): 'pending' | 'selected' | 'disabled' {
  if (selectedComponents[target as keyof typeof selectedComponents]) return 'selected';
  if (getConflictReason(target)) return 'disabled';
  return 'pending';
}

function handleCardClick(target: string) {
  if (isLoadingFabric.value || isLoadingForge.value || isLoadingNeoForge.value) return;
  activeConfiguringComponent.value = target;
  if (target === 'Fabric') {
    if (stableFabricLoaders.value.length === 0 && unstableFabricLoaders.value.length === 0) loadFabricLoaders();
  } else if (target === 'Forge') {
    if (forgeLoaders.value.length === 0) loadForgeLoaders();
  } else if (target === 'NeoForge') {
    if (neoForgeLoaders.value.length === 0) loadNeoForgeLoaders();
  }
  else if (target === 'OptiFine' || target === 'Fabric API') {
    // Mock version selection for OptiFine and Fabric API
    if (!selectedComponents[target as keyof typeof selectedComponents]) {
       selectedComponents[target as keyof typeof selectedComponents] = 'Default Version (Mock)';
    }
  }
}

function removeComponent(target: string) {
  selectedComponents[target as keyof typeof selectedComponents] = null;
  if (activeConfiguringComponent.value === target) {
    activeConfiguringComponent.value = null;
  }

  // Cascade remove dependents whose requirements are no longer met
  for (const [key, value] of Object.entries(selectedComponents)) {
    if (value !== null) {
      const reqs = DEPENDENCY_MATRIX[key];
      if (reqs) {
        const hasAnyReq = reqs.some(req => selectedComponents[req as keyof typeof selectedComponents] !== null);
        if (!hasAnyReq) {
          removeComponent(key);
        }
      }
    }
  }
}

function mapComponentsToLegacyState() {
  installModLoader.value = false;
  selectedLoaderType.value = "fabric";
  selectedFabricLoader.value = "";
  selectedForgeLoader.value = "";
  selectedNeoForgeLoader.value = "";

  if (selectedComponents.Fabric) {
    installModLoader.value = true;
    selectedLoaderType.value = "fabric";
    selectedFabricLoader.value = selectedComponents.Fabric;
  } else if (selectedComponents.Forge) {
    installModLoader.value = true;
    selectedLoaderType.value = "forge";
    selectedForgeLoader.value = selectedComponents.Forge;
  } else if (selectedComponents.NeoForge) {
    installModLoader.value = true;
    selectedLoaderType.value = "neoforge";
    selectedNeoForgeLoader.value = selectedComponents.NeoForge;
  }
}

// Event unlisteners
const unlisteners: UnlistenFn[] = [];

// Computed
const releaseVersions = computed(() =>
  versions.value.filter((v) => v.versionType === "release")
);

// const snapshotVersions = computed(() => {
//   return versions.value.filter((v) => v.versionType === "snapshot").slice(0, 50);
// });

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
    return b.id.localeCompare(a.id, undefined, { numeric: true, sensitivity: 'base' });
  })
);


const canProceedToStep2 = computed(() => {
  return selectedVersion.value !== "";
});

const canProceedToStep3 = computed(() => {
  if (isLoadingFabric.value || isLoadingForge.value || isLoadingNeoForge.value) {
    return false;
  }
  
  if (activeConfiguringComponent.value) {
    const comp = activeConfiguringComponent.value;
    // Forge, Fabric, and NeoForge require a version to be selected
    if (['Forge', 'Fabric', 'NeoForge'].includes(comp)) {
      if (!selectedComponents[comp as keyof typeof selectedComponents]) {
        return false;
      }
    }
  }
  
  return true;
});



// Generate default instance name based on selections
function generateInstanceName(): string {
  if (!selectedVersion.value) return "";
  if (selectedComponents.Fabric) {
    return `Fabric-${selectedVersion.value}-${selectedComponents.Fabric}`;
  } else if (selectedComponents.Forge) {
    return `Forge-${selectedVersion.value}-${selectedComponents.Forge}`;
  } else if (selectedComponents.NeoForge) {
    return `NeoForge-${selectedVersion.value}-${selectedComponents.NeoForge}`;
  }
  return selectedVersion.value;
}

// When dialog opens, load versions if not yet loaded
onMounted(async () => {
    const isOpen = true;
    if (isOpen) {
    // Reset ALL state - critical for proper reopen
    currentStep.value = 1;
    selectedVersion.value = props.initialVersion || "";
    if (props.initialLoader && props.initialLoader !== "vanilla") {
      installModLoader.value = true;
      selectedLoaderType.value = props.initialLoader as any;
    } else {
      installModLoader.value = false;
      selectedLoaderType.value = "fabric";
    }
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
    
    // Load versions
    if (versions.value.length === 0) {
      await loadVersions();
      // Ensure the pre-filled version gets selected if available
      if (props.initialVersion) {
        selectedVersion.value = props.initialVersion;
      }
    }

    // Auto-jump to step 3 if this is an auto-install flow
    if (props.initialVersion) {
      if (installModLoader.value) {
        if (selectedLoaderType.value === "fabric") await loadFabricLoaders();
        else if (selectedLoaderType.value === "forge") await loadForgeLoaders();
        else if (selectedLoaderType.value === "neoforge") await loadNeoForgeLoaders();
      } else {
        customInstanceName.value = generateInstanceName();
      }
      currentStep.value = 3;
    }
  }
});

// removed handleOpenChange
// Watch version changes to reset mod loader if version changes
watch(selectedVersion, (_, oldVal) => {
  if (!oldVal) return; // Do not reset on initial initialization from props
  activeConfiguringComponent.value = null;
  selectedComponents.Forge = null;
  selectedComponents.Fabric = null;
  selectedComponents.NeoForge = null;
  selectedComponents.OptiFine = null;
  selectedComponents['Fabric API'] = null;
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
    if (latestRelease && !selectedVersion.value) {
      selectedVersion.value = latestRelease.id;
    }
  } catch (err) {
    error.value = getErrorMessage(err);
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
      selectedComponents.Fabric = selectedFabricLoader.value;
      customInstanceName.value = generateInstanceName();
      if (activeConfiguringComponent.value === 'Fabric') activeConfiguringComponent.value = null; // Auto close
    }
  } catch (err) {
    error.value = getErrorMessage(err);
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
      selectedComponents.Forge = selectedForgeLoader.value;
      customInstanceName.value = generateInstanceName();
      if (activeConfiguringComponent.value === 'Forge') activeConfiguringComponent.value = null; // Auto close
    }
  } catch (err) {
    error.value = getErrorMessage(err);
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
      selectedComponents.NeoForge = selectedNeoForgeLoader.value;
      customInstanceName.value = generateInstanceName();
      if (activeConfiguringComponent.value === 'NeoForge') activeConfiguringComponent.value = null; // Auto close
    }
  } catch (err) {
    error.value = getErrorMessage(err);
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
    mapComponentsToLegacyState();
    currentStep.value = 3;
    if (!customInstanceName.value) customInstanceName.value = generateInstanceName();
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


watch(currentTask, (newTask) => {
  if (isInstalling.value && !newTask) {
    // If the task was cleared from the manager, we should reset to initial state
    resetToInitialState();
  }
});

function resetToInitialState() {
  currentStep.value = 1;
  isInstalling.value = false;
  currentTaskId.value = null;
  selectedVersion.value = "";
  selectedComponents.Forge = null;
  selectedComponents.Fabric = null;
  selectedComponents.NeoForge = null;
  selectedComponents.OptiFine = null;
  selectedComponents['Fabric API'] = null;
  customInstanceName.value = "";
  activeConfiguringComponent.value = null;
  mapComponentsToLegacyState();
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
    let taskId = "";
    // If mod loader is selected, install the appropriate loader on top
    if (installModLoader.value) {
      if (selectedLoaderType.value === "fabric" && selectedFabricLoader.value) {
        taskId = await invoke<string>("install_fabric_instance", {
          mcVersion: selectedVersion.value,
          fabricVersion: selectedFabricLoader.value,
          customInstanceName: customInstanceName.value,
        });
      } else if (selectedLoaderType.value === "forge" && selectedForgeLoader.value) {
        taskId = await invoke<string>("install_forge_instance", {
          mcVersion: selectedVersion.value,
          loaderVersion: selectedForgeLoader.value,
          loaderType: "forge",
          customInstanceName: customInstanceName.value,
        });
      } else if (selectedLoaderType.value === "neoforge" && selectedNeoForgeLoader.value) {
        taskId = await invoke<string>("install_forge_instance", {
          mcVersion: selectedVersion.value,
          loaderVersion: selectedNeoForgeLoader.value,
          loaderType: "neoforge",
          customInstanceName: customInstanceName.value,
        });
      }
    } else {
      taskId = await invoke<string>("install_vanilla_version", {
        versionId: selectedVersion.value,
        versionJsonUrl: version.url,
        customInstanceName: customInstanceName.value.trim() !== "" ? customInstanceName.value : null,
      });
    }

    currentTaskId.value = taskId;
    
    trackEvent("Instance Install Completed", { 
      version: selectedVersion.value, 
      loader: installModLoader.value ? selectedLoaderType.value : undefined 
    });

    // We do not set isInstalling = false here anymore because the task runs in the background.
    // The user will see the task progress via TaskDetailView.
  } catch (err) {
    trackEvent("Error Occurred", { context: "instance_install", error_type: getErrorType(err) });
    error.value = getErrorMessage(err);
    isInstalling.value = false;
  }
}

// Helper functions removed as they are now handled by TaskDetailView
// Register event listeners once on mount
onMounted(async () => {
  const un1 = await listen<InstallProgress>("install-progress", (event) => {
    installProgress.value = event.payload;

    if (event.payload.phase === "error") {
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
  <div class="h-full flex flex-col p-6 overflow-y-auto w-full">
    
    

    <!-- Step Indicator -->
    <div class="flex items-center justify-center gap-2 py-2">
      <div 
        :class="[
          'flex items-center justify-center w-8 h-8 rounded-full text-sm font-semibold transition-all',
          currentStep === 1 ? 'bg-primary text-primary-foreground ring-2 ring-primary ring-offset-2 dark:ring-offset-zinc-900' :
          currentStep > 1 ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'
        ]"
      >
        <Check v-if="currentStep > 1" class="w-4 h-4" />
        <span v-else>1</span>
      </div>
      <div :class="['h-0.5 w-8 transition-colors', currentStep >= 2 ? 'bg-primary' : 'bg-muted']" />
      <div 
        :class="[
          'flex items-center justify-center w-8 h-8 rounded-full text-sm font-semibold transition-all',
          currentStep === 2 ? 'bg-primary text-primary-foreground ring-2 ring-primary ring-offset-2 dark:ring-offset-zinc-900' :
          currentStep > 2 ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'
        ]"
      >
        <Check v-if="currentStep > 2" class="w-4 h-4" />
        <span v-else>2</span>
      </div>
      <div :class="['h-0.5 w-8 transition-colors', currentStep >= 3 ? 'bg-primary' : 'bg-muted']" />
      <div 
        :class="[
          'flex items-center justify-center w-8 h-8 rounded-full text-sm font-semibold transition-all',
          currentStep === 3 ? 'bg-primary text-primary-foreground ring-2 ring-primary ring-offset-2 dark:ring-offset-zinc-900' :
          currentStep > 3 ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'
        ]"
      >
        <Check v-if="currentStep > 3" class="w-4 h-4" />
        <span v-else>3</span>
      </div>
    </div>

    <!-- Step 1: Base Minecraft Version -->
    <div v-if="currentStep === 1" class="flex-1 flex flex-col min-h-0 space-y-4">
      <!-- Search & Filter bar -->
      <div class="flex items-center gap-3 bg-white dark:bg-zinc-900 p-2 rounded-lg border border-neutral-200 dark:border-zinc-800 shrink-0 shadow-sm">
        <div class="flex-1 flex items-center px-2">
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-neutral-400 mr-2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
          <DInput
            v-model="searchQuery"
            :placeholder="$t('install.searchPlaceholder')"
            class="!border-none !ring-0 !bg-transparent !px-0 !h-auto"
          />
        </div>
        <div class="flex items-center gap-2 px-3 border-l border-neutral-200 dark:border-zinc-800">
          <span class="text-sm text-neutral-500 whitespace-nowrap">{{ $t('install.versionType') }}</span>
          <DSelect
            v-model="selectedTypeFilter"
            :options="typeFilterOptions"
            class="bg-transparent border-none focus:outline-none text-sm font-medium pr-2 text-neutral-900 dark:text-white cursor-pointer w-32"
          />
        </div>
        <button
          @click="loadVersions"
          :disabled="isLoadingVersions"
          class="flex items-center justify-center bg-emerald-600 hover:bg-emerald-700 text-white rounded-md px-4 py-1.5 text-sm font-medium transition-colors disabled:opacity-50 shrink-0"
        >
          <svg v-if="isLoadingVersions" class="w-3.5 h-3.5 animate-spin mr-1.5" xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>
          <svg v-else class="w-3.5 h-3.5 mr-1.5" xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
          {{ $t('install.refresh') }}
        </button>
      </div>

      <!-- List -->
      <div class="flex-1 overflow-y-auto bg-white dark:bg-zinc-900 rounded-lg border border-neutral-200 dark:border-zinc-800 shadow-sm">
        <div v-if="isLoadingVersions" class="flex items-center justify-center h-full text-neutral-500 min-h-[300px]">
          <svg class="w-8 h-8 animate-spin" xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>
        </div>
        <div v-else-if="filteredVersions.length === 0" class="flex items-center justify-center h-full text-neutral-500 min-h-[300px]">
          {{ $t('install.noVersionsFound') }}
        </div>
        <div v-else class="flex flex-col">
          <div
            v-for="v in filteredVersions"
            :key="v.id"
            class="flex items-center justify-between p-4 hover:bg-neutral-50 dark:hover:bg-zinc-800/50 transition-colors group border-b border-neutral-100 dark:border-zinc-800 last:border-0 cursor-pointer"
            @click="selectVersionAndNext(v.id)"
          >
            <div class="flex items-center gap-4">
              <div class="w-10 h-10 bg-neutral-100 dark:bg-zinc-800 rounded flex items-center justify-center shrink-0">
                <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="w-6 h-6 text-neutral-500"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.29 7 12 12 20.71 7"/><line x1="12" x2="12" y1="22" y2="12"/></svg>
              </div>
              <div>
                <div class="flex items-center gap-2">
                  <span class="font-semibold text-base text-neutral-900 dark:text-white">{{ v.id }}</span>
                  <span
                    class="text-[10px] px-1.5 py-0.5 rounded font-medium"
                    :class="v.versionType === 'release' ? 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300' : 'bg-neutral-100 text-neutral-700 dark:bg-zinc-800 dark:text-neutral-300'"
                  >
                    {{ v.versionType === 'release' ? $t('install.releases') : (v.versionType === 'snapshot' ? $t('install.snapshots') : $t('install.oldVersions')) }}
                  </span>
                </div>
                
              </div>
            </div>
            
            <button
              class="w-8 h-8 rounded-full flex items-center justify-center bg-transparent group-hover:bg-indigo-100 dark:group-hover:bg-indigo-900/40 text-neutral-400 group-hover:text-indigo-600 dark:group-hover:text-indigo-400 transition-colors"
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/><path d="m12 5 7 7-7 7"/></svg>
            </button>
          </div>
        </div>
      </div>
    </div>

<!-- Step 2: Modular Installation Workbench -->
    <div v-if="currentStep === 2" class="space-y-4">
      <div class="flex items-center gap-2 text-lg font-medium text-neutral-900 dark:text-white">
        <Puzzle class="w-5 h-5" />
        {{ t('install.modularTitle') }}
      </div>
      <p class="text-sm text-muted-foreground">{{ t('install.modularDesc') }}</p>

      <div ref="gridRef" class="grid grid-cols-2 sm:grid-cols-3 gap-3">
        <InstallCard
          title="Forge"
          iconUrl="/forge.png"
          :status="getCardStatus('Forge')"
          :version="selectedComponents.Forge || undefined"
          :conflictReason="getConflictReason('Forge')"
          @click="handleCardClick('Forge')"
          @remove="removeComponent('Forge')"
          @change="handleCardClick('Forge')"
          :isConfiguring="activeConfiguringComponent === 'Forge'">
          <template #configurator>
            <div v-if="isLoadingForge" class="text-xs text-primary flex justify-center items-center gap-1.5 w-full py-1"><Loader2 class="w-3.5 h-3.5 animate-spin"/>{{ $t('install.fetchingVersions') }}</div>
            <DSelect v-else @click.stop @update:model-value="activeConfiguringComponent = null" v-model="selectedComponents.Forge" :options="forgeLoaderOptions" class="w-full text-xs" />
          </template>
        </InstallCard>
        <InstallCard
          title="Fabric"
          iconUrl="/fabric.png"
          :status="getCardStatus('Fabric')"
          :version="selectedComponents.Fabric || undefined"
          :conflictReason="getConflictReason('Fabric')"
          @click="handleCardClick('Fabric')"
          @remove="removeComponent('Fabric')"
          @change="handleCardClick('Fabric')"
          :isConfiguring="activeConfiguringComponent === 'Fabric'">
          <template #configurator>
            <div v-if="isLoadingFabric" class="text-xs text-primary flex justify-center items-center gap-1.5 w-full py-1"><Loader2 class="w-3.5 h-3.5 animate-spin"/>{{ $t('install.fetchingVersions') }}</div>
            <DSelect v-else @click.stop @update:model-value="activeConfiguringComponent = null" v-model="selectedComponents.Fabric" :options="fabricLoaderOptions" class="w-full text-xs" />
          </template>
        </InstallCard>
        <InstallCard
          title="NeoForge"
          iconUrl="/neoforge.png"
          :status="getCardStatus('NeoForge')"
          :version="selectedComponents.NeoForge || undefined"
          :conflictReason="getConflictReason('NeoForge')"
          @click="handleCardClick('NeoForge')"
          @remove="removeComponent('NeoForge')"
          @change="handleCardClick('NeoForge')"
          :isConfiguring="activeConfiguringComponent === 'NeoForge'">
          <template #configurator>
            <div v-if="isLoadingNeoForge" class="text-xs text-primary flex justify-center items-center gap-1.5 w-full py-1"><Loader2 class="w-3.5 h-3.5 animate-spin"/>{{ $t('install.fetchingVersions') }}</div>
            <DSelect v-else @click.stop @update:model-value="activeConfiguringComponent = null" v-model="selectedComponents.NeoForge" :options="neoForgeLoaderOptions" class="w-full text-xs" />
          </template>
        </InstallCard>
        <InstallCard
          title="OptiFine"
          iconUrl="/optifine.png"
          :status="getCardStatus('OptiFine')"
          :version="selectedComponents.OptiFine || undefined"
          :conflictReason="getConflictReason('OptiFine')"
          @click="handleCardClick('OptiFine')"
          @remove="removeComponent('OptiFine')"
          @change="handleCardClick('OptiFine')"
          :isConfiguring="activeConfiguringComponent === 'OptiFine'">
          <template #configurator>
            <div class="text-xs text-center text-muted-foreground w-full">Automatically handled</div>
          </template>
        </InstallCard>
        <InstallCard
          title="Fabric API"
          iconUrl="/fabric-api.png"
          :status="getCardStatus('Fabric API')"
          :version="selectedComponents['Fabric API'] || undefined"
          :conflictReason="getConflictReason('Fabric API')"
          @click="handleCardClick('Fabric API')"
          @remove="removeComponent('Fabric API')"
          @change="handleCardClick('Fabric API')"
          :isConfiguring="activeConfiguringComponent === 'Fabric API'">
          <template #configurator>
            <div class="text-xs text-center text-muted-foreground w-full">Automatically handled</div>
          </template>
        </InstallCard>
      </div>

      <div class="flex justify-between mt-6">
        <button @click="goBackToStep1" class="px-4 py-2 text-sm text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white font-medium transition-colors">← {{ $t('install.back') }}</button>
        <button @click="goToStep3" :disabled="!canProceedToStep3" class="flex items-center gap-2 px-6 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md transition-colors text-sm font-medium shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">{{ $t('install.next') }} <span class="text-lg">→</span></button>
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

      <!-- Show form only when NOT installing -->
      <template v-if="!isInstalling">
        <!-- Error Message -->
        <div v-if="error" class="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-900/50 rounded-lg text-red-600 dark:text-red-400 text-sm mb-4 flex items-center gap-2">
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
          {{ error }}
        </div>

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
          <div>
            <label class="block text-sm font-medium text-neutral-700 dark:text-neutral-300 mb-1">{{ $t('install.instanceName') }}</label>
            <DInput
              v-model="customInstanceName"
              :placeholder="(currentTask as any)?.metadata?.versionId || $t('install.defaultName')"
            />
          </div>
          <p class="text-xs text-muted-foreground">
            {{ t("install.instanceNameDesc") }}
          </p>
        </div>

        <div class="flex items-center justify-between mt-6">
          <button
            @click="goBackToStep2"
            class="px-4 py-2 text-sm text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white font-medium transition-colors"
          >
            ← {{ t("install.back") }}
          </button>

          <button
            @click="installVersion"
            :disabled="!customInstanceName || isLoadingVersions || isLoadingFabric || isLoadingForge || isLoadingNeoForge"
            class="flex items-center gap-2 px-6 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md transition-colors text-sm font-medium shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Download class="w-4 h-4" />
            <span>
            {{ t("install.installInstance") }}
            </span>
          </button>
        </div>
      </template>
    </div>

    <!-- Installation Task View -->
    <div v-if="isInstalling && currentTask" class="mt-4">
      <TaskDetailView :task="currentTask" />
    </div>

    <!-- Back/Finish floating button (Installing State) -->
    <div v-if="isInstalling && currentTask && ['Completed', 'Failed', 'Cancelled'].includes(currentTask.status)" class="absolute top-6 right-6 z-50">
      <button
        @click="resetToInitialState"
        class="flex items-center gap-2 px-6 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md transition-colors text-sm font-medium shadow-sm"
      >
        {{ currentTask.status === 'Completed' ? $t('install.finish') : $t('install.back') }}
      </button>
    </div>
  </div>
</template>