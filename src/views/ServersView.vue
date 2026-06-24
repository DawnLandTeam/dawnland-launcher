<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, onUnmounted, onActivated, onDeactivated, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import DInput from '../components/ui/DInput.vue';
import { invoke } from "@tauri-apps/api/core";
import DSelect from '../components/ui/DSelect.vue';
import { openUrl } from "@tauri-apps/plugin-opener";
import { Server, Gamepad2, Search, Copy, Check, Loader2, Download, Users, Star, RefreshCw } from "@lucide/vue";
import { getErrorMessage } from "../utils/error";
import { EXTERNAL_URLS } from "../utils/constants";
import { trackEvent } from "../utils/analytics";

// Types matching the Rust Server model
interface ServerInfo {
  id: number;
  name: string;
  ip: string;
  port: number;
  motd: string;
  version: string;
  serverType: string;        // "vanilla", "modded", "custom"
  authType: string;          // "offline", "online", "authlib"
  authlibApi?: string;
  packFileName: string | null;
  packFileSize: number | null;
  packProjectId: string | null;
  packVersionId: string | null;
  packSource: string | null;
  iconUrl: string;
  email: string;
  isActive: boolean;
  tags?: string;
  description?: string;
  contactGroup?: string;
  contactOwner?: string;
}




// Filter options response
interface FilterOptions {
  versions: string[];
  serverTypes: string[];
  authTypes: string[];
}

interface ServerListResponse {
  data: ServerInfo[];
  total: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

interface ServerStatus {
  onlinePlayers: number;
  maxPlayers: number;
  ping: number;
}

// State
const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const servers = ref<ServerInfo[]>([]);
const isLoading = ref(false);
const error = ref<string | null>(null);
const currentPage = ref(1);

const filterMcVersionOptions = computed(() => [
  { label: t('servers.allVersions'), value: '' },
  ...filterOptions.value.versions.map(v => ({ label: v, value: v }))
]);

const filterServerTypeOptions = computed(() => [
  { label: t('servers.filters.allTypes'), value: '' },
  ...filterOptions.value.serverTypes.map(type => ({
    label: type === 'vanilla' ? t('servers.types.vanilla') : type === 'modded' ? t('servers.types.modded') : type === 'custom' ? t('servers.types.custom') : type,
    value: type
  }))
]);

const filterAuthTypeOptions = computed(() => [
  { label: t('servers.filters.allAuth'), value: '' },
  ...filterOptions.value.authTypes.map(auth => ({
    label: auth === 'microsoft' ? t('servers.auth.microsoft') : auth === 'offline' ? t('servers.auth.offline') : auth,
    value: auth
  }))
]);

const totalPages = ref(1);


// Filter version dropdown (separate state)
const showFilterVersionDropdown = ref(false);


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
  servers.value = [];
  await fetchServers();
}

// Watch for filter changes with debounce for search
let searchTimeout: ReturnType<typeof setTimeout> | null = null;
watch(searchQuery, () => {
  if (searchTimeout) clearTimeout(searchTimeout);
  searchTimeout = setTimeout(() => {
    applyFilters();
  }, 500);
});

onBeforeUnmount(() => {
  if (searchTimeout) {
    clearTimeout(searchTimeout);
    searchTimeout = null;
  }
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

onDeactivated(() => {
  searchQuery.value = "";
  filterMcVersion.value = "";
  filterServerType.value = "";
  filterAuthType.value = "";
});


const showDetailsModal = ref(false);
const selectedDetailsServer = ref<ServerInfo | null>(null);

function openServerDetails(server: ServerInfo) {
  selectedDetailsServer.value = server;
  showDetailsModal.value = true;
}

// Server ping status
const serverStatuses = ref<Record<number, ServerStatus>>({});
const serverStatusesLoading = ref<Record<number, boolean>>({});

// Install modpack state
const installingServerId = ref<number | null>(null);

// Copy notification
const copiedServerId = ref<number | null>(null);


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
// Fetch servers from Rust backend
async function fetchServers() {
  isLoading.value = true;
  error.value = null;
  try {
    const response = await invoke<ServerListResponse>("get_servers", {
      page: currentPage.value,
      pageSize: 30,
      search: searchQuery.value,
      version: filterMcVersion.value,
      serverType: filterServerType.value,
      authType: filterAuthType.value
    });
    
    if (currentPage.value === 1) {
      servers.value = response.data;
    } else {
      servers.value = [...servers.value, ...response.data];
    }
    
    totalPages.value = response.totalPages;
    
    // Check deep link after loading
    if (route.query.view_id && typeof route.query.view_id === 'string') {
      const server = servers.value.find(s => s.id === Number(route.query.view_id));
      if (server) {
        openServerDetails(server);
        const newQuery = { ...route.query };
        delete newQuery.view_id;
        router.replace({ query: newQuery });
      }
    }
  } catch (e) {
    error.value = getErrorMessage(e);
    console.error("Failed to fetch servers:", e);
  } finally {
    isLoading.value = false;
  }
}

// Load more servers (infinite scroll)
async function loadMoreServers() {
  if (isLoading.value || currentPage.value >= totalPages.value) return;
  currentPage.value++;
  await fetchServers();
}

const installedInstances = ref<any[]>([]);
const accounts = ref<any[]>([]);

async function fetchLocalData() {
  try {
    installedInstances.value = await invoke("scan_installed_instances");
    accounts.value = await invoke("get_accounts");
  } catch (error) {
    console.error("Failed to load local data:", error);
  }
}

function isClientInstalled(server: ServerInfo): boolean {
  if (server.serverType === 'vanilla') {
    return installedInstances.value.some(i => i.mcVersion === server.version && (!i.loaderType || i.loaderType.toLowerCase() === 'vanilla'));
  }
  
  if (server.serverType === 'modded' || server.serverType === 'custom') {
    if (!server.packFileName && !server.packSource) {
      // Custom server with no pack attached: show Join directly
      return true;
    }
    
    return installedInstances.value.some(i => {
      // Exact match on serverId
      if (i.serverId !== String(server.id)) return false;
      
      // If it's an online modpack, compare packVersionId
      if (server.packVersionId) {
        return i.packVersionId === server.packVersionId;
      }
      
      // If it's a local zip modpack, compare packFileName
      if (server.packFileName) {
        return i.packFileName === server.packFileName;
      }
      
      return true;
    });
  }
  
  return true;
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
  trackEvent("Servers Viewed");
  loadFavorites();
  fetchServers();
  fetchFilterOptions();
  fetchLocalData();
  // Close version dropdown when clicking outside
  document.addEventListener("click", handleClickOutside);
});

// Watch for deep links when already on the page
watch(
  () => route.query.view_id,
  (newId) => {
    if (newId && typeof newId === 'string' && servers.value.length > 0) {
      const server = servers.value.find(s => s.id === Number(newId));
      if (server) {
        openServerDetails(server);
        const newQuery = { ...route.query };
        delete newQuery.view_id;
        router.replace({ query: newQuery });
      }
    }
  }
);

// Refresh when coming back to this view (keep-alive reactivation)
onActivated(() => {
  fetchFilterOptions();
  fetchLocalData();
});

onUnmounted(() => {
  document.removeEventListener("click", handleClickOutside);
});

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement;
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
  let list = [...servers.value];
  
  // Backend already handles search, version, type, and auth
  // We only apply the favorite filter locally if active
  if (showOnlyFavorites.value) {
    list = list.filter(server => favoriteServerIds.value.includes(server.id));
  }
  
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
    // Optionally clear status on failure so it shows offline
    delete serverStatuses.value[server.id];
  } finally {
    serverStatusesLoading.value[server.id] = false;
  }
}

const isRefreshing = ref(false);

async function refreshServers() {
  if (isRefreshing.value || isLoading.value) return;
  isRefreshing.value = true;
  
  try {
    // Clear status cache to show loading
    serverStatuses.value = {};
    serverStatusesLoading.value = {};
    
    // Reset page and refetch data from backend
    currentPage.value = 1;
    await fetchServers();
  } finally {
    isRefreshing.value = false;
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
  if (server.authType === 'authlib') {
    const matchingAccount = accounts.value.find(a => a.accountType === 'authlib' && a.authlibUrl === server.authlibApi);
    if (!matchingAccount) {
      showAlert(t('servers.messages.authlibRequired', { api: server.authlibApi }), {
        label: t('accounts.addAccount', 'Add Account'),
        onClick: () => {
          router.push({ path: '/accounts', query: { addAuthlib: server.authlibApi } });
        }
      });
      return;
    }
  }

  router.push({
    path: '/',
    query: {
      auto_launch: 'true',
      server_id: String(server.id),
      server_name: server.name,
      server_version: server.version,

      server_ip: server.ip,
      server_port: String(server.port),
      auth_type: server.authType,
      authlib_api: server.authlibApi
    }
  });
}
function copyIp(server: ServerInfo) {
  navigator.clipboard.writeText(`${server.ip}:${server.port}`);
  copiedServerId.value = server.id;
  setTimeout(() => {
    copiedServerId.value = null;
  }, 2000);
}


// Install client (modpack or vanilla) from server
async function installClient(server: ServerInfo) {
  if (server.serverType === 'vanilla') {
    router.push({
      path: '/instances',
      query: {
        install_version: server.version,
        install_loader: 'vanilla'
      }
    });
    return;
  }

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
    const backendUrl = import.meta.env.VITE_WEB_BACKEND_URL || 'http://localhost:3030';
    queryParams.online_url = `${backendUrl}/api/servers/${server.id}/pack`;
    queryParams.pack_file_name = server.packFileName;
  }
  
  router.push({
    path: '/downloads',
    query: { tab: 'modpack', ...queryParams }
  });
}

// Custom Dialog States
const alertState = ref<{
  show: boolean;
  message: string;
  action?: {
    label: string;
    onClick: () => void;
  };
}>({ show: false, message: '' });

function showAlert(message: string, action?: { label: string; onClick: () => void }) {
  alertState.value = { show: true, message, action };
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

import ServerDetailsModal from '../components/ServerDetailsModal.vue';

async function openPublishUrl() {
  try {
    await openUrl(EXTERNAL_URLS.PUBLISH_SERVER);
  } catch (err) {
    console.error("Failed to open publish URL:", err);
    showAlert(t('servers.publishError', 'Failed to open the browser. Please manually visit: ' + EXTERNAL_URLS.PUBLISH_SERVER));
  }
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
      <div class="flex items-center gap-2">
        <button
          @click="openPublishUrl"
          class="flex items-center justify-center h-8 px-3 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 transition-colors shadow-sm text-sm font-medium"
        >
          {{ $t('servers.publish') }}
        </button>
        <button
          @click="refreshServers"
          :disabled="isRefreshing"
          class="flex items-center justify-center h-8 w-8 rounded-md border border-neutral-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-zinc-700 transition-colors"
          :title="$t('servers.refreshList', 'Refresh Server List')"
          :aria-label="$t('servers.refreshList', 'Refresh Server List')"
        >
          <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
        </button>
      </div>
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
    <div class="flex flex-wrap gap-3 mb-6 p-4 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md rounded-lg border border-white/20 shadow-sm">
      <!-- Search -->
      <div class="relative flex-1 min-w-[200px]">
        <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <DInput
          v-model="searchQuery"
          :placeholder="$t('servers.searchPlaceholder')"
          class="!pl-10 !pr-4"
        />
      </div>
      
      <!-- MC Version Filter (Dynamic from API) -->
      <DSelect
        v-model="filterMcVersion"
        :options="filterMcVersionOptions"
        class="min-w-[120px]"
      />

      <!-- Server Type Filter (Dynamic from API) -->
      <DSelect
        v-model="filterServerType"
        :options="filterServerTypeOptions"
        class="min-w-[120px]"
      />

      <!-- Auth Type Filter (Dynamic from API) -->
      <DSelect
        v-model="filterAuthType"
        :options="filterAuthTypeOptions"
        class="min-w-[140px]"
      />

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
        class="flex flex-col bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md rounded-xl border border-white/20 overflow-hidden hover:border-primary/50 transition-all hover:shadow-md hover:bg-white/80 dark:hover:bg-zinc-900/80 p-4"
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
          <!-- First Tag Badge -->
          <span
            v-if="server.tags && server.tags.trim()"
            class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold bg-teal-100 text-teal-700 dark:bg-teal-900/40 dark:text-teal-300"
          >
            {{ server.tags.split(',')[0].trim() }}
          </span>
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
            :class="server.authType === 'microsoft' ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300' : server.authType === 'authlib' ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300' : 'bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300'"
          >
            {{ server.authType === 'microsoft' ? $t('servers.auth.microsoftShort') : server.authType === 'authlib' ? 'Authlib' : $t('servers.auth.offlineShort') }}
          </span>
        </div>


        <!-- IP and Actions -->
        <div class="flex items-center justify-between pt-3 border-t gap-2">
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <code class="text-xs bg-muted px-2 py-1 rounded truncate">{{ server.ip }}:{{ server.port }}</code>
            <button
              @click="copyIp(server)"
              class="p-1 hover:bg-muted rounded transition-colors shrink-0"
              title="{{ $t('servers.actions.copyIp') }}"
            >
              <Check v-if="copiedServerId === server.id" class="h-4 w-4 text-green-500" />
              <Copy v-else class="h-4 w-4 text-muted-foreground" />
            </button>
          </div>
          <div class="flex items-center gap-2 shrink-0">
            <!-- Install Client button -->
            <button
              v-if="!isClientInstalled(server)"
              @click="installClient(server)"
              :disabled="installingServerId === server.id"
              class="flex items-center gap-1.5 px-3 py-1.5 bg-primary text-primary-foreground text-sm font-bold rounded-md hover:bg-primary/90 transition-all shadow-sm active:scale-95 disabled:opacity-50 whitespace-nowrap"
              :title="$t('servers.actions.installClient')"
            >
              <Loader2 v-if="installingServerId === server.id" class="h-4 w-4 animate-spin" />
              <Download v-else class="h-4 w-4" />
              {{ server.serverType === 'vanilla' ? $t('servers.actions.installInstance') : $t('servers.actions.install') }}
            </button>
            <button 
              v-else
              @click="launchAndConnect(server)"
              :disabled="isConnecting === server.id"
              class="flex items-center gap-1.5 px-3 py-1.5 bg-primary text-primary-foreground text-sm font-bold rounded-md hover:bg-primary/90 transition-all shadow-sm active:scale-95 disabled:opacity-50 whitespace-nowrap"
            >
              <Loader2 v-if="isConnecting === server.id" class="h-4 w-4 animate-spin" />
              <svg v-else xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
              {{ $t('servers.join') }}
            </button>
            <button
              @click="openServerDetails(server)"
              class="flex items-center gap-1.5 px-3 py-1.5 bg-neutral-200 dark:bg-zinc-700 text-neutral-900 dark:text-white text-sm font-medium rounded-md hover:bg-neutral-300 dark:hover:bg-zinc-600 transition-all shadow-sm active:scale-95 whitespace-nowrap"
            >
              {{ $t('servers.actions.details') }}
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
    <!-- Custom Alert Modal -->
    <Teleport to="body">
      <div v-if="alertState.show" class="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 backdrop-blur-sm">
        <div class="bg-white dark:bg-zinc-900 w-full max-w-sm rounded-xl shadow-2xl overflow-hidden animate-in fade-in zoom-in-95 duration-200">
          <div class="px-4 py-3 border-b border-neutral-200 dark:border-zinc-800">
            <h3 class="font-semibold text-neutral-900 dark:text-white">{{ $t('common.notification', 'Notification') }}</h3>
          </div>
          <div class="p-4">
            <p class="text-sm text-neutral-600 dark:text-neutral-400 mb-6">{{ alertState.message }}</p>
            <div class="flex justify-end gap-2 mt-4">
              <button @click="closeAlert" :class="alertState.action ? 'px-3 py-1.5 text-sm font-medium border border-neutral-300 dark:border-zinc-700 rounded-md hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors text-neutral-700 dark:text-neutral-300' : 'px-3 py-1.5 text-sm font-medium bg-neutral-900 text-white dark:bg-white dark:text-neutral-900 rounded-md hover:bg-neutral-800 dark:hover:bg-neutral-200 transition-colors'">
                {{ alertState.action ? $t('common.cancel', 'Cancel') : $t('common.ok', 'OK') }}
              </button>
              <button v-if="alertState.action" @click="() => { alertState.action?.onClick(); closeAlert(); }" class="px-3 py-1.5 text-sm font-medium bg-neutral-900 text-white dark:bg-white dark:text-neutral-900 rounded-md hover:bg-neutral-800 dark:hover:bg-neutral-200 transition-colors">
                {{ alertState.action.label }}
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
            <DInput 
              v-model="promptState.value" 
              class="mb-2"
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

    <!-- Server Details Modal -->
    <ServerDetailsModal
      v-model:open="showDetailsModal"
      :server="selectedDetailsServer"
    />
  </div>
</template>
