<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Package, Search, Download, User, ExternalLink, Loader2 } from "@lucide/vue";

// Types
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

// State
const selectedSource = ref<"modrinth" | "curseforge">("modrinth");
const searchQuery = ref("");
const isSearching = ref(false);
const searchResults = ref<UnifiedModProject[]>([]);
const error = ref<string | null>(null);

// Filters
const selectedMcVersion = ref("");
const selectedLoader = ref("");

// Available MC versions (common ones)
const mcVersions = [
  "1.20.4", "1.20.3", "1.20.2", "1.20.1", "1.20",
  "1.19.4", "1.19.3", "1.19.2", "1.19.1", "1.19",
  "1.18.2", "1.18.1", "1.18",
  "1.17.1", "1.17",
  "1.16.5", "1.16.4", "1.16.3", "1.16.2", "1.16.1", "1.16",
];

const loaderOptions = [
  { value: "", label: "Any Loader" },
  { value: "fabric", label: "Fabric" },
  { value: "forge", label: "Forge" },
  { value: "neoforge", label: "NeoForge" },
];

// Selected mod for details
const selectedMod = ref<UnifiedModProject | null>(null);
const isLoadingDownloadUrl = ref(false);
const isDownloading = ref(false);

// Source badges
const sourceBadges = {
  modrinth: { bg: "bg-green-100 dark:bg-green-900/40", text: "text-green-700 dark:text-green-300", label: "Modrinth" },
  curseforge: { bg: "bg-orange-100 dark:bg-orange-900/40", text: "text-orange-700 dark:text-orange-300", label: "CurseForge" },
};

// Search debounce
let searchTimeout: ReturnType<typeof setTimeout> | null = null;

// Watch for search query changes
watch([searchQuery, selectedMcVersion, selectedLoader], () => {
  if (searchTimeout) clearTimeout(searchTimeout);
  searchTimeout = setTimeout(() => {
    if (searchQuery.value.trim()) {
      searchMods();
    }
  }, 500);
});

// Watch source changes
watch(selectedSource, () => {
  if (searchQuery.value.trim()) {
    searchMods();
  }
});

// Search function
async function searchMods() {
  if (!searchQuery.value.trim()) {
    searchResults.value = [];
    return;
  }

  isSearching.value = true;
  error.value = null;

  try {
    const command = selectedSource.value === "modrinth" 
      ? "search_modrinth" 
      : "search_curseforge";
    
    const results = await invoke<UnifiedModProject[]>(command, {
      query: searchQuery.value,
      mcVersion: selectedMcVersion.value || "",
      loader: selectedLoader.value || "",
    });

    searchResults.value = results;
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
    searchResults.value = [];
  } finally {
    isSearching.value = false;
  }
}

// Get download URL
async function getDownloadUrl(mod: UnifiedModProject) {
  isLoadingDownloadUrl.value = true;
  selectedMod.value = mod;

  try {
    const command = mod.source === "modrinth"
      ? "get_modrinth_mod_download_url"
      : "get_cf_mod_download_url";
    
    const [downloadUrl, fileId] = await invoke<[string, string]>(command, {
      projectId: mod.project_id,
      mcVersion: selectedMcVersion.value || mod.mc_versions[0] || "1.20.4",
      loader: selectedLoader.value || mod.loaders[0] || "fabric",
    });

    // Update the mod with download URL
    mod.download_url = downloadUrl;
    mod.file_id = fileId;
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
  } finally {
    isLoadingDownloadUrl.value = false;
  }
}

// Download mod
async function downloadMod(mod: UnifiedModProject) {
  if (!mod.download_url) {
    await getDownloadUrl(mod);
    if (!mod.download_url) return;
  }

  isDownloading.value = true;

  try {
    // TODO: Implement actual download using batch_download command
    // For now, we'll open the URL in browser as a fallback
    window.open(mod.download_url, "_blank");
  } catch (err) {
    error.value = typeof err === "string" ? err : String(err);
  } finally {
    isDownloading.value = false;
  }
}

// Format download count
function formatDownloads(count: number): string {
  if (count >= 1000000) {
    return `${(count / 1000000).toFixed(1)}M`;
  }
  if (count >= 1000) {
    return `${(count / 1000).toFixed(1)}K`;
  }
  return count.toString();
}

// Get loader badge class
function getLoaderBadgeClass(loader: string): string {
  switch (loader.toLowerCase()) {
    case "fabric":
      return "bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300";
    case "forge":
      return "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300";
    case "neoforge":
      return "bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300";
    default:
      return "bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300";
  }
}
</script>

<template>
  <div class="flex h-full flex-col p-4">
    <!-- Header -->
    <div class="flex items-center justify-between mb-6">
      <div class="flex items-center gap-3">
        <Package class="h-7 w-7 text-primary" />
        <div>
          <h1 class="text-2xl font-bold">Mod Browser</h1>
          <p class="text-sm text-muted-foreground">
            Browse and install mods from Modrinth and CurseForge
          </p>
        </div>
      </div>
    </div>

    <!-- Search Bar -->
    <div class="mb-6 space-y-4">
      <!-- Source Toggle -->
      <div class="flex gap-2">
        <button
          @click="selectedSource = 'modrinth'"
          :class="[
            'flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors',
            selectedSource === 'modrinth'
              ? 'bg-green-600 text-white'
              : 'bg-muted text-muted-foreground hover:bg-muted/80'
          ]"
        >
          <span class="text-lg">🟢</span>
          Modrinth
        </button>
        <button
          @click="selectedSource = 'curseforge'"
          :class="[
            'flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors',
            selectedSource === 'curseforge'
              ? 'bg-orange-600 text-white'
              : 'bg-muted text-muted-foreground hover:bg-muted/80'
          ]"
        >
          <span class="text-lg">🟠</span>
          CurseForge
        </button>
      </div>

      <!-- Search Input -->
      <div class="flex gap-3">
        <div class="relative flex-1">
          <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-5 w-5 text-muted-foreground" />
          <input
            v-model="searchQuery"
            type="text"
            :placeholder="`Search mods on ${selectedSource === 'modrinth' ? 'Modrinth' : 'CurseForge'}...`"
            class="w-full pl-10 pr-4 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-lg text-base text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 focus:outline-none focus:ring-2 focus:ring-primary"
            @keyup.enter="searchMods"
          />
        </div>
        <button
          @click="searchMods"
          :disabled="isSearching || !searchQuery.trim()"
          class="px-6 py-2 bg-primary text-primary-foreground rounded-lg font-medium hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          <Loader2 v-if="isSearching" class="h-5 w-5 animate-spin" />
          <Search v-else class="h-5 w-5" />
        </button>
      </div>

      <!-- Filters -->
      <div class="flex flex-wrap gap-3">
        <select
          v-model="selectedMcVersion"
          class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
        >
          <option value="">All Versions</option>
          <option v-for="version in mcVersions" :key="version" :value="version">
            {{ version }}
          </option>
        </select>

        <select
          v-model="selectedLoader"
          class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
        >
          <option v-for="opt in loaderOptions" :key="opt.value" :value="opt.value">
            {{ opt.label }}
          </option>
        </select>
      </div>
    </div>

    <!-- Error Message -->
    <div v-if="error" class="mb-4 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
      <p class="text-sm text-red-600 dark:text-red-400">{{ error }}</p>
    </div>

    <!-- Search Results -->
    <div class="flex-1 overflow-y-auto">
      <!-- Loading State -->
      <div v-if="isSearching" class="flex items-center justify-center py-12">
        <Loader2 class="h-8 w-8 animate-spin text-primary" />
        <span class="ml-3 text-muted-foreground">Searching...</span>
      </div>

      <!-- No Results -->
      <div v-else-if="searchQuery && searchResults.length === 0 && !error" class="flex flex-col items-center justify-center py-12">
        <Package class="h-12 w-12 text-muted-foreground mb-4" />
        <h3 class="text-lg font-semibold mb-2">{{ $t('mods.empty') }}</h3>
        <p class="text-sm text-muted-foreground">Try a different search term or filters</p>
      </div>

      <!-- Results Grid -->
      <div v-else-if="searchResults.length > 0" class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <div
          v-for="mod in searchResults"
          :key="`${mod.source}-${mod.project_id}`"
          class="group rounded-lg border bg-card p-4 hover:border-primary/50 transition-colors"
        >
          <!-- Mod Header -->
          <div class="flex items-start gap-3 mb-3">
            <!-- Icon -->
            <div class="flex h-12 w-12 items-center justify-center rounded-lg bg-muted shrink-0">
              <img
                v-if="mod.icon_url"
                :src="mod.icon_url"
                :alt="mod.title"
                class="h-12 w-12 rounded-lg object-cover"
                @error="($event.target as HTMLImageElement).style.display = 'none'"
              />
              <Package v-else class="h-6 w-6 text-muted-foreground" />
            </div>
            
            <!-- Title and Source -->
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2">
                <h3 class="font-semibold truncate">{{ mod.title }}</h3>
                <span
                  :class="[
                    'px-2 py-0.5 rounded text-[10px] font-semibold',
                    sourceBadges[mod.source as keyof typeof sourceBadges].bg,
                    sourceBadges[mod.source as keyof typeof sourceBadges].text
                  ]"
                >
                  {{ sourceBadges[mod.source as keyof typeof sourceBadges].label }}
                </span>
              </div>
              <p class="text-xs text-muted-foreground line-clamp-2 mt-1">
                {{ mod.description }}
              </p>
            </div>
          </div>

          <!-- Meta Info -->
          <div class="flex flex-wrap gap-2 mb-3">
            <!-- MC Versions -->
            <span
              v-for="version in mod.mc_versions.slice(0, 2)"
              :key="version"
              class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold bg-zinc-100 dark:bg-zinc-800 text-neutral-700 dark:text-zinc-300"
            >
              {{ version }}
            </span>
            <span v-if="mod.mc_versions.length > 2" class="text-xs text-muted-foreground">
              +{{ mod.mc_versions.length - 2 }}
            </span>

            <!-- Loaders -->
            <span
              v-for="loader in mod.loaders.slice(0, 2)"
              :key="loader"
              :class="['inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold', getLoaderBadgeClass(loader)]"
            >
              {{ loader }}
            </span>
          </div>

          <!-- Author and Downloads -->
          <div class="flex items-center justify-between text-sm text-muted-foreground mb-3">
            <div class="flex items-center gap-1">
              <User class="h-4 w-4" />
              <span class="truncate max-w-[100px]">{{ mod.author }}</span>
            </div>
            <div class="flex items-center gap-1">
              <Download class="h-4 w-4" />
              <span>{{ formatDownloads(mod.downloads) }}</span>
            </div>
          </div>

          <!-- Actions -->
          <div class="flex justify-end gap-2 pt-3 border-t">
            <button
              @click="selectedMod = mod"
              class="flex items-center gap-1 px-3 py-1.5 text-sm border rounded-md hover:bg-muted transition-colors"
            >
              <ExternalLink class="h-4 w-4" />
              View
            </button>
            <button
              @click="downloadMod(mod)"
              :disabled="isLoadingDownloadUrl || isDownloading"
              class="flex items-center gap-1 px-3 py-1.5 text-sm bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 transition-colors"
            >
              <Loader2 v-if="isLoadingDownloadUrl" class="h-4 w-4 animate-spin" />
              <Download v-else class="h-4 w-4" />
              Download
            </button>
          </div>
        </div>
      </div>

      <!-- Initial State -->
      <div v-else class="flex flex-col items-center justify-center py-12 text-center">
        <Search class="h-16 w-16 text-muted-foreground mb-4" />
        <h3 class="text-xl font-semibold mb-2">Search for Mods</h3>
        <p class="text-sm text-muted-foreground max-w-md">
          Enter a search term to browse mods from {{ selectedSource === 'modrinth' ? 'Modrinth' : 'CurseForge' }}.
          You can also filter by Minecraft version and mod loader.
        </p>
      </div>
    </div>

    <!-- Mod Details Modal -->
    <Teleport to="body">
      <div v-if="selectedMod" class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
        <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"></div>
        <div class="relative z-10 w-full max-w-lg gap-4 border bg-white dark:bg-zinc-900 p-4 shadow-xl rounded-lg pointer-events-auto max-h-[80vh] overflow-y-auto">
          <div class="flex items-center justify-between mb-4">
            <h3 class="font-semibold text-lg text-neutral-900 dark:text-white">Mod Details</h3>
            <button @click="selectedMod = null" class="text-muted-foreground hover:text-foreground text-lg">
              ✕
            </button>
          </div>

          <!-- Mod Info -->
          <div class="flex items-start gap-4 mb-4">
            <div class="flex h-16 w-16 items-center justify-center rounded-lg bg-muted shrink-0">
              <img
                v-if="selectedMod.icon_url"
                :src="selectedMod.icon_url"
                :alt="selectedMod.title"
                class="h-16 w-16 rounded-lg object-cover"
                @error="($event.target as HTMLImageElement).style.display = 'none'"
              />
              <Package v-else class="h-8 w-8 text-muted-foreground" />
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2">
                <h4 class="font-semibold text-neutral-900 dark:text-white">{{ selectedMod.title }}</h4>
                <span
                  :class="[
                    'px-2 py-0.5 rounded text-[10px] font-semibold',
                    sourceBadges[selectedMod.source as keyof typeof sourceBadges].bg,
                    sourceBadges[selectedMod.source as keyof typeof sourceBadges].text
                  ]"
                >
                  {{ sourceBadges[selectedMod.source as keyof typeof sourceBadges].label }}
                </span>
              </div>
              <p class="text-sm text-muted-foreground mt-1">by {{ selectedMod.author }}</p>
            </div>
          </div>

          <!-- Description -->
          <div class="mb-4">
            <h5 class="text-sm font-medium mb-2 text-neutral-900 dark:text-white">Description</h5>
            <p class="text-sm text-muted-foreground">{{ selectedMod.description }}</p>
          </div>

          <!-- Versions & Loaders -->
          <div class="mb-4">
            <h5 class="text-sm font-medium mb-2 text-neutral-900 dark:text-white">Supported Versions</h5>
            <div class="flex flex-wrap gap-1">
              <span
                v-for="version in selectedMod.mc_versions"
                :key="version"
                class="px-2 py-0.5 bg-zinc-100 dark:bg-zinc-800 rounded text-xs"
              >
                {{ version }}
              </span>
            </div>
          </div>

          <div class="mb-4">
            <h5 class="text-sm font-medium mb-2 text-neutral-900 dark:text-white">Supported Loaders</h5>
            <div class="flex flex-wrap gap-1">
              <span
                v-for="loader in selectedMod.loaders"
                :key="loader"
                :class="['px-2 py-0.5 rounded text-xs', getLoaderBadgeClass(loader)]"
              >
                {{ loader }}
              </span>
            </div>
          </div>

          <!-- Stats -->
          <div class="flex items-center gap-4 mb-4 text-sm text-muted-foreground">
            <div class="flex items-center gap-1">
              <Download class="h-4 w-4" />
              <span>{{ formatDownloads(selectedMod.downloads) }} downloads</span>
            </div>
          </div>

          <!-- Actions -->
          <div class="flex justify-end gap-2 pt-4 border-t">
            <button
              @click="selectedMod = null"
              class="px-3 py-1.5 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
            >
              Close
            </button>
            <button
              @click="downloadMod(selectedMod)"
              :disabled="isLoadingDownloadUrl || isDownloading"
              class="flex items-center gap-2 px-3 py-1.5 text-sm font-medium bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 transition-colors"
            >
              <Loader2 v-if="isLoadingDownloadUrl" class="h-4 w-4 animate-spin" />
              <Download v-else class="h-4 w-4" />
              Download
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>