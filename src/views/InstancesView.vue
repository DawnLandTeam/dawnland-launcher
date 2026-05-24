<script setup lang="ts">
import { ref, onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Gamepad2, Plus, Package, Settings, FolderOpen, Save, X, MoreHorizontal, Trash2, Folder } from "@lucide/vue";
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

interface InstanceConfig {
  javaPath?: string;
  maxMemory?: number;
  jvmArgsExtra?: string[];
  windowBehavior?: string;
}

interface SystemMemoryInfo {
  totalMb: number;
  recommendedMaxMb: number;
}

// Router — deep-link support
const route = useRoute();

// State
const showInstallModal = ref(false);
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
});
const isSavingConfig = ref(false);

// System memory for slider
const systemMemory = ref<SystemMemoryInfo>({
  totalMb: 8192,
  recommendedMaxMb: 4096,
});

// Delete confirmation state
const showDeleteDialog = ref(false);
const deletingInstanceId = ref("");
const deletingInstanceName = ref("");
const isDeletingInstance = ref(false);

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
// Lifecycle
// ---------------------------------------------------------------------------
onMounted(async () => {
  await loadInstances();
  await loadSystemMemory();
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
    };
  } catch (e) {
    console.error("Failed to load instance config:", e);
    settingsConfig.value = {
      javaPath: "",
      maxMemory: 4096,
      jvmArgsExtra: [],
      windowBehavior: "keep",
    };
  }

  showSettingsModal.value = true;
}

async function browseJavaPath() {
  try {
    const selected = await open({
      multiple: false,
      title: "Select Java Executable",
      filters: [
        {
          name: "Executable",
          extensions: ["exe", "app", ""],
        },
      ],
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
      jvmArgsExtra: settingsConfig.value.jvmArgsExtra?.length
        ? settingsConfig.value.jvmArgsExtra
        : null,
      windowBehavior: settingsConfig.value.windowBehavior || "keep",
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
      class="flex flex-1 flex-col items-center justify-center gap-4 p-6"
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
              {{ installedInstances.length }} instance{{
                installedInstances.length !== 1 ? "s" : ""
              }}
              installed
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

      <!-- Instance Grid -->
      <div class="grid grid-cols-3 gap-4">
        <div
          v-for="instance in installedInstances"
          :key="instance.id"
          class="group rounded-lg border bg-card p-4 hover:border-primary/50 transition-colors"
        >
          <!-- Instance info — primary visual focus -->
          <div class="flex items-start justify-between">
            <div class="space-y-1.5 min-w-0 flex items-center gap-2">
              <Package class="h-5 w-5 shrink-0 text-muted-foreground" />
              <div>
                <h3 class="font-semibold truncate">{{ instance.name }}</h3>
                <div class="flex items-center gap-2">
                  <span class="text-xs text-muted-foreground font-mono">
                    {{ instance.mcVersion }}
                  </span>
                  <span
                    class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none"
                    :class="loaderBadgeClass(instance.loaderType)"
                  >
                    {{ instance.loaderType }}
                  </span>
                </div>
              </div>
            </div>
          </div>

          <!-- Management actions -->
          <div class="mt-3 flex justify-end">
            <DropdownMenu>
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
                Settings
              </DropdownMenuItem>
              <DropdownMenuItem
                @click="openInstanceFolder(instance.id)"
              >
                <Folder class="h-4 w-4" />
                Open Folder
              </DropdownMenuItem>
              <DropdownMenuItem
                destructive
                @click="confirmDeleteInstance(instance)"
              >
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

    <!-- Instance Settings Modal -->
    <Teleport to="body">
      <div
        v-if="showSettingsModal"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <div
          class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"
          @click="showSettingsModal = false"
        />
        <div
          class="relative z-10 w-full max-w-md gap-4 border bg-white dark:bg-zinc-900 p-5 shadow-xl rounded-lg pointer-events-auto"
        >
          <div class="flex items-center justify-between mb-4">
            <div>
              <h3 class="font-semibold text-lg">Instance Settings</h3>
              <p class="text-sm text-muted-foreground">
                {{ settingsInstanceName }}
              </p>
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
                class="flex-1 px-3 py-2 bg-white dark:bg-zinc-800 border rounded-md text-sm"
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
              MC 1.20.5+ requires Java 21. Leave empty to use system default
              "java".
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

    <!-- Delete Confirmation Dialog -->
    <AlertDialog
      :open="showDeleteDialog"
      @update:open="showDeleteDialog = $event"
    >
      <AlertDialogTitle>Delete Instance?</AlertDialogTitle>
      <AlertDialogDescription class="mt-2">
        Are you sure you want to delete
        <strong>{{ deletingInstanceName }}</strong
        >? This will remove all instance data including saves, mods, and
        resource packs.
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
