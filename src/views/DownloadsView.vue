<script setup lang="ts">
import { ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { Box, Package, Puzzle, FileJson, Sparkles, Globe } from "@lucide/vue";
import InstanceInstallTab from "../components/downloads/InstanceInstallTab.vue";
import ModpackInstallTab from "../components/downloads/ModpackInstallTab.vue";
import ModsInstallTab from "../components/downloads/ModsInstallTab.vue";
import ResourcepackInstallTab from "../components/downloads/ResourcepackInstallTab.vue";
import ShaderpackInstallTab from "../components/downloads/ShaderpackInstallTab.vue";
import WorldInstallTab from "../components/downloads/WorldInstallTab.vue";
import DSidebarTabs from "../components/ui/DSidebarTabs.vue";
import { computed, onMounted } from "vue";
import { trackEvent } from "../utils/analytics";

const route = useRoute();
const router = useRouter();
const { t } = useI18n();

const tabs = [
  { id: 'instance', name: 'downloadsCenter.tabs.instance', icon: Box, group: 'downloadsCenter.groups.game' },
  { id: 'modpack', name: 'downloadsCenter.tabs.modpack', icon: Package, group: 'downloadsCenter.groups.game' },
  { id: 'mod', name: 'downloadsCenter.tabs.mod', icon: Puzzle, group: 'downloadsCenter.groups.resources' },
  { id: 'resourcepack', name: 'downloadsCenter.tabs.resourcepack', icon: FileJson, group: 'downloadsCenter.groups.resources' },
  { id: 'shader', name: 'downloadsCenter.tabs.shader', icon: Sparkles, group: 'downloadsCenter.groups.resources' },
  { id: 'world', name: 'downloadsCenter.tabs.world', icon: Globe, group: 'downloadsCenter.groups.resources' },
];

const translatedTabs = computed(() => tabs.map(tab => ({
  ...tab,
  name: t(tab.name),
  group: t(tab.group)
})));

const activeTab = ref('instance');

// Handle deep-link or routing
watch(
  () => route.query.tab,
  (newTab) => {
    if (newTab && typeof newTab === 'string' && tabs.some(t => t.id === newTab)) {
      activeTab.value = newTab;
    }
  },
  { immediate: true }
);

function switchTab(tabId: string) {
  activeTab.value = tabId;
  router.replace({ query: { tab: tabId } });
}

onMounted(() => {
  trackEvent("Downloads Viewed");
});
</script>

<template>
  <div class="flex h-full p-4 gap-4 bg-transparent">
    <!-- Left Sidebar -->
    <DSidebarTabs
      :title="t('downloadsCenter.title')"
      :tabs="translatedTabs"
      :modelValue="activeTab"
      @update:modelValue="switchTab"
    />

    <!-- Right Content Area -->
    <div class="flex-1 relative bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md rounded-xl border border-neutral-200/50 dark:border-zinc-800/50 shadow-sm overflow-hidden flex flex-col min-w-0">
      <keep-alive>
        <InstanceInstallTab v-if="activeTab === 'instance'" :initial-version="(route.query.install_version as string | undefined)" :initial-loader="(route.query.install_loader as string | undefined)" />
        <ModpackInstallTab v-else-if="activeTab === 'modpack'" />
        <ModsInstallTab v-else-if="activeTab === 'mod'" />
        <ResourcepackInstallTab v-else-if="activeTab === 'resourcepack'" />
        <ShaderpackInstallTab v-else-if="activeTab === 'shader'" />
        <WorldInstallTab v-else-if="activeTab === 'world'" />
      </keep-alive>
      
      <div v-if="!['instance', 'modpack', 'mod', 'resourcepack', 'shader', 'world'].includes(activeTab)" class="flex-1 flex flex-col items-center justify-center text-neutral-400">
        <component :is="tabs.find(t => t.id === activeTab)?.icon" class="w-12 h-12 mb-4 opacity-30" />
        <p class="text-lg font-medium text-neutral-500 dark:text-neutral-400">{{ t(tabs.find(t => t.id === activeTab)?.name || '') }} 下载模块</p>
        <p class="text-sm mt-2">施工中...</p>
      </div>
    </div>
  </div>
</template>
