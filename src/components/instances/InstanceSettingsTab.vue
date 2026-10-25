<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import { Save, FolderOpen, Puzzle, Box, Sparkles, Globe } from '@lucide/vue';
import DInput from '../ui/DInput.vue';
import DSelect from '../ui/DSelect.vue';
import { getErrorMessage } from '../../utils/error';
import { launchingInstances, runningInstances, repairingInstances } from '../../composables/useLaunchState';
import { toast } from '../../composables/useToast';

const props = defineProps<{
  instanceId: string;
  instance?: any;
}>();

const { t } = useI18n();

interface InstanceConfig {
  javaPath?: string;
  maxMemory?: number;
  jvmArgsExtra?: string[];
  jvmTunerMode?: string;
  windowWidth?: number;
  windowHeight?: number;
  windowBehavior?: string;
  showGameLog?: boolean;
}

interface JvmTuningPreview {
  mode: string;
  enabled: boolean;
  globalEnabled: boolean;
  profile: string;
  args: string[];
  javaMajor: number;
}

interface SystemMemoryInfo {
  totalMb: number;
  recommendedMaxMb: number;
}

const quickFolders = [
  { id: '', labelKey: 'folderInstance', fallback: 'Instance Root', icon: FolderOpen },
  { id: 'mods', labelKey: 'folderMods', fallback: 'Mods', icon: Puzzle },
  { id: 'resourcepacks', labelKey: 'folderResourcepacks', fallback: 'Resourcepacks', icon: Box },
  { id: 'shaderpacks', labelKey: 'folderShaderpacks', fallback: 'Shaderpacks', icon: Sparkles },
  { id: 'saves', labelKey: 'folderSaves', fallback: 'Worlds', icon: Globe },
];

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
  jvmTunerMode: 'followGlobal',
  windowWidth: 1280,
  windowHeight: 720,
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
const jvmTuningPreview = ref<JvmTuningPreview | null>(null);
const jvmArgsExtraText = ref('');

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

const resolutionPresets = [
  { width: 854, height: 480 },
  { width: 1280, height: 720, isDefault: true },
  { width: 1366, height: 768 },
  { width: 1600, height: 900 },
  { width: 1920, height: 1080 },
  { width: 2560, height: 1440 },
];

const resolutionPresetOptions = computed(() => [
  ...resolutionPresets.map((preset) => ({
    label: preset.isDefault
      ? `${preset.width} × ${preset.height} (${t('instances.settingsDialog.resolutionDefault', 'Default')})`
      : `${preset.width} × ${preset.height}`,
    value: `${preset.width}x${preset.height}`,
  })),
  { label: t('instances.settingsDialog.resolutionCustom', 'Custom'), value: 'custom' },
]);

const selectedResolutionPreset = ref<string>('1280x720');

function resolveResolutionPreset(w: number, h: number): string {
  const matched = resolutionPresets.find((p) => p.width === w && p.height === h);
  return matched ? `${matched.width}x${matched.height}` : 'custom';
}

function handleResolutionPresetChange(val: string | number) {
  const str = String(val);
  selectedResolutionPreset.value = str;
  if (str === 'custom') return;
  const [wStr, hStr] = str.split('x');
  const w = parseInt(wStr, 10);
  const h = parseInt(hStr, 10);
  if (w > 0 && h > 0) {
    settingsConfig.value.windowWidth = w;
    settingsConfig.value.windowHeight = h;
  }
}

function handleWidthInput(val: string) {
  settingsConfig.value.windowWidth = Number(val) || 0;
  selectedResolutionPreset.value = 'custom';
}

function handleHeightInput(val: string) {
  settingsConfig.value.windowHeight = Number(val) || 0;
  selectedResolutionPreset.value = 'custom';
}

const javaPathOptions = computed(() => [
  { label: t('instances.settingsDialog.defaultAuto'), value: '' },
  ...installedJavas.value.map(java => ({
    label: `Java ${java.majorVersion} (${java.vendor}) [${java.isOpenJ9 ? 'OpenJ9' : (java.isGraalvm ? 'GraalVM' : 'HotSpot')}] - ${java.versionString}`,
    value: java.path
  }))
]);

const jvmTunerModeOptions = computed(() => [
  { label: t('instances.settingsDialog.jvmTunerFollowGlobal', 'Follow global setting'), value: 'followGlobal' },
  { label: t('instances.settingsDialog.jvmTunerOn', 'Force enabled'), value: 'on' },
  { label: t('instances.settingsDialog.jvmTunerOff', 'Force disabled'), value: 'off' },
]);

function jvmTunerProfileLabel(profile: string): string {
  const keyByProfile: Record<string, string> = {
    generationalZgc: 'instances.settingsDialog.jvmTunerProfileGenerationalZgc',
    g1Large: 'instances.settingsDialog.jvmTunerProfileG1Large',
    g1Small: 'instances.settingsDialog.jvmTunerProfileG1Small',
    skipped: 'instances.settingsDialog.jvmTunerProfileSkipped',
  };
  const key = keyByProfile[profile];
  return key ? t(key) : profile;
}

function parseJvmArgs(input: string): string[] {
  const args: string[] = [];
  let current = '';
  let inSingle = false;
  let inDouble = false;
  let hasToken = false;

  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (ch === '\\' && i + 1 < input.length) {
      const next = input[i + 1];
      if (
        (!inSingle && !inDouble && /\s/.test(next)) ||
        (!inSingle && next === '"') ||
        (!inDouble && next === "'") ||
        next === '\\'
      ) {
        current += next;
        hasToken = true;
        i++;
        continue;
      }
    }
    if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
      hasToken = true;
      continue;
    }
    if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
      hasToken = true;
      continue;
    }
    if (!inSingle && !inDouble && /\s/.test(ch)) {
      if (hasToken) {
        args.push(current);
        current = '';
        hasToken = false;
      }
      continue;
    }
    current += ch;
    hasToken = true;
  }

  if (hasToken) {
    args.push(current);
  }
  return args;
}

function formatJvmArgs(args: string[]): string {
  return args
    .map((arg) => {
      if (/[\s"']/.test(arg)) {
        return `"${arg.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
      }
      return arg;
    })
    .join('\n');
}

let previewRequestSeq = 0;

async function loadTuningPreview() {
  if (!props.instanceId) return;
  const requestId = ++previewRequestSeq;
  const requestedInstanceId = props.instanceId;
  const requestedMode = settingsConfig.value.jvmTunerMode || 'followGlobal';
  const requestedJavaPath = settingsConfig.value.javaPath || '';
  const requestedUseGlobalMemory = useGlobalMemory.value;
  const requestedMaxMemory = requestedUseGlobalMemory
    ? null
    : (settingsConfig.value.maxMemory || null);
  const requestedExtraArgs = parseJvmArgs(jvmArgsExtraText.value);

  try {
    const preview = await invoke<JvmTuningPreview>('get_jvm_tuning_preview', {
      versionId: requestedInstanceId,
      javaPath: requestedJavaPath,
      maxMemory: requestedMaxMemory,
      useGlobalMemory: requestedUseGlobalMemory,
      jvmTunerMode: requestedMode,
      jvmArgsExtra: requestedExtraArgs,
    });
    if (
      requestId !== previewRequestSeq ||
      props.instanceId !== requestedInstanceId ||
      (settingsConfig.value.jvmTunerMode || 'followGlobal') !== requestedMode
    ) {
      return;
    }
    jvmTuningPreview.value = preview;
  } catch (e) {
    if (requestId !== previewRequestSeq || props.instanceId !== requestedInstanceId) {
      return;
    }
    console.error('Failed to load JVM tuning preview:', e);
    jvmTuningPreview.value = null;
  }
}

watch(
  [
    () => settingsConfig.value.jvmTunerMode,
    () => settingsConfig.value.javaPath,
    () => settingsConfig.value.maxMemory,
    useGlobalMemory,
    jvmArgsExtraText,
  ],
  () => {
    loadTuningPreview();
  }
);

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
    const extraArgs = Array.isArray(config.jvmArgsExtra) ? config.jvmArgsExtra : [];
    jvmArgsExtraText.value = formatJvmArgs(extraArgs);
    const loadedWidth = config.windowWidth && config.windowWidth > 0 ? config.windowWidth : 1280;
    const loadedHeight = config.windowHeight && config.windowHeight > 0 ? config.windowHeight : 720;
    selectedResolutionPreset.value = resolveResolutionPreset(loadedWidth, loadedHeight);
    settingsConfig.value = {
      javaPath: config.javaPath || '',
      maxMemory: config.maxMemory || globalMaxMemory.value,
      jvmArgsExtra: extraArgs,
      jvmTunerMode: config.jvmTunerMode || 'followGlobal',
      windowWidth: loadedWidth,
      windowHeight: loadedHeight,
      windowBehavior: config.windowBehavior || 'keep',
      showGameLog: config.showGameLog === true,
    };
    await loadTuningPreview();
  } catch (e) {
    console.error('Failed to load instance config:', e);
    useGlobalMemory.value = true;
    jvmArgsExtraText.value = '';
    selectedResolutionPreset.value = '1280x720';
    settingsConfig.value = {
      javaPath: '',
      maxMemory: globalMaxMemory.value,
      jvmArgsExtra: [],
      jvmTunerMode: 'followGlobal',
      windowWidth: 1280,
      windowHeight: 720,
      windowBehavior: 'keep',
      showGameLog: false,
    };
  }
}

watch(() => props.instanceId, loadData, { immediate: true });

async function saveSettings() {
  isSavingConfig.value = true;
  try {
    const parsedJvmArgs = parseJvmArgs(jvmArgsExtraText.value);
    settingsConfig.value.jvmArgsExtra = parsedJvmArgs;

    const windowWidth = Number(settingsConfig.value.windowWidth) > 0
      ? Math.round(Number(settingsConfig.value.windowWidth))
      : 1280;
    const windowHeight = Number(settingsConfig.value.windowHeight) > 0
      ? Math.round(Number(settingsConfig.value.windowHeight))
      : 720;
    settingsConfig.value.windowWidth = windowWidth;
    settingsConfig.value.windowHeight = windowHeight;

    const config = {
      javaPath: settingsConfig.value.javaPath || null,
      maxMemory: useGlobalMemory.value ? null : (settingsConfig.value.maxMemory || null),
      jvmArgsExtra: parsedJvmArgs.length > 0 ? parsedJvmArgs : null,
      jvmTunerMode: settingsConfig.value.jvmTunerMode || 'followGlobal',
      windowWidth,
      windowHeight,
      windowBehavior: settingsConfig.value.windowBehavior || 'keep',
      showGameLog: settingsConfig.value.showGameLog,
    };

    await invoke('save_instance_config', {
      versionId: props.instanceId,
      config,
    });
    toast.success(t('common.saveSuccess', 'Save successful'));
  } catch (e) {
    console.error('Failed to save instance config:', e);
    toast.error(t('common.saveFailed', 'Failed to save'), getErrorMessage(e));
  } finally {
    isSavingConfig.value = false;
  }
}

const isSettingGlobalPreset = ref(false);

async function setAsGlobalPreset() {
  if (!props.instanceId) return;
  isSettingGlobalPreset.value = true;
  try {
    await invoke('set_instance_options_as_global', { version_id: props.instanceId });
    toast.success(t('instances.settingsDialog.setAsGlobalPresetSuccess', 'Successfully set as global preset'));
  } catch (e) {
    console.error('Failed to set global preset:', e);
    toast.error(t('instances.settingsDialog.setAsGlobalPresetFailed', 'Failed to set global preset'), getErrorMessage(e));
  } finally {
    isSettingGlobalPreset.value = false;
  }
}

async function openInstanceSubfolder(subfolder: string) {
  if (!props.instanceId) return;
  try {
    if (subfolder === '') {
      await invoke('open_instance_folder', { versionId: props.instanceId });
    } else {
      await invoke('open_instance_subfolder', { versionId: props.instanceId, subfolder });
    }
  } catch (e) {
    console.error('Failed to open folder:', e);
    toast.error(t('common.operationFailed', 'Operation failed'), getErrorMessage(e));
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
      <div class="max-w-3xl space-y-6">
        <div v-if="isSettingsInstanceRunning" class="p-3 bg-amber-100 dark:bg-amber-900/30 text-amber-800 dark:text-amber-300 rounded-md text-sm flex items-center gap-2">
          <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-4 w-4 shrink-0"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>
          {{ t('instances.cannotEditRunning', '游戏正在运行中，无法修改配置') }}
        </div>

        <!-- Quick Folders -->
        <div class="space-y-4">
          <div>
            <h4 class="text-sm font-medium mb-1">{{ t('instances.settingsDialog.openFolders') || 'Open Resource Folders' }}</h4>
            <p class="text-xs text-muted-foreground">{{ t('instances.settingsDialog.openFoldersDesc') || 'Quickly open relevant resource folders for this instance' }}</p>
          </div>
          <div class="flex flex-wrap items-center gap-3">
            <button
              v-for="folder in quickFolders"
              :key="folder.id"
              @click="openInstanceSubfolder(folder.id)"
              class="flex items-center gap-2 px-3 py-1.5 text-sm font-medium border border-neutral-300 dark:border-zinc-700 rounded-md hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors"
            >
              <component :is="folder.icon" class="h-4 w-4 text-primary" />
              {{ t(`instances.settingsDialog.${folder.labelKey}`) || folder.fallback }}
            </button>
          </div>
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
              <div class="w-9 h-5 bg-gray-200 peer-focus:outline-none rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
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
            v-model="jvmArgsExtraText"
            placeholder="-XX:+UseG1GC&#10;-XX:+ParallelGCThreads=4"
            class="w-full px-3 py-2 bg-white dark:bg-zinc-800 border border-neutral-300 dark:border-zinc-700 rounded-md text-sm font-mono text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 h-24 resize-none"
          />
        </div>

        <!-- Smart JVM Tuning -->
        <div class="space-y-2">
          <label class="text-sm font-medium">{{ t('instances.settingsDialog.jvmTuner') }}</label>
          <DSelect
            v-model="settingsConfig.jvmTunerMode"
            :options="jvmTunerModeOptions"
            class="w-full"
          />
          <p class="text-xs text-muted-foreground">
            {{ t('instances.settingsDialog.jvmTunerDesc') }}
          </p>
          <div
            v-if="jvmTuningPreview && jvmTuningPreview.enabled"
            class="mt-1 p-3 rounded-lg bg-muted/50 border border-neutral-200 dark:border-zinc-800 space-y-2"
          >
            <div class="flex items-center justify-between text-sm">
              <span class="text-muted-foreground">{{ t('instances.settingsDialog.jvmTunerProfileTitle') }}</span>
              <span class="font-medium text-primary">{{ jvmTunerProfileLabel(jvmTuningPreview.profile) }}</span>
            </div>
            <div class="flex items-center justify-between text-sm">
              <span class="text-muted-foreground">{{ t('instances.settingsDialog.jvmTunerJavaDetected') }}</span>
              <span class="font-mono">{{ jvmTuningPreview.javaMajor > 0 ? `Java ${jvmTuningPreview.javaMajor}` : t('instances.settingsDialog.jvmTunerJavaUnknown') }}</span>
            </div>
            <div v-if="jvmTuningPreview.args.length">
              <span class="text-xs text-muted-foreground">{{ t('instances.settingsDialog.jvmTunerArgsTitle') }}</span>
              <div class="mt-1 space-y-0.5">
                <code
                  v-for="arg in jvmTuningPreview.args"
                  :key="arg"
                  class="block text-xs font-mono text-neutral-600 dark:text-zinc-400 break-all"
                >{{ arg }}</code>
              </div>
            </div>
          </div>
        </div>

        <!-- Window Resolution -->
        <div class="space-y-2">
          <label class="text-sm font-medium">{{ t('instances.settingsDialog.resolution') }}</label>
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <DSelect
              :model-value="selectedResolutionPreset"
              @update:model-value="handleResolutionPresetChange"
              :options="resolutionPresetOptions"
              class="w-full"
            />
            <div class="flex items-center gap-2">
              <DInput
                :model-value="settingsConfig.windowWidth"
                @update:model-value="handleWidthInput"
                type="number"
                min="1"
                :placeholder="t('instances.settingsDialog.resolutionWidth', 'Width')"
                class="w-full font-mono"
              />
              <span class="text-sm text-muted-foreground font-mono select-none">×</span>
              <DInput
                :model-value="settingsConfig.windowHeight"
                @update:model-value="handleHeightInput"
                type="number"
                min="1"
                :placeholder="t('instances.settingsDialog.resolutionHeight', 'Height')"
                class="w-full font-mono"
              />
            </div>
          </div>
          <p class="text-xs text-muted-foreground">
            {{ t('instances.settingsDialog.resolutionDesc') }}
          </p>
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
        <!-- Game Settings -->
        <div class="space-y-4 pt-4 border-t border-neutral-200 dark:border-zinc-800">
          <div>
            <h4 class="text-sm font-medium mb-1">{{ t('instances.settingsDialog.gameSettings') }}</h4>
            <p class="text-xs text-muted-foreground">{{ t('instances.settingsDialog.gameSettingsDesc') }}</p>
          </div>
          <div class="flex items-center gap-3">
            <button
              @click="setAsGlobalPreset"
              :disabled="isSettingGlobalPreset"
              class="flex items-center gap-2 px-3 py-1.5 text-sm font-medium border border-neutral-300 dark:border-zinc-700 rounded-md hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors"
            >
              <svg v-if="!isSettingGlobalPreset" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-primary"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
              <svg v-else xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="animate-spin text-primary"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
              {{ t('instances.settingsDialog.setAsGlobalPreset') }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
