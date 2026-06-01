<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Package, Search, Download, Trash2, ToggleLeft, ToggleRight, Loader2 } from "@lucide/vue";
import { DialogContent, DialogTitle, DialogDescription } from "./ui/dialog";

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
const isLoadingLocal = ref(false);
const isLoadingSearch = ref(false);
const localMods = ref<LocalModItem[]>([]);
const searchResults = ref<UnifiedModProject[]>([]);
const searchQuery = ref("");
const error = ref<string | null>(null);
const installingMod = ref<string | null>(null);
const selectedSource = ref<"modrinth" | "curseforge">("curseforge");

// Version Selection State
const selectedModForVersionSelection = ref<UnifiedModProject | null>(null);
const availableModFiles = ref<UnifiedModFile[]>([]);
const isLoadingModFiles = ref(false);
const selectedModFileId = ref<string>("");
const isConfirmingInstall = ref(false);

// Source badges
const sourceBadges = {
  modrinth: { bg: "bg-green-100 dark:bg-green-900/40", text: "text-green-700 dark:text-green-300", label: "Modrinth" },
  curseforge: { bg: "bg-orange-100 dark:bg-orange-900/40", text: "text-orange-700 dark:text-orange-300", label: "CurseForge" },
};

// Load local mods when modal opens or tab changes
watch(() => props.open, async (isOpen) => {
  if (isOpen && props.instance) {
    await loadLocalMods();
  }
});

watch(activeTab, async (tab) => {
  if (tab === "local" && props.instance) {
    await loadLocalMods();
  }
});

// Load local mods
async function loadLocalMods() {
  if (!props.instance) return;
  
  isLoadingLocal.value = true;
  error.value = null;
  
  try {
    localMods.value = await invoke<LocalModItem[]>("get_installed_mods", {
      versionId: props.instance.id,
    });
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    localMods.value = [];
  } finally {
    isLoadingLocal.value = false;
  }
}

// Toggle mod enabled/disabled
async function toggleMod(mod: LocalModItem) {
  if (!props.instance) return;
  
  try {
    await invoke("toggle_mod_status", {
      versionId: props.instance.id,
      filename: mod.filename,
      enable: !mod.enabled,
    });
    // Refresh the list
    await loadLocalMods();
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
  }
}

// Delete mod
async function deleteMod(mod: LocalModItem) {
  if (!props.instance) return;
  
  if (!confirm(`Are you sure you want to delete "${mod.filename}"?`)) {
    return;
  }
  
  try {
    await invoke("delete_local_mod", {
      versionId: props.instance.id,
      filename: mod.filename,
    });
    // Remove from list
    localMods.value = localMods.value.filter(m => m.filename !== mod.filename);
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
  }
}

// Search mods
let searchTimeout: ReturnType<typeof setTimeout> | null = null;

watch(searchQuery, () => {
  if (searchTimeout) clearTimeout(searchTimeout);
  if (searchQuery.value.trim()) {
    searchTimeout = setTimeout(() => searchMods(), 500);
  } else {
    searchResults.value = [];
  }
});

async function searchMods() {
  if (!searchQuery.value.trim() || !props.instance) return;
  
  isLoadingSearch.value = true;
  error.value = null;
  
  try {
    const command = selectedSource.value === "modrinth"
      ? "search_modrinth"
      : "search_curseforge";
    
    // Use instance's mcVersion and loaderType for filtering
    const results = await invoke<UnifiedModProject[]>(command, {
      query: searchQuery.value,
      mcVersion: props.instance.mcVersion,
      loader: props.instance.loaderType.toLowerCase(),
    });
    
    searchResults.value = results;
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    searchResults.value = [];
  } finally {
    isLoadingSearch.value = false;
  }
}

// Fetch available mod files for selection
async function fetchModFilesForSelection(mod: UnifiedModProject) {
  if (!props.instance) return;
  
  installingMod.value = mod.project_id;
  selectedModForVersionSelection.value = mod;
  isLoadingModFiles.value = true;
  availableModFiles.value = [];
  selectedModFileId.value = "";
  error.value = null;
  
  try {
    const command = mod.source === "modrinth"
      ? "get_modrinth_mod_files"
      : "get_cf_mod_files";
    
    availableModFiles.value = await invoke<UnifiedModFile[]>(command, {
      projectId: mod.project_id,
      mcVersion: props.instance.mcVersion,
      loader: props.instance.loaderType.toLowerCase(),
    });
    
    if (availableModFiles.value.length > 0) {
      selectedModFileId.value = availableModFiles.value[0].id;
    }
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    selectedModForVersionSelection.value = null;
  } finally {
    isLoadingModFiles.value = false;
    installingMod.value = null;
  }
}

// Confirm install selected version
async function confirmInstallMod() {
  const mod = selectedModForVersionSelection.value;
  const file = availableModFiles.value.find(f => f.id === selectedModFileId.value);
  if (!mod || !file || !props.instance) return;
  
  isConfirmingInstall.value = true;
  error.value = null;
  
  try {
    // Install to instance
    await invoke("install_mod_to_instance", {
      versionId: props.instance.id,
      modSource: mod.source,
      projectId: mod.project_id,
      fileId: file.id,
      downloadUrl: file.download_url,
    });
    
    // Show success and switch to local tab
    alert(`Mod "${mod.title}" installed successfully!`);
    selectedModForVersionSelection.value = null;
    activeTab.value = "local";
    await loadLocalMods();
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
  } finally {
    isConfirmingInstall.value = false;
  }
}

// Format file size
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// Format download count
function formatDownloads(count: number): string {
  if (count >= 1000000) return `${(count / 1000000).toFixed(1)}M`;
  if (count >= 1000) return `${(count / 1000).toFixed(1)}K`;
  return count.toString();
}

// Get loader badge class
function getLoaderBadgeClass(loader: string): string {
  switch (loader.toLowerCase()) {
    case "fabric": return "bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300";
    case "forge": return "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300";
    case "neoforge": return "bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300";
    default: return "bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300";
  }
}
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-4xl max-h-[85vh] p-0 flex flex-col gap-0 overflow-hidden">
      <!-- Header -->
      <div class="p-4 border-b shrink-0 pr-10">
        <DialogTitle>{{ $t('instanceMods.title') }}</DialogTitle>
        <DialogDescription>
          {{ instance?.name }} ({{ instance?.mcVersion }} - {{ instance?.loaderType }})
        </DialogDescription>
      </div>
        
        <!-- Tabs -->
        <div class="flex border-b">
          <button
            @click="activeTab = 'local'"
            :class="[
              'flex-1 py-2 text-sm font-medium transition-colors border-b-2 -mb-px',
              activeTab === 'local'
                ? 'border-primary text-primary'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            ]"
          >
            <Package class="inline-block w-4 h-4 mr-2" />
            Local Mods ({{ localMods.length }})
          </button>
          <button
            @click="activeTab = 'browse'"
            :class="[
              'flex-1 py-2 text-sm font-medium transition-colors border-b-2 -mb-px',
              activeTab === 'browse'
                ? 'border-primary text-primary'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            ]"
          >
            <Download class="inline-block w-4 h-4 mr-2" />
            Browse & Download
          </button>
        </div>
        
        <!-- Error Message -->
        <div v-if="error" class="mx-4 mt-4 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
          <p class="text-sm text-red-600 dark:text-red-400">{{ error }}</p>
        </div>
        
        <!-- Tab Content -->
        <div class="flex-1 overflow-y-auto p-4">
          <!-- Local Mods Tab -->
          <div v-if="activeTab === 'local'">
            <div v-if="isLoadingLocal" class="flex items-center justify-center py-12">
              <Loader2 class="h-6 w-6 animate-spin text-primary" />
              <span class="ml-3 text-muted-foreground">Loading mods...</span>
            </div>
            
            <div v-else-if="localMods.length === 0" class="flex flex-col items-center justify-center py-12 text-center">
              <Package class="h-12 w-12 text-muted-foreground mb-4" />
              <h3 class="text-lg font-semibold mb-2">No mods installed</h3>
              <p class="text-sm text-muted-foreground mb-4">Switch to "Browse & Download" to find mods for this instance.</p>
              <button
                @click="activeTab = 'browse'"
                class="px-3 py-1.5 text-sm bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors"
              >
                Browse Mods
              </button>
            </div>
            
            <div v-else class="space-y-2">
              <div
                v-for="mod in localMods"
                :key="mod.filename"
                class="flex items-center justify-between p-3 rounded-lg border bg-card hover:bg-muted/50 transition-colors"
              >
                <div class="flex items-center gap-3 min-w-0">
                  <button
                    @click="toggleMod(mod)"
                    class="shrink-0 text-muted-foreground hover:text-foreground transition-colors"
                    :title="mod.enabled ? 'Disable mod' : 'Enable mod'"
                  >
                    <ToggleRight v-if="mod.enabled" class="h-5 w-5 text-green-500" />
                    <ToggleLeft v-else class="h-5 w-5" />
                  </button>
                  <div class="min-w-0">
                    <p class="font-medium text-neutral-900 dark:text-white truncate">{{ mod.filename }}</p>
                    <p class="text-xs text-muted-foreground">{{ formatSize(mod.size) }}</p>
                  </div>
                </div>
                
                <button
                  @click="deleteMod(mod)"
                  class="shrink-0 p-2 text-muted-foreground hover:text-red-500 transition-colors"
                  title="Delete mod"
                >
                  <Trash2 class="h-4 w-4" />
                </button>
              </div>
            </div>
          </div>
          
          <!-- Browse & Download Tab -->
          <div v-if="activeTab === 'browse'">
            <!-- Source Toggle -->
            <div class="flex gap-2 mb-4">
              <button
                @click="selectedSource = 'modrinth'; searchResults = []"
                :class="[
                  'flex items-center gap-2 px-3 py-1.5 rounded-md text-sm font-medium transition-colors',
                  selectedSource === 'modrinth'
                    ? 'bg-green-600 text-white'
                    : 'bg-muted text-muted-foreground hover:bg-muted/80'
                ]"
              >
                🟢 Modrinth
              </button>
              <button
                @click="selectedSource = 'curseforge'; searchResults = []"
                :class="[
                  'flex items-center gap-2 px-3 py-1.5 rounded-md text-sm font-medium transition-colors',
                  selectedSource === 'curseforge'
                    ? 'bg-orange-600 text-white'
                    : 'bg-muted text-muted-foreground hover:bg-muted/80'
                ]"
              >
                🟠 CurseForge
              </button>
            </div>
            
            <!-- Search Input -->
            <div class="flex gap-2 mb-4">
              <div class="relative flex-1">
                <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                <input
                  v-model="searchQuery"
                  type="text"
                  :placeholder="`Search ${selectedSource === 'modrinth' ? 'Modrinth' : 'CurseForge'} for ${instance?.loaderType} ${instance?.mcVersion} mods...`"
                  class="w-full pl-9 pr-4 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500"
                  @keyup.enter="searchMods"
                />
              </div>
              <button
                @click="searchMods"
                :disabled="isLoadingSearch || !searchQuery.trim()"
                class="px-3 py-1.5 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 transition-colors text-sm font-medium"
              >
                <Loader2 v-if="isLoadingSearch" class="h-4 w-4 animate-spin" />
                <Search v-else class="h-4 w-4" />
              </button>
            </div>
            
            <!-- Search Info -->
            <p class="text-xs text-muted-foreground mb-4">
              Searching for mods compatible with <span class="font-medium text-neutral-900 dark:text-white">{{ instance?.mcVersion }}</span> and <span class="font-medium text-neutral-900 dark:text-white">{{ instance?.loaderType }}</span>
            </p>
            
            <!-- Search Results -->
            <div v-if="isLoadingSearch" class="flex items-center justify-center py-8">
              <Loader2 class="h-6 w-6 animate-spin text-primary" />
              <span class="ml-3 text-muted-foreground">Searching...</span>
            </div>
            
            <div v-else-if="searchResults.length === 0 && searchQuery" class="text-center py-8">
              <Package class="h-10 w-10 text-muted-foreground mx-auto mb-3" />
              <p class="text-sm text-muted-foreground">No mods found. Try a different search term.</p>
            </div>
            
            <div v-else-if="searchResults.length > 0" class="grid grid-cols-1 md:grid-cols-2 gap-3">
              <div
                v-for="mod in searchResults"
                :key="`${mod.source}-${mod.project_id}`"
                class="flex items-start gap-3 p-3 rounded-lg border bg-card"
              >
                <div class="flex h-10 w-10 items-center justify-center rounded bg-muted shrink-0">
                  <img
                    v-if="mod.icon_url"
                    :src="mod.icon_url"
                    :alt="mod.title"
                    class="h-10 w-10 rounded object-cover"
                    @error="($event.target as HTMLImageElement).style.display = 'none'"
                  />
                  <Package v-else class="h-5 w-5 text-muted-foreground" />
                </div>
                
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <h4 class="font-medium text-sm text-neutral-900 dark:text-white truncate">{{ mod.title }}</h4>
                    <span
                      :class="[
                        'px-1.5 py-0.5 rounded text-[10px] font-semibold shrink-0',
                        sourceBadges[mod.source as keyof typeof sourceBadges].bg,
                        sourceBadges[mod.source as keyof typeof sourceBadges].text
                      ]"
                    >
                      {{ sourceBadges[mod.source as keyof typeof sourceBadges].label }}
                    </span>
                  </div>
                  <p class="text-xs text-muted-foreground line-clamp-1 mt-0.5">{{ mod.description }}</p>
                  <div class="flex items-center gap-2 mt-1">
                    <span class="text-xs text-muted-foreground flex items-center gap-0.5">
                      <Download class="h-3 w-3" />
                      {{ formatDownloads(mod.downloads) }}
                    </span>
                    <span
                      v-for="loader in mod.loaders.slice(0, 2)"
                      :key="loader"
                      :class="['px-1.5 py-0.5 rounded text-[10px]', getLoaderBadgeClass(loader)]"
                    >
                      {{ loader }}
                    </span>
                  </div>
                </div>
                
                <button
                  @click="fetchModFilesForSelection(mod)"
                  :disabled="installingMod === mod.project_id"
                  class="shrink-0 px-3 py-1.5 text-xs bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 transition-colors"
                >
                  <Loader2 v-if="installingMod === mod.project_id" class="h-3 w-3 animate-spin" />
                  <Download v-else class="h-3 w-3" />
                </button>
              </div>
            </div>
            
            <div v-else class="text-center py-8">
              <Search class="h-10 w-10 text-muted-foreground mx-auto mb-3" />
              <p class="text-sm text-muted-foreground">Enter a search term to find mods</p>
            </div>
          </div>
      </div>
  </DialogContent>
  
  <!-- Version Selection Sub-Modal -->
  <DialogContent :open="!!selectedModForVersionSelection" @update:open="!$event && (selectedModForVersionSelection = null)" class="max-w-md p-4">
    <DialogTitle>Select Version</DialogTitle>
    <DialogDescription>
      Choose which file of <span class="font-semibold text-neutral-900 dark:text-white">{{ selectedModForVersionSelection?.title }}</span> to install.
    </DialogDescription>
    
    <div v-if="isLoadingModFiles" class="flex justify-center py-8">
      <Loader2 class="h-6 w-6 animate-spin text-primary" />
    </div>
    
    <div v-else-if="availableModFiles.length === 0" class="py-8 text-center text-red-500 text-sm">
      No compatible files found for your game version and loader.
    </div>
    
    <div v-else class="space-y-4 mt-4">
      <div class="space-y-2">
        <label class="text-sm font-medium text-neutral-900 dark:text-neutral-100">Compatible Files</label>
        <select 
          v-model="selectedModFileId" 
          class="w-full p-2 border border-neutral-300 dark:border-zinc-700 rounded-md bg-white dark:bg-zinc-800 text-sm text-neutral-900 dark:text-white outline-none focus:ring-2 focus:ring-primary/50"
        >
          <option v-for="file in availableModFiles" :key="file.id" :value="file.id">
            {{ file.filename }} ({{ file.release_type }}) - {{ new Date(file.date).toLocaleDateString() }}
          </option>
        </select>
        <p class="text-xs text-muted-foreground mt-1 text-right">
          Sorted by newest first
        </p>
      </div>
      
      <div class="flex justify-end gap-2 pt-4 border-t">
        <button 
          @click="selectedModForVersionSelection = null" 
          class="px-3 py-1.5 border rounded-md text-sm font-medium hover:bg-muted transition-colors"
        >
          Cancel
        </button>
        <button 
          @click="confirmInstallMod" 
          :disabled="!selectedModFileId || isConfirmingInstall" 
          class="px-3 py-1.5 bg-primary text-primary-foreground rounded-md text-sm font-medium hover:bg-primary/90 disabled:opacity-50 transition-colors flex items-center gap-2"
        >
          <Loader2 v-if="isConfirmingInstall" class="h-4 w-4 animate-spin" />
          Download
        </button>
      </div>
    </div>
  </DialogContent>
</template>