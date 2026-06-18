<script setup lang="ts">
import { ref, watch, computed, onMounted, onUnmounted } from "vue";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { Package, Search, Download, Trash2, Loader2, UploadCloud, CheckCircle2, AlertTriangle } from "@lucide/vue";
import { DialogContent, DialogTitle, DialogDescription } from "./ui/dialog";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "./ui/alert-dialog";
import { getErrorMessage } from "../utils/error";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

// Types
interface InstanceItem {
  id: string;
  name: string;
  mcVersion: string;
  loaderType: string;
}

interface LocalModItem {
  filename: string;
  enabled: boolean;
  size: number;
  modId?: string;
  name?: string;
  version?: string;
  iconUrl?: string;
}

interface UnifiedModProject {
  source: string;
  project_id: string;
  title: string;
  description: string;
  icon_url: string | null;
  downloads: number;
  author: string;
  mc_versions: string[];
  loaders: string[];
  download_url: string | null;
  file_id: string | null;
}

interface UnifiedModFile {
  id: string;
  filename: string;
  version_number: string;
  download_url: string;
  release_type: string;
  date: string;
  dependencies: any[];
}

// Props
const props = defineProps<{
  open: boolean;
  instance: InstanceItem | null;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
}>();

// State
const activeTab = ref<"local" | "browse">("local");

// Local Mods State
const isLoadingLocal = ref(false);
const localMods = ref<LocalModItem[]>([]);
const localSearchQuery = ref("");
const error = ref<string | null>(null);
const isDragging = ref(false);
let unlistenFileDrop: UnlistenFn | null = null;
let unlistenDragEnter: UnlistenFn | null = null;
let unlistenDragLeave: UnlistenFn | null = null;

const filteredLocalMods = computed(() => {
  if (!localSearchQuery.value.trim()) return localMods.value;
  const q = localSearchQuery.value.toLowerCase().replace(/[-_ ]/g, '');
  return localMods.value.filter(mod => {
    const matchName = mod.name && mod.name.toLowerCase().replace(/[-_ ]/g, '').includes(q);
    const matchId = mod.modId && mod.modId.toLowerCase().replace(/[-_ ]/g, '').includes(q);
    const matchFile = mod.filename.toLowerCase().replace(/[-_ ]/g, '').includes(q);
    return matchName || matchId || matchFile;
  });
});

// Browse Mods State
const selectedSource = ref<"modrinth" | "curseforge">("curseforge");
const searchQuery = ref("");
const isSearching = ref(false);
const searchResults = ref<UnifiedModProject[]>([]);

// Selected mod for details
const selectedMod = ref<UnifiedModProject | null>(null);

// Install Modal State
const installModProject = ref<UnifiedModProject | null>(null);
const availableModFiles = ref<UnifiedModFile[]>([]);
const isLoadingModFiles = ref(false);
const selectedModFileId = ref<string>("");

const duplicateDialogState = ref<{
  show: boolean;
  duplicates: string[];
  resolve: ((value: "overwrite" | "keep" | "cancel") => void) | null;
}>({ show: false, duplicates: [], resolve: null });

const isInstalling = ref(false);
const installSuccess = ref(false);
const installProgress = ref<number>(0);
const installStatusText = ref("");
const totalInstallFiles = ref(1);
const currentInstallFileIndex = ref(0);
const currentInstallFilename = ref("");
const currentContentLength = ref(0);
const currentDownloadedBytes = ref(0);

let progressListener: UnlistenFn | null = null;

// Source badges
const sourceBadges = {
  modrinth: { bg: "bg-green-100 dark:bg-green-900/40", text: "text-green-700 dark:text-green-300", label: "Modrinth" },
  curseforge: { bg: "bg-orange-100 dark:bg-orange-900/40", text: "text-orange-700 dark:text-orange-300", label: "CurseForge" },
};

// Watchers
watch(() => props.open, async (isOpen) => {
  if (isOpen && props.instance) {
    if (activeTab.value === "local") {
      localSearchQuery.value = "";
      await loadLocalMods();
    }
  } else {
    localMods.value = [];
    searchResults.value = [];
    searchQuery.value = "";
    installModProject.value = null;
    selectedMod.value = null;
  }
});

watch(activeTab, async (tab) => {
  if (tab === "local" && props.instance && props.open) {
    await loadLocalMods();
  }
});

let searchTimeout: ReturnType<typeof setTimeout> | null = null;
watch(searchQuery, () => {
  if (searchTimeout) clearTimeout(searchTimeout);
  searchTimeout = setTimeout(() => {
    if (searchQuery.value.trim()) {
      searchMods();
    } else {
      searchResults.value = [];
    }
  }, 500);
});

watch(selectedSource, () => {
  if (searchQuery.value.trim()) {
    searchMods();
  }
});

watch(installModProject, async (mod) => {
  if (mod && props.instance) {
    await fetchModFiles(mod, props.instance);
  } else {
    availableModFiles.value = [];
    selectedModFileId.value = "";
  }
});

// Lifecycle
onMounted(async () => {
  try {
    // Setup file drop listener for local mod import
    unlistenFileDrop = await listen<{ paths: string[] }>('tauri://file-drop', async (event) => {
      isDragging.value = false;
      if (!props.open || !props.instance || activeTab.value !== 'local') return;
      
      const jarFiles = event.payload.paths.filter(p => p.toLowerCase().endsWith('.jar'));
      if (jarFiles.length > 0) {
        isLoadingLocal.value = true;
        try {
          for (const path of jarFiles) {
            await invoke('import_local_mod_to_instance', {
              versionId: props.instance.id,
              filePath: path
            });
          }
          await loadLocalMods();
        } catch (err) {
          error.value = getErrorMessage(err);
        } finally {
          isLoadingLocal.value = false;
        }
      }
    });

    unlistenDragEnter = await listen('tauri://file-drop-hover', () => {
      if (props.open && activeTab.value === 'local') isDragging.value = true;
    });

    unlistenDragLeave = await listen('tauri://file-drop-cancelled', () => {
      isDragging.value = false;
    });

    // Setup mod install progress listener
    progressListener = await listen("mod-install-progress", (event) => {
      const payload = event.payload as any;
      const decodedFilename = decodeURIComponent(payload.filename || "");
      
      if (currentInstallFilename.value !== decodedFilename) {
        currentInstallFilename.value = decodedFilename;
        currentInstallFileIndex.value++;
        currentContentLength.value = 0;
        currentDownloadedBytes.value = 0;
      }

      if (payload.event === "Started") {
         currentContentLength.value = payload.contentLength || 0;
         currentDownloadedBytes.value = 0;
      } else if (payload.event === "Progress") {
         currentDownloadedBytes.value += payload.chunkLength || 0;
      } else if (payload.event === "Finished") {
         if (currentContentLength.value > 0) {
             currentDownloadedBytes.value = currentContentLength.value;
         }
      }

      const fileProgress = currentContentLength.value > 0 
        ? Math.min(100, (currentDownloadedBytes.value / currentContentLength.value) * 100) 
        : (payload.event === "Finished" ? 100 : 0);

      const baseProgress = ((currentInstallFileIndex.value - 1) / totalInstallFiles.value) * 100;
      const currentProgress = (fileProgress / 100) * (100 / totalInstallFiles.value);
      
      installProgress.value = Math.round(Math.min(100, Math.max(0, baseProgress + currentProgress)));
      installStatusText.value = `Downloading (${currentInstallFileIndex.value}/${totalInstallFiles.value}): ${decodedFilename}...`;
    });

  } catch (err) {
    console.error("Failed to setup listeners:", err);
  }
});

onUnmounted(() => {
  if (unlistenFileDrop) unlistenFileDrop();
  if (unlistenDragEnter) unlistenDragEnter();
  if (unlistenDragLeave) unlistenDragLeave();
  if (progressListener) progressListener();
});

// Local Mods Logic
async function loadLocalMods() {
  if (!props.instance) return;
  isLoadingLocal.value = true;
  error.value = null;
  
  try {
    localMods.value = await invoke<LocalModItem[]>("get_installed_mods", {
      versionId: props.instance.id,
    });
  } catch (err) {
    error.value = getErrorMessage(err);
    localMods.value = [];
  } finally {
    isLoadingLocal.value = false;
  }
}

async function toggleMod(mod: LocalModItem) {
  if (!props.instance) return;
  try {
    await invoke("toggle_mod_status", {
      versionId: props.instance.id,
      filename: mod.filename,
      enable: !mod.enabled,
    });
    await loadLocalMods();
  } catch (err) {
    error.value = getErrorMessage(err);
  }
}

async function deleteMod(mod: LocalModItem) {
  if (!props.instance) return;
  if (!confirm(`Are you sure you want to delete "${mod.filename}"?`)) return;
  
  try {
    await invoke("delete_local_mod", {
      versionId: props.instance.id,
      filename: mod.filename,
      isEnabled: mod.enabled,
    });
    localMods.value = localMods.value.filter(m => !(m.filename === mod.filename && m.enabled === mod.enabled));
  } catch (err) {
    error.value = getErrorMessage(err);
  }
}

// Browse Mods Logic
let currentSearchId = 0;

async function searchMods() {
  if (!searchQuery.value.trim() || !props.instance) {
    searchResults.value = [];
    return;
  }

  isSearching.value = true;
  error.value = null;
  const searchId = ++currentSearchId;

  try {
    const command = selectedSource.value === "modrinth" ? "search_modrinth" : "search_curseforge";
    const results = await invoke<UnifiedModProject[]>(command, {
      query: searchQuery.value,
      mcVersion: props.instance.mcVersion,
      loader: props.instance.loaderType.toLowerCase() === "vanilla" ? "" : props.instance.loaderType.toLowerCase(),
    });
    
    if (searchId === currentSearchId) {
      searchResults.value = results;
    }
  } catch (err) {
    if (searchId === currentSearchId) {
      error.value = getErrorMessage(err);
      searchResults.value = [];
    }
  } finally {
    if (searchId === currentSearchId) {
      isSearching.value = false;
    }
  }
}

function openInstallModal(mod: UnifiedModProject) {
  installModProject.value = mod;
  isInstalling.value = false;
  installSuccess.value = false;
  installProgress.value = 0;
  installStatusText.value = "";
  error.value = null;
}

async function fetchModFiles(mod: UnifiedModProject, instance: InstanceItem) {
  isLoadingModFiles.value = true;
  availableModFiles.value = [];
  selectedModFileId.value = "";
  error.value = null;

  try {
    const command = mod.source === "modrinth" ? "get_modrinth_mod_files" : "get_cf_mod_files";
    availableModFiles.value = await invoke<UnifiedModFile[]>(command, {
      projectId: mod.project_id,
      mcVersion: instance.mcVersion,
      loader: instance.loaderType.toLowerCase() === "vanilla" ? "" : instance.loaderType.toLowerCase(),
    });

    if (availableModFiles.value.length > 0) {
      const releaseFile = availableModFiles.value.find(f => f.release_type.toLowerCase() === 'release');
      const betaFile = availableModFiles.value.find(f => f.release_type.toLowerCase() === 'beta');
      
      if (releaseFile) {
        selectedModFileId.value = releaseFile.id;
      } else if (betaFile) {
        selectedModFileId.value = betaFile.id;
      } else {
        selectedModFileId.value = availableModFiles.value[0].id;
      }
    }
  } catch (err) {
    error.value = getErrorMessage(err);
  } finally {
    isLoadingModFiles.value = false;
  }
}

async function installMod() {
  const mod = installModProject.value;
  const file = availableModFiles.value.find(f => f.id === selectedModFileId.value);
  if (!mod || !file || !props.instance) return;

  const reqDeps = (file.dependencies || []).filter(d => d.required).length;
  totalInstallFiles.value = 1 + reqDeps;
  currentInstallFileIndex.value = 0;
  currentInstallFilename.value = "";
  currentContentLength.value = 0;
  currentDownloadedBytes.value = 0;
  
  isInstalling.value = true;
  installSuccess.value = false;
  error.value = null;
  installProgress.value = 0;
  installStatusText.value = "Checking existing mods...";

  const getCoreName = (f: string) => {
    let name = f.toLowerCase().replace(/\.jar$/, '');
    name = name.replace(/^\[.*?\]\s*/, '').replace(/^\(.*?\)\s*/, '');
    let chunks = name.split(/[-_+ ]/);
    
    let coreChunks = [];
    for (let chunk of chunks) {
      if (['fabric', 'forge', 'neoforge', 'quilt', 'mc', 'minecraft'].includes(chunk)) {
        continue;
      }
      if (/^[a-z]+$/.test(chunk)) {
        coreChunks.push(chunk);
      } else {
        break;
      }
    }
    return coreChunks.join('-');
  };
  
  const newCore = getCoreName(file.filename);
  const possibleDuplicates = localMods.value.filter(m => {
    if (m.filename === file.filename) return true;
    
    const existingCore = getCoreName(m.filename);

    // Layer 1: Exact Metadata Match
    if (mod.project_id && m.modId && m.modId === mod.project_id) return true;
    if (mod.title && m.name && m.name.toLowerCase() === mod.title.toLowerCase()) return true;
    if (m.modId && m.modId.toLowerCase() === newCore) return true;
    if (mod.project_id && existingCore === mod.project_id.toLowerCase()) return true;

    // Layer 2: Regex Fallback
    return existingCore.length > 0 && existingCore === newCore;
  });

  let keepBoth = false;
  if (possibleDuplicates.length > 0) {
    const dupNames = possibleDuplicates.map(d => d.filename);
    
    const userChoice = await new Promise<"overwrite" | "keep" | "cancel">((resolve) => {
      duplicateDialogState.value = {
        show: true,
        duplicates: dupNames,
        resolve
      };
    });

    if (userChoice === "cancel") {
      isInstalling.value = false;
      return;
    } else if (userChoice === "overwrite") {
      for (const dup of possibleDuplicates) {
        try {
          await invoke("delete_local_mod", {
            versionId: props.instance.id,
            filename: dup.filename,
            isEnabled: dup.enabled,
          });
        } catch (e) {
          console.error("Failed to delete duplicate mod", e);
        }
      }
      await loadLocalMods();
    }
    
    keepBoth = userChoice === "keep";
  }

  installStatusText.value = "Starting installation...";

  try {
    await invoke("install_mod_to_instance", {
      versionId: props.instance.id,
      modSource: mod.source,
      projectId: mod.project_id,
      fileId: file.id,
      downloadUrl: file.download_url,
      dependencies: file.dependencies,
      keepBoth: keepBoth,
    });

    installProgress.value = 100;
    installSuccess.value = true;
    installStatusText.value = "Installed successfully!";
    
    setTimeout(() => {
      if (installSuccess.value) {
        installModProject.value = null;
        activeTab.value = "local"; // Switch back to local to see the installed mod
      }
    }, 2000);
  } catch (err) {
    error.value = getErrorMessage(err);
    installProgress.value = 0;
  } finally {
    isInstalling.value = false;
  }
}

// Formatters
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDownloads(count: number): string {
  if (count >= 1000000) return `${(count / 1000000).toFixed(1)}M`;
  if (count >= 1000) return `${(count / 1000).toFixed(1)}K`;
  return count.toString();
}

function getLoaderBadgeClass(loader: string): string {
  switch (loader.toLowerCase()) {
    case "fabric": return "bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300";
    case "forge": return "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300";
    case "neoforge": return "bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300";
    default: return "bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300";
  }
}

const groupedModFiles = computed(() => {
  const groups: Record<string, UnifiedModFile[]> = {
    Release: [],
    Beta: [],
    Alpha: [],
    Unknown: [],
  };
  
  availableModFiles.value.forEach(file => {
    let rType = file.release_type;
    if (rType) {
      rType = rType.charAt(0).toUpperCase() + rType.slice(1).toLowerCase();
    }
    if (rType && groups[rType]) {
      groups[rType].push(file);
    } else {
      groups.Unknown.push(file);
    }
  });

  return [
    { label: 'Release', items: groups.Release },
    { label: 'Beta', items: groups.Beta },
    { label: 'Alpha', items: groups.Alpha },
    { label: 'Unknown', items: groups.Unknown },
  ].filter(g => g.items.length > 0);
});
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-4xl max-h-[85vh] p-0 flex flex-col gap-0 overflow-hidden bg-background">
    
    <!-- Drag Drop Overlay -->
    <div 
      v-if="isDragging"
      class="absolute inset-0 z-50 bg-primary/10 backdrop-blur-sm border-2 border-primary border-dashed m-4 rounded-xl flex flex-col items-center justify-center transition-all pointer-events-none"
    >
      <div class="bg-background/80 p-6 rounded-2xl shadow-xl flex flex-col items-center gap-3">
        <UploadCloud class="h-12 w-12 text-primary animate-bounce" />
        <h3 class="text-xl font-bold text-foreground">Drop Mod Files Here</h3>
        <p class="text-sm text-muted-foreground">Release to install .jar files to this instance</p>
      </div>
    </div>

    <!-- Header & Tabs -->
    <div class="p-5 border-b shrink-0 pr-12 bg-muted/10 flex justify-between items-center">
      <div>
        <DialogTitle class="flex items-center gap-2 text-xl">
          <Package class="h-5 w-5 text-primary" />
          {{ t('instanceMods.title', 'Instance Mods') }}
        </DialogTitle>
        <DialogDescription class="mt-1">
          {{ instance?.name }} ({{ instance?.mcVersion }} - {{ instance?.loaderType }})
        </DialogDescription>
      </div>
      
      <!-- Segmented Control for Tabs -->
      <div class="bg-muted p-1 rounded-lg flex items-center shadow-inner gap-1">
        <button
          @click="activeTab = 'local'"
          :class="[
            'px-4 py-1.5 text-sm font-medium transition-all rounded-md flex items-center gap-2',
            activeTab === 'local' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground hover:bg-background/50'
          ]"
        >
          <Package class="w-4 h-4" />
          {{ t('instanceMods.localMods', 'Local Mods') }}
          <span v-if="localMods.length" :class="[
            'ml-1 px-1.5 rounded-md text-xs font-semibold',
            activeTab === 'local' ? 'bg-muted text-foreground' : 'bg-background/50 text-muted-foreground'
          ]">{{ localMods.length }}</span>
        </button>
        <button
          @click="activeTab = 'browse'"
          :class="[
            'px-4 py-1.5 text-sm font-medium transition-all rounded-md flex items-center gap-2',
            activeTab === 'browse' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground hover:bg-background/50'
          ]"
        >
          <Download class="w-4 h-4" />
          {{ t('instanceMods.browseMods', 'Browse & Download') }}
        </button>
      </div>
    </div>
      
    <!-- Error Message -->
    <div v-if="error && !installModProject" class="m-4 mb-0 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg shrink-0">
      <p class="text-sm text-red-600 dark:text-red-400 font-medium">{{ error }}</p>
    </div>
      
    <!-- Content Area -->
    <div class="flex-1 overflow-hidden relative flex flex-col">
      <!-- LOCAL MODS TAB -->
      <div v-if="activeTab === 'local'" class="flex-1 overflow-y-auto p-5 flex flex-col gap-4">
        <!-- Local Search -->
        <div class="relative shrink-0 shadow-sm">
          <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <input
            v-model="localSearchQuery"
            type="text"
            :placeholder="t('instanceMods.searchLocal', 'Search installed mods...')"
            class="w-full pl-9 pr-4 py-2.5 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-lg text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 focus:outline-none focus:ring-2 focus:ring-primary/50 transition-all"
          />
        </div>

        <div v-if="isLoadingLocal" class="flex flex-col items-center justify-center py-16 flex-1 text-center">
          <Loader2 class="h-8 w-8 animate-spin text-primary mb-3" />
          <span class="text-muted-foreground font-medium">Scanning mods directory...</span>
        </div>
        
        <div v-else-if="localMods.length === 0" class="flex flex-col items-center justify-center py-16 text-center flex-1 bg-muted/30 rounded-xl border border-dashed">
          <div class="p-4 bg-background rounded-full shadow-sm border mb-4">
            <UploadCloud class="h-8 w-8 text-primary" />
          </div>
          <h3 class="text-lg font-semibold mb-1">No mods installed</h3>
          <p class="text-sm text-muted-foreground max-w-[250px] mb-4">
            Drag and drop <code class="bg-muted px-1.5 py-0.5 rounded text-xs font-mono">.jar</code> files here, or switch to the Browse tab to download.
          </p>
          <button @click="activeTab = 'browse'" class="px-4 py-2 bg-primary text-primary-foreground rounded-lg shadow-sm hover:bg-primary/90 text-sm font-medium transition-colors">
            Browse Mods
          </button>
        </div>

        <div v-else-if="filteredLocalMods.length === 0" class="flex flex-col items-center justify-center py-16 text-center flex-1">
          <Search class="h-10 w-10 text-muted-foreground/50 mb-3 mx-auto" />
          <p class="text-sm text-muted-foreground font-medium">No matching mods found.</p>
        </div>
        
        <div v-else class="grid grid-cols-1 md:grid-cols-2 gap-3 flex-1 pb-4 content-start">
          <div
            v-for="mod in filteredLocalMods"
            :key="mod.filename"
            :class="[
              'flex items-center justify-between p-3.5 rounded-xl border transition-all hover:shadow-sm',
              mod.enabled ? 'bg-card hover:border-primary/30' : 'bg-muted/40 border-dashed opacity-75 grayscale hover:grayscale-0 hover:opacity-100'
            ]"
          >
            <div class="flex items-center gap-3.5 min-w-0">
              <div :class="['p-2 rounded-lg shrink-0 flex items-center justify-center overflow-hidden w-10 h-10', mod.enabled ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground']">
                <img v-if="mod.iconUrl" :src="convertFileSrc(mod.iconUrl)" class="w-full h-full object-contain" />
                <Package v-else class="h-5 w-5" />
              </div>
              <div class="min-w-0" :class="!mod.enabled ? 'line-through opacity-70' : ''">
                <p class="font-medium text-sm text-neutral-900 dark:text-white truncate" :title="mod.name || mod.filename">{{ mod.name || mod.filename }}</p>
                <div class="text-xs text-muted-foreground mt-0.5 flex items-center gap-2">
                  <span v-if="mod.version" class="px-1.5 py-0.5 rounded-md bg-muted/50 border font-mono text-[10px]">{{ mod.version }}</span>
                  <span>{{ formatSize(mod.size) }}</span>
                </div>
              </div>
            </div>
            
            <div class="flex items-center gap-2 shrink-0 ml-4">
              <button
                @click="toggleMod(mod)"
                class="px-3 py-1.5 text-xs font-semibold rounded-lg transition-all shadow-sm"
                :class="mod.enabled 
                  ? 'bg-amber-100 text-amber-700 hover:bg-amber-200 dark:bg-amber-900/40 dark:text-amber-400 border border-amber-200 dark:border-amber-900/60' 
                  : 'bg-green-100 text-green-700 hover:bg-green-200 dark:bg-green-900/40 dark:text-green-400 border border-green-200 dark:border-green-900/60'"
              >
                {{ mod.enabled ? t('instanceMods.disable', 'Disable') : t('instanceMods.enable', 'Enable') }}
              </button>
              <button
                @click="deleteMod(mod)"
                class="p-1.5 text-muted-foreground hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-all"
                title="Delete mod"
              >
                <Trash2 class="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- BROWSE MODS TAB -->
      <div v-else-if="activeTab === 'browse'" class="flex-1 overflow-y-auto p-5 flex flex-col gap-4">
        <div class="flex gap-2">
          <div class="relative w-40 shrink-0 shadow-sm">
            <select
              v-model="selectedSource"
              class="w-full h-full pl-3 pr-8 py-2.5 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-lg text-sm font-medium text-neutral-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-primary/50 transition-all appearance-none cursor-pointer"
            >
              <option value="modrinth">🟢 Modrinth</option>
              <option value="curseforge">🟠 CurseForge</option>
            </select>
            <div class="absolute inset-y-0 right-0 flex items-center px-3 pointer-events-none text-muted-foreground">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path></svg>
            </div>
          </div>

          <div class="relative flex-1 shadow-sm">
            <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-5 w-5 text-muted-foreground" />
            <input
              v-model="searchQuery"
              type="text"
              :placeholder="`Search ${selectedSource === 'modrinth' ? 'Modrinth' : 'CurseForge'} for ${instance?.loaderType} ${instance?.mcVersion} mods...`"
              class="w-full pl-10 pr-4 py-2.5 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-lg text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 focus:outline-none focus:ring-2 focus:ring-primary/50 transition-all"
              @keyup.enter="searchMods"
            />
          </div>
          <button
            @click="searchMods"
            :disabled="isSearching || !searchQuery.trim()"
            class="px-6 py-2.5 bg-primary text-primary-foreground rounded-lg font-medium hover:bg-primary/90 disabled:opacity-50 transition-all shadow-sm flex items-center justify-center min-w-[100px] text-sm"
          >
            <Loader2 v-if="isSearching" class="h-4 w-4 animate-spin" />
            <span v-else>Search</span>
          </button>
        </div>

        <!-- Browse Results -->
        <div v-if="isSearching" class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4 pb-4">
          <div v-for="i in 6" :key="i" class="rounded-lg border bg-card p-4 animate-pulse">
            <div class="flex gap-3 mb-3">
              <div class="h-12 w-12 rounded-lg bg-muted"></div>
              <div class="flex-1 space-y-2">
                <div class="h-4 bg-muted rounded w-3/4"></div>
                <div class="h-3 bg-muted rounded w-1/4"></div>
              </div>
            </div>
            <div class="space-y-2">
              <div class="h-3 bg-muted rounded w-full"></div>
              <div class="h-3 bg-muted rounded w-5/6"></div>
            </div>
          </div>
        </div>

        <div v-else-if="searchQuery && searchResults.length === 0 && !error" class="flex flex-col items-center justify-center py-16 flex-1 opacity-70">
          <Package class="h-16 w-16 text-muted-foreground/50 mb-4" />
          <h3 class="text-lg font-semibold mb-2">No mods found</h3>
          <p class="text-sm text-muted-foreground">Try adjusting your search term.</p>
        </div>

        <div v-else-if="searchResults.length > 0" class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3 pb-4">
          <div
            v-for="mod in searchResults"
            :key="`${mod.source}-${mod.project_id}`"
            class="group rounded-lg border bg-card p-3 hover:shadow-sm hover:border-primary/40 transition-all flex flex-col"
          >
            <!-- Mod Header -->
            <div class="flex items-start gap-2.5 mb-2">
              <div class="flex h-10 w-10 items-center justify-center rounded-lg bg-muted shrink-0 shadow-sm overflow-hidden border border-border/50">
                <img
                  v-if="mod.icon_url"
                  :src="mod.icon_url"
                  :alt="mod.title"
                  class="h-full w-full object-cover"
                  @error="($event.target as HTMLImageElement).style.display = 'none'"
                />
                <Package v-else class="h-5 w-5 text-muted-foreground/50" />
              </div>
              <div class="flex-1 min-w-0">
                <div class="flex items-start justify-between gap-2">
                  <h3 class="font-semibold truncate text-sm leading-tight" :title="mod.title">{{ mod.title }}</h3>
                </div>
                <p class="text-[11px] text-muted-foreground mt-0.5 truncate">
                  by {{ mod.author }} • <Download class="inline h-2.5 w-2.5 mr-0.5" />{{ formatDownloads(mod.downloads) }}
                </p>
              </div>
            </div>
            
            <p class="text-xs text-muted-foreground line-clamp-2 mb-2.5 flex-1 leading-snug">{{ mod.description }}</p>

            <!-- Actions -->
            <div class="flex items-center justify-between pt-2.5 border-t mt-auto">
              <div class="flex flex-wrap gap-1 flex-1 overflow-hidden pr-2">
                <span :class="['px-1.5 py-0.5 rounded text-[9px] font-semibold border', sourceBadges[mod.source as keyof typeof sourceBadges].bg, sourceBadges[mod.source as keyof typeof sourceBadges].text]">
                  {{ sourceBadges[mod.source as keyof typeof sourceBadges].label }}
                </span>
                <span v-for="loader in mod.loaders.slice(0, 2)" :key="loader" :class="['inline-flex items-center rounded-full px-1.5 py-0.5 text-[9px] font-semibold border', getLoaderBadgeClass(loader)]">
                  {{ loader }}
                </span>
              </div>
              <div class="flex gap-1.5 shrink-0">
                <button
                  @click="selectedMod = mod"
                  class="px-2 py-1 text-[11px] border rounded hover:bg-muted transition-colors font-medium"
                >
                  {{ t('instanceMods.details', 'Details') }}
                </button>
                <button
                  @click="openInstallModal(mod)"
                  class="flex items-center gap-1 px-2 py-1 text-[11px] bg-primary text-primary-foreground rounded hover:bg-primary/90 transition-colors font-medium shadow-sm"
                >
                  <Download class="h-3 w-3" />
                  {{ t('instanceMods.install', 'Install') }}
                </button>
              </div>
            </div>
          </div>
        </div>

        <div v-else class="flex flex-col items-center justify-center py-20 text-center opacity-60 flex-1">
          <Search class="h-16 w-16 text-muted-foreground/40 mb-4" />
          <h3 class="text-lg font-medium mb-2">{{ t('instanceMods.searchTitle', 'Search for Mods') }}</h3>
          <p class="text-sm text-muted-foreground max-w-sm">
            {{ t('instanceMods.searchDesc', 'Find and install mods directly for this instance.') }}
          </p>
        </div>
      </div>
    </div>

    <!-- Mod Details Modal -->
    <Teleport to="body">
      <div v-if="selectedMod" class="fixed inset-0 z-[100] flex items-center justify-center pointer-events-none">
        <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto" @click="selectedMod = null"></div>
        <div class="relative z-10 w-full max-w-xl border bg-white dark:bg-zinc-900 shadow-2xl rounded-xl pointer-events-auto flex flex-col overflow-hidden max-h-[85vh]">
          <div class="p-4 border-b flex items-start gap-4 bg-muted/30">
            <div class="flex h-16 w-16 items-center justify-center rounded-xl bg-white dark:bg-zinc-800 shrink-0 border shadow-sm">
              <img v-if="selectedMod?.icon_url" :src="selectedMod?.icon_url" :alt="selectedMod?.title" class="h-16 w-16 rounded-xl object-cover" />
              <Package v-else class="h-8 w-8 text-muted-foreground" />
            </div>
            <div class="flex-1 min-w-0 pt-1">
              <h3 class="font-bold text-xl text-neutral-900 dark:text-white truncate">{{ selectedMod?.title }}</h3>
              <p class="text-sm text-muted-foreground mt-1">
                by {{ selectedMod?.author }} • {{ formatDownloads(selectedMod?.downloads || 0) }} downloads
              </p>
            </div>
          </div>
          <div class="p-5 overflow-y-auto max-h-[50vh]">
            <div class="flex items-center gap-4 mb-4 pb-4 border-b text-xs text-muted-foreground">
              <div><strong class="text-foreground">Source:</strong> <span class="capitalize">{{ selectedMod?.source }}</span></div>
              <div><strong class="text-foreground">Project ID:</strong> {{ selectedMod?.project_id }}</div>
              <div v-if="selectedMod?.file_id"><strong class="text-foreground">File ID:</strong> {{ selectedMod?.file_id }}</div>
            </div>

            <h4 class="text-sm font-semibold mb-2">{{ t('instanceMods.description', 'Description') }}</h4>
            <p class="text-sm text-muted-foreground mb-6 leading-relaxed">{{ selectedMod?.description }}</p>
            
          </div>
          <div class="p-4 border-t bg-muted/10 flex justify-end gap-3">
            <button @click="selectedMod = null" class="px-4 py-2 text-sm font-medium border rounded-lg hover:bg-muted transition-colors">
              {{ t('instanceMods.close', 'Close') }}
            </button>
            <button @click="selectedMod && openInstallModal(selectedMod); selectedMod = null;" class="flex items-center gap-2 px-5 py-2 text-sm font-medium bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-all shadow-sm">
              <Download class="h-4 w-4" /> {{ t('instanceMods.installMod', 'Install Mod') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Version Selection Install Modal -->
    <Teleport to="body">
      <div v-if="installModProject" class="fixed inset-0 z-[100] flex items-center justify-center pointer-events-none">
        <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto" @click="!isInstalling && (installModProject = null)"></div>
        <div class="relative z-10 w-full max-w-md border bg-white dark:bg-zinc-900 shadow-2xl rounded-xl pointer-events-auto flex flex-col overflow-hidden">
          <div class="p-4 border-b bg-muted/20">
            <h3 class="flex items-center gap-2 text-lg font-bold">
              <Download class="h-5 w-5 text-primary" />
              {{ t('instanceMods.installMod', 'Install Mod') }}
            </h3>
            <p class="mt-1 text-sm text-muted-foreground">
              {{ t('instanceMods.installingTo', 'Installing to') }} <strong class="text-foreground">{{ instance?.name }}</strong>
            </p>
          </div>

          <div class="p-5 space-y-5">
            <!-- Success State -->
            <div v-if="installSuccess" class="flex flex-col items-center justify-center py-6 text-center space-y-3">
              <div class="h-12 w-12 rounded-full bg-green-100 dark:bg-green-900/30 flex items-center justify-center text-green-600 dark:text-green-400">
                <CheckCircle2 class="h-6 w-6" />
              </div>
              <div>
                <h3 class="font-medium text-lg">{{ t('instanceMods.installComplete', 'Installation Complete') }}</h3>
                <p class="text-sm text-muted-foreground mt-1">{{ t('instanceMods.installCompleteDesc', 'The mod and its dependencies have been installed.') }}</p>
              </div>
            </div>

            <template v-else>
              <!-- File Version Selection -->
              <div class="space-y-2">
                <label class="text-sm font-medium text-foreground">{{ t('instanceMods.modVersion', 'Mod Version') }}</label>
                <div class="relative">
                  <select 
                    v-model="selectedModFileId" 
                    class="w-full p-2.5 border border-neutral-300 dark:border-zinc-700 rounded-lg bg-white dark:bg-zinc-800 text-sm text-neutral-900 dark:text-white outline-none focus:ring-2 focus:ring-primary/50 shadow-sm disabled:opacity-60"
                    :disabled="isLoadingModFiles || availableModFiles.length === 0 || isInstalling"
                  >
                    <option v-if="isLoadingModFiles" value="">{{ t('instanceMods.loadingFiles', 'Loading compatible files...') }}</option>
                    <option v-else-if="availableModFiles.length === 0" value="">{{ t('instanceMods.noCompatibleFiles', 'No compatible files found') }}</option>
                    <template v-else>
                      <optgroup v-for="group in groupedModFiles" :key="group.label" :label="group.label">
                        <option v-for="file in group.items" :key="file.id" :value="file.id">
                          {{ file.filename }}
                        </option>
                      </optgroup>
                    </template>
                  </select>
                  <Loader2 v-if="isLoadingModFiles" class="absolute right-3 top-1/2 -translate-y-1/2 h-4 w-4 animate-spin text-muted-foreground" />
                </div>
                <p v-if="availableModFiles.length > 0 && selectedModFileId" class="text-xs text-muted-foreground mt-1 flex items-center justify-between">
                  <span>{{ t('instanceMods.requiredDependencies', 'Required Dependencies') }}: {{ availableModFiles.find(f => f.id === selectedModFileId)?.dependencies?.filter(d => d.required).length || 0 }}</span>
                </p>
              </div>

              <!-- Error Message -->
              <div v-if="error" class="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-600 dark:text-red-400">
                {{ error }}
              </div>

              <!-- Progress Bar -->
              <div v-if="isInstalling" class="space-y-2 p-4 border rounded-lg bg-muted/20">
                <div class="flex justify-between text-xs font-medium">
                  <span class="text-muted-foreground truncate pr-4">{{ installStatusText }}</span>
                  <span class="text-primary shrink-0">{{ installProgress }}%</span>
                </div>
                <div class="h-2 w-full bg-secondary rounded-full overflow-hidden">
                  <div class="h-full bg-primary transition-all duration-300 ease-out rounded-full" :style="`width: ${installProgress}%`"></div>
                </div>
              </div>
            </template>
          </div>

          <div class="p-4 border-t bg-muted/10 flex justify-end gap-3" v-if="!installSuccess">
            <button @click="installModProject = null" class="px-4 py-2 border rounded-lg text-sm font-medium hover:bg-muted transition-colors disabled:opacity-50" :disabled="isInstalling">
              {{ t('instanceMods.cancel', 'Cancel') }}
            </button>
            <button @click="installMod" :disabled="!selectedModFileId || isInstalling" class="px-6 py-2 bg-primary text-primary-foreground rounded-lg text-sm font-medium hover:bg-primary/90 disabled:opacity-50 transition-all shadow-sm flex items-center gap-2">
              <Loader2 v-if="isInstalling" class="h-4 w-4 animate-spin" />
              <Download v-else class="h-4 w-4" /> {{ t('instanceMods.install', 'Install') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    </DialogContent>

  <AlertDialog :open="duplicateDialogState.show" @update:open="
    duplicateDialogState.show = $event; 
    if (!$event && duplicateDialogState.resolve) { 
      duplicateDialogState.resolve('cancel'); 
      duplicateDialogState.resolve = null; 
    }
  ">
    <div class="flex flex-col gap-5">
      <div class="flex gap-4">
        <div class="shrink-0 w-10 h-10 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
          <AlertTriangle class="h-5 w-5 text-red-600 dark:text-red-400" />
        </div>
        <div class="flex-1 space-y-2 pt-0.5">
          <AlertDialogTitle class="text-lg font-bold text-neutral-900 dark:text-white">
            {{ t('instanceMods.duplicateWarningTitle', 'Duplicate Mod Detected') }}
          </AlertDialogTitle>
          <AlertDialogDescription class="text-sm text-neutral-600 dark:text-neutral-300">
            {{ t('instanceMods.duplicateWarningDesc', 'Similar mods are already installed in your instance:') }}
          </AlertDialogDescription>
          <div class="mt-3 bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-200 dark:border-zinc-700/50 rounded-lg p-3 max-h-32 overflow-y-auto space-y-1.5">
            <div v-for="name in duplicateDialogState.duplicates" :key="name" class="flex items-start gap-2 text-sm text-neutral-700 dark:text-neutral-300">
              <span class="w-1.5 h-1.5 rounded-full bg-neutral-400 dark:bg-neutral-500 shrink-0 mt-1.5"></span>
              <span class="font-mono break-all">{{ name }}</span>
            </div>
          </div>
          <p class="text-sm font-medium text-neutral-800 dark:text-neutral-200 mt-4">
            {{ t('instanceMods.duplicateWarningAction', 'Do you want to OVERWRITE them with the new version?') }}
          </p>
        </div>
      </div>
      <div class="flex justify-end gap-3 pt-2 border-t border-neutral-100 dark:border-zinc-800">
        <button
          class="px-4 py-2 rounded-lg border border-neutral-200 dark:border-zinc-700 text-sm font-medium hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors focus:ring-2 focus:ring-neutral-200 outline-none"
          @click="
            if (duplicateDialogState.resolve) { duplicateDialogState.resolve('cancel'); duplicateDialogState.resolve = null; }
            duplicateDialogState.show = false;
          "
        >
          {{ t('instanceMods.cancel', 'Cancel') }}
        </button>
        <button
          class="px-4 py-2 rounded-lg bg-neutral-800 text-white dark:bg-neutral-200 dark:text-neutral-900 text-sm font-medium hover:bg-neutral-700 dark:hover:bg-neutral-300 transition-colors shadow-sm focus:ring-2 focus:ring-neutral-500 outline-none"
          @click="
            if (duplicateDialogState.resolve) { duplicateDialogState.resolve('keep'); duplicateDialogState.resolve = null; }
            duplicateDialogState.show = false;
          "
        >
          {{ t('instanceMods.keepBoth', 'Keep Both') }}
        </button>
        <button
          class="px-6 py-2 rounded-lg bg-red-600 text-white text-sm font-medium hover:bg-red-700 transition-colors shadow-sm focus:ring-2 focus:ring-red-600/50 outline-none flex items-center gap-2"
          @click="
            if (duplicateDialogState.resolve) { duplicateDialogState.resolve('overwrite'); duplicateDialogState.resolve = null; }
            duplicateDialogState.show = false;
          "
        >
          <AlertTriangle class="h-4 w-4" />
          {{ t('instanceMods.overwrite', 'Overwrite') }}
        </button>
      </div>
    </div>
  </AlertDialog>
</template>
