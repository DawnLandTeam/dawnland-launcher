<script lang="ts">
export default {
  name: 'InstanceManagementView'
}
</script>

<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { Settings, Puzzle, Package, Box, Globe, Sparkles, FolderArchive, ArrowLeft, RefreshCw, Share2, AlertTriangle, BarChart, Pencil } from "@lucide/vue";
import DSidebarTabs from "../components/ui/DSidebarTabs.vue";



import { getErrorMessage } from "../utils/error";

import { useInstances } from "../composables/useInstances";
import { useInstanceIconUpdates } from "../composables/useInstanceIconUpdates";
import InstanceIcon from "../components/InstanceIcon.vue";

import InstanceSettingsTab from "../components/instances/InstanceSettingsTab.vue";
import InstanceExportModal from "../components/instances/InstanceExportModal.vue";
import InstanceRenameModal from "../components/instances/InstanceRenameModal.vue";
import InstanceModsTab from "../components/instances/InstanceModsTab.vue";
import InstanceDatapacksTab from "../components/instances/InstanceDatapacksTab.vue";
import InstanceResourcepacksTab from "../components/instances/InstanceResourcepacksTab.vue";
import InstanceWorldsTab from "../components/instances/InstanceWorldsTab.vue";
import InstanceShadersTab from "../components/instances/InstanceShadersTab.vue";
import InstanceModGroupTab from "../components/instances/InstanceModGroupTab.vue";
import InstanceCustomShadersTab from "../components/instances/InstanceCustomShadersTab.vue";
import InstanceCustomResourcepacksTab from "../components/instances/InstanceCustomResourcepacksTab.vue";
import InstanceStatisticsTab from "../components/instances/InstanceStatisticsTab.vue";
import ModpackInstallTab from "../components/downloads/ModpackInstallTab.vue";
import CrashHistoryPanel from "../components/CrashHistoryPanel.vue";
import { isInstanceBusy } from "../types";
import { useTaskStore } from "../composables/useTaskStore";
import { useModpackInstallState } from "../composables/useModpackInstallState";
import { useTaskStatusReload } from "../composables/useTaskStatusReload";
import { runningInstances, launchingInstances, jvmSpawnedInstances } from "../composables/useLaunchState";

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const { instances, fetchInstances } = useInstances();
const taskStore = useTaskStore();
const { hasOngoingInstanceUpdateTask: checkOngoingInstanceUpdateTask } = useModpackInstallState();

const instanceId = computed(() => route.params.id as string);
const currentInstance = computed(() => instances.value.find(i => i.id === instanceId.value));

const hasOngoingUpdateTask = computed(() => {
  if (!currentInstance.value) return false;
  return checkOngoingInstanceUpdateTask(taskStore.tasks.value, currentInstance.value.name);
});

// Patch icons cached in the background without rescanning all instances
useInstanceIconUpdates(({ instanceId: updatedId, iconPath }) => {
  const target = instances.value.find(i => i.id === updatedId);
  if (target) target.iconPath = iconPath;
});

// Auto-reload instance details when background tasks (e.g. loader/mod install) complete
useTaskStatusReload(async () => {
  await fetchInstances(true);
});

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
  { id: 'crash_history', name: 'instances.crashHistory', icon: AlertTriangle },
  { id: 'statistics', name: 'instances.statistics', icon: BarChart },
];

const showExportModal = ref(false);
const showRenameModal = ref(false);

const isRunning = computed(() => {
  if (!instanceId.value) return false;
  const idLower = instanceId.value.toLowerCase();
  for (const id of runningInstances.value) {
    if (id.toLowerCase() === idLower) return true;
  }
  for (const id of launchingInstances.value) {
    if (id.toLowerCase() === idLower) return true;
  }
  for (const id of jvmSpawnedInstances.value) {
    if (id.toLowerCase() === idLower) return true;
  }
  return currentInstance.value?.status === 'running';
});

const canRenameInstance = computed(() => {
  if (!currentInstance.value) return false;
  return !isInstanceBusy(currentInstance.value) && !isRunning.value;
});

function handleInstanceRenamed(payload: { oldId: string; newId: string }) {
  router.replace({
    name: 'instance-management',
    params: { id: payload.newId },
    query: route.query,
  });
}

const translatedTabs = computed(() => {
  if (!currentInstance.value) return [];

  const result = tabs.map(tab => ({
    ...tab,
    name: t(tab.name),
    group: tab.group ? t(tab.group) : undefined,
    disabled: tab.id !== 'settings' && isInstanceBusy(currentInstance.value)
  }));
  
  const additionalTabs = [];
  
  if (currentInstance.value?.modpackType) {
    const isUpdatingThisInstance = currentInstance.value?.status === 'updating' || hasOngoingUpdateTask.value;
    additionalTabs.push({
      id: 'update_modpack',
      name: t('install.updateModpackTitle'),
      icon: RefreshCw,
      hasDot: isUpdatingThisInstance,
      disabled: (isInstanceBusy(currentInstance.value) && !isUpdatingThisInstance) || isRunning.value
    });
  }
  
  result.splice(1, 0, ...additionalTabs as any);
  
  return result;
});

const validTabIds = computed(() => {
  const ids = tabs.map(t => t.id);
  ids.push('update_modpack');
  return ids;
});

const activeTab = ref('settings');
const previousTab = ref('settings');

watch(() => route.query.tab, (newTab) => {
  if (newTab && typeof newTab === 'string' && validTabIds.value.includes(newTab)) {
    if (activeTab.value !== 'update_modpack' && newTab === 'update_modpack') {
      previousTab.value = activeTab.value;
    }
    activeTab.value = newTab;
  }
}, { immediate: true });

function switchTab(tabId: string) {
  if (activeTab.value !== 'update_modpack' && tabId === 'update_modpack') {
    previousTab.value = activeTab.value;
  }
  activeTab.value = tabId;
  router.replace({ query: { ...route.query, tab: tabId } });
}

function handleCancelUpdate() {
  const targetTab = (previousTab.value && previousTab.value !== 'update_modpack')
    ? previousTab.value
    : 'settings';
  switchTab(targetTab);
}

watch(currentInstance, (inst) => {
  if (!inst) return;
  if (!route.query.tab && hasOngoingUpdateTask.value) {
    switchTab('update_modpack');
  }
}, { immediate: true });

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
      <div class="flex flex-col gap-2 w-56 shrink-0">
        <!-- Back + Instance Header (Combined to keep sidebar fits without vertical scrollbar) -->
        <div class="flex items-center gap-2.5 rounded-xl bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md px-2.5 py-2 shadow-sm border border-neutral-200/50 dark:border-zinc-800/50 w-full">
          <button
            @click="router.push('/instances')"
            :title="t('common.back')"
            class="p-1.5 rounded-lg hover:bg-neutral-200/60 dark:hover:bg-zinc-800/60 transition-colors text-neutral-600 dark:text-neutral-400 shrink-0"
          >
            <ArrowLeft class="h-4 w-4" />
          </button>
          <InstanceIcon
            :icon-path="currentInstance.iconPath"
            :alt="currentInstance.name"
            class="w-7 h-7 shrink-0 rounded-md"
          />
          <div class="min-w-0 flex-1 flex flex-col">
            <span class="font-bold text-xs truncate leading-tight" :title="currentInstance.name">{{ currentInstance.name }}</span>
            <span class="text-[10px] text-muted-foreground truncate leading-tight mt-0.5">{{ currentInstance.mcVersion }} · {{ currentInstance.loaderType }}{{ currentInstance.loaderVersion ? ` (${currentInstance.loaderVersion})` : '' }}</span>
          </div>
          <button
            @click="showRenameModal = true"
            :disabled="!canRenameInstance"
            :title="t('instances.rename', 'Rename')"
            class="p-1.5 rounded-lg hover:bg-neutral-200/60 dark:hover:bg-zinc-800/60 transition-colors text-neutral-600 dark:text-neutral-400 hover:text-primary disabled:opacity-40 disabled:cursor-not-allowed shrink-0"
          >
            <Pencil class="h-3.5 w-3.5" />
          </button>
        </div>
        
        <div class="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
          <DSidebarTabs
            :tabs="translatedTabs"
            :modelValue="activeTab"
            @update:modelValue="switchTab"
            class="!w-full"
          />
        </div>

        <button
            @click="showExportModal = true"
            class="flex items-center gap-2 rounded-xl bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md px-4 py-3 text-sm font-medium hover:bg-emerald-50 dark:hover:bg-emerald-900/20 hover:text-emerald-600 dark:hover:text-emerald-400 transition-colors shadow-sm border border-neutral-200/50 dark:border-zinc-800/50 w-full text-left justify-start text-neutral-600 dark:text-neutral-400"
        >
          <Share2 class="h-4 w-4 shrink-0" />
          {{ t('instances.export.title', 'Export Modpack') }}
        </button>
      </div>

      <!-- Right Content Area -->
      <div class="flex-1 relative bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md rounded-xl border border-neutral-200/50 dark:border-zinc-800/50 shadow-sm overflow-hidden flex flex-col min-w-0">
        <keep-alive>
          <InstanceSettingsTab v-if="activeTab === 'settings'" :instance-id="instanceId" :instance="currentInstance" />
          <InstanceModsTab v-else-if="activeTab === 'mods'" :instance-id="instanceId" :instance="currentInstance" />
          <InstanceDatapacksTab v-else-if="activeTab === 'datapacks'" :instance-id="instanceId" />
          <InstanceResourcepacksTab v-else-if="activeTab === 'resourcepacks'" :instance-id="instanceId" />
          <InstanceWorldsTab v-else-if="activeTab === 'worlds'" :instance-id="instanceId" />
          <InstanceShadersTab v-else-if="activeTab === 'shaders'" :instance-id="instanceId" />
          <InstanceModGroupTab v-else-if="activeTab === 'mod_group'" :instance-id="instanceId" />
          <InstanceCustomShadersTab v-else-if="activeTab === 'custom_shaders'" :instance-id="instanceId" />
          <InstanceCustomResourcepacksTab v-else-if="activeTab === 'custom_resourcepacks'" :instance-id="instanceId" />
          <CrashHistoryPanel v-else-if="activeTab === 'crash_history'" :instance-id="instanceId" />
          <InstanceStatisticsTab v-else-if="activeTab === 'statistics'" :instance-id="instanceId" />
          <ModpackInstallTab 
            v-else-if="activeTab === 'update_modpack'"
            :is-modal-update="true"
            :update-id="currentInstance.name"
            :update-project-id="currentInstance.modpackProjectId"
            :update-source="currentInstance.modpackType?.toLowerCase() || ''"
            :update-current-version="currentInstance.modpackVersion"
            @cancel-update="handleCancelUpdate"
          />
        </keep-alive>
      </div>

      <InstanceExportModal
        :open="showExportModal"
        :instance-id="instanceId"
        @update:open="showExportModal = $event"
      />

      <InstanceRenameModal
        :open="showRenameModal"
        :instance-id="instanceId"
        :instance-name="currentInstance.name"
        @update:open="showRenameModal = $event"
        @renamed="handleInstanceRenamed"
      />
    </template>
  </div>
</template>
