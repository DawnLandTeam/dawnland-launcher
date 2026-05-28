<script setup lang="ts">
import { ref, computed } from "vue";
import { Server, Users, Gamepad2, Plus, Search, ExternalLink, Copy, Check } from "@lucide/vue";

// Types
interface ServerInfo {
  id: string;
  name: string;
  description: string;
  ip: string;
  port: number;
  mcVersion: string;
  loaderType: string;
  onlineMode: boolean;
  onlinePlayers: number;
  maxPlayers: number;
  logoUrl?: string;
}

// Mock server data
const servers = ref<ServerInfo[]>([
  {
    id: "1",
    name: "CraftLife Survival",
    description: "A vanilla+ survival server with custom biomes and economy",
    ip: "play.craftlife.net",
    port: 25565,
    mcVersion: "1.20.4",
    loaderType: "Fabric",
    onlineMode: true,
    onlinePlayers: 128,
    maxPlayers: 500,
  },
  {
    id: "2",
    name: "PixelCraft factions",
    description: "Raid, build, and conquer in our intense factions server",
    ip: "mc.pixelcraft.net",
    port: 25565,
    mcVersion: "1.20.1",
    loaderType: "Fabric",
    onlineMode: false,
    onlinePlayers: 256,
    maxPlayers: 1000,
  },
  {
    id: "3",
    name: "SkyBlock Pro",
    description: "Custom skyblock experience with 100+ quests",
    ip: "skyblock.pro",
    port: 25565,
    mcVersion: "1.19.4",
    loaderType: "Paper",
    onlineMode: true,
    onlinePlayers: 89,
    maxPlayers: 200,
  },
]);

// Filter state
const searchQuery = ref("");
const filterMcVersion = ref<string>("");
const filterLoaderType = ref<string>("");
const filterOnlineMode = ref<string>("");

// Dialog state
const showPublishDialog = ref(false);
const newServer = ref({
  name: "",
  ip: "",
  port: "25565",
  mcVersion: "",
  loaderType: "Vanilla",
  onlineMode: true,
  description: "",
});

// Copy notification
const copiedServerId = ref<string | null>(null);

// Filtered servers
const filteredServers = computed(() => {
  return servers.value.filter((server) => {
    // Search filter
    if (searchQuery.value) {
      const query = searchQuery.value.toLowerCase();
      if (
        !server.name.toLowerCase().includes(query) &&
        !server.description.toLowerCase().includes(query) &&
        !server.ip.toLowerCase().includes(query)
      ) {
        return false;
      }
    }
    // MC Version filter
    if (filterMcVersion.value && server.mcVersion !== filterMcVersion.value) {
      return false;
    }
    // Loader type filter
    if (filterLoaderType.value && server.loaderType !== filterLoaderType.value) {
      return false;
    }
    // Online mode filter
    if (filterOnlineMode.value === "online" && !server.onlineMode) {
      return false;
    }
    if (filterOnlineMode.value === "offline" && server.onlineMode) {
      return false;
    }
    return true;
  });
});

// Unique versions and loaders for filters
const mcVersions = computed(() => {
  const versions = new Set(servers.value.map((s) => s.mcVersion));
  return Array.from(versions).sort();
});

const loaderTypes = computed(() => {
  const loaders = new Set(servers.value.map((s) => s.loaderType));
  return Array.from(loaders).sort();
});

// Functions
function copyIp(server: ServerInfo) {
  navigator.clipboard.writeText(`${server.ip}:${server.port}`);
  copiedServerId.value = server.id;
  setTimeout(() => {
    copiedServerId.value = null;
  }, 2000);
}

function submitServer() {
  // Mock submission - in production this would call an API
  alert(`Server "${newServer.value.name}" submitted for review!`);
  showPublishDialog.value = false;
  // Reset form
  newServer.value = {
    name: "",
    ip: "",
    port: "25565",
    mcVersion: "",
    loaderType: "Vanilla",
    onlineMode: true,
    description: "",
  };
}

function loaderBadgeClass(loaderType: string): string {
  switch (loaderType.toLowerCase()) {
    case "fabric":
      return "bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300";
    case "forge":
      return "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300";
    case "paper":
      return "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300";
    default:
      return "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300";
  }
}
</script>

<template>
  <div class="flex h-full flex-col p-6">
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
        @click="showPublishDialog = true"
        class="flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
      >
        <Plus class="h-4 w-4" />
        {{ $t('servers.publish') }}
      </button>
    </div>

    <!-- Filters -->
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
      
      <!-- MC Version Filter -->
      <select
        v-model="filterMcVersion"
        class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
      >
        <option value="">{{ $t('servers.allVersions') }}</option>
        <option v-for="version in mcVersions" :key="version" :value="version">
          {{ version }}
        </option>
      </select>

      <!-- Loader Type Filter -->
      <select
        v-model="filterLoaderType"
        class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
      >
        <option value="">{{ $t('servers.allLoaders') }}</option>
        <option v-for="loader in loaderTypes" :key="loader" :value="loader">
          {{ loader }}
        </option>
      </select>

      <!-- Online Mode Filter -->
      <select
        v-model="filterOnlineMode"
        class="px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
      >
        <option value="">{{ $t('servers.allModes') }}</option>
        <option value="online">{{ $t('servers.onlineMode') }}</option>
        <option value="offline">{{ $t('servers.offlineMode') }}</option>
      </select>
    </div>

    <!-- Server Grid -->
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 overflow-y-auto">
      <div
        v-for="server in filteredServers"
        :key="server.id"
        class="group rounded-lg border bg-card p-4 hover:border-primary/50 transition-colors"
      >
        <!-- Server Header -->
        <div class="flex items-start gap-3 mb-3">
          <div class="flex h-12 w-12 items-center justify-center rounded-lg bg-muted">
            <Gamepad2 class="h-6 w-6 text-muted-foreground" />
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="font-semibold truncate">{{ server.name }}</h3>
            <p class="text-xs text-muted-foreground line-clamp-2">{{ server.description }}</p>
          </div>
        </div>

        <!-- Server Info -->
        <div class="flex flex-wrap gap-2 mb-3">
          <span class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold bg-zinc-100 dark:bg-zinc-800">
            {{ server.mcVersion }}
          </span>
          <span
            class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold"
            :class="loaderBadgeClass(server.loaderType)"
          >
            {{ server.loaderType }}
          </span>
          <span
            class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold"
            :class="server.onlineMode ? 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300' : 'bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300'"
          >
            {{ server.onlineMode ? $t('servers.onlineMode') : $t('servers.offlineMode') }}
          </span>
        </div>

        <!-- Player Count -->
        <div class="flex items-center gap-1 text-sm text-muted-foreground mb-3">
          <Users class="h-4 w-4" />
          <span>{{ $t('servers.players', { online: server.onlinePlayers, max: server.maxPlayers }) }}</span>
        </div>

        <!-- IP and Actions -->
        <div class="flex items-center justify-between pt-3 border-t">
          <div class="flex items-center gap-2">
            <code class="text-xs bg-muted px-2 py-1 rounded">{{ server.ip }}:{{ server.port }}</code>
            <button
              @click="copyIp(server)"
              class="p-1 hover:bg-muted rounded transition-colors"
              title="Copy IP"
            >
              <Check v-if="copiedServerId === server.id" class="h-4 w-4 text-green-500" />
              <Copy v-else class="h-4 w-4 text-muted-foreground" />
            </button>
          </div>
          <button class="flex items-center gap-1 text-sm text-primary hover:underline">
            <ExternalLink class="h-4 w-4" />
            {{ $t('servers.join') }}
          </button>
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
        <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto" @click="showPublishDialog = false"></div>
        <div class="relative z-10 w-full max-w-lg gap-4 border bg-white dark:bg-zinc-900 p-6 shadow-xl rounded-lg pointer-events-auto">
          <div class="flex items-center justify-between mb-4">
            <h3 class="font-semibold text-lg text-neutral-900 dark:text-white">{{ $t('servers.publishTitle') }}</h3>
            <button @click="showPublishDialog = false" class="text-muted-foreground hover:text-foreground text-lg">
              ✕
            </button>
          </div>

          <div class="space-y-4">
            <!-- Server Name -->
            <div class="space-y-1">
              <label class="text-sm font-medium">{{ $t('servers.serverName') }}</label>
              <input v-model="newServer.name" type="text" placeholder="My Awesome Server" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500" />
            </div>

            <!-- IP and Port -->
            <div class="flex gap-2">
              <div class="flex-1 space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.ipAddress') }}</label>
                <input v-model="newServer.ip" type="text" placeholder="play.myserver.net" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500" />
              </div>
              <div class="w-24 space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.port') }}</label>
                <input v-model="newServer.port" type="text" placeholder="25565" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500" />
              </div>
            </div>

            <!-- Version and Loader -->
            <div class="flex gap-2">
              <div class="flex-1 space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.mcVersion') }}</label>
                <select v-model="newServer.mcVersion" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white">
                  <option value="">{{ $t('servers.selectVersion') }}</option>
                  <option value="1.20.4">1.20.4</option>
                  <option value="1.20.1">1.20.1</option>
                  <option value="1.19.4">1.19.4</option>
                  <option value="1.18.2">1.18.2</option>
                </select>
              </div>
              <div class="flex-1 space-y-1">
                <label class="text-sm font-medium">{{ $t('servers.loaderType') }}</label>
                <select v-model="newServer.loaderType" class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white">
                  <option value="Vanilla">Vanilla</option>
                  <option value="Fabric">Fabric</option>
                  <option value="Forge">Forge</option>
                  <option value="Paper">Paper</option>
                </select>
              </div>
            </div>

            <!-- Online Mode -->
            <div class="flex items-center gap-2">
              <input v-model="newServer.onlineMode" type="checkbox" id="onlineMode" class="rounded" />
              <label for="onlineMode" class="text-sm font-medium">{{ $t('servers.requireOnline') }}</label>
            </div>

            <!-- Description -->
            <div class="space-y-1">
              <label class="text-sm font-medium">{{ $t('servers.description') }}</label>
              <textarea v-model="newServer.description" placeholder="Describe your server..." class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 h-20 resize-none"></textarea>
            </div>
          </div>

          <div class="flex justify-end gap-2 mt-6">
            <button @click="showPublishDialog = false" class="px-4 py-2 text-sm font-medium border rounded-md hover:bg-muted transition-colors">
              {{ $t('servers.cancel') }}
            </button>
            <button @click="submitServer" class="px-4 py-2 text-sm font-medium bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors">
              {{ $t('servers.submit') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>