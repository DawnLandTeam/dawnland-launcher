<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import { Save } from '@lucide/vue';
import DSelect from '../ui/DSelect.vue';
import { getErrorMessage } from '../../utils/error';
import { launchingInstances, runningInstances, repairingInstances } from '../../composables/useLaunchState';

const props = defineProps<{
  instanceId: string;
  instance?: any;
}>();

const { t } = useI18n();

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
  isOpenJ9: boolean;
  isGraalvm: boolean;
}

const settingsConfig = ref<InstanceConfig>({
  javaPath: '',
  maxMemory: 4096,
  jvmArgsExtra: [],
  windowBehavior: 'keep',
  showGameLog: false,
});

const useGlobalMemory = ref(true);
const globalMaxMemory = ref(4096);
const systemMemory = ref<SystemMemoryInfo>({
  totalMb: 8192,
  recommendedMaxMb: 4096,
});
const installedJavas = ref<JavaInfo[]>([]);
const isSavingConfig = ref(false);

const isSettingsInstanceRunning = computed(() => {
  return launchingInstances.value.has(props.instanceId) ||
         runningInstances.value.has(props.instanceId) ||
         repairingInstances.value.has(props.instanceId);
});

const windowBehaviorOptions = computed(() => [
  { label: t('instances.settingsDialog.keepVisible'), value: 'keep' },
  { label: t('instances.settingsDialog.hideLauncher'), value: 'hide' },
  { label: t('instances.settingsDialog.minimizeTaskbar'), value: 'minimize' }
]);

const javaPathOptions = computed(() => [
  { label: t('instances.settingsDialog.defaultAuto'), value: '' },
  ...installedJavas.value.map(java => ({
    label: `Java ${java.majorVersion} (${java.vendor}) [${java.isOpenJ9 ? 'OpenJ9' : (java.isGraalvm ? 'GraalVM' : 'HotSpot')}] - ${java.versionString}`,
    value: java.path
  }))
]);

async function loadData() {
  if (!props.instanceId) return;

  try {
    const memInfo = await invoke<SystemMemoryInfo>('get_system_memory');
    systemMemory.value = memInfo;
    globalMaxMemory.value = memInfo.recommendedMaxMb;

    const launcherSettings = await invoke<any>('load_launcher_settings');
    if (launcherSettings.globalMaxMemory) {
      globalMaxMemory.value = launcherSettings.globalMaxMemory;
    }
  } catch (e) {
    console.error('Failed to load system memory:', e);
  }

  try {
    installedJavas.value = await invoke<JavaInfo[]>('scan_local_javas');
  } catch (e) {
    console.error('Failed to load installed Javas:', e);
  }

  try {
    const config = await invoke<InstanceConfig>('get_instance_config', {
      versionId: props.instanceId,
    });
    useGlobalMemory.value = !config.maxMemory;
    settingsConfig.value = {
      javaPath: config.javaPath || '',
      maxMemory: config.maxMemory || globalMaxMemory.value,
      jvmArgsExtra: config.jvmArgsExtra || [],
      windowBehavior: config.windowBehavior || 'keep',
      showGameLog: config.showGameLog === true,
    };
  } catch (e) {
    console.error('Failed to load instance config:', e);
    useGlobalMemory.value = true;
    settingsConfig.value = {
      javaPath: '',
      maxMemory: globalMaxMemory.value,
      jvmArgsExtra: [],
      windowBehavior: 'keep',
      showGameLog: false,
    };
  }
}

watch(() => props.instanceId, loadData, { immediate: true });

async function saveSettings() {
  isSavingConfig.value = true;
  try {
    const config = {
      javaPath: settingsConfig.value.javaPath || null,
      maxMemory: useGlobalMemory.value ? null : (settingsConfig.value.maxMemory || null),
      jvmArgsExtra: settingsConfig.value.jvmArgsExtra?.length
        ? settingsConfig.value.jvmArgsExtra
        : null,
      windowBehavior: settingsConfig.value.windowBehavior || 'keep',
      showGameLog: settingsConfig.value.showGameLog,
    };

    await invoke('save_instance_config', {
      versionId: props.instanceId,
      config,
    });
    alert(t('common.saveSuccess', '保存成功'));
  } catch (e) {
    console.error('Failed to save instance config:', e);
    alert(`Failed to save: ${getErrorMessage(e)}`);
  } finally {
    isSavingConfig.value = false;
  }
}
</script>

<template>
  <div class="h-full flex flex-col min-h-0 bg-white/40 dark:bg-zinc-900/40">
    <div class="px-6 py-4 border-b border-neutral-200/50 dark:border-zinc-800/50 flex-shrink-0 flex items-center justify-between">
      <h3 class="text-lg font-semibold">{{ t('instances.settingsDialog.title') }}</h3>
      <button
        @click="saveSettings"
        :disabled="isSavingConfig || isSettingsInstanceRunning"
        class="flex items-center gap-2 px-4 py-2 text-sm font-medium bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 transition-colors shadow-sm"
      >
        <Save class="h-4 w-4" />
        {{ t('common.save', 'Save') }}
      </button>
    </div>

    <div class="flex-1 overflow-y-auto p-6">
      <div class="max-w-3xl space-y-8">
        <div v-if="isSettingsInstanceRunning" class="p-3 bg-amber-100 dark:bg-amber-900/30 text-amber-800 dark:text-amber-300 rounded-md text-sm flex items-center gap-2">
          <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-4 w-4 shrink-0"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>
          {{ t('instances.cannotEditRunning', '游戏正在运行中，无法修改配置') }}
        </div>

        <!-- Java Path -->
        <div class="space-y-2">
          <label class="text-sm font-medium">{{ t('instances.settingsDialog.javaVersion') }}</label>
          <DSelect
            v-model="settingsConfig.javaPath"
            :options="javaPathOptions"
            class="w-full"
          />
          <p class="text-xs text-muted-foreground">
            {{ t('instances.settingsDialog.javaWarning') }}
          </p>
        </div>

        <!-- Max Memory -->
        <div class="space-y-2">
          <div class="flex items-center justify-between">
            <label class="text-sm font-medium">{{ t('instances.settingsDialog.maxMemory') }}</label>
            <label class="relative inline-flex items-center cursor-pointer">
              <input type="checkbox" v-model="useGlobalMemory" class="sr-only peer">
              <div class="w-9 h-5 bg-gray-200 peer-focus:outline-none rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
              <span class="ml-2 text-sm font-medium text-gray-900 dark:text-gray-300">{{ t('instances.settingsDialog.useGlobalMemory') }}</span>
            </label>
          </div>
          <div v-if="!useGlobalMemory">
            <div class="flex items-center justify-end">
              <span class="text-sm font-mono text-primary">{{ settingsConfig.maxMemory }} MB</span>
            </div>
            <input
              v-model.number="settingsConfig.maxMemory"
              type="range"
              min="512"
              :max="systemMemory.totalMb"
              step="512"
              class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-blue-500 mt-2"
            />
            <div class="flex justify-between text-xs text-muted-foreground mt-1">
              <span>512 MB</span>
              <span>{{ t('instances.settingsDialog.systemMemory', { system: systemMemory.totalMb }) }}</span>
            </div>
          </div>
          <div v-else class="text-sm text-muted-foreground bg-muted/50 p-3 rounded-lg flex items-center justify-between mt-2">
            <span>{{ t('instances.settingsDialog.globalMemoryCurrently') }}</span>
            <span class="font-mono text-primary">{{ globalMaxMemory }} MB</span>
          </div>
          <p class="text-xs text-muted-foreground mt-1">
            {{ t('instances.settingsDialog.recommendedMemory', { recommended: systemMemory.recommendedMaxMb }) }}
          </p>
        </div>

        <!-- Extra JVM Args -->
        <div class="space-y-2">
          <label class="text-sm font-medium">{{ t('instances.settingsDialog.jvmArgs') }}</label>
          <textarea
            v-model="settingsConfig.jvmArgsExtra"
            placeholder="-XX:+UseG1GC&#10;-XX:+ParallelGCThreads=4"
            class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm font-mono text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 h-24 resize-none"
          />
        </div>

        <!-- Window Behavior -->
        <div class="space-y-2">
          <label class="text-sm font-medium">{{ t('instances.settingsDialog.windowBehavior') }}</label>
          <DSelect
            v-model="settingsConfig.windowBehavior"
            :options="windowBehaviorOptions"
            class="w-full"
          />
          <p class="text-xs text-muted-foreground">
            {{ t('instances.settingsDialog.windowBehaviorDesc') }}
          </p>
        </div>

        <!-- Show Game Log -->
        <div class="flex items-center gap-3 p-4 border border-neutral-200 dark:border-zinc-800 rounded-lg bg-white/50 dark:bg-zinc-900/50">
          <input
            type="checkbox"
            id="showGameLog"
            v-model="settingsConfig.showGameLog"
            class="w-5 h-5 rounded border-gray-300 text-primary focus:ring-primary"
          />
          <label for="showGameLog" class="flex-1 cursor-pointer">
            <span class="font-medium text-sm">{{ t('instances.settingsDialog.showGameLog') }}</span>
            <p class="text-xs text-muted-foreground mt-0.5">
              {{ t('instances.settingsDialog.showGameLogDesc') }}
            </p>
          </label>
        </div>
      </div>
    </div>
  </div>
</template>
