<script setup lang="ts">
import { ref, onMounted, computed, onActivated, watch, onUnmounted } from "vue";
import { useRouter, useRoute } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { trackEvent, getErrorType } from "../utils/analytics";
import {
  Play,
  Loader2,
  Settings,
  X,
  Gamepad2,
  ChevronDown,
  Package,
  MonitorCheck,
  WifiOff,
  Square,
  Globe,
  Plus
} from "@lucide/vue";
import { DropdownMenu, DropdownMenuItem } from "../components/ui/dropdown-menu";
import CrashReportModal from "../components/CrashReportModal.vue";
import { useI18n } from "vue-i18n";
import { fetchApi } from "../utils/api";
// Types
interface InstanceItem {
  id: string;
  name: string;
  mcVersion: string;
  loaderType: string;
  modpackVersion?: string;
  modpackType?: string;
  modpackProjectId?: string;
  serverId?: string;
  packVersionId?: string;
  packFileName?: string;
  isInstalling?: boolean;
}

interface Account {
  id: string;
  username: string;
  accountType: string; // 'microsoft', 'offline', or 'authlib'
  authlibUrl?: string;
}

interface GameLog {
  type: string;
  line: string;
}

interface InstanceState {
  versionId: string;
  status: "running" | "exited" | "repairing" | "repairing_complete";
  exitCode?: number;
  missingCount?: number;
  isOpenJ9?: boolean;
}

// Router for navigation to settings
const router = useRouter();
const route = useRoute();

// State
const installedInstances = ref<InstanceItem[]>([]);
const { t } = useI18n();
const accounts = ref<Account[]>([]);
const selectedInstanceId = ref<string>("");
const selectedAccountId = ref<string>("");

import { launchingInstances, jvmSpawnedInstances, runningInstances, repairingInstances } from '../composables/useLaunchState';

// Running state
const gameLogs = ref<string[]>([]);
const showGameLog = ref(false);

// Kill tracking state
const intentionallyKilledInstances = ref<Set<string>>(new Set());

// Crash alert state
const showCrashAlert = ref(false);
const crashExitCode = ref(0);
const crashVersionId = ref("");
const crashIsOpenJ9 = ref(false);

// Repair progress state
interface DownloadProgress {
  taskId: string;
  downloaded: number;
  total: number;
  speed: number;
  completed: boolean;
  error?: string;
}
const isRepairing = ref(false);
const repairTotalFiles = ref(0);
const repairCompletedFiles = ref(0);
const repairDownloadSpeed = ref(0);
const repairActiveTasks = ref(new Map<string, DownloadProgress>());
const repairingDependencyName = ref("");
const repairPhase = ref("");

const repairProgressPercentage = computed(() => {
  if (repairTotalFiles.value === 0) return 0;
  return (repairCompletedFiles.value / repairTotalFiles.value) * 100;
});

// ---------------------------------------------------------------------------
const announcements = ref<string[]>([]);

// Computed
// ---------------------------------------------------------------------------
const selectedInstance = computed(() => {
  return (
    installedInstances.value.find((i) => i.id === selectedInstanceId.value) ||
    null
  );
});

const selectedAccount = computed(() => {
  return accounts.value.find((a) => a.id === selectedAccountId.value) || null;
});

const isActionDisabled = computed(() => {
  if (!selectedInstanceId.value || !selectedAccountId.value) return true;
  
  if (selectedInstance.value?.isInstalling) return true;
  
  if (launchingInstances.value.has(selectedInstanceId.value) || 
      repairingInstances.value.has(selectedInstanceId.value)) {
    return true;
  }
  
  return false;
});

const isLaunching = computed(() => {
  const instanceId = selectedInstanceId.value;
  return (
    launchingInstances.value.has(instanceId) ||
    repairingInstances.value.has(instanceId)
  );
});

const isRunning = computed(() => {
  return runningInstances.value.has(selectedInstanceId.value);
});

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
const handleTaskAdded = () => {
  loadInstances();
};

const handleTaskStatusChanged = (e: Event) => {
  const customEvent = e as CustomEvent;
  const status = customEvent.detail?.status;
  if (status === 'Completed' || status === 'Failed' || status === 'Cancelled') {
    loadInstances();
  }
};

onMounted(async () => {
  window.addEventListener('task-added', handleTaskAdded);
  window.addEventListener('task-status-changed', handleTaskStatusChanged);

  await loadInstances();
  await loadAccounts();

  // Fetch announcement
  try {
    const backendUrl = import.meta.env.VITE_WEB_BACKEND_URL || 'http://localhost:3030';
    const res = await fetchApi(`${backendUrl}/api/launcher/announcement`);
    if (res.ok) {
      const data = await res.json();
      if (data.announcements && data.announcements.length > 0) {
        announcements.value = data.announcements;
      }
    }
  } catch (err) {
    console.error("Failed to fetch announcement:", err);
  }

  // Listen for accounts changes from other pages (e.g., AccountsView)
  listen("accounts-updated", async () => {
    await loadAccounts();
  });

  // Listen for game logs
  listen<GameLog>("game-log", (event) => {
    gameLogs.value.push(event.payload.line);
    if (gameLogs.value.length > 2000) {
      gameLogs.value = gameLogs.value.slice(-2000);
    }
    
    // Heuristic: detect when Minecraft window is likely appearing
    const line = event.payload.line || "";
    if (line.includes("Backend library:") || line.includes("LWJGL") || line.includes("Setting user") || line.includes("OpenAL initialized")) {
      // Transition all spawned JVMs to fully running state
      for (const id of launchingInstances.value) {
        if (jvmSpawnedInstances.value.has(id)) {
          runningInstances.value.add(id);
          launchingInstances.value.delete(id);
        }
      }
    }
  });

  // Listen for instance state changes
  listen<InstanceState>("instance-state-changed", (event) => {
    const { versionId, status, exitCode, missingCount, isOpenJ9 } = event.payload;

    if (status === "running") {
      jvmSpawnedInstances.value.add(versionId);
      
      // Fallback: If logs don't match, auto-transition after 8 seconds
      setTimeout(() => {
        if (launchingInstances.value.has(versionId)) {
          runningInstances.value.add(versionId);
          launchingInstances.value.delete(versionId);
        }
      }, 8000);

      repairingInstances.value.delete(versionId);
      isRepairing.value = false;
      repairingDependencyName.value = "";
      repairPhase.value = "";
    } else if (status === "exited") {
      jvmSpawnedInstances.value.delete(versionId);
      runningInstances.value.delete(versionId);
      launchingInstances.value.delete(versionId);
      repairingInstances.value.delete(versionId);
      isRepairing.value = false;
      repairingDependencyName.value = "";
      repairPhase.value = "";

      // Check if this was an intentional kill
      const wasIntentionallyKilled = intentionallyKilledInstances.value.has(versionId);
      intentionallyKilledInstances.value.delete(versionId);

      // Show crash alert if exit code is non-zero and it wasn't intentionally killed
      if (exitCode !== 0 && !wasIntentionallyKilled) {
        crashVersionId.value = versionId;
        crashExitCode.value = exitCode ?? -1;
        crashIsOpenJ9.value = !!isOpenJ9;
        showCrashAlert.value = true;
      }
    } else if (status === "repairing") {
      repairingInstances.value.add(versionId);
      isRepairing.value = true;
      repairTotalFiles.value = missingCount || 0;
      repairCompletedFiles.value = 0;
      repairDownloadSpeed.value = 0;
      repairActiveTasks.value.clear();
    } else if (status === "repairing_complete") {
      repairingInstances.value.delete(versionId);
      isRepairing.value = false;
    }
  });

  // Listen for launch-status (repair events)
  listen("launch-status", (event: any) => {
    const payload = event.payload;
    if (payload.status === "repairing_mods") {
      isRepairing.value = true;
      repairTotalFiles.value = payload.count || 0;
      repairCompletedFiles.value = 0;
      repairDownloadSpeed.value = 0;
      repairActiveTasks.value.clear();
      repairingDependencyName.value = "";
      repairPhase.value = "";
    } else if (payload.status === "repairing_dependency") {
      isRepairing.value = true;
      repairTotalFiles.value = 0;
      repairCompletedFiles.value = 0;
      repairDownloadSpeed.value = 0;
      repairActiveTasks.value.clear();
      repairingDependencyName.value = payload.version || "";
      repairPhase.value = "";
    } else if (payload.status === "repairing_complete") {
      isRepairing.value = false;
      repairingDependencyName.value = "";
    }
  });

  // Listen to install-progress during dependency repair
  listen("install-progress", (event: any) => {
    if (!isRepairing.value || !repairingDependencyName.value) return;
    const payload = event.payload;
    if (payload.phase) {
      repairPhase.value = payload.phase;
    }
    if (payload.phase === "downloading" && payload.totalTasks) {
       repairTotalFiles.value = payload.totalTasks;
    }
  });

  // Listen for download-progress during repair
  listen<DownloadProgress>("download-progress", (event) => {
    if (!isRepairing.value) return;
    
    const progress = event.payload;
    repairActiveTasks.value.set(progress.taskId, progress);

    if (progress.completed) {
      repairCompletedFiles.value++;
    }

    // Calculate total speed
    let currentSpeed = 0;
    for (const task of repairActiveTasks.value.values()) {
      if (!task.completed) {
        currentSpeed += task.speed;
      }
    }
    repairDownloadSpeed.value = currentSpeed;
  });
});

onUnmounted(() => {
  window.removeEventListener('task-added', handleTaskAdded);
  window.removeEventListener('task-status-changed', handleTaskStatusChanged);
});

onActivated(() => {
  // Refresh instances when returning to HomeView
  loadInstances();
  loadAccounts();
});

watch(() => route.query.auto_launch, async (isAutoLaunch) => {
  if (isAutoLaunch === 'true') {
    // Force refresh instances and accounts to ensure we have the latest data before matching
    await loadInstances();
    await loadAccounts();
    
    let matchingInstance = null;
    const serverName = route.query.server_name as string;
    
    // First try matching by serverId, then fallback to instance name (for modpacks)
    if (serverName) {
      const serverId = route.query.server_id as string;
      matchingInstance = installedInstances.value.find(i => {
        if (serverId && i.serverId === serverId) return true;
        return i.name === serverName;
      });
    }
    
    // Fallback: match by version and loader if not found
    if (!matchingInstance) {
      const serverVersion = route.query.server_version as string;
      const serverLoader = route.query.server_loader as string;
      
      matchingInstance = installedInstances.value.find(i => 
        i.mcVersion === serverVersion && 
        (!serverLoader || serverLoader === 'vanilla' || (i.loaderType && i.loaderType.toLowerCase().includes(serverLoader.toLowerCase())))
      );
    }
    
    if (matchingInstance) {
      selectedInstanceId.value = matchingInstance.id;
    } else {
      alert(t('home.noInstanceFound', { name: serverName || 'Unknown' }));
      router.replace({ query: {} });
      return;
    }
    
    // Select matching account
    let matchingAccount = null;
    const authType = route.query.auth_type as string;
    
    if (authType === 'online' || authType === 'microsoft') {
      matchingAccount = accounts.value.find(a => a.accountType === 'microsoft');
    } else if (authType === 'authlib') {
      const authlibApi = route.query.authlib_api as string;
      matchingAccount = accounts.value.find(a => a.accountType === 'authlib' && a.authlibUrl === authlibApi);
    } else {
      matchingAccount = accounts.value.find(a => a.accountType === 'offline');
    }
    
    if (!matchingAccount && accounts.value.length > 0) {
      matchingAccount = accounts.value[0];
    }
    
    if (matchingAccount) {
      selectedAccountId.value = matchingAccount.id;
    } else {
      alert(t('home.noAccountFound'));
      router.replace({ query: {} });
      return;
    }
    
    // Launch
    launchingInstances.value.add(selectedInstanceId.value);
    gameLogs.value = [];
    
    try {
      trackEvent("game_launch_started", { instanceId: selectedInstanceId.value, auto: true });
      await invoke("launch_instance", {
        versionId: selectedInstanceId.value,
        accountUuid: selectedAccountId.value,
        serverIp: (route.query.server_ip as string) || undefined,
        serverPort: parseInt(route.query.server_port as string) || undefined
      });
      trackEvent("game_launched", { instanceId: selectedInstanceId.value, auto: true });
    } catch (e) {
      console.error("Failed to launch auto instance:", e);
      trackEvent("error_occurred", { context: "auto_launch", error_type: getErrorType(e) });
      launchingInstances.value.delete(selectedInstanceId.value);
      repairingInstances.value.delete(selectedInstanceId.value);
      isRepairing.value = false;
      await handleLaunchError(e);
    }
    
    // Clean up query
    router.replace({ query: {} });
  }
}, { immediate: true });

// ---------------------------------------------------------------------------
// Data loading
// ---------------------------------------------------------------------------
async function loadInstances() {
  try {
    const instances = await invoke<InstanceItem[]>("scan_installed_instances");
    installedInstances.value = instances;
    // Auto-select first non-installing instance if none selected
    if (!selectedInstanceId.value && instances.length > 0) {
      const validInstance = instances.find(i => !i.isInstalling);
      if (validInstance) {
        selectedInstanceId.value = validInstance.id;
      }
    }
  } catch (e) {
    console.error("Failed to load instances:", e);
  }
}

async function loadAccounts() {
  try {
    accounts.value = await invoke<Account[]>("get_accounts");
    if (accounts.value.length > 0 && !selectedAccountId.value) {
      selectedAccountId.value = accounts.value[0].id;
    }
  } catch (e) {
    console.error("Failed to load accounts:", e);
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------
async function handlePrimaryAction() {
  if (isRunning.value) {
    const confirmed = await confirm(
      t('home.stopGameConfirm'),
      { title: t('home.stopGameConfirmTitle'), kind: "warning" }
    );
    if (confirmed) {
      try {
        await invoke("kill_instance", { versionId: selectedInstanceId.value });
        intentionallyKilledInstances.value.add(selectedInstanceId.value);
      } catch (e) {
        console.error("Failed to kill instance:", e);
        alert(t('home.stopGameFailed', { error: e }));
      }
    }
    return;
  }

  if (!selectedInstanceId.value || !selectedAccountId.value || isActionDisabled.value) {
    return;
  }

  // Add to launching set immediately for UI feedback
  launchingInstances.value.add(selectedInstanceId.value);
  gameLogs.value = [];

  try {
    const config = await invoke<any>("get_instance_config", { versionId: selectedInstanceId.value });
    if (config && config.showGameLog) {
      showGameLog.value = true;
    }
  } catch (e) {
    console.warn("Failed to get instance config for log display preference:", e);
  }

  try {
    trackEvent("game_launch_started", { instanceId: selectedInstanceId.value, auto: false });
    await invoke("launch_instance", {
      versionId: selectedInstanceId.value,
      accountUuid: selectedAccountId.value,
    });
    trackEvent("game_launched", { instanceId: selectedInstanceId.value, auto: false });
  } catch (e) {
    console.error("Failed to launch instance:", e);
    trackEvent("error_occurred", { context: "manual_launch", error_type: getErrorType(e) });
    launchingInstances.value.delete(selectedInstanceId.value);
    repairingInstances.value.delete(selectedInstanceId.value);
    isRepairing.value = false;
    await handleLaunchError(e);
  }
}

async function handleLaunchError(e: any) {
  const errorObj = e as any;
  if (errorObj && errorObj.type === "NoCompatibleJava") {
    alert(t('home.noCompatibleJava', { version: errorObj.data.required_version }));
  } else {
    const errorStr = typeof e === 'string' ? e : (errorObj.data || String(e));
    if (errorStr.includes("login session has expired") || errorStr.includes("REAUTH_REQUIRED")) {
      const confirmed = await confirm(
        t('home.sessionExpiredConfirm'),
        { title: t('home.sessionExpiredConfirmTitle'), kind: "warning" }
      );
      if (confirmed) {
        try {
          const newAccount = await invoke<Account>("login_microsoft_oauth");
          await loadAccounts();
          selectedAccountId.value = newAccount.id;
          handlePrimaryAction();
        } catch (loginErr) {
          console.error("Re-login failed:", loginErr);
        }
      }
    } else {
      alert(t('home.launchFailed', { error: errorStr }));
    }
  }
}

function openInstanceSettings() {
  if (!selectedInstanceId.value) return;
  router.push({
    path: "/instances",
    query: { manage: selectedInstanceId.value },
  });
}

// ---------------------------------------------------------------------------
// UI helpers
// ---------------------------------------------------------------------------
function formatLoaderType(loaderType: string): string {
  switch (loaderType.toLowerCase()) {
    case "fabric":
      return "Fabric";
    case "forge":
      return "Forge";
    case "neoforge":
      return "NeoForge";
    default:
      return "Vanilla";
  }
}

function loaderBadgeClass(loaderType: string): string {
  switch (loaderType.toLowerCase()) {
    case "fabric":
      return "bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300";
    case "forge":
      return "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300";
    case "neoforge":
      return "bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300";
    default:
      return "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300";
  }
}
</script>

<template>
  <div class="relative flex h-full flex-col overflow-hidden">

    <!-- Marquee Notice Bar -->
    <div class="relative z-10 w-full overflow-hidden bg-gradient-to-r from-blue-600/80 via-purple-600/80 to-pink-600/80 py-2 backdrop-blur-sm border-b border-white/10 shadow-sm">
      <div class="whitespace-nowrap" :class="{'animate-marquee': announcements.length > 1}">
        <span class="inline-block px-8 text-white font-medium">
          <template v-if="announcements.length === 0">
            {{ $t('home.marquee') }}
          </template>
          <template v-else>
            <span v-for="(announcement, index) in announcements" :key="index">
              {{ announcement }}
              <span v-if="index < announcements.length - 1"> · </span>
            </span>
            <!-- Duplicate for seamless looping only if there are multiple announcements -->
            <span v-if="announcements.length > 1">
              <span v-for="(announcement, index) in announcements" :key="'dup-'+index">
                · {{ announcement }}
              </span>
            </span>
          </template>
        </span>
      </div>
    </div>

    <!-- Main Content Area -->
    <div class="relative z-10 flex-1 flex flex-col p-4 md:p-8 overflow-hidden">
      <!-- Main Dashboard -->
      <div class="flex-1 flex flex-col items-center justify-center max-w-7xl mx-auto w-full">
        
        <div class="w-full max-w-lg flex flex-col items-center justify-center shrink-0">
          <!-- Header Content -->
          <div class="text-center space-y-2 mb-8">
            <h1 class="text-6xl font-extrabold tracking-tight text-white drop-shadow-2xl">Dawnland</h1>
            <p class="text-xl text-white/90 drop-shadow-md">{{ $t('home.subtitle') }}</p>
          </div>

          <!-- Empty State (No Instances) -->
          <div v-if="installedInstances.length === 0" class="w-full flex flex-col items-center justify-center gap-4 p-8 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-xl rounded-3xl border border-white/20 shadow-2xl">
            <div class="flex h-20 w-20 items-center justify-center rounded-2xl bg-primary/10">
              <Gamepad2 class="h-10 w-10 text-primary" />
            </div>
            <div class="text-center space-y-2">
              <h2 class="text-2xl font-bold">{{ $t('home.welcome') }}</h2>
              <p class="text-muted-foreground">{{ $t('home.noInstances') }}</p>
            </div>
            <router-link to="/instances" class="mt-4 flex items-center gap-2 rounded-xl bg-primary px-8 py-3 text-lg font-bold text-primary-foreground hover:bg-primary/90 transition-all hover:scale-105 active:scale-95 shadow-lg">
              <Play class="h-5 w-5 fill-current" />
              {{ $t('home.installInstance') }}
            </router-link>
          </div>

          <!-- Control Panel -->
          <div v-else class="w-full bg-white/60 dark:bg-zinc-900/60 backdrop-blur-xl border border-white/20 dark:border-zinc-800/50 rounded-3xl p-6 shadow-2xl">
            <!-- Instance Selector -->
            <div class="flex items-center gap-3">
              <label class="text-sm font-medium shrink-0 w-16">{{ $t('home.selectInstance') }}</label>
              <DropdownMenu class="flex-1 min-w-0">
                <template #trigger>
                  <button class="w-full flex items-center justify-between px-3 py-2.5 bg-white/40 dark:bg-zinc-800/40 border border-white/20 rounded-xl hover:border-primary/50 transition-colors">
                    <div v-if="selectedInstance" class="flex items-center gap-2 overflow-hidden">
                      <Package class="h-5 w-5 text-primary shrink-0" />
                      <span class="font-medium truncate">{{ selectedInstance.name }}</span>
                    </div>
                    <span v-else class="text-muted-foreground">{{ $t('home.selectInstancePlaceholder') }}</span>
                    <ChevronDown class="h-5 w-5 text-muted-foreground shrink-0 ml-2" />
                  </button>
                </template>
                <div class="max-h-60 overflow-y-auto">
                  <DropdownMenuItem v-for="instance in installedInstances" :key="instance.id" @click="!instance.isInstalling && (selectedInstanceId = instance.id)" class="flex items-center gap-3 p-2 rounded-lg" :class="instance.isInstalling ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'" :disabled="instance.isInstalling">
                    <Package class="h-4 w-4 shrink-0 text-muted-foreground" />
                    <span class="truncate font-medium flex-1 text-left">{{ instance.name }}</span>
                    <Loader2 v-if="instance.isInstalling" class="h-3 w-3 animate-spin text-muted-foreground shrink-0" />
                  </DropdownMenuItem>

                  <div v-if="installedInstances.length > 0" class="h-px bg-border my-1 mx-2"></div>
                  
                  <router-link to="/instances" class="flex items-center gap-2 w-full p-2 rounded-lg cursor-pointer hover:bg-muted text-primary transition-colors">
                    <div class="flex items-center justify-center w-4 h-4 rounded-full bg-primary/10 shrink-0">
                      <Plus class="h-3 w-3 text-primary" />
                    </div>
                    <span class="font-medium text-sm">{{ $t('home.installInstance') }}</span>
                  </router-link>
                </div>
              </DropdownMenu>
            </div>

            <!-- Account Selector -->
            <div class="flex items-center gap-3 mt-4">
              <label class="text-sm font-medium shrink-0 w-16">{{ $t('home.selectAccount') }}</label>
              <DropdownMenu class="flex-1 min-w-0">
                <template #trigger>
                  <button class="w-full flex items-center justify-between px-3 py-2.5 bg-white/40 dark:bg-zinc-800/40 border border-white/20 rounded-xl hover:border-primary/50 transition-colors overflow-hidden">
                    <div v-if="selectedAccount" class="flex items-center gap-2 overflow-hidden">
                      <MonitorCheck v-if="selectedAccount.accountType === 'microsoft'" class="h-5 w-5 text-green-500 shrink-0" />
                      <Globe v-else-if="selectedAccount.accountType === 'authlib'" class="h-5 w-5 text-purple-500 shrink-0" />
                      <WifiOff v-else class="h-5 w-5 text-muted-foreground shrink-0" />
                      <span class="font-medium truncate flex-1 min-w-0 flex items-center gap-1.5">
                        <span class="truncate">{{ selectedAccount.username }}</span>
                        <span class="text-xs text-muted-foreground font-normal shrink-0">
                          ({{ selectedAccount.accountType === 'microsoft' ? $t('accounts.microsoft') : (selectedAccount.accountType === 'authlib' ? $t('accounts.authlib') : $t('accounts.offline')) }})
                        </span>
                      </span>
                    </div>
                    <span v-else class="text-muted-foreground">{{ $t('home.selectAccountPlaceholder') }}</span>
                    <ChevronDown class="h-5 w-5 text-muted-foreground shrink-0 ml-2" />
                  </button>
                </template>
                <div class="max-h-60 overflow-y-auto">
                  <DropdownMenuItem v-for="account in accounts" :key="account.id" @click="selectedAccountId = account.id" class="flex items-center justify-between w-full p-2 rounded-lg cursor-pointer">
                    <div class="flex items-center gap-3 overflow-hidden">
                      <MonitorCheck v-if="account.accountType === 'microsoft'" class="h-4 w-4 text-green-500 shrink-0" />
                      <Globe v-else-if="account.accountType === 'authlib'" class="h-4 w-4 text-purple-500 shrink-0" />
                      <WifiOff v-else class="h-4 w-4 text-muted-foreground shrink-0" />
                      <span class="truncate font-medium">{{ account.username }}</span>
                    </div>
                    <span class="text-xs text-muted-foreground shrink-0 ml-3 bg-muted px-2 py-0.5 rounded-full">
                      {{ account.accountType === 'microsoft' ? $t('accounts.microsoft') : (account.accountType === 'authlib' ? $t('accounts.authlib') : $t('accounts.offline')) }}
                    </span>
                  </DropdownMenuItem>

                  <div v-if="accounts.length > 0" class="h-px bg-border my-1 mx-2"></div>
                  
                  <router-link to="/accounts" class="flex items-center gap-2 w-full p-2 rounded-lg cursor-pointer hover:bg-muted text-primary transition-colors">
                    <div class="flex items-center justify-center w-4 h-4 rounded-full bg-primary/10 shrink-0">
                      <Plus class="h-3 w-3 text-primary" />
                    </div>
                    <span class="font-medium text-sm">{{ $t('home.addAccount') }}</span>
                  </router-link>
                </div>
              </DropdownMenu>
            </div>

            <!-- Action Buttons -->
            <div class="flex items-center justify-center gap-3 mt-6">
              <button @click="openInstanceSettings" :disabled="!selectedInstanceId || selectedInstance?.isInstalling" class="flex items-center gap-2 px-4 py-3 border border-white/20 dark:border-zinc-700 bg-white/40 dark:bg-zinc-800/40 rounded-xl hover:bg-white/60 dark:hover:bg-zinc-700/60 disabled:opacity-50 disabled:cursor-not-allowed transition-colors font-medium shadow-sm shrink-0" title="Configure instance">
                <Settings class="h-5 w-5" />
              </button>
              <button @click="handlePrimaryAction" :disabled="isActionDisabled" 
                :class="isRunning ? 'bg-zinc-700 hover:bg-zinc-800 text-white dark:bg-zinc-800 dark:hover:bg-zinc-700' : 'bg-green-600 hover:bg-green-500 text-white'"
                class="flex-1 flex items-center justify-center gap-3 rounded-xl py-3 text-xl font-bold shadow-lg disabled:opacity-50 disabled:cursor-not-allowed transition-all hover:scale-[1.02] active:scale-95">
                <Loader2 v-if="isLaunching" class="h-6 w-6 animate-spin" />
                <Square v-else-if="isRunning" class="h-6 w-6 fill-current" />
                <Play v-else class="h-6 w-6 fill-current" />
                {{ isLaunching ? $t('home.launching') : (isRunning ? $t('home.stopGame', '停止运行') : $t('home.play')) }}
              </button>
            </div>

            <!-- Instance Info Badge -->
            <div v-if="selectedInstance" class="flex flex-wrap items-center justify-center gap-2 mt-5">
              <span class="inline-flex items-center rounded-full px-2.5 py-1 text-xs font-semibold bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300 border shadow-sm">
                MC {{ selectedInstance.mcVersion }}
              </span>
              <span class="inline-flex items-center rounded-full px-2.5 py-1 text-xs font-semibold shadow-sm border" :class="loaderBadgeClass(selectedInstance.loaderType)">
                {{ formatLoaderType(selectedInstance.loaderType) }}
              </span>
              <span v-if="selectedInstance.modpackType" class="inline-flex items-center rounded-full px-2.5 py-1 text-xs font-semibold bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300 shadow-sm border border-purple-200 dark:border-purple-800">
                {{ selectedInstance.modpackType }}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Game Log Modal -->
    <Teleport to="body">
      <div v-if="showGameLog" class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
        <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"></div>
        <div class="relative z-10 w-full max-w-3xl h-[70vh] gap-4 border bg-white dark:bg-zinc-900 p-4 shadow-xl rounded-lg flex flex-col pointer-events-auto">
          <div class="flex items-center justify-between">
            <h3 class="font-semibold">{{ $t('home.gameOutput') }}</h3>
            <button @click="showGameLog = false" class="text-muted-foreground hover:text-foreground">
              <X class="h-5 w-5" />
            </button>
          </div>
          <div class="flex-1 overflow-auto font-mono text-xs bg-black text-green-400 p-3 rounded custom-scrollbar">
            <div v-for="(line, idx) in gameLogs" :key="idx">{{ line }}</div>
            <div v-if="gameLogs.length === 0" class="text-gray-500">{{ $t('home.waitingOutput') }}</div>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Crash Report Modal -->
    <CrashReportModal :open="showCrashAlert" :exit-code="crashExitCode" :version-id="crashVersionId" :logs="gameLogs" :is-open-j9="crashIsOpenJ9" @update:open="showCrashAlert = $event" />

    <!-- Repair Progress Modal -->
    <Teleport to="body">
      <Transition name="dialog">
        <div v-if="isRepairing" class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
          <!-- Backdrop -->
          <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto" />
          
          <!-- Content -->
          <div class="relative z-10 w-full max-w-md bg-white dark:bg-zinc-900 border rounded-2xl p-6 shadow-2xl space-y-4 pointer-events-auto">
            <div class="flex items-center gap-3">
              <Loader2 class="h-6 w-6 animate-spin text-primary" />
              <h3 class="text-xl font-bold" v-if="!repairingDependencyName">{{ $t('home.repairing') }}</h3>
              <h3 class="text-xl font-bold" v-else>{{ $t('home.repairingDepTitle') }}</h3>
            </div>
            
            <p class="text-sm text-muted-foreground" v-if="!repairingDependencyName">
              {{ $t('home.repairFound', { total: repairTotalFiles }) }}
            </p>
            <p class="text-sm text-muted-foreground" v-else>
              {{ $t('home.repairingDependency', { version: repairingDependencyName }) }}
            </p>

            <!-- Progress Bar -->
            <div class="space-y-2" v-if="repairPhase !== 'running_processors'">
              <div class="flex justify-between text-xs font-medium">
                <span>{{ repairCompletedFiles }} / {{ repairTotalFiles }} files</span>
                <span>{{ (repairDownloadSpeed / 1024 / 1024).toFixed(2) }} MB/s</span>
              </div>
              <div class="h-2 w-full bg-secondary rounded-full overflow-hidden">
                <div class="h-full bg-primary transition-all duration-300 ease-out" :style="{ width: `${repairProgressPercentage}%` }" />
              </div>
            </div>
            <div class="space-y-2" v-else>
              <div class="flex justify-center text-sm font-medium text-amber-500">
                {{ $t('install.status.runningProcessors') }}
              </div>
              <div class="h-2 w-full bg-secondary rounded-full overflow-hidden animate-pulse">
                <div class="h-full bg-amber-500 w-full" />
              </div>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.dialog-enter-active,
.dialog-leave-active {
  transition: opacity 150ms ease;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}

.dialog-enter-active .relative,
.dialog-leave-active .relative {
  transition: transform 150ms ease, opacity 150ms ease;
}

.dialog-enter-from .relative {
  transform: scale(0.95);
  opacity: 0;
}

.dialog-leave-to .relative {
  transform: scale(0.95);
  opacity: 0;
}
</style>