<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { Gamepad2, Plus, Package, Play, Loader2, Settings, FolderOpen, Save, X, MoreHorizontal, Trash2, Folder } from "@lucide/vue";
import InstallInstanceModal from "../components/InstallInstanceModal.vue";
import { DropdownMenu, DropdownMenuItem } from "../components/ui/dropdown-menu";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../components/ui/alert-dialog";

// Types
interface InstanceItem {
  id: string;
  name: string;
  mcVersion: string;
  loaderType: string;
}

interface GameLog {
  type: string;
  line: string;
}

interface Account {
  id: string;
  username: string;
  accountType: string;
}

interface InstanceConfig {
  javaPath?: string;
  maxMemory?: number;
  jvmArgsExtra?: string[];
  windowBehavior?: string;
}

interface InstanceState {
  versionId: string;
  status: "running" | "exited" | "repairing" | "repairing_complete";
  exitCode?: number;
  missingCount?: number;
}

interface SystemMemoryInfo {
  totalMb: number;
  recommendedMaxMb: number;
}

// State
const showInstallModal = ref(false);
const installedInstances = ref<InstanceItem[]>([]);
const accounts = ref<Account[]>([]);
const selectedAccount = ref<string>("");

// System memory for slider
const systemMemory = ref<SystemMemoryInfo>({
  totalMb: 8192,
  recommendedMaxMb: 4096
});

// Track running state per instance
const runningInstances = ref<Set<string>>(new Set());
const repairingInstances = ref<Set<string>>(new Set());
const gameLogs = ref<string[]>([]);
const showGameLog = ref(false);

// Settings modal state
const showSettingsModal = ref(false);
const settingsInstanceId = ref("");
const settingsInstanceName = ref("");
const settingsConfig = ref<InstanceConfig>({
  javaPath: "",
  maxMemory: 4096,
  jvmArgsExtra: [],
  windowBehavior: "keep"
});
const isSavingConfig = ref(false);

// Crash alert state
const showCrashAlert = ref(false);
const crashExitCode = ref(0);
const crashVersionId = ref("");

// Delete confirmation state
const showDeleteDialog = ref(false);
const deletingInstanceId = ref("");
const deletingInstanceName = ref("");
const isDeletingInstance = ref(false);

// Load installed instances on mount
onMounted(async () => {
  await loadInstances();
  await loadAccounts();
  await loadSystemMemory();
  
  // Listen for game logs
  listen<GameLog>("game-log", (event) => {
    gameLogs.value.push(event.payload.line);
    if (gameLogs.value.length > 500) {
      gameLogs.value = gameLogs.value.slice(-500);
    }
  });
  
  // Listen for instance state changes
  listen<InstanceState>("instance-state-changed", (event) => {
    const { versionId, status, exitCode, missingCount } = event.payload;
    
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
      // Show repairing state in UI
      repairingInstances.value.add(versionId);
      console.log(`Repairing ${versionId}: ${missingCount ?? 0} missing files...`);
    } else if (status === "repairing_complete") {
      repairingInstances.value.delete(versionId);
      console.log(`Repair complete for ${versionId}`);
    }
  });
});

async function loadInstances() {
  try {
    const instances = await invoke<InstanceItem[]>("scan_installed_instances");
    installedInstances.value = instances;
  } catch (e) {
    console.error("Failed to load instances:", e);
  }
}

// Refresh instance list (called after installation)
async function refreshInstancesList() {
  await loadInstances();
}

async function loadAccounts() {
  try {
    accounts.value = await invoke<Account[]>("get_accounts");
    if (accounts.value.length > 0) {
      selectedAccount.value = accounts.value[0].id;
    }
  } catch (e) {
    console.error("Failed to load accounts:", e);
  }
}

async function loadSystemMemory() {
  try {
    systemMemory.value = await invoke<SystemMemoryInfo>("get_system_memory");
  } catch (e) {
    console.error("Failed to load system memory:", e);
  }
}

async function launchInstance(instanceId: string) {
  if (!selectedAccount.value) {
    alert("Please select an account first");
    return;
  }
  
  // Add to running set immediately for UI feedback
  runningInstances.value.add(instanceId);
  gameLogs.value = [];
  showGameLog.value = true;
  
  try {
    await invoke("launch_instance", {
      versionId: instanceId,
      accountUuid: selectedAccount.value
    });
    // Don't set isLaunching here - let the event handle it
  } catch (e) {
    console.error("Failed to launch instance:", e);
    runningInstances.value.delete(instanceId);
    alert(`Failed to launch: ${e}`);
  }
}

async function openSettings(instance: InstanceItem) {
  settingsInstanceId.value = instance.id;
  settingsInstanceName.value = instance.name;
  
  try {
    const config = await invoke<InstanceConfig>("get_instance_config", {
      versionId: instance.id
    });
    settingsConfig.value = {
      javaPath: config.javaPath || "",
      maxMemory: config.maxMemory || 4096,
      jvmArgsExtra: config.jvmArgsExtra || [],
      windowBehavior: config.windowBehavior || "keep"
    };
  } catch (e) {
    console.error("Failed to load instance config:", e);
    settingsConfig.value = {
      javaPath: "",
      maxMemory: 4096,
      jvmArgsExtra: [],
      windowBehavior: "keep"
    };
  }
  
  showSettingsModal.value = true;
}

async function browseJavaPath() {
  try {
    const selected = await open({
      multiple: false,
      title: "Select Java Executable",
      filters: [{
        name: "Executable",
        extensions: ["exe", "app", ""]
      }]
    });
    
    if (selected) {
      settingsConfig.value.javaPath = selected as string;
    }
  } catch (e) {
    console.error("Failed to open file dialog:", e);
  }
}

async function saveSettings() {
  isSavingConfig.value = true;
  
  try {
    const config = {
      javaPath: settingsConfig.value.javaPath || null,
      maxMemory: settingsConfig.value.maxMemory || null,
      jvmArgsExtra: settingsConfig.value.jvmArgsExtra?.length ? settingsConfig.value.jvmArgsExtra : null,
      windowBehavior: settingsConfig.value.windowBehavior || "keep"
    };
    
    await invoke("save_instance_config", {
      versionId: settingsInstanceId.value,
      config
    });
    
    showSettingsModal.value = false;
  } catch (e) {
    console.error("Failed to save instance config:", e);
    alert(`Failed to save: ${e}`);
  } finally {
    isSavingConfig.value = false;
  }
}

async function openInstanceFolder(instanceId: string) {
  try {
    await invoke("open_instance_folder", { versionId: instanceId });
  } catch (e) {
    console.error("Failed to open instance folder:", e);
    alert(`Failed to open folder: ${e}`);
  }
}

function confirmDeleteInstance(instance: InstanceItem) {
  deletingInstanceId.value = instance.id;
  deletingInstanceName.value = instance.name;
  showDeleteDialog.value = true;
}

async function deleteInstance() {
  if (!deletingInstanceId.value) return;
  
  isDeletingInstance.value = true;
  
  try {
    await invoke("delete_instance", { versionId: deletingInstanceId.value });
    showDeleteDialog.value = false;
    // Refresh the instance list
    await refreshInstancesList();
  } catch (e) {
    console.error("Failed to delete instance:", e);
    alert(`Failed to delete: ${e}`);
  } finally {
    isDeletingInstance.value = false;
    deletingInstanceId.value = "";
    deletingInstanceName.value = "";
  }
}
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- Empty State -->
    <div
      v-if="installedInstances.length === 0"
      class="flex flex-1 flex-col items-center justify-center gap-4 p-6"
    >
      <div class="flex h-20 w-20 items-center justify-center rounded-2xl bg-muted">
        <Package class="h-10 w-10 text-muted-foreground" />
      </div>
      <div class="text-center space-y-1">
        <h2 class="text-xl font-semibold">No instances yet</h2>
        <p class="text-sm text-muted-foreground">
          Install a Minecraft version to get started.
        </p>
      </div>
      <button
        @click="showInstallModal = true"
        class="flex items-center gap-2 rounded-md bg-primary px-5 py-2.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
      >
        <Plus class="h-4 w-4" />
        Install Instance
      </button>
    </div>

    <!-- List State -->
    <div v-else class="flex flex-1 flex-col p-6 space-y-6">
      <!-- Header -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-3">
          <Gamepad2 class="h-7 w-7 text-primary" />
          <div>
            <h1 class="text-2xl font-bold">Game Instances</h1>
            <p class="text-sm text-muted-foreground">
              {{ installedInstances.length }} instance{{ installedInstances.length !== 1 ? 's' : '' }} installed
            </p>
          </div>
        </div>
        <button
          @click="showInstallModal = true"
          class="flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
        >
          <Plus class="h-4 w-4" />
          New Instance
        </button>
      </div>

      <!-- Account Selector -->
      <div class="flex items-center gap-3">
        <label class="text-sm font-medium">Account:</label>
        <select
          v-model="selectedAccount"
          class="px-3 py-1.5 bg-background border rounded-md text-sm"
        >
          <option value="" disabled>Select account...</option>
          <option v-for="account in accounts" :key="account.id" :value="account.id">
            {{ account.username }} ({{ account.accountType }})
          </option>
        </select>
      </div>

      <!-- Instance Grid -->
      <div class="grid grid-cols-3 gap-4">
        <div
          v-for="instance in installedInstances"
          :key="instance.id"
          class="group rounded-lg border bg-card p-4 hover:border-primary/50 transition-colors"
        >
          <div class="flex items-start justify-between">
            <div class="space-y-1">
              <h3 class="font-medium">{{ instance.name }}</h3>
              <p class="text-xs text-muted-foreground">
                {{ instance.mcVersion }} · {{ instance.loaderType }}
              </p>
            </div>
            <Gamepad2 class="h-5 w-5 text-muted-foreground group-hover:text-primary transition-colors" />
          </div>
          
          <!-- Action Buttons -->
          <div class="mt-3 flex gap-2">
            <button
              @click="launchInstance(instance.id)"
              :disabled="runningInstances.has(instance.id) || repairingInstances.has(instance.id) || !selectedAccount"
              class="flex-1 flex items-center justify-center gap-2 rounded-md bg-green-600 px-3 py-2 text-sm font-medium text-white hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              <Loader2 v-if="runningInstances.has(instance.id) || repairingInstances.has(instance.id)" class="h-4 w-4 animate-spin" />
              <Play v-else class="h-4 w-4" />
              <span v-if="repairingInstances.has(instance.id)">Repairing...</span>
              <span v-else-if="runningInstances.has(instance.id)">Running...</span>
              <span v-else>Play</span>
            </button>
            
            <!-- Dropdown Menu -->
            <DropdownMenu>
              <template #trigger>
                <button
                  class="flex items-center justify-center rounded-md border bg-background px-3 py-2 text-sm font-medium hover:bg-muted transition-colors"
                  title="More options"
                  :disabled="runningInstances.has(instance.id)"
                  :class="{ 'opacity-50 cursor-not-allowed': runningInstances.has(instance.id) }"
                >
                  <MoreHorizontal class="h-4 w-4" />
                </button>
              </template>
              <DropdownMenuItem @click="openSettings(instance)">
                <Settings class="h-4 w-4" />
                Settings
              </DropdownMenuItem>
              <DropdownMenuItem @click="openInstanceFolder(instance.id)">
                <Folder class="h-4 w-4" />
                Open Folder
              </DropdownMenuItem>
              <DropdownMenuItem destructive @click="confirmDeleteInstance(instance)">
                <Trash2 class="h-4 w-4" />
                Delete
              </DropdownMenuItem>
            </DropdownMenu>
          </div>
        </div>
      </div>
    </div>

    <!-- Install Instance Modal -->
    <InstallInstanceModal 
      v-model:open="showInstallModal" 
      @installed-success="refreshInstancesList"
    />

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
              ✕
            </button>
          </div>
          <div class="flex-1 overflow-auto font-mono text-xs bg-black text-green-400 p-3 rounded">
            <div v-for="(line, idx) in gameLogs" :key="idx">{{ line }}</div>
            <div v-if="gameLogs.length === 0" class="text-gray-500">Waiting for game output...</div>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Instance Settings Modal -->
    <Teleport to="body">
      <div
        v-if="showSettingsModal"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <div 
          class="absolute inset-0 bg-background/80 backdrop-blur-sm pointer-events-auto"
          @click="showSettingsModal = false"
        />
        <div
          class="relative z-10 w-full max-w-md gap-4 border bg-card p-5 shadow-xl rounded-lg pointer-events-auto"
        >
          <div class="flex items-center justify-between mb-4">
            <div>
              <h3 class="font-semibold text-lg">Instance Settings</h3>
              <p class="text-sm text-muted-foreground">{{ settingsInstanceName }}</p>
            </div>
            <button
              @click="showSettingsModal = false"
              class="text-muted-foreground hover:text-foreground"
            >
              <X class="h-5 w-5" />
            </button>
          </div>

          <!-- Java Path -->
          <div class="space-y-2">
            <label class="text-sm font-medium">Java Executable Path</label>
            <div class="flex gap-2">
              <input
                v-model="settingsConfig.javaPath"
                type="text"
                placeholder="Leave empty for system default"
                class="flex-1 px-3 py-2 bg-background border rounded-md text-sm"
              />
              <button
                @click="browseJavaPath"
                class="flex items-center gap-1 px-3 py-2 border rounded-md text-sm hover:bg-muted transition-colors"
                title="Browse"
              >
                <FolderOpen class="h-4 w-4" />
              </button>
            </div>
            <p class="text-xs text-muted-foreground">
              MC 1.20.5+ requires Java 21. Leave empty to use system default "java".
            </p>
          </div>

          <!-- Max Memory -->
          <div class="space-y-2 mt-4">
            <div class="flex items-center justify-between">
              <label class="text-sm font-medium">Max Memory (MB)</label>
              <span class="text-sm font-mono text-primary">{{ settingsConfig.maxMemory }} MB</span>
            </div>
            <input
              v-model.number="settingsConfig.maxMemory"
              type="range"
              min="512"
              :max="systemMemory.totalMb"
              step="512"
              class="w-full h-2 bg-muted rounded-lg appearance-none cursor-pointer accent-primary"
            />
            <div class="flex justify-between text-xs text-muted-foreground">
              <span>512 MB</span>
              <span>System: {{ systemMemory.totalMb }} MB</span>
            </div>
            <p class="text-xs text-muted-foreground">
              Recommended: {{ systemMemory.recommendedMaxMb }} MB (1/3 of system RAM)
            </p>
          </div>

          <!-- Extra JVM Args -->
          <div class="space-y-2 mt-4">
            <label class="text-sm font-medium">Extra JVM Arguments (advanced)</label>
            <textarea
              v-model="settingsConfig.jvmArgsExtra"
              placeholder="-XX:+UseG1GC&#10;-XX:+ParallelGCThreads=4"
              class="w-full px-3 py-2 bg-background border rounded-md text-sm font-mono h-20 resize-none"
            />
          </div>

          <!-- Window Behavior -->
          <div class="space-y-2 mt-4">
            <label class="text-sm font-medium">Window Behavior</label>
            <select
              v-model="settingsConfig.windowBehavior"
              class="w-full px-3 py-2 bg-background border rounded-md text-sm"
            >
              <option value="keep">Keep visible (default)</option>
              <option value="hide">Hide launcher</option>
              <option value="minimize">Minimize to taskbar</option>
            </select>
            <p class="text-xs text-muted-foreground">
              Choose what happens to the launcher window when the game starts.
            </p>
          </div>

          <!-- Save Button -->
          <div class="flex justify-end gap-2 mt-6">
            <button
              @click="showSettingsModal = false"
              class="px-4 py-2 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
            >
              Cancel
            </button>
            <button
              @click="saveSettings"
              :disabled="isSavingConfig"
              class="flex items-center gap-2 px-4 py-2 text-sm font-medium bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 transition-colors"
            >
              <Save class="h-4 w-4" />
              Save
            </button>
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
            <strong>Exit Code:</strong> {{ crashExitCode }}<br/>
            <strong>Version:</strong> {{ crashVersionId }}
          </p>
          <p class="text-xs text-muted-foreground">
            Please check the game console logs for more details about the crash.
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

    <!-- Delete Confirmation Dialog -->
    <AlertDialog :open="showDeleteDialog" @update:open="showDeleteDialog = $event">
      <AlertDialogTitle>Delete Instance?</AlertDialogTitle>
      <AlertDialogDescription class="mt-2">
        Are you sure you want to delete <strong>{{ deletingInstanceName }}</strong>?
        This will remove all instance data including saves, mods, and resource packs.
        <span class="text-red-600 font-medium">This action cannot be undone.</span>
      </AlertDialogDescription>
      <div class="flex justify-end gap-2 mt-6">
        <button
          @click="showDeleteDialog = false"
          class="px-4 py-2 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
          :disabled="isDeletingInstance"
        >
          Cancel
        </button>
        <button
          @click="deleteInstance"
          :disabled="isDeletingInstance"
          class="flex items-center gap-2 px-4 py-2 text-sm font-medium bg-red-600 text-white rounded-md hover:bg-red-700 disabled:opacity-50 transition-colors"
        >
          <Trash2 v-if="isDeletingInstance" class="h-4 w-4 animate-spin" />
          <Trash2 v-else class="h-4 w-4" />
          Delete
        </button>
      </div>
    </AlertDialog>
  </div>
</template>