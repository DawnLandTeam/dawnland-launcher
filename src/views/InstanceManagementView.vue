<script lang="ts">
export default {
  name: 'InstanceManagementView'
}
</script>

<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { Settings, Puzzle, Package, Box, Globe, Sparkles, FolderArchive, ArrowLeft, RefreshCw, Share2, Trash2, Check } from "@lucide/vue";
import DSidebarTabs from "../components/ui/DSidebarTabs.vue";
import DButton from "../components/ui/DButton.vue";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from '../components/ui/alert-dialog';
import { invoke } from "@tauri-apps/api/core";
import { getErrorMessage } from "../utils/error";
import { toast } from "../composables/useToast";
import { useInstances } from "../composables/useInstances";

// Import tabs
import InstanceSettingsTab from "../components/instances/InstanceSettingsTab.vue";
import InstanceModsTab from "../components/instances/InstanceModsTab.vue";
import InstanceDatapacksTab from "../components/instances/InstanceDatapacksTab.vue";
import InstanceResourcepacksTab from "../components/instances/InstanceResourcepacksTab.vue";
import InstanceWorldsTab from "../components/instances/InstanceWorldsTab.vue";
import InstanceShadersTab from "../components/instances/InstanceShadersTab.vue";
import InstanceModGroupTab from "../components/instances/InstanceModGroupTab.vue";
import InstanceCustomShadersTab from "../components/instances/InstanceCustomShadersTab.vue";
import InstanceCustomResourcepacksTab from "../components/instances/InstanceCustomResourcepacksTab.vue";
import ModpackInstallTab from "../components/downloads/ModpackInstallTab.vue";

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const { instances, fetchInstances } = useInstances();

const instanceId = computed(() => route.params.id as string);
const currentInstance = computed(() => instances.value.find(i => i.id === instanceId.value));

const tabs = [
  { id: 'settings', name: 'instances.settings', icon: Settings }, // ungrouped
  { id: 'mods', name: 'instances.mods', icon: Puzzle, group: 'downloadsCenter.groups.game' },
  { id: 'resourcepacks', name: 'instances.resourcepacks', icon: Box, group: 'downloadsCenter.groups.game' },
  { id: 'worlds', name: 'downloadsCenter.tabs.world', icon: Globe, group: 'downloadsCenter.groups.game' },
  { id: 'datapacks', name: 'instances.datapacks', icon: Package, group: 'downloadsCenter.groups.game' },
  { id: 'shaders', name: 'downloadsCenter.tabs.shader', icon: Sparkles, group: 'downloadsCenter.groups.game' },
  
  { id: 'mod_group', name: 'instances.customModGroup', icon: FolderArchive, group: 'downloadsCenter.groups.custom' },
  { id: 'custom_shaders', name: 'instances.customShaders', icon: Sparkles, group: 'downloadsCenter.groups.custom' },
  { id: 'custom_resourcepacks', name: 'instances.customResourcepacks', icon: Box, group: 'downloadsCenter.groups.custom' },
];

const copiedShareInstanceId = ref<string | null>(null);
const showDeleteDialog = ref(false);
const deletingInstanceId = ref("");
const deletingInstanceName = ref("");
const isDeletingInstance = ref(false);
const hasDataToDelete = ref(false);

async function shareModpack() {
  const instance = currentInstance.value;
  if (!instance || !instance.modpackProjectId || !instance.modpackVersion || !instance.modpackType) return;
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

async function confirmDeleteInstance() {
  const instance = currentInstance.value;
  if (!instance) return;
  
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
    router.push('/instances');
  } catch (e) {
    console.error("Failed to delete instance:", e);
    alert(`Failed to delete: ${getErrorMessage(e)}`);
  } finally {
    isDeletingInstance.value = false;
    deletingInstanceId.value = "";
    deletingInstanceName.value = "";
  }
}

const translatedTabs = computed(() => {
  if (!currentInstance.value) return [];

  const result = tabs.map(tab => ({
    ...tab,
    name: t(tab.name),
    group: tab.group ? t(tab.group) : undefined,
    disabled: tab.id !== 'settings' && (currentInstance.value?.isInstalling || currentInstance.value?.isUpdating)
  }));
  
  const additionalTabs = [];
  
  if (currentInstance.value?.modpackType) {
    additionalTabs.push({
      id: 'update_modpack',
      name: t('install.updateModpackTitle', 'Update Modpack'),
      icon: RefreshCw,
      action: () => { activeTab.value = 'update_modpack'; },
      disabled: currentInstance.value?.isInstalling || currentInstance.value?.isUpdating
    });
    
    additionalTabs.push({
      id: 'share_modpack',
      name: t('instances.shareModpack', 'Share Modpack'),
      icon: copiedShareInstanceId.value === currentInstance.value.id ? Check : Share2,
      action: shareModpack,
      disabled: currentInstance.value?.isInstalling || currentInstance.value?.isUpdating
    });
  }
  
  additionalTabs.push({
    id: 'delete_instance',
    name: t('instances.delete', 'Delete'),
    icon: Trash2,
    action: confirmDeleteInstance,
    disabled: currentInstance.value?.isInstalling || currentInstance.value?.isUpdating
  });
  
  result.splice(1, 0, ...additionalTabs as any);
  
  return result;
});

const activeTab = ref('settings');

watch(() => route.query.tab, (newTab) => {
  if (newTab && typeof newTab === 'string' && tabs.some(t => t.id === newTab)) {
    activeTab.value = newTab;
  }
}, { immediate: true });

function switchTab(tabId: string) {
  activeTab.value = tabId;
  router.replace({ query: { ...route.query, tab: tabId } });
}

watch(instanceId, async (newId) => {
  if (!newId) return;
  try {
    await fetchInstances();
  } catch (e) {
    console.error("Failed to load instance name:", getErrorMessage(e));
  }
}, { immediate: true });
</script>

<template>
  <div class="flex h-full p-4 gap-4 bg-transparent">
    <div v-if="!currentInstance" class="flex-1 flex items-center justify-center">
      <div class="w-8 h-8 border-4 border-primary border-t-transparent rounded-full animate-spin"></div>
    </div>
    
    <template v-else>
      <!-- Left Sidebar Area -->
      <div class="flex flex-col gap-2">
        <button
            @click="router.push('/instances')"
            class="flex items-center gap-2 rounded-xl bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md px-4 py-3 text-sm font-medium hover:bg-white/80 dark:hover:bg-zinc-900/80 transition-colors shadow-sm border border-neutral-200/50 dark:border-zinc-800/50 w-56 text-left justify-start text-neutral-600 dark:text-neutral-400"
        >
          <ArrowLeft class="h-4 w-4 shrink-0" />
          {{ t('common.back', '返回') }}
        </button>
        
        <div class="flex-1 min-h-0 overflow-y-auto">
          <DSidebarTabs
            :tabs="translatedTabs"
            :modelValue="activeTab"
            @update:modelValue="switchTab"
          />
        </div>
      </div>

      <!-- Right Content Area -->
      <div class="flex-1 relative bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md rounded-xl border border-neutral-200/50 dark:border-zinc-800/50 shadow-sm overflow-hidden flex flex-col min-w-0">
        <keep-alive>
          <InstanceSettingsTab v-if="activeTab === 'settings'" :instance-id="instanceId" :instance="currentInstance" />
          <InstanceModsTab v-else-if="activeTab === 'mods'" :instance-id="instanceId" />
          <InstanceDatapacksTab v-else-if="activeTab === 'datapacks'" :instance-id="instanceId" />
          <InstanceResourcepacksTab v-else-if="activeTab === 'resourcepacks'" :instance-id="instanceId" />
          <InstanceWorldsTab v-else-if="activeTab === 'worlds'" :instance-id="instanceId" />
          <InstanceShadersTab v-else-if="activeTab === 'shaders'" :instance-id="instanceId" />
          <InstanceModGroupTab v-else-if="activeTab === 'mod_group'" :instance-id="instanceId" />
          <InstanceCustomShadersTab v-else-if="activeTab === 'custom_shaders'" :instance-id="instanceId" />
          <InstanceCustomResourcepacksTab v-else-if="activeTab === 'custom_resourcepacks'" :instance-id="instanceId" />
          <ModpackInstallTab 
            v-else-if="activeTab === 'update_modpack'"
            :is-modal-update="true"
            :update-id="currentInstance.name"
            :update-project-id="currentInstance.modpackProjectId"
            :update-source="currentInstance.modpackType.toLowerCase()"
            :update-current-version="currentInstance.modpackVersion"
            @cancel-update="activeTab = 'settings'"
          />
        </keep-alive>
      </div>

      <!-- Delete Confirmation Dialog -->
      <AlertDialog :open="showDeleteDialog" @update:open="showDeleteDialog = $event">
        <div class="fixed inset-0 z-50 bg-black/80 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0" v-if="showDeleteDialog"></div>
        <div class="fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 border bg-background p-6 shadow-lg duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 sm:rounded-lg" v-if="showDeleteDialog">
          <AlertDialogTitle class="text-xl font-semibold text-neutral-900 dark:text-white">{{ $t('instances.settingsDialog.deleteTitle') }}</AlertDialogTitle>
          <AlertDialogDescription class="text-sm text-neutral-500 dark:text-zinc-400">
            {{ $t('instances.settingsDialog.deleteDescPrefix') }}
            <strong class="text-neutral-900 dark:text-white font-semibold">{{ deletingInstanceName }}</strong>
            {{ $t('instances.settingsDialog.deleteDescSuffix') }}
            <span class="block mt-2 text-red-600 dark:text-red-500 font-medium">{{ $t('instances.settingsDialog.deleteUndone') }}</span>
            
            <span v-if="hasDataToDelete" class="block mt-4 p-3 bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400 font-bold rounded border border-red-200 dark:border-red-800">
              ⚠️ {{ $t('instances.settingsDialog.deleteWarning') }}
            </span>
          </AlertDialogDescription>
          
          <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2 mt-4">
            <DButton variant="outline" @click="showDeleteDialog = false">{{ $t('common.cancel', '取消') }}</DButton>
            <DButton variant="danger" @click="deleteInstance" :disabled="isDeletingInstance">
               <template v-if="isDeletingInstance">
                 <div class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin mr-2"></div>
               </template>
               {{ $t('instances.delete', '删除') }}
            </DButton>
          </div>
        </div>
      </AlertDialog>
    </template>
  </div>
</template>
