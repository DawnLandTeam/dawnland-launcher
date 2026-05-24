<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Play,
  Loader2,
  Settings,
  X,
  Gamepad2,
  User,
  ChevronDown,
  Trash2,
} from "@lucide/vue";
import { DropdownMenu, DropdownMenuItem } from "../components/ui/dropdown-menu";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../components/ui/alert-dialog";

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
  accountType: string;
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
const runningInstances = ref<Set<string>>(new Set());
const repairingInstances = ref<Set<string>>(new Set());
const gameLogs = ref<string[]>([]);
const showGameLog = ref(false);

// Crash alert state
const showCrashAlert = ref(false);
const crashExitCode = ref(0);
const crashVersionId = ref("");

// Delete account state
const showDeleteAccountDialog = ref(false);
const deletingAccountId = ref("");
const deletingAccountName = ref("");

// ---------------------------------------------------------------------------
// Computed
// ---------------------------------------------------------------------------
const selectedInstance = computed(() => {
  return (
    installedInstances.value.find((i) => i.id === selectedInstanceId.value) ||
    null
  );
});

const canLaunch = computed(() => {
  return (
    selectedInstanceId.value &&
    selectedAccountId.value &&
    !runningInstances.value.has(selectedInstanceId.value) &&
    !repairingInstances.value.has(selectedInstanceId.value)
  );
});

const launchButtonText = computed(() => {
  const instanceId = selectedInstanceId.value;
  if (repairingInstances.value.has(instanceId)) return "Repairing...";
  if (runningInstances.value.has(instanceId)) return "Running...";
  return "Launch";
});

const isLaunching = computed(() => {
  const instanceId = selectedInstanceId.value;
  return (
    runningInstances.value.has(instanceId) ||
    repairingInstances.value.has(instanceId)
  );
});

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
onMounted(async () => {
  await loadInstances();
  await loadAccounts();

  // Listen for game logs
  listen<GameLog>("game-log", (event) => {
    gameLogs.value.push(event.payload.line);
    if (gameLogs.value.length > 500) {
      gameLogs.value = gameLogs.value.slice(-500);
    }
  });

  // Listen for instance state changes
  listen<InstanceState>("instance-state-changed", (event) => {
    const { versionId, status, exitCode } = event.payload;

    if (status === "running") {
      runningInstances.value.add(versionId);
      repairingInstances.value.delete(versionId);
    } else if (status === "exited") {
      runningInstances.value.delete(versionId);
      repairingInstances.value.delete(versionId);

      // Show crash alert if exit code is non-zero
      if (exitCode !== 0) {
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
async function launchInstance() {
  if (!selectedInstanceId.value || !selectedAccountId.value) {
    return;
  }

  // Add to running set immediately for UI feedback
  runningInstances.value.add(selectedInstanceId.value);
  gameLogs.value = [];
  showGameLog.value = true;

  try {
    await invoke("launch_instance", {
      versionId: selectedInstanceId.value,
      accountUuid: selectedAccountId.value,
    });
  } catch (e) {
    console.error("Failed to launch instance:", e);
    runningInstances.value.delete(selectedInstanceId.value);
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

function confirmDeleteAccount(account: Account) {
  deletingAccountId.value = account.id;
  deletingAccountName.value = account.username;
  showDeleteAccountDialog.value = true;
}

async function deleteAccount() {
  if (!deletingAccountId.value) return;

  try {
    await invoke("remove_account", { accountUuid: deletingAccountId.value });
    showDeleteAccountDialog.value = false;
    await loadAccounts();
  } catch (e) {
    console.error("Failed to delete account:", e);
    alert(`Failed to delete account: ${e}`);
  }

  deletingAccountId.value = "";
  deletingAccountName.value = "";
}

async function addOfflineAccount() {
  const username = prompt("Enter player name:");
  if (!username || username.trim() === "") return;

  try {
    await invoke("add_offline_account", { username: username.trim() });
    await loadAccounts();
  } catch (e) {
    console.error("Failed to add offline account:", e);
    alert(`Failed to add account: ${e}`);
  }
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
    default:
      return "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300";
  }
}
</script>

<template>
  <div class="flex h-full flex-col p-8">
    <!-- Empty State: No instances -->
    <div
      v-if="installedInstances.length === 0"
      class="flex flex-1 flex-col items-center justify-center gap-6"
    >
      <div
        class="flex h-24 w-24 items-center justify-center rounded-3xl bg-muted"
      >
        <Gamepad2 class="h-12 w-12 text-muted-foreground" />
      </div>
      <div class="text-center space-y-2">
        <h2 class="text-2xl font-bold">Welcome to Dawnland</h2>
        <p class="text-muted-foreground">
          Install your first Minecraft instance to get started.
        </p>
      </div>
      <router-link
        to="/instances"
        class="flex items-center gap-2 rounded-md bg-primary px-6 py-3 text-base font-semibold text-primary-foreground hover:bg-primary/90 transition-colors"
      >
        <Play class="h-5 w-5" />
        Install Instance
      </router-link>
    </div>

    <!-- Main Dashboard -->
    <div v-else class="flex flex-1 flex-col items-center justify-center space-y-10">
      <!-- Header -->
      <div class="text-center space-y-2">
        <h1 class="text-4xl font-extrabold tracking-tight">Dawnland</h1>
        <p class="text-lg text-muted-foreground">Minecraft Launcher</p>
      </div>

      <!-- Instance Selector -->
      <div class="w-full max-w-md space-y-2">
        <label class="text-sm font-medium text-muted-foreground"
          >Select Instance</label
        >
        <DropdownMenu>
          <template #trigger>
            <button
              class="w-full flex items-center justify-between px-4 py-3 bg-white dark:bg-zinc-900 border rounded-lg hover:border-primary/50 transition-colors"
            >
              <div v-if="selectedInstance" class="flex items-center gap-3">
                <Gamepad2 class="h-5 w-5 text-primary" />
                <div class="text-left">
                  <div class="font-medium">{{ selectedInstance.name }}</div>
                  <div class="text-xs text-muted-foreground">
                    {{ selectedInstance.mcVersion }} ·
                    {{ formatLoaderType(selectedInstance.loaderType) }}
                  </div>
                </div>
              </div>
              <span v-else class="text-muted-foreground"
                >Select an instance...</span
              >
              <ChevronDown class="h-5 w-5 text-muted-foreground" />
            </button>
          </template>
          <div class="max-h-60 overflow-y-auto">
            <DropdownMenuItem
              v-for="instance in installedInstances"
              :key="instance.id"
              @click="selectedInstanceId = instance.id"
              class="flex items-center gap-3"
            >
              <Gamepad2 class="h-4 w-4" />
              <div class="flex-1">
                <div class="font-medium">{{ instance.name }}</div>
                <div class="text-xs text-muted-foreground">
                  {{ instance.mcVersion }} ·
                  {{ formatLoaderType(instance.loaderType) }}
                </div>
              </div>
            </DropdownMenuItem>
          </div>
        </DropdownMenu>
      </div>

      <!-- Account Selector -->
      <div class="w-full max-w-md space-y-2">
        <label class="text-sm font-medium text-muted-foreground"
          >Select Account</label
        >
        <DropdownMenu>
          <template #trigger>
            <button
              class="w-full flex items-center justify-between px-4 py-3 bg-white dark:bg-zinc-900 border rounded-lg hover:border-primary/50 transition-colors"
            >
              <div v-if="selectedAccountId" class="flex items-center gap-3">
                <div
                  class="flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-foreground text-sm font-bold"
                >
                  {{
                    accounts.find((a) => a.id === selectedAccountId)?.username
                      .charAt(0)
                      .toUpperCase() || "?"
                  }}
                </div>
                <div class="text-left">
                  <div class="font-medium">
                    {{
                      accounts.find((a) => a.id === selectedAccountId)?.username
                    }}
                  </div>
                  <div class="text-xs text-muted-foreground">
                    {{
                      accounts.find((a) => a.id === selectedAccountId)
                        ?.accountType
                    }}
                  </div>
                </div>
              </div>
              <span v-else class="text-muted-foreground"
                >Select an account...</span
              >
              <ChevronDown class="h-5 w-5 text-muted-foreground" />
            </button>
          </template>
          <DropdownMenuItem
            v-for="account in accounts"
            :key="account.id"
            @click="selectedAccountId = account.id"
            class="flex items-center gap-3"
          >
            <div
              class="flex h-6 w-6 items-center justify-center rounded-full bg-primary text-primary-foreground text-xs font-bold"
            >
              {{ account.username.charAt(0).toUpperCase() }}
            </div>
            <div class="flex-1">
              <div class="font-medium">{{ account.username }}</div>
              <div class="text-xs text-muted-foreground">
                {{ account.accountType }}
              </div>
            </div>
          </DropdownMenuItem>
          <div class="border-t my-1"></div>
          <DropdownMenuItem @click="addOfflineAccount">
            <User class="h-4 w-4" />
            Add Offline Account
          </DropdownMenuItem>
        </DropdownMenu>

        <!-- Account management quick links -->
        <div
          v-if="accounts.length > 0"
          class="flex justify-end pt-1"
        >
          <button
            @click="confirmDeleteAccount(accounts.find(a => a.id === selectedAccountId)!)"
            class="text-xs text-muted-foreground hover:text-red-500 transition-colors flex items-center gap-1"
            :disabled="!selectedAccountId"
          >
            <Trash2 class="h-3 w-3" />
            Remove selected account
          </button>
        </div>
      </div>

      <!-- Action Buttons -->
      <div class="flex items-center gap-4">
        <!-- Configure Button -->
        <button
          @click="openInstanceSettings"
          :disabled="!selectedInstanceId"
          class="flex items-center gap-2 px-4 py-2 border rounded-lg hover:bg-muted disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          title="Configure instance"
        >
          <Settings class="h-5 w-5" />
          Configure
        </button>

        <!-- Launch Button (Large) -->
        <button
          @click="launchInstance"
          :disabled="!canLaunch"
          class="flex items-center gap-3 rounded-xl bg-green-600 px-10 py-4 text-xl font-bold text-white shadow-lg hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed transition-all hover:scale-105 active:scale-95"
        >
          <Loader2
            v-if="isLaunching"
            class="h-6 w-6 animate-spin"
          />
          <Play v-else class="h-6 w-6" />
          {{ launchButtonText }}
        </button>
      </div>

      <!-- Instance Info Badge -->
      <div v-if="selectedInstance" class="flex items-center gap-2">
        <span
          class="inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold"
          :class="loaderBadgeClass(selectedInstance.loaderType)"
        >
          {{ formatLoaderType(selectedInstance.loaderType) }}
        </span>
        <span class="text-sm text-muted-foreground">
          Minecraft {{ selectedInstance.mcVersion }}
        </span>
      </div>
    </div>

    <!-- Game Log Modal -->
    <Teleport to="body">
      <div
        v-if="showGameLog"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <div
          class="absolute inset-0 bg-background/80 backdrop-blur-sm pointer-events-auto"
          @click="showGameLog = false"
        />
        <div
          class="relative z-10 w-full max-w-3xl h-[70vh] gap-4 border bg-card p-4 shadow-xl rounded-lg flex flex-col pointer-events-auto"
        >
          <div class="flex items-center justify-between">
            <h3 class="font-semibold">Game Output</h3>
            <button
              @click="showGameLog = false"
              class="text-muted-foreground hover:text-foreground"
            >
              <X class="h-5 w-5" />
            </button>
          </div>
          <div
            class="flex-1 overflow-auto font-mono text-xs bg-black text-green-400 p-3 rounded"
          >
            <div v-for="(line, idx) in gameLogs" :key="idx">
              {{ line }}
            </div>
            <div v-if="gameLogs.length === 0" class="text-gray-500">
              Waiting for game output...
            </div>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Crash Alert Modal -->
    <Teleport to="body">
      <div
        v-if="showCrashAlert"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <div
          class="absolute inset-0 bg-background/80 backdrop-blur-sm pointer-events-auto"
          @click="showCrashAlert = false"
        />
        <div
          class="relative z-10 w-full max-w-sm gap-4 border bg-card p-5 shadow-xl rounded-lg pointer-events-auto"
        >
          <div class="flex items-center justify-between mb-4">
            <h3 class="font-semibold text-lg text-red-600">Game Crashed!</h3>
            <button
              @click="showCrashAlert = false"
              class="text-muted-foreground hover:text-foreground"
            >
              <X class="h-5 w-5" />
            </button>
          </div>

          <p class="text-sm text-foreground mb-2">
            The game has exited unexpectedly.
          </p>
          <p class="text-sm text-muted-foreground mb-4">
            <strong>Exit Code:</strong> {{ crashExitCode }}<br />
            <strong>Version:</strong> {{ crashVersionId }}
          </p>
          <p class="text-xs text-muted-foreground">
            Please check the game console logs for more details about the
            crash.
          </p>

          <div class="flex justify-end gap-2 mt-6">
            <button
              @click="showCrashAlert = false"
              class="px-4 py-2 text-sm font-medium bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors"
            >
              OK
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Delete Account Confirmation Dialog -->
    <AlertDialog
      :open="showDeleteAccountDialog"
      @update:open="showDeleteAccountDialog = $event"
    >
      <AlertDialogTitle>Remove Account?</AlertDialogTitle>
      <AlertDialogDescription class="mt-2">
        Are you sure you want to remove
        <strong>{{ deletingAccountName }}</strong
        >? This action cannot be undone.
      </AlertDialogDescription>
      <div class="flex justify-end gap-2 mt-6">
        <button
          @click="showDeleteAccountDialog = false"
          class="px-4 py-2 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
        >
          Cancel
        </button>
        <button
          @click="deleteAccount"
          class="flex items-center gap-2 px-4 py-2 text-sm font-medium bg-red-600 text-white rounded-md hover:bg-red-700 transition-colors"
        >
          <Trash2 class="h-4 w-4" />
          Remove
        </button>
      </div>
    </AlertDialog>
  </div>
</template>