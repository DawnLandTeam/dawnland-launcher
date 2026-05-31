<script setup lang="ts">
import { ref, computed, nextTick, onMounted } from "vue";
import { useRouter, useRoute } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "vue-i18n";
import { Package, UploadCloud, Loader2 } from "@lucide/vue";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../components/ui/alert-dialog";

const { t } = useI18n();
const router = useRouter();
const route = useRoute();

const isUpdate = computed(() => !!route.query.update_id);

const zipPath = ref("");
const instanceName = ref("");
const isInstalling = ref(false);
const showSuccessModal = ref(false);

onMounted(() => {
  if (route.query.update_id && route.query.zip) {
    instanceName.value = route.query.update_id as string;
    zipPath.value = route.query.zip as string;
  }
});

const currentPhase = ref("");
const statusMessage = ref("");
const completedMods = ref(new Set<string>());
const totalMods = ref(0);
const currentFile = ref("");
const forgeLogs = ref<string[]>([]);
const logContainer = ref<HTMLElement | null>(null);
listen("modpack-install-status", (e: any) => {
  if (e.payload.phase === "downloading_mods" && currentPhase.value !== "downloading_mods") {
    completedMods.value.clear();
    forgeLogs.value = [];
  }
  currentPhase.value = e.payload.phase;
  if (e.payload.message) {
    statusMessage.value = e.payload.message;
  }
  if (e.payload.totalTasks) {
    totalMods.value = e.payload.totalTasks;
  }
  if (e.payload.phase === "complete") {
    totalMods.value = 0;
    forgeLogs.value = [];
    currentFile.value = "";
  }
});

listen("install-progress", (e: any) => {
  // Always update phase if it's explicitly emitted by forge installer
  if (e.payload.phase) {
    currentPhase.value = e.payload.phase;
  }

  if (e.payload.phase === "downloading" && e.payload.totalTasks) {
    totalMods.value = e.payload.totalTasks;
    completedMods.value.clear();
  } else if (e.payload.phase === "running_processors") {
    totalMods.value = 0;
  } else if (e.payload.phase === "complete") {
    totalMods.value = 0;
    forgeLogs.value = [];
  }

  if (e.payload.currentFile) {
    if (e.payload.phase === "running_processors") {
      statusMessage.value = "Running Forge Installer...";
      forgeLogs.value.push(e.payload.currentFile);
      if (forgeLogs.value.length > 500) {
        forgeLogs.value.shift();
      }
      nextTick(() => {
        if (logContainer.value) {
          logContainer.value.scrollTop = logContainer.value.scrollHeight;
        }
      });
    } else {
      statusMessage.value = "Installing dependency: " + e.payload.currentFile;
    }
  } else if (e.payload.phase && e.payload.phase !== "complete" && e.payload.phase !== "running_processors") {
    statusMessage.value = "Installing dependency...";
  }
});

listen("download-batch-complete", () => {
  // Batch download completed
  statusMessage.value = "Mod downloading completed";
});

const activeDownloads = ref(new Map<string, any>());

listen("download-progress", (e: any) => {
  const p = e.payload;
  if (p.taskId) {
    const parts = p.taskId.split(/[/\\]/);
    currentFile.value = parts[parts.length - 1] || "file";
    
    if (p.completed || p.error) {
      activeDownloads.value.delete(p.taskId);
      if (p.completed) completedMods.value.add(p.taskId);
    } else {
      activeDownloads.value.set(p.taskId, p);
    }
  }
});

const totalSpeed = computed(() => {
  let sum = 0;
  for (const [_, p] of activeDownloads.value) {
    sum += p.speed || 0;
  }
  return sum;
});

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec === 0) return "";
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
}

const progressPercent = computed(() => {
  if (totalMods.value === 0) return 0;
  return Math.min(100, Math.round((completedMods.value.size / totalMods.value) * 100));
});

const selectZip = async () => {
  const selected = await open({
    multiple: false,
    filters: [
      {
        name: "Modpack Archives",
        extensions: ["zip", "mrpack"],
      },
    ],
  });
  if (selected && typeof selected === "string") {
    zipPath.value = selected;
    
    try {
      const manifestName = await invoke("get_modpack_name", { zipPath: selected });
      if (manifestName) {
        // Sanitize the name for folder usage (remove illegal chars)
        instanceName.value = String(manifestName).replace(/[<>:"/\\|?*]/g, "").trim();
      }
    } catch (e) {
      console.warn("Failed to read modpack name from manifest:", e);
      // Fallback to filename
      const match = selected.match(/[\\\/]([^\\\/]+)\.(zip|mrpack)$/i);
      if (match) {
        instanceName.value = match[1];
      }
    }
  }
};

const installModpack = async () => {
  if (!zipPath.value || !instanceName.value) return;

  isInstalling.value = true;
  completedMods.value.clear();
  forgeLogs.value = [];
  totalMods.value = 0;
  currentPhase.value = "starting";
  statusMessage.value = "Starting installation...";

  try {
    console.log("Invoking install_modpack...");
    await invoke("install_modpack", {
      zipPath: zipPath.value,
      instanceName: instanceName.value,
      isUpdate: isUpdate.value,
    });
    
    console.log("invoke install_modpack finished successfully. Showing success modal...");
    // Finished successfully
    isInstalling.value = false;
    showSuccessModal.value = true;
  } catch (error) {
    console.error("Installation failed:", error);
    statusMessage.value = `Installation failed: ${error}`;
    isInstalling.value = false;
  }
};

const handleSuccessConfirm = () => {
  showSuccessModal.value = false;
  router.push("/instances");
};
</script>

<template>
  <div class="h-full flex flex-col max-w-2xl mx-auto py-10 px-6">
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-gray-900 dark:text-white flex items-center gap-3">
        <Package class="w-8 h-8 text-emerald-600" />
        {{ isUpdate ? t('install.updateModpackTitle', 'Update Modpack') : t('install.modpackTitle', 'Install Modpack') }}
      </h1>
      <p class="text-gray-500 dark:text-gray-400 mt-2">
        {{ isUpdate ? t('install.updateModpackDesc', 'Update your instance using a newer modpack archive.') : t('install.modpackDesc', 'Install a CurseForge (.zip) or Modrinth (.mrpack) modpack from your local computer.') }}
      </p>
    </div>

    <!-- Upload Zone -->
    <div
      v-if="!isInstalling"
      class="border-2 border-dashed border-gray-300 dark:border-gray-700 rounded-xl p-10 flex flex-col items-center justify-center text-center cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors"
      @click="selectZip"
    >
      <UploadCloud class="w-12 h-12 text-gray-400 mb-4" />
      <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-1">
        {{ zipPath ? t('install.fileSelected', 'File Selected') : t('install.selectFile', 'Select Modpack Archive') }}
      </h3>
      <p class="text-sm text-gray-500 max-w-sm overflow-hidden text-ellipsis whitespace-nowrap">
        {{ zipPath || t('install.supportedFormats', 'Supports .zip and .mrpack formats') }}
      </p>
    </div>

    <!-- Form -->
    <div v-if="!isInstalling" class="mt-8 space-y-6">
      <div>
        <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          {{ t('install.instanceName', 'Instance Name') }}
        </label>
        <input
          v-model="instanceName"
          type="text"
          :disabled="isUpdate"
          class="w-full px-4 py-2 bg-white dark:bg-gray-900 border border-gray-300 dark:border-gray-700 rounded-lg focus:ring-2 focus:ring-emerald-500 outline-none disabled:opacity-60 disabled:cursor-not-allowed"
          :placeholder="t('install.instanceNamePlaceholder', 'My Awesome Modpack')"
        />
      </div>

      <div v-if="isUpdate" class="p-4 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800/50 rounded-lg flex items-start gap-3">
        <div class="mt-0.5 text-yellow-600 dark:text-yellow-400 font-bold">!</div>
        <div>
          <h4 class="text-sm font-semibold text-yellow-800 dark:text-yellow-300">Update Notice</h4>
          <p class="text-sm text-yellow-700 dark:text-yellow-400/80 mt-1">
            Updating will automatically clean up outdated modpack mods and apply the new ones. Don't worry, your manually installed mods will be preserved.
          </p>
        </div>
      </div>

      <div class="flex justify-end gap-3 pt-4">
        <button
          class="px-5 py-2 rounded-lg text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
          @click="router.back()"
        >
          {{ t('common.cancel', 'Cancel') }}
        </button>
        <button
          class="px-5 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg font-medium transition-colors disabled:opacity-50"
          :disabled="!zipPath || !instanceName"
          @click="installModpack"
        >
          {{ t('install.installButton', 'Install') }}
        </button>
      </div>
    </div>

    <!-- Progress State -->
    <div v-else class="mt-10 flex flex-col items-center justify-center py-10">
      <Loader2 class="w-12 h-12 text-emerald-600 animate-spin mb-6" />
      <h3 class="text-xl font-medium text-gray-900 dark:text-white mb-2">
        {{ statusMessage }}
      </h3>
      
      <p class="text-sm text-gray-500 mb-8 capitalize">
        {{ currentPhase.replace(/_/g, ' ') }}
      </p>

      <!-- Progress bar for mod downloads -->
      <div v-if="totalMods > 0" class="w-full max-w-md">
        <div class="flex justify-between text-sm mb-1">
          <span class="text-gray-600 dark:text-gray-400">
            {{ completedMods.size }} / {{ totalMods }} files
          </span>
          <span class="font-medium text-emerald-600">
            {{ progressPercent }}%
            <span v-if="totalSpeed > 0" class="text-gray-500 text-xs ml-2 font-normal">{{ formatSpeed(totalSpeed) }}</span>
          </span>
        </div>
        <div class="w-full bg-gray-200 dark:bg-gray-800 rounded-full h-2.5 overflow-hidden mb-2">
          <div 
            class="bg-emerald-600 h-2.5 rounded-full transition-all duration-300" 
            :style="{ width: `${progressPercent}%` }"
          ></div>
        </div>
        <div class="text-xs text-gray-500 truncate text-center" v-if="currentFile">
          Downloading: {{ currentFile }}
        </div>
      </div>

      <!-- Forge Log Box -->
      <div v-if="forgeLogs.length > 0" class="w-full max-w-2xl mt-8 bg-gray-950 rounded-xl p-4 h-64 overflow-y-auto border border-gray-800 shadow-inner" ref="logContainer">
        <div class="text-xs text-emerald-400 font-mono space-y-1">
          <div v-for="(log, idx) in forgeLogs" :key="idx" class="break-words">
            {{ log }}
          </div>
        </div>
      </div>
    </div>
    
    <AlertDialog :open="showSuccessModal" @update:open="val => { if (!val) handleSuccessConfirm() }">
      <div class="p-2">
        <AlertDialogTitle class="text-xl font-bold text-gray-900 dark:text-white mb-2">
          {{ t('install.successTitle', 'Installation Complete') }}
        </AlertDialogTitle>
        <AlertDialogDescription class="text-gray-600 dark:text-gray-300 mb-6">
          {{ t('install.successDesc', 'The modpack has been successfully installed. You can now find it in your Instances page.') }}
        </AlertDialogDescription>
        <div class="flex justify-end gap-3 mt-4">
          <button
            class="px-5 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg font-medium transition-colors"
            @click="handleSuccessConfirm"
          >
            {{ t('common.confirm', 'OK') }}
          </button>
        </div>
      </div>
    </AlertDialog>
  </div>
</template>
