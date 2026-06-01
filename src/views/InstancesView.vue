<script setup lang="ts">
import { ref, onMounted, watch, onActivated } from "vue";
import { useRoute, useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { Gamepad2, Plus, Package, Settings, Save, MoreHorizontal, Trash2, Folder, Puzzle, RefreshCw } from "@lucide/vue";
import InstallInstanceModal from "../components/InstallInstanceModal.vue";
import InstanceModsModal from "../components/InstanceModsModal.vue";
import { DropdownMenu, DropdownMenuItem } from "../components/ui/dropdown-menu";
import { DialogContent, DialogTitle, DialogDescription } from "../components/ui/dialog";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../components/ui/alert-dialog";

// Types
interface InstanceItem {
  id: string;
  name: string;
  mcVersion: string;
  loaderType: string;
  modpackVersion?: string;
  modpackType?: string;
  modpackProjectId?: string;
}

interface InstanceConfig {
  javaPath?: string;
  maxMemory?: number;
  jvmArgsExtra?: string[];
  windowBehavior?: string;
  showGameLog?: boolean;
}

interface SystemMemoryInfo {
  totalMb: number;
  recommendedMaxMb: number;
}

interface JavaInfo {
  path: string;
  majorVersion: number;
  versionString: string;
  vendor: string;
  is64Bit: boolean;
}

// Router — deep-link support
const route = useRoute();
const router = useRouter();

// State
const showInstallModal = ref(false);
const prefillVersion = ref("");
const prefillLoader = ref("");
const installedInstances = ref<InstanceItem[]>([]);

// Settings modal state
const showSettingsModal = ref(false);
const settingsInstanceId = ref("");
const settingsInstanceName = ref("");
const settingsConfig = ref<InstanceConfig>({
  javaPath: "",
  maxMemory: 4096,
  jvmArgsExtra: [],
  windowBehavior: "keep",
  showGameLog: false,
});
const isSavingConfig = ref(false);

// System memory for slider
const systemMemory = ref<SystemMemoryInfo>({
  totalMb: 8192,
  recommendedMaxMb: 4096,
});

// Installed Javas
const installedJavas = ref<JavaInfo[]>([]);

// Delete confirmation state
const showDeleteDialog = ref(false);
const deletingInstanceId = ref("");
const deletingInstanceName = ref("");
const isDeletingInstance = ref(false);

// Mods modal state
const showModsModal = ref(false);
const modsInstance = ref<InstanceItem | null>(null);

// ---------------------------------------------------------------------------
// Deep-link: route.query.manage → auto-open settings for a specific instance
// ---------------------------------------------------------------------------
const openSettingsForInstance = async (instanceId: string) => {
  // Find the instance in the list so we can display its name
  const instance = installedInstances.value.find((i) => i.id === instanceId);
  if (!instance) {
    console.warn(`Instance "${instanceId}" not found — cannot open settings`);
    return;
  }
  await openSettings(instance);
};

watch(
  () => route.query.manage,
  (newId) => {
    if (newId && typeof newId === "string") {
      openSettingsForInstance(newId);
    }
  },
  { immediate: true },
);

// ---------------------------------------------------------------------------
// Deep-link: route.query.install_version & install_loader
// ---------------------------------------------------------------------------
watch(
  () => route.query,
  (query) => {
    if (query.install_version && typeof query.install_version === "string") {
      prefillVersion.value = query.install_version;
      if (query.install_loader && typeof query.install_loader === "string") {
        prefillLoader.value = query.install_loader.toLowerCase();
      } else {
        prefillLoader.value = "vanilla";
      }
      showInstallModal.value = true;
      
      // Clean up the URL so a refresh doesn't trigger it again
      const newQuery = { ...query };
      delete newQuery.install_version;
      delete newQuery.install_loader;
      router.replace({ query: newQuery });
    }
  },
  { immediate: true },
);

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
onMounted(async () => {
  await loadInstances();
  await loadJavas();
});

onActivated(async () => {
  await loadInstances();
  await loadSystemMemory();
  await loadJavas();
});

// ---------------------------------------------------------------------------
// Data loading
// ---------------------------------------------------------------------------
async function loadInstances() {
  try {
    const instances = await invoke<InstanceItem[]>("scan_installed_instances");
    installedInstances.value = instances;
  } catch (e) {
    console.error("Failed to load instances:", e);
  }
}

async function refreshInstancesList() {
  await loadInstances();
}

async function loadSystemMemory() {
  try {
    systemMemory.value = await invoke<SystemMemoryInfo>("get_system_memory");
  } catch (e) {
    console.error("Failed to load system memory:", e);
  }
}

async function loadJavas() {
  try {
    installedJavas.value = await invoke<JavaInfo[]>("scan_local_javas");
  } catch (e) {
    console.error("Failed to load installed Javas:", e);
  }
}

// ---------------------------------------------------------------------------
// Settings modal
// ---------------------------------------------------------------------------
async function openSettings(instance: InstanceItem) {
  settingsInstanceId.value = instance.id;
  settingsInstanceName.value = instance.name;

  try {
    const config = await invoke<InstanceConfig>("get_instance_config", {
      versionId: instance.id,
    });
    settingsConfig.value = {
      javaPath: config.javaPath || "",
      maxMemory: config.maxMemory || 4096,
      jvmArgsExtra: config.jvmArgsExtra || [],
      windowBehavior: config.windowBehavior || "keep",
      showGameLog: config.showGameLog === true,
    };
  } catch (e) {
    console.error("Failed to load instance config:", e);
    settingsConfig.value = {
      javaPath: "",
      maxMemory: 4096,
      jvmArgsExtra: [],
      windowBehavior: "keep",
      showGameLog: false,
    };
  }

  showSettingsModal.value = true;
}

function openMods(instance: InstanceItem) {
  modsInstance.value = instance;
  showModsModal.value = true;
}

// Remove browseJavaPath since we are using select now

async function saveSettings() {
  isSavingConfig.value = true;

  try {
    const config = {
      javaPath: settingsConfig.value.javaPath || null,
      maxMemory: settingsConfig.value.maxMemory || null,
      jvmArgsExtra: settingsConfig.value.jvmArgsExtra?.length
        ? settingsConfig.value.jvmArgsExtra
        : null,
      windowBehavior: settingsConfig.value.windowBehavior || "keep",
      showGameLog: settingsConfig.value.showGameLog,
    };

    await invoke("save_instance_config", {
      versionId: settingsInstanceId.value,
      config,
    });

    showSettingsModal.value = false;
  } catch (e) {
    console.error("Failed to save instance config:", e);
    alert(`Failed to save: ${e}`);
  } finally {
    isSavingConfig.value = false;
  }
}

// ---------------------------------------------------------------------------
// Instance management actions
// ---------------------------------------------------------------------------
async function openInstanceFolder(instanceId: string) {
  try {
    await invoke("open_instance_folder", { versionId: instanceId });
  } catch (e) {
    console.error("Failed to open instance folder:", e);
    alert(`Failed to open folder: ${e}`);
  }
}

function updateModpack(instance: InstanceItem) {
  router.push({
    path: '/modpack-install',
    query: { 
      update_id: instance.name,
      source: instance.modpackType?.toLowerCase() || '',
      current_version: instance.modpackVersion || '',
      project_id: instance.modpackProjectId || ''
    }
  });
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

// ---------------------------------------------------------------------------
// Loader type badge colour helper
// ---------------------------------------------------------------------------
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
  <div class="flex h-full flex-col">
    <!-- Empty State -->
    <div
      v-if="installedInstances.length === 0"
      class="flex flex-1 flex-col items-center justify-center gap-4 p-4"
    >
      <div
        class="flex h-20 w-20 items-center justify-center rounded-2xl bg-muted"
      >
        <Package class="h-10 w-10 text-muted-foreground" />
      </div>
      <div class="text-center space-y-1">
        <h2 class="text-xl font-semibold">No instances yet</h2>
        <p class="text-sm text-muted-foreground">
          Install a Minecraft version to get started.
        </p>
      </div>
      <div class="flex items-center gap-3">
        <button
          @click="showInstallModal = true"
          class="flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
        >
          <Plus class="h-4 w-4" />
          Install Instance
        </button>
        <button
          @click="router.push('/modpack-install')"
          class="flex items-center gap-2 rounded-md border px-4 py-2 text-sm font-medium hover:bg-accent hover:text-accent-foreground transition-colors"
        >
          <Package class="h-4 w-4" />
          Install Modpack
        </button>
      </div>
    </div>

    <!-- List State -->
    <div v-else class="flex flex-1 flex-col p-4 space-y-6">
      <!-- Header -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-3">
          <Gamepad2 class="h-7 w-7 text-primary" />
          <div>
            <h1 class="text-2xl font-bold">{{ $t('instances.title') }}</h1>
            <p class="text-sm text-muted-foreground">
              {{ $t('instances.installedCount', { count: installedInstances.length }, installedInstances.length) }}
            </p>
          </div>
        </div>
        <div class="flex items-center gap-2">
          <button
            @click="router.push('/modpack-install')"
            class="flex items-center gap-2 rounded-md bg-secondary px-3 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/90 transition-colors"
          >
            <Package class="h-4 w-4" />
            {{ $t('install.modpackTitle', 'Install Modpack') }}
          </button>
          <button
            @click="showInstallModal = true"
            class="flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            <Plus class="h-4 w-4" />
            {{ $t('instances.add') }}
          </button>
        </div>
      </div>

      <!-- Instance Grid -->
      <div class="grid grid-cols-3 gap-4">
        <div
          v-for="instance in installedInstances"
          :key="instance.id"
          class="group rounded-lg border bg-card p-4 hover:border-primary/50 transition-colors"
        >
          <!-- Instance info — primary visual focus -->
          <div class="flex items-start justify-between">
            <div class="min-w-0 flex items-center gap-3 flex-1">
              <Package class="h-5 w-5 shrink-0 text-muted-foreground" />
              <div class="min-w-0 flex-1">
                <h3 class="font-semibold truncate" :title="instance.name">{{ instance.name }}</h3>
                <div class="flex items-center gap-2 mt-1 flex-wrap">
                  <span class="text-xs text-muted-foreground font-mono">
                    {{ instance.mcVersion }}
                  </span>
                  <span
                    class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none"
                    :class="loaderBadgeClass(instance.loaderType)"
                  >
                    {{ instance.loaderType }}
                  </span>
                  <span
                    v-if="instance.modpackType"
                    class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300"
                  >
                    {{ instance.modpackType }}
                  </span>
                  <span
                    v-if="instance.modpackVersion"
                    class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-400"
                  >
                    v{{ instance.modpackVersion }}
                  </span>
                </div>
              </div>
            </div>
          </div>

          <!-- Management actions -->
          <div class="mt-3 flex justify-end">
            <DropdownMenu align="end">
              <template #trigger>
                <button
                  class="flex items-center justify-center rounded-md border bg-background px-3 py-1.5 text-sm font-medium hover:bg-muted transition-colors"
                  title="More options"
                >
                  <MoreHorizontal class="h-4 w-4" />
                </button>
              </template>
              <DropdownMenuItem @click="openSettings(instance)">
                <Settings class="h-4 w-4" />
                {{ $t('instances.settings') }}
              </DropdownMenuItem>
              <DropdownMenuItem
                @click="openInstanceFolder(instance.id)"
              >
                <Folder class="h-4 w-4" />
                Open Folder
              </DropdownMenuItem>
              <DropdownMenuItem
                @click="updateModpack(instance)"
              >
                <RefreshCw class="h-4 w-4" />
                {{ $t('instances.updateModpack', 'Update Modpack') }}
              </DropdownMenuItem>
              <DropdownMenuItem
                @click="openMods(instance)"
              >
                <Puzzle class="h-4 w-4" />
                {{ $t('instances.mods') }}
              </DropdownMenuItem>
              <DropdownMenuItem
                destructive
                @click="confirmDeleteInstance(instance)"
              >
                <Trash2 class="h-4 w-4" />
                {{ $t('instances.delete') }}
              </DropdownMenuItem>
            </DropdownMenu>
          </div>
        </div>
      </div>
    </div>

    <!-- Install Instance Modal -->
    <InstallInstanceModal
      v-model:open="showInstallModal"
      :initial-version="prefillVersion"
      :initial-loader="prefillLoader"
      @installed-success="refreshInstancesList"
    />

    <!-- Instance Mods Modal -->
    <InstanceModsModal
      v-model:open="showModsModal"
      :instance="modsInstance"
    />

    <!-- Instance Settings Modal -->
    <DialogContent :open="showSettingsModal" @update:open="showSettingsModal = $event" class="max-w-md">
        <DialogTitle>Instance Settings</DialogTitle>
        <DialogDescription>
          {{ settingsInstanceName }}
        </DialogDescription>

          <!-- Java Path -->
          <div class="space-y-2">
            <label class="text-sm font-medium">Java Version</label>
            <select
              v-model="settingsConfig.javaPath"
              class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
            >
              <option value="">Default (Auto-detect)</option>
              <option v-for="java in installedJavas" :key="java.path" :value="java.path">
                Java {{ java.majorVersion }} ({{ java.vendor }}) - {{ java.versionString }}
              </option>
            </select>
            <p class="text-xs text-muted-foreground">
              MC 1.20.5+ requires Java 21. Leave Default to use the system default Java or recommended version. If your desired Java is not listed, you can add it in Settings > Java Management.
            </p>
          </div>

          <!-- Max Memory -->
          <div class="space-y-2 mt-4">
            <div class="flex items-center justify-between">
              <label class="text-sm font-medium">Max Memory (MB)</label>
              <span class="text-sm font-mono text-primary"
                >{{ settingsConfig.maxMemory }} MB</span
              >
            </div>
            <input
              v-model.number="settingsConfig.maxMemory"
              type="range"
              min="512"
              :max="systemMemory.totalMb"
              step="512"
              class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-blue-500"
            />
            <div class="flex justify-between text-xs text-muted-foreground">
              <span>512 MB</span>
              <span>System: {{ systemMemory.totalMb }} MB</span>
            </div>
            <p class="text-xs text-muted-foreground">
              Recommended: {{ systemMemory.recommendedMaxMb }} MB (1/3 of
              system RAM)
            </p>
          </div>

          <!-- Extra JVM Args -->
          <div class="space-y-2 mt-4">
            <label class="text-sm font-medium"
              >Extra JVM Arguments (advanced)</label
            >
            <textarea
              v-model="settingsConfig.jvmArgsExtra"
              placeholder="-XX:+UseG1GC&#10;-XX:+ParallelGCThreads=4"
              class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm font-mono text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 h-20 resize-none"
            />
          </div>

          <!-- Window Behavior -->
          <div class="space-y-2 mt-4">
            <label class="text-sm font-medium">Window Behavior</label>
            <select
              v-model="settingsConfig.windowBehavior"
              class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm text-neutral-900 dark:text-white"
            >
              <option value="keep">Keep visible (default)</option>
              <option value="hide">Hide launcher</option>
              <option value="minimize">Minimize to taskbar</option>
            </select>
            <p class="text-xs text-muted-foreground">
              Choose what happens to the launcher window when the game starts.
            </p>
          </div>

          <!-- Show Game Log -->
          <div class="flex items-center gap-3 mt-4 p-4 border rounded-lg">
            <input
              type="checkbox"
              id="showGameLog"
              v-model="settingsConfig.showGameLog"
              class="w-5 h-5 rounded border-gray-300 text-primary focus:ring-primary"
            />
            <label for="showGameLog" class="flex-1">
              <span class="font-medium">Show Game Log on Launch</span>
              <p class="text-sm text-muted-foreground">
                Enable to automatically show the console log window when starting the instance.
              </p>
            </label>
          </div>

          <!-- Save Button -->
          <div class="flex justify-end gap-2 mt-6">
            <button
              @click="showSettingsModal = false"
              class="px-3 py-1.5 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
            >
              Cancel
            </button>
            <button
              @click="saveSettings"
              :disabled="isSavingConfig"
              class="flex items-center gap-2 px-3 py-1.5 text-sm font-medium bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 transition-colors"
            >
              <Save class="h-4 w-4" />
              Save
            </button>
          </div>
      </DialogContent>

    <!-- Delete Confirmation Dialog -->
    <AlertDialog
      :open="showDeleteDialog"
      @update:open="showDeleteDialog = $event"
    >
      <AlertDialogTitle class="text-xl font-semibold text-neutral-900 dark:text-white">Delete Instance?</AlertDialogTitle>
      <AlertDialogDescription class="mt-2 text-sm text-neutral-600 dark:text-neutral-400">
        Are you sure you want to delete
        <strong class="text-neutral-900 dark:text-white">{{ deletingInstanceName }}</strong
        >? This will remove all instance data including saves, mods, and
        resource packs.
        <span class="block mt-2 text-red-600 dark:text-red-500 font-medium">This action cannot be undone.</span>
      </AlertDialogDescription>
      <div class="flex justify-end gap-3 mt-6">
        <button
          @click="showDeleteDialog = false"
          class="px-3 py-1.5 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
          :disabled="isDeletingInstance"
        >
          Cancel
        </button>
        <button
          @click="deleteInstance"
          :disabled="isDeletingInstance"
          class="flex items-center gap-2 px-3 py-1.5 text-sm font-medium bg-red-600 dark:bg-red-700 text-white rounded-md hover:bg-red-700 dark:hover:bg-red-600 disabled:opacity-50 transition-colors"
        >
          <Trash2 v-if="isDeletingInstance" class="h-4 w-4 animate-spin" />
          <Trash2 v-else class="h-4 w-4" />
          Delete
        </button>
      </div>
    </AlertDialog>
  </div>
</template>
