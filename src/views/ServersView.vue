<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, onActivated, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Server, Gamepad2, Plus, Search, Copy, Check, Loader2, Download, Package, ChevronDown, Users, Star } from "@lucide/vue";

// Types matching the Rust Server model
interface ServerInfo {
  id: number;
  name: string;
  ip: string;
  port: number;
  motd: string;
  version: string;
  loaderType: string;
  serverType: string;        // "vanilla", "modded", "custom"
  authType: string;          // "offline", "online"
  packFileName: string | null;
  packFileSize: number | null;
  packProjectId: string | null;
  packVersionId: string | null;
  packSource: string | null;
  iconUrl: string;
  email: string;
  isActive: boolean;
}

interface CreateServerInput {
  name: string;
  ip: string;
  port: number;
  motd: string;
  version: string;
  loaderType: string;
  serverType: string;
  authType: string;
  packFileName: string;
  packProjectId: string;
  packVersionId: string;
  packSource: string;
  iconUrl: string;
  email: string;
}

// {{ $t('servers.types.vanilla') }} version from Mojang API
interface VanillaVersion {
  id: string;
  versionType: string;
  url: string;
}

// Extended type to handle API variations
interface VanillaVersionExtended extends VanillaVersion {
  type?: string;
}

// Filter options response
interface FilterOptions {
  versions: string[];
  serverTypes: string[];
  authTypes: string[];
}

// interface ServerListResponse {
//   data: ServerInfo[];
//   total: number;
//   page: number;
//   pageSize: number;
//   totalPages: number;
// }

interface ServerStatus {
  onlinePlayers: number;
  maxPlayers: number;
  ping: number;
}

// State
const { t } = useI18n();
const router = useRouter();
const servers = ref<ServerInfo[]>([]);
const isLoading = ref(false);
const error = ref<string | null>(null);
const currentPage = ref(1);
const totalPages = ref(1);

// Minecraft versions
const mcVersions = ref<VanillaVersion[]>([]);
const mcVersionsLoading = ref(false);
const mcVersionsError = ref<string | null>(null);
const showVersionDropdown = ref(false);
const versionSearchQuery = ref("");

// Filter version dropdown (separate state)
const showFilterVersionDropdown = ref(false);
const filterVersionSearchQuery = ref("");

// Filter options (from API)
const filterOptions = ref<FilterOptions>({
  versions: [],
  serverTypes: [],
  authTypes: [],
});
const filterOptionsLoading = ref(false);

// Filter state
const searchQuery = ref("");
const filterMcVersion = ref<string>("");
const filterServerType = ref<string>("");
const filterAuthType = ref<string>("");

// Re-fetch servers when filters change
async function applyFilters() {
  currentPage.value = 1;
  await fetchServers();
}

// Watch for filter changes
watch(searchQuery, () => {
  applyFilters();
});
watch(filterMcVersion, () => {
  applyFilters();
});
watch(filterServerType, () => {
  applyFilters();
});
watch(filterAuthType, () => {
  applyFilters();
});

// Dialog state
const showPublishDialog = ref(false);
const publishStep = ref(1);
const totalSteps = 4;
const isSubmitting = ref(false);

// Step validation
const canGoToStep = (step: number): boolean => {
  if (step === 1) {
    return newServer.value.name.trim() !== "" && newServer.value.ip.trim() !== "";
  }
  if (step === 2) {
    return newServer.value.version !== "";
  }
  if (step === 3) {
    if (needsPackFile(newServer.value.serverType)) {
      if (packBindMode.value === 'local') {
        return selectedPackFile.value !== null;
      } else {
        return selectedOnlineVersion.value !== null;
      }
    }
    return true;
  }
  return true;
};

const newServer = ref<CreateServerInput>({
  name: "",
  ip: "",
  port: 25565,
  motd: "",
  version: "",
  loaderType: "",
  serverType: "vanilla",
  authType: "offline",
  packFileName: "",
  packProjectId: "",
  packVersionId: "",
  packSource: "",
  iconUrl: "",
  email: "",
} as any);

// Auto-split IP and Port when user pastes "domain:port"
watch(() => newServer.value.ip, (newVal) => {
  if (newVal && newVal.includes(':')) {
    const parts = newVal.split(':');
    if (parts.length === 2) {
      const portStr = parts[1].trim();
      const portNum = parseInt(portStr, 10);
      if (!isNaN(portNum) && portNum >= 1 && portNum <= 65535) {
        newServer.value.ip = parts[0].trim();
        newServer.value.port = portNum;
      }
    }
  }
});

// Pack file upload state
const packBindMode = ref<'local' | 'online'>('local');
const selectedPackFile = ref<string | null>(null);
const selectedPackFileName = ref<string>("");
const isUploadingPack = ref(false);

// Online modpack search state
const onlineSearchQuery = ref('');
const onlineSource = ref<'modrinth' | 'curseforge'>('modrinth');
const isSearchingOnline = ref(false);
const onlineModpacks = ref<any[]>([]);
const isFetchingOnlineVersions = ref(false);
const onlineModpackVersions = ref<any[]>([]);
const selectedOnlineProject = ref<any>(null);
const selectedOnlineVersion = ref<any>(null);

async function searchOnlineModpacks() {
  if (!onlineSearchQuery.value) return;
  isSearchingOnline.value = true;
  onlineModpacks.value = [];
  selectedOnlineProject.value = null;
  onlineModpackVersions.value = [];
  try {
    if (onlineSource.value === 'modrinth') {
      onlineModpacks.value = await invoke('search_modrinth_modpacks', { query: onlineSearchQuery.value });
    } else {
      onlineModpacks.value = await invoke('search_curseforge_modpacks', { query: onlineSearchQuery.value });
    }
  } catch (error) {
    console.error("Failed to search modpacks:", error);
  } finally {
    isSearchingOnline.value = false;
  }
}

async function selectOnlineProject(project: any) {
  selectedOnlineProject.value = project;
  isFetchingOnlineVersions.value = true;
  onlineModpackVersions.value = [];
  selectedOnlineVersion.value = null;
  try {
    if (onlineSource.value === 'modrinth') {
      onlineModpackVersions.value = await invoke('get_modrinth_modpack_versions', { projectId: project.project_id });
    } else {
      onlineModpackVersions.value = await invoke('get_curseforge_modpack_versions', { projectId: project.project_id });
    }
  } catch (error) {
    console.error("Failed to fetch modpack versions:", error);
  } finally {
    isFetchingOnlineVersions.value = false;
  }
}

// Server ping status
const serverStatuses = ref<Record<number, ServerStatus>>({});
const serverStatusesLoading = ref<Record<number, boolean>>({});

// Install modpack state
const installingServerId = ref<number | null>(null);

// Copy notification
const copiedServerId = ref<number | null>(null);

// Check if server needs a pack file (modded or custom)
function needsPackFile(serverType: string): boolean {
  return serverType === "modded" || serverType === "custom";
}

// Handle pack file selection
async function selectPackFile() {
  const selected = await open({
    multiple: false,
    filters: [{ name: "Modpack", extensions: ["zip"] }],
  });
  
  if (selected) {
    selectedPackFile.value = selected as string;
    // Extract filename from path
    const parts = (selected as string).split(/[/\\]/);
    selectedPackFileName.value = parts[parts.length - 1];
  }
}

// Fetch Minecraft versions from Mojang API
async function fetchMcVersions() {
  if (mcVersions.value.length > 0) return; // Already loaded
  
  mcVersionsLoading.value = true;
  mcVersionsError.value = null;
  try {
    const versions = await invoke<VanillaVersion[]>("get_vanilla_versions");
    mcVersions.value = versions;
  } catch (e) {
    mcVersionsError.value = e instanceof Error ? e.message : String(e);
    console.error("Failed to fetch MC versions:", e);
  } finally {
    mcVersionsLoading.value = false;
  }
}

// Fetch filter options from API
async function fetchFilterOptions() {
  filterOptionsLoading.value = true;
  try {
    const options = await invoke<FilterOptions>("get_filter_options");
    filterOptions.value = options;
  } catch (e) {
    console.error("Failed to fetch filter options:", e);
  } finally {
    filterOptionsLoading.value = false;
  }
}

// Grouped versions for filter dropdown (release first, then snapshot, old_beta, old_alpha)
const groupedVersions = computed(() => {
  const query = filterVersionSearchQuery.value.toLowerCase();
  const allVersions = mcVersions.value as VanillaVersionExtended[];
  const filtered = (allVersions || []).filter(v => 
    v.id.toLowerCase().includes(query)
  );
  
  const release: VanillaVersionExtended[] = [];
  const snapshot: VanillaVersionExtended[] = [];
  const old_beta: VanillaVersionExtended[] = [];
  const old_alpha: VanillaVersionExtended[] = [];
  
  for (const v of filtered) {
    const type = v.versionType || v.type || '';
    switch (type) {
      case 'release':
        release.push(v);
        break;
      case 'snapshot':
        snapshot.push(v);
        break;
      case 'old_beta':
        old_beta.push(v);
        break;
      case 'old_alpha':
        old_alpha.push(v);
        break;
      default:
        // Default to release for unknown types
        release.push(v);
        break;
    }
  }
  
  return { release, snapshot, old_beta, old_alpha };
});

// Select a version from dropdown
function selectVersion(version: VanillaVersion) {
  newServer.value.version = version.id;
  versionSearchQuery.value = version.id;
  showVersionDropdown.value = false;
}

// Fetch servers from Rust backend
async function fetchServers() {
  isLoading.value = true;
  error.value = null;
  try {
    const response = await invoke<ServerInfo[]>("get_recommended_servers");
    servers.value = response;
    currentPage.value = 1;
    totalPages.value = 1;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    console.error("Failed to fetch servers:", e);
  } finally {
    isLoading.value = false;
  }
}

// Load more servers (infinite scroll)
async function loadMoreServers() {
  // Infinite scroll is disabled for the recommended ecosystem lobby.
}

// Infinite scroll handler
function handleScroll(event: Event) {
  const target = event.target as HTMLElement;
  const scrollBottom = target.scrollHeight - target.scrollTop - target.clientHeight;
  if (scrollBottom < 200) {
    loadMoreServers();
  }
}

// Initial load
onMounted(() => {
  loadFavorites();
  fetchServers();
  fetchFilterOptions();
  // Close version dropdown when clicking outside
  document.addEventListener("click", handleClickOutside);
});

// Refresh when coming back to this view (keep-alive reactivation)
onActivated(() => {
  fetchFilterOptions();
});

onUnmounted(() => {
  document.removeEventListener("click", handleClickOutside);
});

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement;
  if (!target.closest(".version-dropdown")) {
    showVersionDropdown.value = false;
  }
  if (!target.closest(".filter-version-dropdown")) {
    showFilterVersionDropdown.value = false;
  }
}

// Local Favorites State
const favoriteServerIds = ref<number[]>([]);
const showOnlyFavorites = ref(false);

const loadFavorites = () => {
  const saved = localStorage.getItem('dawnland_favorite_servers');
  if (saved) {
    try {
      favoriteServerIds.value = JSON.parse(saved);
    } catch (e) {
      favoriteServerIds.value = [];
    }
  }
};

const toggleFavorite = (serverId: number, event: Event) => {
  event.stopPropagation();
  const index = favoriteServerIds.value.indexOf(serverId);
  if (index > -1) {
    favoriteServerIds.value.splice(index, 1);
  } else {
    favoriteServerIds.value.push(serverId);
  }
  localStorage.setItem('dawnland_favorite_servers', JSON.stringify(favoriteServerIds.value));
};

// Client-side filtering and sorting for the recommended ecosystem lobby
const filteredServers = computed(() => {
  let list = servers.value.filter(server => {
    // Search match
    const searchMatch = !searchQuery.value || 
      server.name.toLowerCase().includes(searchQuery.value.toLowerCase()) || 
      server.ip.toLowerCase().includes(searchQuery.value.toLowerCase());
      
    // Version match
    const versionMatch = !filterMcVersion.value || server.version === filterMcVersion.value;
    
    // Type match
    const typeMatch = !filterServerType.value || server.serverType === filterServerType.value;
    
    // Auth match
    const authMatch = !filterAuthType.value || server.authType === filterAuthType.value;
    
    // Favorite match
    const favoriteMatch = !showOnlyFavorites.value || favoriteServerIds.value.includes(server.id);
    
    return searchMatch && versionMatch && typeMatch && authMatch && favoriteMatch;
  });
  
  // Sort to bring favorites to top
  list.sort((a, b) => {
    const aFav = favoriteServerIds.value.includes(a.id) ? 1 : 0;
    const bFav = favoriteServerIds.value.includes(b.id) ? 1 : 0;
    return bFav - aFav;
  });
  
  return list;
});

async function fetchServerStatus(server: ServerInfo) {
  if (serverStatusesLoading.value[server.id]) return;
  serverStatusesLoading.value[server.id] = true;
  try {
    const status = await invoke<ServerStatus>("ping_server", { ip: server.ip, port: server.port });
    serverStatuses.value[server.id] = status;
  } catch (e) {
    console.error(`Failed to ping server ${server.id}:`, e);
  } finally {
    serverStatusesLoading.value[server.id] = false;
  }
}

watch(servers, (newServers) => {
  newServers.forEach(server => {
    if (!serverStatuses.value[server.id] && !serverStatusesLoading.value[server.id]) {
      fetchServerStatus(server);
    }
  });
});

// Functions
const isConnecting = ref<number | null>(null);

async function launchAndConnect(server: ServerInfo) {
  isConnecting.value = server.id;
  error.value = null;
  try {
    const accounts = await invoke<{id: string, username: string}[]>("get_accounts");
    if (!accounts || accounts.length === 0) {
      error.value = "No accounts found. Please add an account in Settings first.";
      isConnecting.value = null;
      return;
    }
    const accountUuid = accounts[0].id;

    const instances = await invoke<{id: string, mcVersion: string}[]>("scan_installed_instances");
    const matchingInstance = instances.find(i => i.mcVersion === server.version);
    
    if (!matchingInstance) {
      error.value = `No installed instance found for Minecraft ${server.version}. Redirecting to installer...`;
      setTimeout(() => {
        router.push({
          path: '/instances',
          query: {
            install_version: server.version,
            install_loader: server.serverType === 'modded' ? (server.loaderType || 'forge') : 'vanilla'
          }
        });
      }, 1500);
      isConnecting.value = null;
      return;
    }

    await invoke("launch_instance", {
      versionId: matchingInstance.id,
      accountUuid,
      serverIp: server.ip,
      serverPort: server.port
    });

  } catch (e) {
    console.error("Failed to launch and connect:", e);
    error.value = `Failed to connect: ${e}`;
  } finally {
    isConnecting.value = null;
  }
}
function copyIp(server: ServerInfo) {
  navigator.clipboard.writeText(`${server.ip}:${server.port}`);
  copiedServerId.value = server.id;
  setTimeout(() => {
    copiedServerId.value = null;
  }, 2000);
}

async function submitServer() {
  isSubmitting.value = true;
  error.value = null;
  
  try {
    // Create the server first
    const server = await invoke<ServerInfo>("create_server", { input: newServer.value });
    publishStep.value = 4;
    
    // If pack file is selected and server type needs it, upload the pack file
    if (selectedPackFile.value && needsPackFile(newServer.value.serverType)) {
      isUploadingPack.value = true;
      try {
        await invoke("upload_pack_file", {
          serverId: String(server.id),
          filePath: selectedPackFile.value,
        });
      } catch (uploadError) {
        console.error("Failed to upload pack file:", uploadError);
      } finally {
        isUploadingPack.value = false;
      }
    }
    
    // Refresh server list
    await fetchServers();
    showPublishDialog.value = false;
    showAlert(t('servers.messages.submitted'));
    // Reset form
    resetPublishForm();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    console.error("Failed to create server:", e);
  } finally {
    isSubmitting.value = false;
  }
}

function resetPublishForm() {
  newServer.value = {
    name: "",
    ip: "",
    port: 25565,
    motd: "",
    version: "",
    loaderType: "",
    serverType: "vanilla",
    authType: "offline",
    packFileName: "",
    packProjectId: "",
    packVersionId: "",
    packSource: "",
    iconUrl: "",
    email: "",
  } as any;
  selectedPackFile.value = null;
  selectedPackFileName.value = "";
  packBindMode.value = 'local';
  onlineSearchQuery.value = '';
  onlineModpacks.value = [];
  selectedOnlineProject.value = null;
  selectedOnlineVersion.value = null;
  publishStep.value = 1;
}

function openPublishDialog() {
  resetPublishForm();
  showPublishDialog.value = true;
}

function closePublishDialog() {
  showPublishDialog.value = false;
  resetPublishForm();
}

// Install modpack from server
async function installModpack(server: ServerInfo) {
  if (!server.packFileName && !server.packSource) {
    showAlert(t('servers.messages.noModpack'));
    return;
  }
  
  // Route to ModpackInstallView with appropriate query params
  const queryParams: Record<string, string> = {
    server_id: String(server.id),
    update_id: server.name, // Use server name as instance name
  };

  if (server.packSource && server.packProjectId && server.packVersionId) {
    // Online bound modpack
    queryParams.source = server.packSource;
    queryParams.project_id = server.packProjectId;
    queryParams.version_id = server.packVersionId;
  } else if (server.packFileName) {
    // Local zip pack
    queryParams.online_url = `http://localhost:8080/api/servers/${server.id}/pack`;
  }
  
  router.push({
    path: '/modpack-install',
    query: queryParams
  });
}

// Custom Dialog States
const alertState = ref<{
  show: boolean;
  message: string;
}>({ show: false, message: '' });

function showAlert(message: string) {
  alertState.value = { show: true, message };
}

function closeAlert() {
  alertState.value.show = false;
}

const promptState = ref<{
  show: boolean;
  message: string;
  value: string;
  resolve: ((val: string | null) => void) | null;
}>({ show: false, message: '', value: '', resolve: null });

function confirmPrompt() {
  if (promptState.value.resolve) {
    promptState.value.resolve(promptState.value.value);
  }
  promptState.value.show = false;
}

function cancelPrompt() {
  if (promptState.value.resolve) {
    promptState.value.resolve(null);
  }
  promptState.value.show = false;
}
</script>

<template>
  <div class="flex h-full flex-col p-4">
    <!-- Header -->
    <div class="flex items-center justify-between mb-6">
      <div class="flex items-center gap-3">
        <Server class="h-7 w-7 text-primary" />
        <div>
          <h1 class="text-2xl font-bold">{{ $t('servers.title') }}</h1>
          <p class="text-sm text-muted-foreground">
            {{ $t('servers.available', { count: filteredServers.length }, filteredServers.length) }}
          </p>
        </div>
      </div>
      <button
        @click="openPublishDialog"
        class="flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
      >
        <Plus class="h-4 w-4" />
        {{ $t('servers.publish') }}
      </button>
    </div>

    <!-- Error display -->
    <div v-if="error" class="mb-4 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
      <p class="text-sm text-red-600 dark:text-red-400">{{ error }}</p>
    </div>

    <!-- Loading state -->
    <div v-if="isLoading && servers.length === 0" class="flex items-center justify-center py-12">
      <Loader2 class="h-8 w-8 animate-spin text-primary" />
    </div>

    <!-- Filters - always visible, even while loading more -->
    <div class="flex flex-wrap gap-3 mb-6 p-4 bg-card rounded-lg border">
      <!-- Search -->
      <div class="relative flex-1 min-w-[200px]">
        <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <input
          v-model="searchQuery"
          type="text"
          :placeholder="$t('servers.searchPlaceholder')"
          class="w-full pl-10 pr-4 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
        />
      </div>
      
      <!-- MC Version Filter (Dynamic from API) -->
      <select
        v-model="filterMcVersion"
        class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white min-w-[120px]"
      >
        <option value="">{{ $t('servers.allVersions') }}</option>
        <option v-for="version in filterOptions.versions" :key="version" :value="version">
          {{ version }}
        </option>
      </select>

      <!-- Server Type Filter (Dynamic from API) -->
      <select
        v-model="filterServerType"
        class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white min-w-[120px]"
      >
        <option value="">{{ $t('servers.filters.allTypes') }}</option>
        <option v-for="type in filterOptions.serverTypes" :key="type" :value="type">
          {{ type === 'vanilla' ? $t('servers.types.vanilla') : type === 'modded' ? $t('servers.types.modded') : type === 'custom' ? $t('servers.types.custom') : type }}
        </option>
      </select>

      <!-- Auth Type Filter (Dynamic from API) -->
      <select
        v-model="filterAuthType"
        class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white min-w-[140px]"
      >
        <option value="">{{ $t('servers.filters.allAuth') }}</option>
        <option v-for="auth in filterOptions.authTypes" :key="auth" :value="auth">
          {{ auth === 'microsoft' ? $t('servers.auth.microsoft') : auth === 'offline' ? $t('servers.auth.offline') : auth }}
        </option>
      </select>

      <!-- Show Only Favorites Filter -->
      <button 
        @click="showOnlyFavorites = !showOnlyFavorites"
        class="flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors border"
        :class="showOnlyFavorites ? 'bg-yellow-100 border-yellow-300 text-yellow-800 dark:bg-yellow-900/30 dark:border-yellow-700 dark:text-yellow-400' : 'bg-white dark:bg-zinc-800 border-neutral-300 dark:border-zinc-700 text-neutral-900 dark:text-white'"
      >
        <Star class="w-4 h-4" :class="{ 'fill-current': showOnlyFavorites }" />
        {{ $t('servers.filters.favorites', 'Favorites') }}
      </button>
    </div>

    <!-- Server Grid -->
    <div v-if="servers.length > 0" class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 overflow-y-auto" @scroll="handleScroll">
      <div
        v-for="server in filteredServers"
        :key="server.id"
        class="group relative rounded-lg border bg-card p-4 hover:border-primary/50 hover:shadow-md transition-all"
      >
        <!-- Favorite Button -->
        <button 
          @click="toggleFavorite(server.id, $event)"
          class="absolute top-3 right-3 p-1.5 rounded-full hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors z-10"
          :title="$t('servers.favorite', 'Favorite')"
        >
          <Star 
            class="w-5 h-5 transition-all"
            :class="favoriteServerIds.includes(server.id) ? 'text-yellow-500 fill-yellow-500 scale-110' : 'text-neutral-400 dark:text-neutral-500 group-hover:text-yellow-500'"
          />
        </button>

        <!-- Server Header -->
        <div class="flex items-start gap-3 mb-3 pr-8">
          <div class="flex h-12 w-12 items-center justify-center rounded-lg bg-muted shrink-0">
            <Gamepad2 class="h-6 w-6 text-muted-foreground" />
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="font-semibold truncate">{{ server.name }}</h3>
            <p class="text-xs text-muted-foreground line-clamp-2">{{ server.motd }}</p>
          </div>
          
          <!-- Server Status (Ping & Players) - Top Right -->
          <div class="flex flex-col items-end gap-1 text-xs text-muted-foreground shrink-0 mt-0.5">
            <!-- Ping Status (Top) -->
            <div class="flex items-center gap-1.5" v-if="serverStatuses[server.id]">
              <span class="w-2 h-2 rounded-full" :class="serverStatuses[server.id].ping <= 50 ? 'bg-green-500' : serverStatuses[server.id].ping <= 150 ? 'bg-yellow-500' : 'bg-red-500'"></span>
              <span>{{ serverStatuses[server.id].ping }} ms</span>
            </div>
            <div class="flex items-center gap-1.5" v-else-if="serverStatusesLoading[server.id]">
              <span class="w-2 h-2 rounded-full bg-gray-400"></span>
              <span class="italic">{{ $t('servers.pinging', 'Ping...') }}</span>
            </div>
            <div class="flex items-center gap-1.5" v-else>
              <span class="w-2 h-2 rounded-full bg-red-500"></span>
              <span class="italic text-red-500">Timeout</span>
            </div>
            
            <!-- Player Count (Bottom) -->
            <div class="flex items-center gap-1" v-if="serverStatuses[server.id]">
              <Users class="h-3 w-3" />
              <span>{{ serverStatuses[server.id].onlinePlayers }} / {{ serverStatuses[server.id].maxPlayers }}</span>
            </div>
          </div>
        </div>

        <!-- Server Info -->
        <div class="flex flex-wrap gap-2 mb-3">
          <span class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold bg-zinc-100 dark:bg-zinc-800">
            {{ server.version }}
          </span>
          <!-- Server Type Badge -->
          <span
            class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold"
            :class="{
              'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300': server.serverType === 'vanilla',
              'bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300': server.serverType === 'modded',
              'bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300': server.serverType === 'custom'
            }"
          >
            {{ server.serverType === 'vanilla' ? $t('servers.types.vanilla') : server.serverType === 'modded' ? $t('servers.types.modded') : $t('servers.types.custom') }}
          </span>
          <!-- Auth Type Badge -->
          <span
            v-if="server.authType"
            class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold"
            :class="server.authType === 'microsoft' ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300' : 'bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300'"
          >
            {{ server.authType === 'microsoft' ? $t('servers.auth.microsoftShort') : $t('servers.auth.offlineShort') }}
          </span>
        </div>


        <!-- IP and Actions -->
        <div class="flex items-center justify-between pt-3 border-t">
          <div class="flex items-center gap-2">
            <code class="text-xs bg-muted px-2 py-1 rounded">{{ server.ip }}:{{ server.port }}</code>
            <button
              @click="copyIp(server)"
              class="p-1 hover:bg-muted rounded transition-colors"
              title="{{ $t('servers.actions.copyIp') }}"
            >
              <Check v-if="copiedServerId === server.id" class="h-4 w-4 text-green-500" />
              <Copy v-else class="h-4 w-4 text-muted-foreground" />
            </button>
          </div>
          <div class="flex items-center gap-2">
            <!-- Install Client button (only for modded/custom servers with pack) -->
            <button
              v-if="(server.packFileName || server.packSource) && server.isActive"
              @click="installModpack(server)"
              :disabled="installingServerId === server.id"
              class="flex items-center gap-1 text-sm text-purple-600 hover:text-purple-700 dark:text-purple-400 disabled:opacity-50"
              title="{{ $t('servers.actions.installClient') }}"
            >
              <Loader2 v-if="installingServerId === server.id" class="h-4 w-4 animate-spin" />
              <Download v-else class="h-4 w-4" />
              Install
            </button>
            <button 
              @click="launchAndConnect(server)"
              :disabled="isConnecting === server.id"
              class="flex items-center gap-1.5 px-4 py-1.5 bg-primary text-primary-foreground text-sm font-bold rounded-md hover:bg-primary/90 transition-all shadow-sm active:scale-95 disabled:opacity-50"
            >
              <Loader2 v-if="isConnecting === server.id" class="h-4 w-4 animate-spin" />
              <svg v-else xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
              {{ $t('servers.join') }}
            </button>
          </div>
        </div>
      </div>

      <!-- Empty State -->
      <div v-if="filteredServers.length === 0" class="col-span-full flex flex-col items-center justify-center py-12 text-center">
        <Server class="h-12 w-12 text-muted-foreground mb-4" />
        <h3 class="text-lg font-semibold text-neutral-900 dark:text-white mb-2">{{ $t('servers.noServers') }}</h3>
        <p class="text-sm text-muted-foreground">{{ $t('servers.noServersDesc') }}</p>
      </div>
    </div>

    <!-- Publish Server Dialog -->
    <Teleport to="body">
      <div v-if="showPublishDialog" class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
        <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto" @click="closePublishDialog"></div>
        <div class="relative z-10 w-full max-w-lg gap-4 border bg-white dark:bg-zinc-900 p-4 shadow-xl rounded-lg pointer-events-auto">
          <!-- Header with Progress -->
          <div class="flex items-center justify-between mb-4">
            <h3 class="font-semibold text-lg text-neutral-900 dark:text-white">{{ $t('servers.publishTitle') }}</h3>
            <button @click="closePublishDialog" class="text-muted-foreground hover:text-foreground text-lg">
              ✕
            </button>
          </div>
          
          <!-- Progress Steps -->
          <div class="flex items-center justify-between mb-6">
            <div v-for="step in totalSteps" :key="step" class="flex items-center flex-1">
              <div 
                class="flex items-center justify-center w-8 h-8 rounded-full text-sm font-semibold transition-all"
                :class="{
                  'bg-primary text-primary-foreground ring-2 ring-primary ring-offset-2 dark:ring-offset-zinc-900': publishStep === step,
                  'bg-primary text-primary-foreground': publishStep > step,
                  'bg-muted text-muted-foreground': publishStep < step
                }"
              >
                <Check v-if="publishStep > step" class="w-4 h-4" />
                <span v-else>{{ step }}</span>
              </div>
              <div v-if="step < totalSteps" class="flex-1 h-0.5 mx-2 transition-colors" :class="publishStep > step ? 'bg-primary' : 'bg-muted'"></div>
            </div>
          </div>
          
          <p class="text-xs text-muted-foreground mb-4">
            {{ $t('servers.publishDialog.stepOf', { step: publishStep, total: totalSteps }) }} 
            {{ publishStep === 1 ? $t('servers.publishDialog.steps.basicInfo') : publishStep === 2 ? $t('servers.publishDialog.steps.versionType') : publishStep === 3 ? $t('servers.publishDialog.steps.modpack') : $t('servers.publishDialog.steps.review') }}
          </p>

          <div class="space-y-4">
            <!-- Step 1: Basic Info -->
            <template v-if="publishStep === 1">
              <div class="space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.publishDialog.serverName') }} <span class="text-red-500">*</span></label>
                <input v-model="newServer.name" type="text" :placeholder="$t('servers.publishDialog.serverNamePlaceholder')" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500" />
              </div>
              <div class="flex gap-2">
                <div class="flex-1 space-y-1">
                  <label class="text-sm font-medium">{{ $t('servers.publishDialog.ipAddress') }} <span class="text-red-500">*</span></label>
                  <input v-model="newServer.ip" type="text" :placeholder="$t('servers.publishDialog.ipPlaceholder')" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500" />
                </div>
                <div class="w-24 space-y-1">
                  <label class="text-sm font-medium">{{ $t('servers.publishDialog.port') }}</label>
                  <input v-model.number="newServer.port" type="number" placeholder="25565" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500" />
                </div>
              </div>
            </template>

            <!-- Step 2: Version & Type -->
            <template v-if="publishStep === 2">
              <!-- Minecraft Version (Searchable Combobox with Groups) -->
              <div class="space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.publishDialog.mcVersion') }} <span class="text-red-500">*</span></label>
                <div class="relative version-dropdown">
                  <div 
                    class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white cursor-pointer flex items-center justify-between"
                    @click="showVersionDropdown = !showVersionDropdown; fetchMcVersions(); versionSearchQuery = ''"
                  >
                    <span :class="newServer.version ? 'text-neutral-900 dark:text-white' : 'text-neutral-400'">
                      {{ newServer.version || ($t('servers.selectVersion') as string) }}
                    </span>
                    <ChevronDown class="h-4 w-4 text-muted-foreground" />
                  </div>
                  
                  <!-- Dropdown -->
                  <div v-if="showVersionDropdown" class="absolute z-20 w-full mt-1 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md shadow-lg max-h-80 overflow-hidden">
                    <!-- Search Input -->
                    <div class="p-2 border-b border-neutral-200 dark:border-zinc-700">
                      <div class="relative">
                        <Search class="absolute left-2 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                        <input 
                          v-model="versionSearchQuery"
                          type="text"
                          :placeholder="$t('servers.publishDialog.searchVersions')"
                          class="w-full pl-8 pr-3 py-1.5 text-sm bg-neutral-100 dark:bg-zinc-700 border-0 rounded text-neutral-900 dark:text-white placeholder:text-neutral-400"
                          @click.stop
                          autofocus
                        />
                      </div>
                    </div>
                    
                    <!-- Loading State -->
                    <div v-if="mcVersionsLoading" class="p-4 text-center">
                      <Loader2 class="h-5 w-5 animate-spin mx-auto text-muted-foreground" />
                    </div>
                    
                    <!-- Error State -->
                    <div v-else-if="mcVersionsError" class="p-4 text-center text-red-500 text-sm">
                      {{ mcVersionsError }}
                    </div>
                    
                    <!-- Version List (Grouped by Type) -->
                    <div v-else class="max-h-64 overflow-y-auto">
                      <!-- Release Versions -->
                      <template v-if="groupedVersions.release.length > 0">
                        <div class="px-3 py-1.5 text-xs font-semibold bg-muted text-muted-foreground">Release</div>
                        <div
                          v-for="v in groupedVersions.release"
                          :key="v.id"
                          class="px-3 py-2 cursor-pointer hover:bg-muted flex items-center justify-between"
                          :class="newServer.version === v.id ? 'bg-primary/10' : ''"
                          @click="selectVersion(v)"
                        >
                          <span class="text-neutral-900 dark:text-white">{{ v.id }}</span>
                        </div>
                      </template>
                      
                      <!-- Snapshot Versions -->
                      <template v-if="groupedVersions.snapshot.length > 0">
                        <div class="px-3 py-1.5 text-xs font-semibold bg-muted text-muted-foreground">Snapshot</div>
                        <div
                          v-for="v in groupedVersions.snapshot"
                          :key="v.id"
                          class="px-3 py-2 cursor-pointer hover:bg-muted flex items-center justify-between"
                          :class="newServer.version === v.id ? 'bg-primary/10' : ''"
                          @click="selectVersion(v)"
                        >
                          <span class="text-neutral-900 dark:text-white">{{ v.id }}</span>
                        </div>
                      </template>
                      
                      <!-- Old Beta Versions -->
                      <template v-if="groupedVersions.old_beta.length > 0">
                        <div class="px-3 py-1.5 text-xs font-semibold bg-muted text-muted-foreground">Old Beta</div>
                        <div
                          v-for="v in groupedVersions.old_beta"
                          :key="v.id"
                          class="px-3 py-2 cursor-pointer hover:bg-muted flex items-center justify-between"
                          :class="newServer.version === v.id ? 'bg-primary/10' : ''"
                          @click="selectVersion(v)"
                        >
                          <span class="text-neutral-900 dark:text-white">{{ v.id }}</span>
                        </div>
                      </template>
                      
                      <!-- Old Alpha Versions -->
                      <template v-if="groupedVersions.old_alpha.length > 0">
                        <div class="px-3 py-1.5 text-xs font-semibold bg-muted text-muted-foreground">Old Alpha</div>
                        <div
                          v-for="v in groupedVersions.old_alpha"
                          :key="v.id"
                          class="px-3 py-2 cursor-pointer hover:bg-muted flex items-center justify-between"
                          :class="newServer.version === v.id ? 'bg-primary/10' : ''"
                          @click="selectVersion(v)"
                        >
                          <span class="text-neutral-900 dark:text-white">{{ v.id }}</span>
                        </div>
                      </template>
                      
                      <div v-if="(groupedVersions.release.length + groupedVersions.snapshot.length + groupedVersions.old_beta.length + groupedVersions.old_alpha.length) === 0 && versionSearchQuery" class="p-4 text-center text-muted-foreground text-sm">
                        {{ $t('servers.publishDialog.noVersions') }}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
              
              <div class="space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.publishDialog.serverType') }}</label>
                <select v-model="newServer.serverType" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white">
                  <option value="vanilla">{{ $t('servers.publishDialog.typeVanilla') }}</option>
                  <option value="modded">{{ $t('servers.publishDialog.typeModded') }}</option>
                  <option value="custom">{{ $t('servers.publishDialog.typeCustom') }}</option>
                </select>
                <p class="text-xs text-muted-foreground">
                  {{ newServer.serverType === 'vanilla' ? $t('servers.publishDialog.descVanilla') : $t('servers.publishDialog.descModded') }}
                </p>
              </div>
              
              <div class="space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.publishDialog.authType', 'Authentication Type') }}</label>
                <select v-model="newServer.authType" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white">
                  <option value="offline">{{ $t('servers.auth.offline') }}</option>
                  <option value="microsoft">{{ $t('servers.auth.microsoft') }}</option>
                </select>
              </div>
            </template>

            <!-- Step 3: Modpack (Optional) -->
            <template v-if="publishStep === 3">
              <template v-if="needsPackFile(newServer.serverType)">
                <div class="flex bg-neutral-100 dark:bg-zinc-800 rounded-lg p-1 mb-4 shrink-0">
                  <button 
                    @click="packBindMode = 'local'"
                    class="flex-1 py-1.5 text-sm font-medium rounded-md transition-all"
                    :class="packBindMode === 'local' ? 'bg-white dark:bg-zinc-700 shadow-sm text-neutral-900 dark:text-white' : 'text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300'"
                  >
                    Upload Local ZIP
                  </button>
                  <button 
                    @click="packBindMode = 'online'"
                    class="flex-1 py-1.5 text-sm font-medium rounded-md transition-all"
                    :class="packBindMode === 'online' ? 'bg-white dark:bg-zinc-700 shadow-sm text-neutral-900 dark:text-white' : 'text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300'"
                  >
                    Bind Online Modpack
                  </button>
                </div>

                <div v-if="packBindMode === 'local'" class="space-y-2">
                  <label class="text-sm font-medium">{{ $t('servers.publishDialog.modpackZip') }} <span class="text-red-500">*</span></label>
                  <div class="flex items-center gap-2">
                    <button
                      @click="selectPackFile"
                      class="flex-1 px-3 py-2 text-sm border border-neutral-300 dark:border-zinc-700 rounded-md hover:bg-muted transition-colors text-left text-neutral-900 dark:text-white truncate"
                    >
                      {{ selectedPackFileName || $t('servers.publishDialog.selectZip') }}
                    </button>
                  </div>
                  <p class="text-xs text-muted-foreground">{{ $t('servers.publishDialog.uploadDesc') }}</p>
                  <p v-if="isUploadingPack" class="text-xs text-blue-600 dark:text-blue-400">
                    <Loader2 class="h-3 w-3 animate-spin inline mr-1" />
                    {{ $t('servers.publishDialog.uploading') }}
                  </p>
                </div>

                <div v-else class="space-y-4">
                  <div class="flex gap-2">
                    <select v-model="onlineSource" class="w-1/3 px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white">
                      <option value="modrinth">Modrinth</option>
                      <option value="curseforge">CurseForge</option>
                    </select>
                    <div class="flex-1 relative">
                      <input 
                        v-model="onlineSearchQuery" 
                        type="text" 
                        placeholder="Search modpacks..." 
                        class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400"
                        @keydown.enter="searchOnlineModpacks"
                      />
                    </div>
                    <button @click="searchOnlineModpacks" :disabled="isSearchingOnline" class="px-3 py-2 bg-primary text-primary-foreground rounded-md text-sm font-medium disabled:opacity-50 hover:bg-primary/90">
                      <Loader2 v-if="isSearchingOnline" class="h-4 w-4 animate-spin" />
                      <Search v-else class="h-4 w-4" />
                    </button>
                  </div>

                  <div v-if="selectedOnlineProject" class="p-3 bg-neutral-50 dark:bg-zinc-800/50 rounded-lg border border-neutral-200 dark:border-zinc-700 space-y-3">
                    <div class="flex items-center justify-between">
                      <div class="flex items-center gap-2 overflow-hidden">
                        <img v-if="selectedOnlineProject.icon_url" :src="selectedOnlineProject.icon_url" class="h-6 w-6 rounded" />
                        <span class="text-sm font-medium truncate">{{ selectedOnlineProject.title }}</span>
                      </div>
                      <button @click="selectedOnlineProject = null" class="text-xs text-muted-foreground hover:text-foreground">Change</button>
                    </div>
                    <div class="space-y-1">
                      <label class="text-xs font-medium text-muted-foreground">Select Version <span class="text-red-500">*</span></label>
                      <select 
                        v-model="selectedOnlineVersion" 
                        class="w-full px-3 py-1.5 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
                        :disabled="isFetchingOnlineVersions"
                        @change="() => { newServer.packProjectId = selectedOnlineProject?.project_id || ''; newServer.packSource = onlineSource || ''; newServer.packVersionId = selectedOnlineVersion?.id ? String(selectedOnlineVersion.id) : '' }"
                      >
                        <option :value="null" disabled>{{ isFetchingOnlineVersions ? 'Loading versions...' : 'Select a version' }}</option>
                        <option v-for="v in onlineModpackVersions" :key="v.id" :value="v">
                          {{ v.name }} ({{ v.version_number || v.name }})
                        </option>
                      </select>
                    </div>
                  </div>

                  <div v-else-if="onlineModpacks.length > 0" class="max-h-48 overflow-y-auto space-y-2 border border-neutral-200 dark:border-zinc-700 rounded-lg p-2 bg-neutral-50 dark:bg-zinc-800/30">
                    <div 
                      v-for="pack in onlineModpacks" 
                      :key="pack.project_id"
                      class="flex items-center gap-3 p-2 hover:bg-neutral-100 dark:hover:bg-zinc-800 rounded-md cursor-pointer transition-colors"
                      @click="selectOnlineProject(pack)"
                    >
                      <img v-if="pack.icon_url" :src="pack.icon_url" class="h-8 w-8 rounded object-cover bg-neutral-200 dark:bg-zinc-700" />
                      <div class="flex-1 min-w-0">
                        <p class="text-sm font-medium text-neutral-900 dark:text-white truncate">{{ pack.title }}</p>
                        <p class="text-xs text-muted-foreground truncate">{{ pack.author || pack.description }}</p>
                      </div>
                    </div>
                  </div>
                </div>
              </template>
              <template v-else>
                <div class="py-8 text-center">
                  <Package class="h-12 w-12 mx-auto text-muted-foreground mb-3" />
                  <p class="text-muted-foreground">{{ $t('servers.publishDialog.noModpackReq') }}</p>
                  <p class="text-xs text-muted-foreground mt-2">{{ $t('servers.publishDialog.skipStep') }}</p>
                </div>
              </template>
            </template>

            <!-- Step 4: Review & Submit -->
            <template v-if="publishStep === 4">
              <div class="space-y-3 py-2">
                <h4 class="font-medium text-neutral-900 dark:text-white">{{ $t('servers.publishDialog.reviewTitle') }}</h4>
                <div class="bg-muted rounded-md p-3 space-y-2 text-sm">
                  <div class="flex justify-between">
                    <span class="text-muted-foreground">{{ $t('servers.publishDialog.name') }}</span>
                    <span class="font-medium">{{ newServer.name }}</span>
                  </div>
                  <div class="flex justify-between">
                    <span class="text-muted-foreground">{{ $t('servers.publishDialog.address') }}</span>
                    <span>{{ newServer.ip }}:{{ newServer.port }}</span>
                  </div>
                  <div class="flex justify-between">
                    <span class="text-muted-foreground">{{ $t('servers.publishDialog.version') }}</span>
                    <span>{{ newServer.version }}</span>
                  </div>
                  <div class="flex justify-between">
                    <span class="text-muted-foreground">{{ $t('servers.publishDialog.type') }}</span>
                    <span class="capitalize">{{ newServer.serverType }}</span>
                  </div>
                  <div v-if="selectedPackFileName" class="flex justify-between">
                    <span class="text-muted-foreground">{{ $t('servers.publishDialog.modpack') }}</span>
                    <span>{{ selectedPackFileName }}</span>
                  </div>
                </div>
                
                <!-- Admin Email -->
                <div class="space-y-1 pt-2 border-t">
                  <label class="text-sm font-medium">{{ $t('servers.publishDialog.adminEmail') }} <span class="text-red-500">*</span></label>
                  <input v-model="newServer.email" type="email" :placeholder="$t('servers.publishDialog.emailPlaceholder')" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500" />
                  <p class="text-xs text-muted-foreground">{{ $t('servers.publishDialog.emailDesc') }}</p>
                </div>
                
                <!-- Error display -->
                <div v-if="error" class="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                  <p class="text-sm text-red-600 dark:text-red-400">{{ error }}</p>
                </div>
              </div>
            </template>
          </div>

          <!-- Navigation Buttons -->
          <div class="flex justify-between gap-2 mt-6">
            <button 
              v-if="publishStep > 1" 
              @click="publishStep--"
              class="px-3 py-1.5 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
            >
              Back
            </button>
            <div v-else></div>
            
            <button 
              v-if="publishStep < 4" 
              @click="publishStep++"
              :disabled="!canGoToStep(publishStep)"
              class="px-3 py-1.5 text-sm font-medium bg-neutral-900 text-white dark:bg-white dark:text-neutral-900 rounded-md hover:bg-neutral-800 dark:hover:bg-neutral-200 transition-colors disabled:opacity-50"
            >
              Next
            </button>
            <button 
              v-else
              @click="submitServer" 
              :disabled="isSubmitting || isUploadingPack || !newServer.email"
              class="px-3 py-1.5 text-sm font-medium bg-neutral-900 text-white dark:bg-white dark:text-neutral-900 rounded-md hover:bg-neutral-800 dark:hover:bg-neutral-200 transition-colors disabled:opacity-50"
            >
              <Loader2 v-if="isSubmitting || isUploadingPack" class="h-4 w-4 animate-spin inline mr-2" />
              {{ isUploadingPack ? $t('servers.publishDialog.submitting') : $t('servers.publishDialog.submit') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
    <!-- Custom Alert Modal -->
    <Teleport to="body">
      <div v-if="alertState.show" class="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 backdrop-blur-sm">
        <div class="bg-white dark:bg-zinc-900 w-full max-w-sm rounded-xl shadow-2xl overflow-hidden animate-in fade-in zoom-in-95 duration-200">
          <div class="px-4 py-3 border-b border-neutral-200 dark:border-zinc-800">
            <h3 class="font-semibold text-neutral-900 dark:text-white">{{ $t('common.notification', 'Notification') }}</h3>
          </div>
          <div class="p-4">
            <p class="text-sm text-neutral-600 dark:text-neutral-400 mb-6">{{ alertState.message }}</p>
            <div class="flex justify-end mt-4">
              <button @click="closeAlert" class="px-3 py-1.5 text-sm font-medium bg-neutral-900 text-white dark:bg-white dark:text-neutral-900 rounded-md hover:bg-neutral-800 dark:hover:bg-neutral-200 transition-colors">
                {{ $t('common.ok', 'OK') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Custom Prompt Modal -->
    <Teleport to="body">
      <div v-if="promptState.show" class="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 backdrop-blur-sm">
        <div class="bg-white dark:bg-zinc-900 w-full max-w-sm rounded-xl shadow-2xl overflow-hidden animate-in fade-in zoom-in-95 duration-200">
          <div class="px-4 py-3 border-b border-neutral-200 dark:border-zinc-800">
            <h3 class="font-semibold text-neutral-900 dark:text-white">{{ $t('common.inputRequired', 'Input Required') }}</h3>
          </div>
          <div class="p-4">
            <p class="text-sm text-neutral-600 dark:text-neutral-400 mb-4">{{ promptState.message }}</p>
            <input 
              v-model="promptState.value" 
              type="text" 
              class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 focus:outline-none focus:ring-2 focus:ring-neutral-500 mb-2"
              @keyup.enter="confirmPrompt"
            />
            <div class="flex justify-end gap-2 mt-4">
              <button @click="cancelPrompt" class="px-3 py-1.5 text-sm font-medium border border-neutral-300 dark:border-zinc-700 rounded-md hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors text-neutral-700 dark:text-neutral-300">
                {{ $t('common.cancel', 'Cancel') }}
              </button>
              <button @click="confirmPrompt" class="px-3 py-1.5 text-sm font-medium bg-neutral-900 text-white dark:bg-white dark:text-neutral-900 rounded-md hover:bg-neutral-800 dark:hover:bg-neutral-200 transition-colors">
                {{ $t('common.confirm', 'Confirm') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>