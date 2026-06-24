<script setup lang="ts">
import { ref, onMounted, computed, watch, h, onActivated, onUnmounted } from "vue";
import DMultiSelect from "../ui/DMultiSelect.vue";
import DSelect from "../ui/DSelect.vue";
import { invoke } from "@tauri-apps/api/core";
import { trackEvent, getErrorType } from "../../utils/analytics";
import DInput from "../ui/DInput.vue";
import { Search, Check, AlertCircle, Loader2, Info } from "@lucide/vue";
import { getErrorMessage } from "../../utils/error";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { open } from '@tauri-apps/plugin-dialog';
import { toast } from "../../composables/useToast";
import { useTaskStatusReload } from "../../composables/useTaskStatusReload";

// UI Components
import { DialogContent, DialogTitle, DialogDescription } from "../../components/ui/dialog";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../../components/ui/alert-dialog";

const route = useRoute();
const { t, te } = useI18n();

// Types
interface InstanceItem {
  id: string;
  name: string;
  mcVersion: string;
  loaderType: string;
}


interface UnifiedCategory {
  id: string;
  name: string;
  icon: string;
}
interface UnifiedModProject {
  source: string;
  project_id: string;
  title: string;
  description: string;
  icon_url?: string;
  downloads: number;
  author: string;
  mc_versions: string[];
  loaders: string[];
}

interface UnifiedDependency {
  project_id: string;
  version_id?: string;
  required: boolean;
}

interface UnifiedModFile {
  id: string;
  filename: string;
  version_number: string;
  download_url: string;
  dependencies: UnifiedDependency[];
  mc_versions: string[];
  loaders: string[];
}

interface LocalModItem {
  filename: string;
  enabled: boolean;
  size: number;
  mod_id?: string;
  name?: string;
  version?: string;
  icon_url?: string;
}

// State
const installedInstances = ref<InstanceItem[]>([]);
const selectedInstanceId = ref<string>("");


const categories = ref<UnifiedCategory[]>([]);
const selectedCategory = ref<string[]>([]);
const currentPage = ref(0);
const hasMore = ref(true);
const isLoadingMore = ref(false);
const includeDependencies = ref(true);
const intersectionObserver = ref<IntersectionObserver | null>(null);
const bottomSentinel = ref<HTMLElement | null>(null);
const scrollContainer = ref<HTMLElement | null>(null);

const searchQuery = ref("");
const selectedMcVersion = ref<string[]>([]);
const selectedLoader = ref<string[]>([]);
const availableVersions = ref<string[]>([]);
const availableLoaders = ref<string[]>([]);
const searchSource = ref<"modrinth" | "curseforge">("curseforge");

const isSearching = ref(false);
const hasSearched = ref(false);
const searchResults = ref<UnifiedModProject[]>([]);
const error = ref("");

// Downloading State
const isDownloading = ref(false);


// Version/Install Dialog State
const showInstallDialog = ref(false);
const installFiles = ref<UnifiedModFile[]>([]);
const selectedGroupVersion = ref<string>("");
const selectedFileId = ref<string>("");
const isFetchingFiles = ref(false);

// Dependency Dialog State
const showDependencyDialog = ref(false);
const pendingMod = ref<UnifiedModProject | null>(null);
const pendingTargetDir = ref<string | null>(null);
const pendingDependencies = ref<UnifiedDependency[]>([]);
const pendingDependencyTitles = ref<Record<string, string>>({});
const isCheckingDependencies = ref(false);

// Duplicate Mod Dialog State
const showDuplicateWarning = ref(false);
const duplicateModsFound = ref<LocalModItem[]>([]);
const hasExactFilenameConflict = ref(false);

// Format numbers
const formatNumber = (num: number) => {
  return new Intl.NumberFormat("en-US", { notation: "compact", compactDisplay: "short" }).format(num);
};

const getCategoryName = (name: string) => {
  if (!name) return name;
  if (te(`modCategories.${name}`)) return t(`modCategories.${name}`);
  const lower = name.toLowerCase();
  if (te(`modCategories.${lower}`)) return t(`modCategories.${lower}`);
  const snake = lower.replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '');
  if (te(`modCategories.${snake}`)) return t(`modCategories.${snake}`);
  return name;
};

// Computed
const currentMcVersion = computed(() => {
  if (selectedInstanceId.value) {
    const inst = installedInstances.value.find((i) => i.id === selectedInstanceId.value);
    return inst ? inst.mcVersion : selectedMcVersion.value[0] || "";
  }
  return selectedMcVersion.value[0] || "";
});


const loadOptions = async () => {
  try {
    if (searchSource.value === "modrinth") {
      availableVersions.value = await invoke("get_modrinth_game_versions");
      availableLoaders.value = await invoke("get_modrinth_loaders");
    } else {
      availableVersions.value = await invoke("get_curseforge_game_versions");
      availableLoaders.value = await invoke("get_curseforge_loaders");
    }
  } catch (e) {
    console.error("Failed to fetch options:", e);
  }
};

watch(searchSource, () => {
  selectedCategory.value = [];
  selectedMcVersion.value = [];
  selectedLoader.value = [];
  loadCategories();
  loadOptions();
  performSearch(false);
});

const currentLoaders = computed(() => {
  if (selectedInstanceId.value) {
    const inst = installedInstances.value.find((i) => i.id === selectedInstanceId.value);
    return inst ? [inst.loaderType.toLowerCase()] : selectedLoader.value;
  }
  return selectedLoader.value;
});

const activeMcVersion = computed({
  get: () => {
    if (selectedInstanceId.value) {
      const inst = installedInstances.value.find((i) => i.id === selectedInstanceId.value);
      return inst ? [inst.mcVersion] : [];
    }
    return selectedMcVersion.value;
  },
  set: (val) => {
    if (!selectedInstanceId.value) selectedMcVersion.value = val;
  }
});

const activeLoader = computed({
  get: () => {
    if (selectedInstanceId.value) {
      const inst = installedInstances.value.find((i) => i.id === selectedInstanceId.value);
      return inst ? [inst.loaderType.toLowerCase()] : [];
    }
    return selectedLoader.value;
  },
  set: (val) => {
    if (!selectedInstanceId.value) selectedLoader.value = val;
  }
});

// Load Instances

async function loadCategories() {
  try {
    if (searchSource.value === "modrinth") {
      categories.value = await invoke<UnifiedCategory[]>("get_modrinth_categories");
    } else {
      categories.value = await invoke<UnifiedCategory[]>("get_curseforge_categories");
    }
  } catch (err) {
    console.error("Failed to load categories:", err);
    categories.value = [];
  }
}

async function loadInstances() {
  try {
    const instances = await invoke<InstanceItem[]>("scan_installed_instances");
    
    // Sort instances: vanilla instances go to the bottom
    instances.sort((a, b) => {
      const aIsVanilla = a.loaderType.toLowerCase() === "vanilla";
      const bIsVanilla = b.loaderType.toLowerCase() === "vanilla";
      if (aIsVanilla && !bIsVanilla) return 1;
      if (!aIsVanilla && bIsVanilla) return -1;
      return 0;
    });
    
    installedInstances.value = instances;

    // Check if deep linked
    if (route.query.instanceId && typeof route.query.instanceId === "string") {
      const exists = instances.some(i => i.id === route.query.instanceId);
      if (exists) {
        selectedInstanceId.value = route.query.instanceId;
      }
    }
  } catch (err) {
    console.error("Failed to load instances:", err);
  }
}

// --- Selection Options Computed ---
const instanceOptions = computed(() => [
  { label: t('downloads.noInstanceSelected'), value: '' },
  ...installedInstances.value.map(inst => ({
    label: `${inst.name} (${inst.mcVersion} - ${inst.loaderType.toLowerCase() === 'vanilla' ? t('downloads.noModLoaderInstalled') : inst.loaderType})`,
    value: inst.id,
    disabled: inst.loaderType.toLowerCase() === 'vanilla'
  }))
]);

const sourceOptions = [
  { label: 'CurseForge', value: 'curseforge' },
  { label: 'Modrinth', value: 'modrinth' }
];

const getValidMcVersions = (f: UnifiedModFile): string[] => {
  if (!f.mc_versions || f.mc_versions.length === 0) return ["Other"];
  const excluded = ['forge', 'fabric', 'quilt', 'neoforge', 'liteloader', 'rift', 'vanilla', 'client', 'server'];
  const valid = f.mc_versions.filter(v => {
    const lower = v.toLowerCase();
    if (lower.startsWith('java ')) return false;
    if (excluded.some(ex => lower.includes(ex))) return false;
    return true;
  });
  return valid.length > 0 ? valid : ["Other"];
};

const getLoaderName = (f: UnifiedModFile) => {
  const versions = f.mc_versions || [];
  const lowerVers = versions.map(v => v.toLowerCase());
  if (lowerVers.includes('forge')) return 'Forge';
  if (lowerVers.includes('neoforge')) return 'NeoForge';
  if (lowerVers.includes('fabric')) return 'Fabric';
  if (lowerVers.includes('quilt')) return 'Quilt';
  return 'Unknown';
};

const getLoaderIcon = (f: UnifiedModFile) => {
  let loader = getLoaderName(f).toLowerCase();
  if (loader !== 'unknown') {
    // using fabric.png since fabric-api.png might just be the api
    if (loader === 'fabric-api') loader = 'fabric';
    return h('img', { src: `/${loader}.png`, class: 'w-4 h-4 object-contain opacity-80', alt: loader });
  }
  return undefined;
};

const mcVersionOptions = computed(() => {
  if (!pendingMod.value) return [];
  const versions = new Set<string>(pendingMod.value.mc_versions);
  installFiles.value.forEach(f => {
    getValidMcVersions(f).forEach(v => versions.add(v));
  });
  
  return Array.from(versions).sort((a, b) => {
    if (a === 'Other') return 1;
    if (b === 'Other') return -1;
    return b.localeCompare(a, undefined, { numeric: true, sensitivity: 'base' });
  }).map(v => ({
    label: v,
    value: v
  }));
});

const filteredFileOptions = computed(() => {
  return installFiles.value
    .filter(f => getValidMcVersions(f).includes(selectedGroupVersion.value))
    .map(f => ({
      label: `${f.version_number} (${f.filename})`,
      value: f.id,
      icon: getLoaderIcon(f),
      group: getLoaderName(f)
    }));
});

async function fetchModFilesForSelectedVersion() {
  if (!pendingMod.value || !selectedGroupVersion.value) return;
  
  isFetchingFiles.value = true;
  installFiles.value = [];
  selectedFileId.value = "";
  
  try {
    let files: UnifiedModFile[] = [];
    if (pendingMod.value.source === "modrinth") {
      files = await invoke("get_modrinth_mod_files", {
        projectId: pendingMod.value.project_id,
        mcVersion: selectedGroupVersion.value,
        loaders: currentLoaders.value
      });
    } else {
      files = await invoke("get_cf_mod_files", {
        projectId: pendingMod.value.project_id,
        mcVersion: selectedGroupVersion.value,
        loaders: currentLoaders.value
      });
    }

    installFiles.value = files;
    
    const filtered = files.filter(f => getValidMcVersions(f).includes(selectedGroupVersion.value));
    if (filtered.length > 0) {
      selectedFileId.value = filtered[0].id;
      await checkDependenciesForSelectedFile();
    }
  } catch (err) {
    error.value = getErrorMessage(err);
  } finally {
    isFetchingFiles.value = false;
  }
}

watch(selectedGroupVersion, (newVal, oldVal) => {
  if (newVal && newVal !== oldVal && showInstallDialog.value) {
    fetchModFilesForSelectedVersion();
  }
});

async function performSearch(isLoadMore = false) {
  if (!isLoadMore) {
    currentPage.value = 0;
    searchResults.value = [];
    hasSearched.value = true;
    hasMore.value = true;
  }
  
  if (!hasMore.value) return;

  if (isLoadMore) {
    isLoadingMore.value = true;
  } else {
    isSearching.value = true;
  }
  error.value = "";

  try {
    let results: UnifiedModProject[] = [];
    const args = {
      query: searchQuery.value,
      mcVersions: selectedInstanceId.value 
        ? [installedInstances.value.find(i => i.id === selectedInstanceId.value)?.mcVersion].filter(Boolean) as string[]
        : selectedMcVersion.value,
      loaders: selectedInstanceId.value 
        ? [installedInstances.value.find(i => i.id === selectedInstanceId.value)?.loaderType.toLowerCase()].filter(Boolean) as string[]
        : selectedLoader.value,
      categories: selectedCategory.value,
      offset: currentPage.value * 20,
      limit: 20
    };

    if (args.mcVersions.length > 0 && args.loaders.length === 0) {
      toast.error("查询受限", "在使用游戏版本进行精确过滤时，必须同时选择至少一个模组加载器 (Mod Loader)", { transient: true });
      if (!isLoadMore) hasSearched.value = false;
      return;
    }
    
    if (searchSource.value === "modrinth") {
      results = await invoke<UnifiedModProject[]>("search_modrinth", args);
    } else {
      results = await invoke<UnifiedModProject[]>("search_curseforge", args);
    }
    
    if (results.length < 20) {
      hasMore.value = false;
    }
    
    if (isLoadMore) {
      searchResults.value.push(...results);
    } else {
      searchResults.value = results;
    }
    currentPage.value++;
  } catch (err) {
    error.value = getErrorMessage(err);
  } finally {
    isSearching.value = false;
    isLoadingMore.value = false;
  }
}

function setupIntersectionObserver() {
  if (intersectionObserver.value) intersectionObserver.value.disconnect();
  intersectionObserver.value = new IntersectionObserver((entries) => {
    if (entries[0].isIntersecting && !isSearching.value && !isLoadingMore.value && hasMore.value && hasSearched.value) {
      performSearch(true);
    }
  }, { 
    root: scrollContainer.value,
    rootMargin: '1200px' 
  });
  
  if (bottomSentinel.value) {
    intersectionObserver.value.observe(bottomSentinel.value);
  }
}


// Handle Card Click
async function handleCardClick(mod: UnifiedModProject) {
  pendingMod.value = mod;
  installFiles.value = [];
  selectedFileId.value = "";
  pendingDependencies.value = [];
  pendingDependencyTitles.value = {};
  showInstallDialog.value = true;
  includeDependencies.value = true; // default to true

  if (currentMcVersion.value && mod.mc_versions.includes(currentMcVersion.value)) {
    selectedGroupVersion.value = currentMcVersion.value;
  } else if (mod.mc_versions.length > 0) {
    let sorted = [...mod.mc_versions].sort((a, b) => b.localeCompare(a, undefined, { numeric: true, sensitivity: 'base' }));
    selectedGroupVersion.value = sorted[0];
  } else {
    selectedGroupVersion.value = "Other";
  }

  // The watcher on selectedGroupVersion will trigger fetchModFilesForSelectedVersion if it changed.
  // If it didn't change (e.g. re-opening the dialog with the same version), we need to trigger it manually.
  await fetchModFilesForSelectedVersion();
}

async function checkDependenciesForSelectedFile() {
  const file = installFiles.value.find(f => f.id === selectedFileId.value);
  if (!file) {
    pendingDependencies.value = [];
    return;
  }

  isCheckingDependencies.value = true;
  try {
    const requiredDeps = file.dependencies.filter(d => d.required);
    let missingDeps: UnifiedDependency[] = [];
    
    if (requiredDeps.length > 0) {
      if (selectedInstanceId.value) {
        const mapping = await invoke<{ [key: string]: string }>("get_instance_mod_mapping", {
          versionId: selectedInstanceId.value
        });
        
        missingDeps = requiredDeps.filter(dep => {
          const key = `${pendingMod.value?.source}_${dep.project_id}`;
          return !mapping[key];
        });
      } else {
        missingDeps = requiredDeps;
      }
    }
    pendingDependencies.value = missingDeps;
    
    // Resolve titles asynchronously
    missingDeps.forEach(async (dep) => {
      try {
        let title = "";
        if (pendingMod.value?.source === "modrinth") {
          const mod = await invoke<UnifiedModProject>("get_modrinth_mod_details", { projectId: dep.project_id });
          title = mod.title;
        } else {
          const mod = await invoke<UnifiedModProject>("get_cf_mod_details", { projectId: dep.project_id });
          title = mod.title;
        }
        pendingDependencyTitles.value[dep.project_id] = title;
        (dep as any).name = title;
      } catch (err) {
        console.error("Failed to resolve dependency title", dep.project_id, err);
      }
    });
  } catch(err) {
    console.error("Failed to check deps", err);
    pendingDependencies.value = [];
  } finally {
    isCheckingDependencies.value = false;
  }
}

async function startActualDownload(mod: UnifiedModProject, fileId: string, targetDir: string | null, dependenciesToInstall: UnifiedDependency[]) {
  isDownloading.value = true;
  showInstallDialog.value = false;
  showDependencyDialog.value = false;

  try {
    const latestFile = installFiles.value.find(f => f.id === fileId);
    if (!latestFile) throw new Error("File not found in list.");

    if (targetDir) {
      await invoke("task_create", {
        taskType: "install-mod",
        payload: {
          source: mod.source,
          project_id: mod.project_id,
          mod_name: mod.title,
          file_id: latestFile.id,
          download_url: latestFile.download_url,
          target_dir: targetDir,
          dependencies: dependenciesToInstall.length > 0 ? dependenciesToInstall : null,
        }
      });
    } else if (selectedInstanceId.value) {
      await invoke("task_create", {
        taskType: "install-mod",
        payload: {
          instance_id: selectedInstanceId.value,
          source: mod.source,
          project_id: mod.project_id,
          mod_name: mod.title,
          file_id: latestFile.id,
          download_url: latestFile.download_url,
          dependencies: dependenciesToInstall.length > 0 ? dependenciesToInstall : null,
        }
      });
    }
    trackEvent("Mod Install Completed", { name: mod.title, projectId: mod.project_id, versionId: latestFile.id });
  } catch (err) {
    trackEvent("Error Occurred", { context: "mod_install", error_type: getErrorType(err) });
    toast.error(getErrorMessage(err));
  } finally {
    isDownloading.value = false;
    pendingMod.value = null;
    pendingTargetDir.value = null;
    pendingDependencies.value = [];
  }
}

async function startDownloadToDir() {
  if (!pendingMod.value) return;
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Select Directory to Save Mod"
  });
  if (!selected || typeof selected !== "string") return;
  
  const deps = includeDependencies.value ? pendingDependencies.value : [];
  startActualDownload(pendingMod.value, selectedFileId.value, selected, deps);
}

async function startInstallToInstance() {
  if (!pendingMod.value || !selectedInstanceId.value) return;
  const deps = includeDependencies.value ? pendingDependencies.value : [];
  
  try {
    const installedMods = await invoke<LocalModItem[]>('get_installed_mods', { versionId: selectedInstanceId.value });
    const searchName = pendingMod.value.title.toLowerCase();
    const duplicates = installedMods.filter(m => {
      return (m.name && searchName.includes(m.name.toLowerCase())) ||
             (m.name && m.name.toLowerCase().includes(searchName)) ||
             (m.mod_id && m.mod_id === pendingMod.value?.project_id);
    });
    
    if (duplicates.length > 0) {
      const targetFile = installFiles.value.find(f => f.id === selectedFileId.value);
      hasExactFilenameConflict.value = duplicates.some(m => m.filename === targetFile?.filename);
      duplicateModsFound.value = duplicates;
      showDuplicateWarning.value = true;
      return;
    }
  } catch (err) {
    console.error("Failed to check duplicate mods", err);
  }
  
  startActualDownload(pendingMod.value, selectedFileId.value, null, deps);
}

async function handleReplaceOldMods() {
  if (!selectedInstanceId.value || duplicateModsFound.value.length === 0) return;
  
  try {
    for (const dup of duplicateModsFound.value) {
      await invoke('delete_local_mod', {
        versionId: selectedInstanceId.value,
        filename: dup.filename,
        isEnabled: dup.enabled
      });
    }
  } catch(err) {
    console.error("Failed to delete old duplicate mod", err);
  }
  showDuplicateWarning.value = false;
  const deps = includeDependencies.value ? pendingDependencies.value : [];
  startActualDownload(pendingMod.value!, selectedFileId.value, null, deps);
}

function handleKeepBothMods() {
  showDuplicateWarning.value = false;
  const deps = includeDependencies.value ? pendingDependencies.value : [];
  startActualDownload(pendingMod.value!, selectedFileId.value, null, deps);
}


watch([selectedInstanceId], () => {
  performSearch(false);
});


useTaskStatusReload(loadInstances);

onMounted(async () => {
  await loadInstances();
  loadCategories();
  loadOptions();
  performSearch(false);
  setupIntersectionObserver();
});

onActivated(async () => {
  await loadInstances();
});

onUnmounted(() => {
});
</script>

<template>
  <div ref="scrollContainer" class="h-full flex flex-col p-6 overflow-y-auto w-full relative">
    <div class="max-w-4xl mx-auto w-full space-y-6">
      
      <!-- Search Header -->
      <div class="bg-white/80 dark:bg-zinc-900/80 backdrop-blur-md p-4 rounded-xl border border-neutral-200 dark:border-zinc-800 shadow-sm space-y-4 relative z-10">
        <!-- Row 1: Instance Selector (Prominent) & Search Source -->
        <div class="flex items-center gap-3 w-full">
          <div class="flex items-center gap-3 flex-1 min-w-0">
            <span class="text-sm font-medium shrink-0 text-neutral-700 dark:text-neutral-300">{{ $t('downloads.targetInstance') }}</span>
            <DSelect
              v-model="selectedInstanceId"
              :options="instanceOptions"
              class="flex-1 min-w-0"
            />
          </div>
          <DSelect
            v-model="searchSource"
            :options="sourceOptions"
            class="shrink-0 w-36"
          />
        </div>

        <!-- Row 2: Search Input & Filters -->
        <div class="flex flex-col md:flex-row gap-3">
          <div class="relative flex-1">
            <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-neutral-500 dark:text-zinc-400" />
            <DInput 
              v-model="searchQuery"
              @keydown.enter="performSearch(false)"
              :placeholder="$t('downloads.searchPlaceholder')"
              class="!pl-10"
            />
          </div>

          <div class="flex gap-2 shrink-0">
            <DMultiSelect
              v-model="activeMcVersion"
              :options="availableVersions.map(v => ({label: v, value: v}))"
              :placeholder="$t('downloads.anyVersion')"
              :disabled="!!selectedInstanceId"
              class="w-full md:w-36"
            />

            <DMultiSelect
              v-model="activeLoader"
              :options="availableLoaders.map(l => ({label: l.charAt(0).toUpperCase() + l.slice(1).toLowerCase(), value: l.toLowerCase()}))"
              :placeholder="$t('downloads.anyLoader')"
              :disabled="!!selectedInstanceId"
              class="w-full md:w-36"
            />
            
            <DMultiSelect
              v-model="selectedCategory"
              :options="categories.map(c => ({label: getCategoryName(c.name), value: c.id}))"
              :placeholder="$t('downloads.allCategories')"
              class="w-full md:w-44"
            />

            <button
              @click="performSearch(false)"
              :disabled="isSearching"
              class="flex items-center gap-2 px-4 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md transition-colors text-sm font-medium shadow-sm disabled:opacity-50"
            >
              <Loader2 v-if="isSearching" class="h-4 w-4 animate-spin" />
              {{ $t('downloads.search') }}
            </button>
          </div>
        </div>
      </div>

      <!-- Error message -->
      <div v-if="error" class="p-4 bg-red-50 text-red-600 dark:bg-red-900/20 dark:text-red-400 rounded-lg text-sm">
        {{ error }}
      </div>

      <!-- Loading State (Initial Search) -->
      <div v-if="isSearching && searchResults.length === 0" class="py-20 flex flex-col items-center justify-center text-neutral-500 dark:text-zinc-400">
        <Loader2 class="h-10 w-10 animate-spin text-emerald-600 dark:text-emerald-500 mb-4 drop-shadow-sm" />
        <p class="text-sm font-medium animate-pulse">{{ $t('install.loading', '加载中...') }}</p>
      </div>

      <!-- Search Results -->
      <div v-else-if="searchResults.length > 0" class="space-y-3 pb-8 relative z-0" :class="{ 'opacity-50 pointer-events-none transition-opacity duration-300': isSearching && !isLoadingMore }">
        <div
          v-for="mod in searchResults"
          :key="mod.project_id"
          @click="handleCardClick(mod)"
          class="cursor-pointer flex items-center gap-4 p-4 bg-white dark:bg-zinc-900 border border-neutral-200 dark:border-zinc-800 rounded-xl shadow-sm hover:shadow-md transition-all group hover:border-emerald-500/50"
        >
          <!-- Icon -->
          <div class="w-12 h-12 shrink-0 bg-neutral-100 dark:bg-zinc-800 rounded-lg overflow-hidden border border-neutral-200 dark:border-zinc-700/50">
            <img v-if="mod.icon_url" :src="mod.icon_url" alt="Icon" class="w-full h-full object-cover" />
            <div v-else class="w-full h-full flex items-center justify-center text-neutral-400">
              <Puzzle class="w-6 h-6 opacity-50" />
            </div>
          </div>

          <!-- Info -->
          <div class="flex-1 min-w-0 flex flex-col justify-center gap-1.5">
            <!-- Line 1: Title and Badges -->
            <div class="flex items-center gap-3">
              <h3 class="font-bold text-base text-neutral-900 dark:text-zinc-100 truncate" :title="mod.title">
                {{ mod.title }}
              </h3>
              <div class="flex items-center gap-1.5 shrink-0">
                <span v-for="l in mod.loaders" :key="l" class="flex items-center gap-1 px-1.5 py-0.5 bg-neutral-100 dark:bg-zinc-800/50 border border-neutral-200/80 dark:border-zinc-700/80 rounded text-[10px] font-medium text-neutral-600 dark:text-zinc-300 capitalize">
                  <svg v-if="l.toLowerCase() === 'fabric'" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="currentColor"><path d="M12 0C5.373 0 0 5.373 0 12s5.373 12 12 12 12-5.373 12-12S18.627 0 12 0zm0 2.4c5.302 0 9.6 4.298 9.6 9.6s-4.298 9.6-9.6 9.6S2.4 17.302 2.4 12 6.698 2.4 12 2.4zm-4.8 4.8v9.6h2.4V7.2H7.2zm4.8 0v9.6h2.4V7.2h-2.4z"/></svg>
                  <svg v-else-if="l.toLowerCase() === 'forge'" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg>
                  <svg v-else-if="l.toLowerCase() === 'quilt'" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z"/></svg>
                  <svg v-else-if="l.toLowerCase() === 'neoforge'" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l8 4.5v9L12 22l-8-4.5v-9L12 2zm0 2.3l-6 3.4v6.6l6 3.4 6-3.4V7.7l-6-3.4z"/></svg>
                  <div v-else class="w-1.5 h-1.5 rounded-full bg-emerald-500"></div>
                  {{ l }}
                </span>
              </div>
            </div>
            
            <!-- Line 2: Description and Stats -->
            <div class="flex items-center justify-between gap-4">
              <p class="text-sm text-neutral-500 dark:text-zinc-400 truncate">
                {{ mod.description }}
              </p>
              <p class="text-xs text-neutral-400 dark:text-zinc-500 shrink-0">
                By {{ mod.author }} • {{ formatNumber(mod.downloads) }} ↓
              </p>
            </div>
          </div>
        </div>
      
      <!-- Loading More Indicator -->
      <div v-if="isLoadingMore" class="py-8 flex justify-center items-center">
        <Loader2 class="w-8 h-8 animate-spin text-emerald-500" />
      </div>
      </div>

      <!-- Empty State -->
      <div v-else-if="!isSearching && hasSearched" class="py-12 flex flex-col items-center justify-center text-neutral-500">
        <AlertCircle class="w-12 h-12 mb-4 opacity-20" />
        <p class="text-lg font-medium">{{ $t('downloads.noModsFound') }}</p>
        <p class="text-sm mt-1">{{ $t('downloads.tryDifferentSearch') }}</p>
      </div>

      <!-- Sentinel for infinite scroll -->
      <div ref="bottomSentinel" class="h-4 w-full"></div>
    </div>
  </div>

  <!-- Install Dialog -->
  <DialogContent :open="showInstallDialog" @update:open="showInstallDialog = $event" class="max-w-[600px] !overflow-visible">
      <div class="space-y-1.5">
        <DialogTitle>{{ $t('downloads.installMod', { name: pendingMod?.title }) }}</DialogTitle>
        <DialogDescription>
          {{ $t('downloads.selectModVersion') }}
        </DialogDescription>
      </div>
      
      <div class="py-4 space-y-4">
        <!-- Loading State -->
        <div v-if="isFetchingFiles" class="flex items-center justify-center py-4 text-neutral-500">
          <Loader2 class="w-6 h-6 animate-spin mr-2" />
          <span class="text-sm">{{ $t('downloads.fetchingVersions') }}</span>
        </div>

        <template v-else-if="installFiles.length > 0">
          <!-- Version Selector -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div class="space-y-2">
              <label class="text-sm font-medium text-neutral-700 dark:text-neutral-300">游戏版本 (MC Version)</label>
              <DSelect
                v-model="selectedGroupVersion"
                :options="mcVersionOptions"
                class="w-full"
              />
            </div>
            <div class="space-y-2">
              <label class="text-sm font-medium text-neutral-700 dark:text-neutral-300">{{ $t('downloads.modFileVersion') }}</label>
              <DSelect
                v-model="selectedFileId"
                :options="filteredFileOptions"
                @update:model-value="checkDependenciesForSelectedFile"
                class="w-full"
              />
            </div>
          </div>

          <!-- Dependencies Check -->
          <div v-if="isCheckingDependencies" class="flex items-center gap-2 text-sm text-neutral-500 py-2">
            <Loader2 class="w-4 h-4 animate-spin" />
            <span>{{ $t('downloads.checkingDependencies') }}</span>
          </div>
          
          <div v-else-if="pendingDependencies.length > 0" class="space-y-2">
            <label class="text-sm font-medium text-neutral-700 dark:text-neutral-300">{{ $t('downloads.detectedDependencies') }}</label>
            <div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800/50 rounded-lg p-3 space-y-2 max-h-40 overflow-y-auto">
              <div v-for="dep in pendingDependencies" :key="dep.project_id" class="flex items-center gap-2 text-sm text-amber-800 dark:text-amber-300">
                <Info class="h-4 w-4 shrink-0" />
                <span class="truncate">{{ pendingDependencyTitles[dep.project_id] || dep.project_id }}</span>
              </div>
            </div>
          </div>
          
          <div v-else class="flex items-center gap-2 text-sm text-emerald-600 dark:text-emerald-400 py-2 bg-emerald-50 dark:bg-emerald-900/20 px-3 rounded-lg border border-emerald-100 dark:border-emerald-800/50">
            <Check class="h-4 w-4 shrink-0" />
            <span>{{ $t('downloads.noDependenciesNeeded') }}</span>
          </div>
        </template>
        
        <div v-else class="text-center py-4 text-sm text-neutral-500">
          {{ $t('downloads.noCompatibleFiles') }}
        </div>
      </div>

      
      <div class="mt-4 pt-4 border-t border-neutral-200 dark:border-zinc-800 flex flex-col gap-4">
        <label v-if="pendingDependencies.length > 0" class="flex items-center gap-2 text-sm cursor-pointer select-none text-neutral-700 dark:text-neutral-300">
          <input type="checkbox" v-model="includeDependencies" class="rounded border-gray-300 text-emerald-600 focus:ring-emerald-600" />
          {{ $t('downloads.downloadDependencies') }}
        </label>
        
        <div class="flex flex-col-reverse sm:flex-row sm:justify-end gap-2">
          <button
            @click="startDownloadToDir"
            :disabled="isDownloading || isFetchingFiles || isCheckingDependencies || installFiles.length === 0"
            class="px-4 py-2 text-sm font-medium border border-neutral-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-neutral-700 dark:text-neutral-300 hover:bg-neutral-50 dark:hover:bg-zinc-800 rounded-md transition-colors disabled:opacity-50"
          >
            {{ $t('downloads.downloadTo') }}
          </button>
          <button
            v-if="selectedInstanceId"
            @click="startInstallToInstance"
            :disabled="isDownloading || isFetchingFiles || isCheckingDependencies || installFiles.length === 0"
            class="flex items-center justify-center gap-2 px-4 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md transition-colors text-sm font-medium shadow-sm disabled:opacity-50"
          >
            <Loader2 v-if="isDownloading" class="w-4 h-4 animate-spin" />
            {{ $t('downloads.installToSelectedInstance') }}
          </button>
        </div>
      </div>
  </DialogContent>

  <!-- Duplicate Warning Dialog -->
  <AlertDialog :open="showDuplicateWarning" @update:open="showDuplicateWarning = $event">
    <div class="space-y-3">
      <AlertDialogTitle class="flex items-center gap-2 text-amber-600 dark:text-amber-500">
        <AlertCircle class="w-5 h-5" />
        {{ $t('downloads.duplicateModTitle') }}
      </AlertDialogTitle>
      <AlertDialogDescription class="text-neutral-600 dark:text-neutral-400">
        {{ $t('downloads.duplicateModDesc', { name: pendingMod?.title }) }}
        <ul class="mt-2 text-xs opacity-80 list-disc list-inside">
          <li v-for="mod in duplicateModsFound" :key="mod.filename">{{ mod.filename }}</li>
        </ul>
      </AlertDialogDescription>
    </div>
    <div class="flex justify-end gap-3 mt-6">
      <button @click="showDuplicateWarning = false" class="px-4 py-2 rounded-lg text-sm font-medium border border-neutral-200 dark:border-zinc-700 hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors">
        {{ $t('downloads.cancel') }}
      </button>
      <button v-if="!hasExactFilenameConflict" @click="handleKeepBothMods" class="px-4 py-2 rounded-lg text-sm font-medium border border-neutral-200 dark:border-zinc-700 hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors">
        {{ $t('downloads.keepBoth') }}
      </button>
      <button @click="handleReplaceOldMods" class="px-4 py-2 rounded-lg text-sm font-medium bg-amber-600 hover:bg-amber-700 text-white transition-colors">
        {{ $t('downloads.replaceOld') }}
      </button>
    </div>
  </AlertDialog>
</template>
