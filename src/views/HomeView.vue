<script setup lang="ts">
import { ref, onMounted, computed, onActivated } from "vue";
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
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
} from "@lucide/vue";
import { DropdownMenu, DropdownMenuItem } from "../components/ui/dropdown-menu";
import CrashReportModal from "../components/CrashReportModal.vue";

// Types
interface InstanceItem {
  id: string;
  name: string;
  mcVersion: string;
  loaderType: string;
}

interface Account {
  id: string;
  username: string;
  accountType: string; // 'microsoft' or 'offline'
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
}

// Router for navigation to settings
const router = useRouter();

// State
const installedInstances = ref<InstanceItem[]>([]);
const accounts = ref<Account[]>([]);
const selectedInstanceId = ref<string>("");
const selectedAccountId = ref<string>("");

// Running state
const launchingInstances = ref<Set<string>>(new Set());
const jvmSpawnedInstances = ref<Set<string>>(new Set());
const runningInstances = ref<Set<string>>(new Set());
const repairingInstances = ref<Set<string>>(new Set());
const gameLogs = ref<string[]>([]);
const showGameLog = ref(false);

// Kill tracking state
const intentionallyKilledInstances = ref<Set<string>>(new Set());

// Crash alert state
const showCrashAlert = ref(false);
const crashExitCode = ref(0);
const crashVersionId = ref("");

// ---------------------------------------------------------------------------
// Computed
// ---------------------------------------------------------------------------
const selectedInstance = computed(() => {
  return (
    installedInstances.value.find((i) => i.id === selectedInstanceId.value) ||
    null
  );
});

const isActionDisabled = computed(() => {
  if (!selectedInstanceId.value || !selectedAccountId.value) return true;
  
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
onMounted(async () => {
  await loadInstances();
  await loadAccounts();

  // Listen for accounts changes from other pages (e.g., AccountsView)
  listen("accounts-updated", async () => {
    await loadAccounts();
  });

  // Listen for game logs
  listen<GameLog>("game-log", (event) => {
    gameLogs.value.push(event.payload.line);
    if (gameLogs.value.length > 500) {
      gameLogs.value = gameLogs.value.slice(-500);
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
    const { versionId, status, exitCode } = event.payload;

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
    } else if (status === "exited") {
      jvmSpawnedInstances.value.delete(versionId);
      runningInstances.value.delete(versionId);
      launchingInstances.value.delete(versionId);
      repairingInstances.value.delete(versionId);

      // Check if this was an intentional kill
      const wasIntentionallyKilled = intentionallyKilledInstances.value.has(versionId);
      intentionallyKilledInstances.value.delete(versionId);

      // Show crash alert if exit code is non-zero and it wasn't intentionally killed
      if (exitCode !== 0 && !wasIntentionallyKilled) {
        crashVersionId.value = versionId;
        crashExitCode.value = exitCode ?? -1;
        showCrashAlert.value = true;
      }
    } else if (status === "repairing") {
      repairingInstances.value.add(versionId);
    } else if (status === "repairing_complete") {
      repairingInstances.value.delete(versionId);
    }
  });
});

onActivated(() => {
  // Refresh instances when returning to HomeView
  loadInstances();
});

// ---------------------------------------------------------------------------
// Data loading
// ---------------------------------------------------------------------------
async function loadInstances() {
  try {
    const instances = await invoke<InstanceItem[]>("scan_installed_instances");
    installedInstances.value = instances;
    // Auto-select first instance if none selected
    if (!selectedInstanceId.value && instances.length > 0) {
      selectedInstanceId.value = instances[0].id;
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
      "你确定要强制结束正在运行的游戏进程吗？",
      { title: "强制停止游戏", kind: "warning" }
    );
    if (confirmed) {
      try {
        await invoke("kill_instance", { versionId: selectedInstanceId.value });
        intentionallyKilledInstances.value.add(selectedInstanceId.value);
      } catch (e) {
        console.error("Failed to kill instance:", e);
        alert(`停止游戏失败: ${e}`);
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
    await invoke("launch_instance", {
      versionId: selectedInstanceId.value,
      accountUuid: selectedAccountId.value,
    });
  } catch (e) {
    console.error("Failed to launch instance:", e);
    launchingInstances.value.delete(selectedInstanceId.value);
    alert(`Failed to launch: ${e}`);
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

// Account type helper: returns true for Microsoft account (premium)
function isMsaAccount(account: Account): boolean {
  return account.accountType === "microsoft";
}
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- Marquee Notice Bar -->
    <div class="w-full overflow-hidden bg-gradient-to-r from-blue-600 via-purple-600 to-pink-600 py-2">
      <div class="whitespace-nowrap animate-marquee">
        <span class="inline-block px-8 text-white font-medium">
          {{ $t('home.marquee') }} · 
          {{ $t('home.marquee') }} · 
          {{ $t('home.marquee') }} · 
          {{ $t('home.marquee') }} · 
          {{ $t('home.marquee') }}
        </span>
      </div>
    </div>

    <!-- Main Content Area -->
    <div class="flex-1 flex flex-col">
      <!-- Empty State: No instances -->
      <div v-if="installedInstances.length === 0" class="flex flex-1 flex-col items-center justify-center gap-4">
        <div class="flex h-24 w-24 items-center justify-center rounded-3xl bg-muted">
          <Gamepad2 class="h-12 w-12 text-muted-foreground" />
        </div>
        <div class="text-center space-y-2">
          <h2 class="text-2xl font-bold">{{ $t('home.welcome') }}</h2>
          <p class="text-muted-foreground">{{ $t('home.noInstances') }}</p>
        </div>
        <router-link to="/instances" class="flex items-center gap-2 rounded-md bg-primary px-6 py-2 text-base font-semibold text-primary-foreground hover:bg-primary/90 transition-colors">
          <Play class="h-5 w-5" />
          {{ $t('home.installInstance') }}
        </router-link>
      </div>

      <!-- Main Dashboard -->
      <div v-else class="flex flex-1 flex-col items-center justify-center p-4">
        <!-- Header -->
        <div class="text-center space-y-2 mb-8">
          <h1 class="text-5xl font-extrabold tracking-tight">Dawnland</h1>
          <p class="text-xl text-muted-foreground">{{ $t('home.subtitle') }}</p>
        </div>

        <!-- Control Panel -->
        <div class="w-full max-w-lg bg-card border rounded-2xl p-4 shadow-sm">
          <!-- Instance Selector -->
          <div class="flex items-center gap-3">
            <label class="text-sm font-medium shrink-0">{{ $t('home.selectInstance') }}</label>
            <DropdownMenu class="flex-1">
              <template #trigger>
                <button class="w-full flex items-center justify-between px-3 py-2 bg-background border rounded-lg hover:border-primary/50 transition-colors">
                  <div v-if="selectedInstance" class="flex items-center gap-2">
                    <Package class="h-5 w-5 text-primary" />
                    <span class="font-medium truncate">{{ selectedInstance.name }}</span>
                  </div>
                  <span v-else class="text-muted-foreground">{{ $t('home.selectInstancePlaceholder') }}</span>
                  <ChevronDown class="h-5 w-5 text-muted-foreground shrink-0" />
                </button>
              </template>
              <div class="max-h-60 overflow-y-auto bg-background">
                <DropdownMenuItem v-for="instance in installedInstances" :key="instance.id" @click="selectedInstanceId = instance.id" class="flex items-center gap-3">
                  <Package class="h-4 w-4" />
                  <span class="truncate">{{ instance.name }}</span>
                </DropdownMenuItem>
              </div>
            </DropdownMenu>
          </div>

          <!-- Account Selector -->
          <div class="flex items-center gap-3 mt-4">
            <label class="text-sm font-medium shrink-0">{{ $t('home.selectAccount') }}</label>
            <DropdownMenu class="flex-1">
              <template #trigger>
                <button class="w-full flex items-center justify-between px-3 py-2 bg-background border rounded-lg hover:border-primary/50 transition-colors">
                  <div v-if="selectedAccountId" class="flex items-center gap-2">
                    <component :is="isMsaAccount(accounts.find((a) => a.id === selectedAccountId)!) ? MonitorCheck : WifiOff" :class="isMsaAccount(accounts.find((a) => a.id === selectedAccountId)!) ? 'h-5 w-5 text-green-500' : 'h-5 w-5 text-muted-foreground'" />
                    <span class="font-medium truncate">{{ accounts.find((a) => a.id === selectedAccountId)?.username }}</span>
                  </div>
                  <span v-else class="text-muted-foreground">{{ $t('home.selectAccountPlaceholder') }}</span>
                  <ChevronDown class="h-5 w-5 text-muted-foreground shrink-0" />
                </button>
              </template>
              <div class="bg-background">
                <DropdownMenuItem v-for="account in accounts" :key="account.id" @click="selectedAccountId = account.id" class="flex items-center gap-3">
                  <MonitorCheck v-if="isMsaAccount(account)" class="h-4 w-4 text-green-500" />
                  <WifiOff v-else class="h-4 w-4 text-muted-foreground" />
                  <span class="truncate">{{ account.username }}</span>
                </DropdownMenuItem>
              </div>
            </DropdownMenu>
          </div>

          <!-- Action Buttons -->
          <div class="flex items-center justify-center gap-4 mt-6">
            <button @click="openInstanceSettings" :disabled="!selectedInstanceId" class="flex items-center gap-2 px-3 py-1.5.5 border rounded-lg hover:bg-muted disabled:opacity-50 disabled:cursor-not-allowed transition-colors" title="Configure instance">
              <Settings class="h-5 w-5" />
              {{ $t('home.configure') }}
            </button>
            <button @click="handlePrimaryAction" :disabled="isActionDisabled" 
              :class="isRunning ? 'bg-zinc-700 hover:bg-zinc-800 text-white dark:bg-zinc-800 dark:hover:bg-zinc-700' : 'bg-green-600 hover:bg-green-700 text-white'"
              class="flex items-center gap-3 rounded-xl px-10 py-2 text-xl font-bold shadow-lg disabled:opacity-50 disabled:cursor-not-allowed transition-all hover:scale-105 active:scale-95">
              <Loader2 v-if="isLaunching" class="h-6 w-6 animate-spin" />
              <Square v-else-if="isRunning" class="h-6 w-6 fill-current" />
              <Play v-else class="h-6 w-6" />
              {{ isLaunching ? $t('home.launching') : (isRunning ? $t('home.stopGame', '停止运行') : $t('home.play')) }}
            </button>
          </div>

          <!-- Instance Info Badge -->
          <div v-if="selectedInstance" class="flex items-center justify-center gap-2 mt-4">
            <span class="text-sm text-muted-foreground mr-1">Minecraft {{ selectedInstance.mcVersion }}</span>
            <span class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none" :class="loaderBadgeClass(selectedInstance.loaderType)">
              {{ formatLoaderType(selectedInstance.loaderType) }}
            </span>
            <span v-if="selectedInstance.modpackType" class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300">
              {{ selectedInstance.modpackType }}
            </span>
            <span v-if="selectedInstance.modpackVersion" class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-400">
              v{{ selectedInstance.modpackVersion }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- Game Log Modal -->
    <Teleport to="body">
      <div v-if="showGameLog" class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
        <div class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto" @click="showGameLog = false"></div>
        <div class="relative z-10 w-full max-w-3xl h-[70vh] gap-4 border bg-white dark:bg-zinc-900 p-4 shadow-xl rounded-lg flex flex-col pointer-events-auto">
          <div class="flex items-center justify-between">
            <h3 class="font-semibold">{{ $t('home.gameOutput') }}</h3>
            <button @click="showGameLog = false" class="text-muted-foreground hover:text-foreground">
              <X class="h-5 w-5" />
            </button>
          </div>
          <div class="flex-1 overflow-auto font-mono text-xs bg-black text-green-400 p-3 rounded">
            <div v-for="(line, idx) in gameLogs" :key="idx">{{ line }}</div>
            <div v-if="gameLogs.length === 0" class="text-gray-500">{{ $t('home.waitingOutput') }}</div>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Crash Report Modal -->
    <CrashReportModal :open="showCrashAlert" :exit-code="crashExitCode" :version-id="crashVersionId" :logs="gameLogs" @update:open="showCrashAlert = $event" />
  </div>
</template>