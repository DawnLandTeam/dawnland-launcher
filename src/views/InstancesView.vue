<script setup lang="ts">
import { ref, onMounted, watch, onActivated, onUnmounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { invoke } from "@tauri-apps/api/core";
import { Gamepad2, Plus, Package, Trash2, Share2, Check } from "@lucide/vue";
import { getErrorMessage } from "../utils/error";
import { useTaskStatusReload } from "../composables/useTaskStatusReload";
import { useInstances } from "../composables/useInstances";

import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../components/ui/alert-dialog";
import { trackEvent } from "../utils/analytics";

// Types
interface InstanceItem {
  id: string;
  name: string;
  mcVersion: string;
  loaderType: string;
  modpackVersion?: string;
  modpackType?: string;
  modpackProjectId?: string;
  isInstalling?: boolean;
  isUpdating?: boolean;
}

// Router — deep-link support

import { toast } from '../composables/useToast';

const route = useRoute();
const router = useRouter();
useI18n();

// State
const { instances: installedInstances, fetchInstances: loadInstances } = useInstances();
const copiedShareInstanceId = ref<string | null>(null);

// Delete confirmation state
const showDeleteDialog = ref(false);
const deletingInstanceId = ref("");
const deletingInstanceName = ref("");
const isDeletingInstance = ref(false);
const hasDataToDelete = ref(false);

const openDropdownId = ref<string | null>(null);

// ---------------------------------------------------------------------------
// Deep-link: route.query.manage → auto-open settings for a specific instance
// ---------------------------------------------------------------------------
const openSettingsForInstance = async (instanceId: string) => {
  const instance = installedInstances.value.find((i) => i.id === instanceId);
  if (!instance) {
    console.warn(`Instance "${instanceId}" not found — cannot open settings`);
    return;
  }
  router.push('/instances/' + instance.id);
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
      const newQuery = { ...query, tab: 'instance' };
      router.replace({ path: '/downloads', query: newQuery });
    }
  },
  { immediate: true },
);

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
const handleTaskAdded = () => {
  loadInstances();
};

useTaskStatusReload(loadInstances);

onMounted(async () => {
  trackEvent("Instances Viewed");
  window.addEventListener('task-added', handleTaskAdded);
  await loadInstances();
});

onActivated(async () => {
  await loadInstances();
});

onUnmounted(() => {
  window.removeEventListener('task-added', handleTaskAdded);
});

// ---------------------------------------------------------------------------
// Data loading
// Data loading
// ---------------------------------------------------------------------------
// `loadInstances` is now provided by useInstances composable

async function refreshInstancesList() {
  await loadInstances(true);
}

// ---------------------------------------------------------------------------
// Settings modal
// ---------------------------------------------------------------------------

function openSettings(instance: InstanceItem) {
  router.push('/instances/' + instance.id);
}

// ---------------------------------------------------------------------------
// Instance management actions
// ---------------------------------------------------------------------------
async function shareModpack(instance: InstanceItem) {
  if (!instance.modpackProjectId || !instance.modpackVersion || !instance.modpackType) return;
  const rawLink = `dlml://modpack/install?id=${encodeURIComponent(instance.modpackProjectId)}&source=${encodeURIComponent(instance.modpackType.toLowerCase())}&version_id=${encodeURIComponent(instance.modpackVersion)}&name=${encodeURIComponent(instance.name)}`;
  const backendUrl = import.meta.env.VITE_WEB_BACKEND_URL || 'https://api.dawnland.cn';
  const b64 = btoa(unescape(encodeURIComponent(rawLink)));
  const link = `${backendUrl}/link?b64=${b64}`;
  
  try {
    await navigator.clipboard.writeText(link);
    copiedShareInstanceId.value = instance.id;
    setTimeout(() => {
      copiedShareInstanceId.value = null;
    }, 2000);
  } catch (err) {
    console.error("Failed to write to clipboard:", err);
    toast.error('无法复制链接到剪贴板，请检查浏览器权限。');
  }
}
async function confirmDeleteInstance(instance: InstanceItem) {
  deletingInstanceId.value = instance.id;
  deletingInstanceName.value = instance.name;
  try {
    hasDataToDelete.value = await invoke("check_instance_data", { versionId: instance.id });
  } catch (e) {
    hasDataToDelete.value = false;
  }
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
    alert(`Failed to delete: ${getErrorMessage(e)}`);
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

function normalizedModpackVersion(version: string): string {
  return version.toLowerCase().startsWith('v') ? version : `v${version}`;
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
        <h2 class="text-xl font-semibold">{{ $t('instances.noInstances') }}</h2>
        <p class="text-sm text-muted-foreground">
          {{ $t('instances.noInstancesDesc') }}
        </p>
      </div>
      <div class="flex items-center gap-3">
        <button
          @click="router.push('/downloads?tab=instance')"
          class="flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
        >
          <Plus class="h-4 w-4" />
          {{ $t('instances.installInstance') }}
        </button>
        <button
          @click="router.push('/downloads?tab=modpack')"
          class="flex items-center gap-2 rounded-md border px-4 py-2 text-sm font-medium hover:bg-accent hover:text-accent-foreground transition-colors"
        >
          <Package class="h-4 w-4" />
          {{ $t('instances.installModpack') }}
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
            @click="router.push('/downloads?tab=modpack')"
            class="flex items-center gap-2 rounded-md bg-secondary px-3 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/90 transition-colors"
          >
            <Package class="h-4 w-4" />
            {{ $t('instances.installModpack') }}
          </button>
          <button
            @click="router.push('/downloads?tab=instance')"
            class="flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            <Plus class="h-4 w-4" />
            {{ $t('instances.add') }}
          </button>
        </div>
      </div>

      <!-- Instance Grid -->
      <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5 gap-4">
        <div
          v-for="instance in installedInstances"
          :key="instance.id"
          @click="openSettings(instance)"
          class="group flex flex-col h-32 rounded-lg border border-white/20 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md p-4 hover:border-primary/50 hover:bg-white/80 dark:hover:bg-zinc-900/80 transition-all shadow-sm relative hover:z-50 focus-within:z-50 cursor-pointer"
          :class="openDropdownId === instance.id ? 'z-50' : ''"
        >
          <!-- Installing/Updating Overlay -->
          <div v-if="instance.isInstalling || instance.isUpdating" class="absolute inset-0 z-10 bg-white/50 dark:bg-black/50 backdrop-blur-[1px] flex items-center justify-center rounded-lg">
            <div class="bg-background/90 px-3 py-1.5 rounded-full flex items-center gap-2 shadow-sm border border-border">
              <Loader2 class="h-4 w-4 animate-spin text-primary" />
              <span class="text-xs font-medium">
                {{ instance.isUpdating ? $t('instances.updating', '更新中...') : $t('instances.installing', '正在安装...') }}
              </span>
            </div>
          </div>

          <!-- Instance info — primary visual focus -->
          <div class="flex items-start justify-between">
            <div class="min-w-0 flex items-center gap-3 flex-1">
              <Package class="h-5 w-5 shrink-0 text-muted-foreground" />
              <div class="min-w-0 flex-1 overflow-hidden">
                <h3 class="font-semibold line-clamp-2 break-words" :title="instance.name">{{ instance.name }}</h3>
                <div class="flex items-center gap-2 mt-2 flex-wrap overflow-hidden max-h-[40px]">
                    <span class="text-xs text-muted-foreground font-mono shrink-0">
                      {{ instance.mcVersion }}
                    </span>
                    <span
                      class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none shrink-0"
                      :class="loaderBadgeClass(instance.loaderType)"
                    >
                      {{ instance.loaderType }}
                    </span>
                    <span
                      v-if="instance.modpackType"
                      class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300 shrink-0"
                    >
                      {{ instance.modpackType }}
                    </span>
                    <span
                      v-if="instance.modpackVersion"
                      class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold leading-none bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-400 shrink-0"
                    >
                      {{ normalizedModpackVersion(instance.modpackVersion) }}
                    </span>
                  </div>
                </div>
            </div>
            <div class="flex items-center gap-1 shrink-0 ml-2 relative z-20">
              <button 
                v-if="instance.modpackType && instance.modpackProjectId && instance.modpackVersion"
                @click.stop="shareModpack(instance)"
                class="p-1.5 rounded-md hover:bg-primary/10 text-muted-foreground hover:text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                :disabled="instance.isInstalling || instance.isUpdating"
                :title="$t('instances.shareModpack', 'Share Modpack')"
              >
                <Check v-if="copiedShareInstanceId === instance.id" class="h-4 w-4 text-green-500" />
                <Share2 v-else class="h-4 w-4" />
              </button>
              <button
                @click.stop="confirmDeleteInstance(instance)"
                class="p-1.5 rounded-md hover:bg-red-500/10 text-muted-foreground hover:text-red-500 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                :disabled="instance.isInstalling || instance.isUpdating"
                :title="$t('instances.delete')"
              >
                <Trash2 class="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    

    <!-- Delete Confirmation Dialog -->
    <AlertDialog
      :open="showDeleteDialog"
      @update:open="showDeleteDialog = $event"
    >
      <AlertDialogTitle class="text-xl font-semibold text-neutral-900 dark:text-white">{{ $t('instances.settingsDialog.deleteTitle') }}</AlertDialogTitle>
      <AlertDialogDescription class="mt-2 text-sm text-neutral-600 dark:text-neutral-400">
        {{ $t('instances.settingsDialog.deleteDescPrefix') }}
        <strong class="text-neutral-900 dark:text-white">{{ deletingInstanceName }}</strong
        >{{ $t('instances.settingsDialog.deleteDescSuffix') }}
        <span class="block mt-2 text-red-600 dark:text-red-500 font-medium">{{ $t('instances.settingsDialog.deleteUndone') }}</span>
        <span v-if="hasDataToDelete" class="block mt-4 p-3 bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400 font-bold rounded border border-red-200 dark:border-red-800">
          ⚠️ {{ $t('instances.settingsDialog.deleteWarning') }}
        </span>
      </AlertDialogDescription>
      <div class="flex justify-end gap-3 mt-6">
        <button
          @click="showDeleteDialog = false"
          class="px-3 py-1.5 text-sm font-medium border rounded-md hover:bg-muted transition-colors"
          :disabled="isDeletingInstance"
        >
          {{ $t('common.cancel', 'Cancel') }}
        </button>
        <button
          @click="deleteInstance"
          :disabled="isDeletingInstance"
          class="flex items-center gap-2 px-3 py-1.5 text-sm font-medium bg-red-600 dark:bg-red-700 text-white rounded-md hover:bg-red-700 dark:hover:bg-red-600 disabled:opacity-50 transition-colors"
        >
          <Trash2 v-if="isDeletingInstance" class="h-4 w-4 animate-spin" />
          <Trash2 v-else class="h-4 w-4" />
          {{ $t('instances.delete') }}
        </button>
      </div>
    </AlertDialog>
  </div>
</template>
