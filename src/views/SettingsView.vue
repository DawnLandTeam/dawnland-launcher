<script setup lang="ts">
import { ref, onMounted, onActivated, watch, onUnmounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from '@tauri-apps/plugin-dialog';
import DInput from '../components/ui/DInput.vue';
import DCombobox from "../components/ui/DCombobox.vue";
import DSidebarTabs, { type SidebarTab } from '../components/ui/DSidebarTabs.vue';
import { Loader2, Download, Coffee, Trash2, FolderOpen, Plus, Search, Package, Languages, Settings, Shield, Info, Bot, ScrollText, Edit, X, RefreshCw, Gauge, CircleHelp, Sparkles } from "@lucide/vue";
import { useI18n } from 'vue-i18n';
import DSelect from '../components/ui/DSelect.vue';
import DMultiSelect from '../components/ui/DMultiSelect.vue';
import DButton from '../components/ui/DButton.vue';
import DTooltip from '../components/ui/DTooltip.vue';
import {
  DEFAULT_RESOURCE_NAMING_SETTINGS,
  notifyResourceNamingUpdated,
} from '../composables/useResourceNaming';
import type {
  AssetCategory,
  CacheGcReport,
  CacheRetentionPolicy,
  FilenameFormatMode,
  FilenamePreset,
  LauncherSettings,
  ManagementDisplayStyle,
  TemplatePreviewResult,
  TranslationDbStats,
} from '../types/resourceNaming';
import { useLogStore } from '../composables/useLogStore';
import type { LogFileInfo, LogLevelSetting } from '../types/log';
import { createLogger } from '../utils/logger';
import { useRoute, useRouter } from "vue-router";
import { getVersion } from "@tauri-apps/api/app";
import { setUpdateAvailable, hasUpdateAvailable, openUpdateModal, type CustomUpdate } from "../composables/useUpdate";
import { trackEvent, getErrorType, sanitizeTrackingUrl } from "../utils/analytics";
import { getErrorMessage } from "../utils/error";
import { normalizeUpdateChannel } from "../utils/updateChannel";
import { toast } from "../composables/useToast";
import { formatSpeed, formatBytes } from "../utils/format";

const logger = createLogger('SettingsView');

const route = useRoute();
const router = useRouter();

const { t, locale } = useI18n();

// App version state
const appVersion = ref('0.0.0');

let isSettingsUnmounted = false;
let unlistenCacheGc: UnlistenFn | null = null;

onMounted(async () => {
  trackEvent("Settings Viewed");
  try {
    appVersion.value = await getVersion();
  } catch (err) {
    logger.error("Failed to get app version:", err);
  }

  window.addEventListener('authlib-servers-updated', loadAuthlibServers);

  listen<any>('download-progress', (event) => {
    const { taskId, downloaded, total, speed, error } = event.payload;
    const isSplit = taskId.includes('|');
    const mainId = isSplit ? taskId.split('|')[0] : taskId;

    if (downloadingModels.value[mainId]) {
      if (error) {
        delete downloadingModels.value[mainId];
      } else {
        if (isSplit) {
          const modelData = downloadingModels.value[mainId];
          if (!modelData.parts) modelData.parts = {};
          modelData.parts[taskId] = { downloaded, total, speed };
          
          let sumDownloaded = 0;
          let sumTotal = 0;
          let sumSpeed = 0;
          
          for (const key in modelData.parts) {
            sumDownloaded += modelData.parts[key].downloaded;
            sumTotal += modelData.parts[key].total;
            sumSpeed += modelData.parts[key].speed;
          }
          
          modelData.downloaded = sumDownloaded;
          modelData.total = sumTotal;
          modelData.speed = sumSpeed;
        } else {
          downloadingModels.value[mainId] = { downloaded, total, speed };
        }
      }
    }
  });
  
  listen<string>('model-download-complete', (event) => {
    const filename = event.payload;
    delete downloadingModels.value[filename];
    loadLocalModels(); // refresh list
  });

  listen<CacheGcReport>('cache-gc-completed', (event) => {
    if (!isSettingsUnmounted && event.payload.skippedReason == null) {
      lastGcReport.value = event.payload;
    }
  }).then((unlistenFn) => {
    if (isSettingsUnmounted) {
      unlistenFn();
    } else {
      unlistenCacheGc = unlistenFn;
    }
  });
});

onUnmounted(() => {
  isSettingsUnmounted = true;
  if (unlistenCacheGc) {
    unlistenCacheGc();
    unlistenCacheGc = null;
  }
  window.removeEventListener('authlib-servers-updated', loadAuthlibServers);
});

onActivated(async () => {
  updateChannel.value = normalizeUpdateChannel(localStorage.getItem('updateChannel'));
  await loadSystemMemory();
  await loadLauncherSettings();
  await loadAuthlibServers();
  await scanLocalJavas();
  await loadJavaDownloadPath();
  await loadAvailableJavas();
  await loadLocalModels();
});

const tabs = [
  { id: 'general', name: 'settings.tabs.general', icon: Settings },
  { id: 'java', name: 'settings.tabs.java', icon: Coffee },
  { id: 'authlib', name: 'settings.authlib.tab', icon: Shield },
  { id: 'ai', name: 'settings.ai.tab', icon: Bot },
  { id: 'logs', name: 'settings.tabs.logs', icon: ScrollText },
  { id: 'about', name: 'settings.tabs.about', icon: Info },
] as const;

type TabId = typeof tabs[number]['id'];
const activeTab = ref<TabId>('general');

const translatedTabs = computed<SidebarTab[]>(() => tabs.map(tab => ({
  ...tab,
  name: t(tab.name),
  hasDot: tab.id === 'about' && hasUpdateAvailable.value
})));

watch(
  () => route.query.tab,
  (newTab) => {
    if (newTab) {
      if (['general', 'java', 'authlib', 'ai', 'logs', 'about'].includes(newTab as string)) {
        activeTab.value = newTab as any;
      }
      // Clean up the query so it doesn't persist
      router.replace({ query: { ...route.query, tab: undefined } });
    }
  },
  { immediate: true }
);

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

interface DownloadProgress {
  taskId: string;
  downloaded: number;
  total: number;
  speed?: number;
  completed?: boolean;
}

interface AuthlibServer {
  url: string;
  name: string;
}

// Global settings

const systemMemory = ref<SystemMemoryInfo>({ totalMb: 8192, recommendedMaxMb: 4096 });
const defaultMaxMemory = ref(4096);
const hasCustomMaxMemory = ref(false);

// Updater state
const isCheckingUpdate = ref(false);
const updateChannel = ref(normalizeUpdateChannel(localStorage.getItem('updateChannel')));
let silentUpdateCheckSeq = 0;

// Launcher Settings state
const enableInstanceInheritance = ref(false);
const downloadSource = ref<'official' | 'bmclapi'>('official');
const maxConcurrentDownloads = ref(32);
const enableTelemetry = ref(false);
const enableGlobalGameSettings = ref(false);
const enableGlobalModCache = ref(true);
const autoCleanCache = ref(true);
const cacheRetentionPolicy = ref<CacheRetentionPolicy>('immediate');
const autoCleanSharedAssets = ref(false);
const lastGcReport = ref<CacheGcReport | null>(null);
const enableJvmTuner = ref(true);

const cacheRetentionOptions = computed(() => [
  { label: t('settings.general.cacheRetentionImmediate'), value: 'immediate' },
  { label: t('settings.general.cacheRetention24h'), value: '24h' },
  { label: t('settings.general.cacheRetention7d'), value: '7d' },
]);

// Community resource naming & translation state
const filenameMode = ref<FilenameFormatMode>(DEFAULT_RESOURCE_NAMING_SETTINGS.filenameMode);
const filenamePreset = ref<FilenamePreset>(DEFAULT_RESOURCE_NAMING_SETTINGS.filenamePreset);
const customTemplate = ref<string>(DEFAULT_RESOURCE_NAMING_SETTINGS.customTemplate);
const renameTargets = ref<AssetCategory[]>([...DEFAULT_RESOURCE_NAMING_SETTINGS.renameTargets]);
const managementDisplayStyle = ref<ManagementDisplayStyle>(
  DEFAULT_RESOURCE_NAMING_SETTINGS.managementDisplayStyle,
);
const enableOnlineTranslation = ref<boolean>(DEFAULT_RESOURCE_NAMING_SETTINGS.enableOnlineTranslation);
const autoSyncTranslations = ref<boolean>(DEFAULT_RESOURCE_NAMING_SETTINGS.autoSyncTranslations);
const templatePreview = ref<TemplatePreviewResult>({
  rendered: '[机械动力] create-1.21.1-6.0.4.jar',
  fallbackRendered: 'create-1.21.1-6.0.4.jar',
  warnings: [],
});
const translationStats = ref<TranslationDbStats | null>(null);
const isSyncingTranslations = ref(false);

const PLACEHOLDER_TOKENS = [
  { token: '{zh}', labelKey: 'settings.resourceNaming.tokenZh' },
  { token: '{zhFull}', labelKey: 'settings.resourceNaming.tokenZhFull' },
  { token: '{en}', labelKey: 'settings.resourceNaming.tokenEn' },
  { token: '{slug}', labelKey: 'settings.resourceNaming.tokenSlug' },
  { token: '{stem}', labelKey: 'settings.resourceNaming.tokenStem' },
  { token: '{ext}', labelKey: 'settings.resourceNaming.tokenExt' },
  { token: '{version}', labelKey: 'settings.resourceNaming.tokenVersion' },
  { token: '{mcVersion}', labelKey: 'settings.resourceNaming.tokenMcVersion' },
  { token: '{loader}', labelKey: 'settings.resourceNaming.tokenLoader' },
  { token: '{source}', labelKey: 'settings.resourceNaming.tokenSource' },
] as const;

const filenameModeOptions = computed(() => [
  { label: t('settings.resourceNaming.modeAuto'), value: 'auto' },
  { label: t('settings.resourceNaming.modePreset'), value: 'preset' },
  { label: t('settings.resourceNaming.modeCustom'), value: 'custom' },
]);

const filenamePresetOptions = computed(() => [
  { label: '[机械动力] create-1.21.1-6.0.4.jar', value: 'bracketPrefix' },
  { label: '【机械动力】create-1.21.1-6.0.4.jar', value: 'cnBracketPrefix' },
  { label: '机械动力-create-1.21.1-6.0.4.jar', value: 'dashPrefix' },
  { label: 'create-1.21.1-6.0.4-机械动力.jar', value: 'dashSuffix' },
  { label: '机械动力-6.0.4.jar', value: 'zhVersion' },
  { label: t('settings.resourceNaming.presetOriginal'), value: 'original' },
]);

const managementDisplayOptions = computed(() => [
  { label: t('settings.resourceNaming.displayTranslationFirst'), value: 'translationFirst' },
  { label: t('settings.resourceNaming.displayFilenameFirst'), value: 'filenameFirst' },
]);

const renameTargetOptions = computed(() => [
  { label: t('instances.mods', '模组'), value: 'Mod' },
  { label: t('instances.resourcepacks', '资源包'), value: 'ResourcePack' },
  { label: t('downloadsCenter.tabs.shader', '光影'), value: 'ShaderPack' },
  { label: t('downloadsCenter.tabs.datapack', '数据包'), value: 'Datapack' },
]);

function effectivePreviewTemplate(): string {
  if (filenameMode.value === 'auto') {
    return locale.value.toLowerCase().startsWith('zh') ? '[{zh}] {stem}{ext}' : '{stem}{ext}';
  }
  if (filenameMode.value === 'preset') {
    switch (filenamePreset.value) {
      case 'bracketPrefix':
        return '[{zh}] {stem}{ext}';
      case 'cnBracketPrefix':
        return '【{zh}】{stem}{ext}';
      case 'dashPrefix':
        return '{zh}-{stem}{ext}';
      case 'dashSuffix':
        return '{stem}-{zh}{ext}';
      case 'zhVersion':
        return '{zh}-{version}{ext}';
      case 'original':
        return '{stem}{ext}';
    }
  }
  return customTemplate.value || '{stem}{ext}';
}

async function refreshTemplatePreview() {
  try {
    templatePreview.value = await invoke<TemplatePreviewResult>('preview_resource_template', {
      template: effectivePreviewTemplate(),
    });
  } catch (err) {
    logger.warn('Failed to preview template:', err);
  }
}

async function loadTranslationStats() {
  try {
    translationStats.value = await invoke<TranslationDbStats>('get_translation_db_stats');
  } catch (err) {
    logger.warn('Failed to load translation DB stats:', err);
  }
}

async function syncTranslationsNow() {
  if (isSyncingTranslations.value) return;
  isSyncingTranslations.value = true;
  try {
    translationStats.value = await invoke<TranslationDbStats>('sync_translations_from_backend', {
      forceFull: true,
    });
    toast.success(
      t('settings.resourceNaming.syncSuccess', {
        count: translationStats.value?.totalEntries ?? 0,
      }),
    );
  } catch (err) {
    toast.error(t('common.operationFailed'), getErrorMessage(err));
  } finally {
    isSyncingTranslations.value = false;
  }
}

function appendPlaceholderToken(token: string) {
  filenameMode.value = 'custom';
  customTemplate.value = `${customTemplate.value}${token}`;
  refreshTemplatePreview();
  saveLauncherSettings();
}

watch(
  [filenameMode, filenamePreset, customTemplate, () => locale.value],
  () => {
    refreshTemplatePreview();
  },
  { immediate: true },
);

const showGlobalGameSettingsModal = ref(false);
const globalGameSettingsContent = ref('');
const isSavingGlobalSettings = ref(false);
const isCleaningCache = ref(false);

async function cleanGlobalModCache() {
  if (isCleaningCache.value) return;
  isCleaningCache.value = true;
  try {
    const report = await invoke<CacheGcReport>('clean_unreferenced_cache');
    if (report.skippedReason === 'active_task') {
      toast.info(t('settings.general.cleanCacheSkippedActive'));
      return;
    }
    lastGcReport.value = report;
    const formattedSize = formatBytes(report.freedBytes, 2);
    toast.success(t('settings.general.cleanCacheSuccess', { size: formattedSize }));
  } catch (e) {
    logger.error('Failed to clean cache:', e);
    toast.error(t('settings.general.cleanCacheFailed'), getErrorMessage(e));
  } finally {
    isCleaningCache.value = false;
  }
}

async function openGlobalGameSettings() {
  try {
    globalGameSettingsContent.value = await invoke<string>('get_global_game_settings');
    showGlobalGameSettingsModal.value = true;
  } catch (e) {
    logger.error('Failed to load global game settings:', e);
    toast.error(t('settings.general.globalGameSettingsLoadFailed'), getErrorMessage(e));
  }
}

async function saveGlobalGameSettingsContent() {
  isSavingGlobalSettings.value = true;
  try {
    await invoke('save_global_game_settings', { content: globalGameSettingsContent.value });
    toast.success(t('settings.general.globalGameSettingsSaveSuccess'));
    showGlobalGameSettingsModal.value = false;
  } catch (e) {
    logger.error('Failed to save global game settings:', e);
    toast.error(t('settings.general.globalGameSettingsSaveFailed'), getErrorMessage(e));
  } finally {
    isSavingGlobalSettings.value = false;
  }
}

const configuredLogLevel = ref<LogLevelSetting | null>(null);
const logRetentionDays = ref<number>(7);

async function loadLauncherSettings() {
  try {
    const settings = await invoke<LauncherSettings>('load_launcher_settings');
    enableInstanceInheritance.value = settings.enableInstanceInheritance;
    downloadSource.value = settings.downloadSource === 'bmclapi' ? 'bmclapi' : 'official';
    maxConcurrentDownloads.value = settings.maxConcurrentDownloads || 32;
    enableTelemetry.value = settings.enableTelemetry === true;
    enableGlobalGameSettings.value = settings.enableGlobalGameSettings === true;
    enableGlobalModCache.value = settings.enableGlobalModCache ?? true;
    autoCleanCache.value = settings.autoCleanCache ?? true;
    cacheRetentionPolicy.value = settings.cacheRetentionPolicy ?? 'immediate';
    autoCleanSharedAssets.value = settings.autoCleanSharedAssets === true;
    enableJvmTuner.value = settings.enableJvmTuner ?? true;
    configuredLogLevel.value = settings.logLevel ?? null;
    logRetentionDays.value = settings.logRetentionDays ?? 7;
    if (settings.globalMaxMemory) {
      defaultMaxMemory.value = settings.globalMaxMemory;
      hasCustomMaxMemory.value = true;
    } else {
      hasCustomMaxMemory.value = false;
    }
    if (settings.resourceNaming) {
      filenameMode.value = settings.resourceNaming.filenameMode ?? 'auto';
      filenamePreset.value = settings.resourceNaming.filenamePreset ?? 'bracketPrefix';
      customTemplate.value = settings.resourceNaming.customTemplate || '[{zh}] {stem}{ext}';
      renameTargets.value = settings.resourceNaming.renameTargets ?? [
        'Mod',
        'ResourcePack',
        'ShaderPack',
        'Datapack',
      ];
      managementDisplayStyle.value =
        settings.resourceNaming.managementDisplayStyle ?? 'translationFirst';
      enableOnlineTranslation.value = settings.resourceNaming.enableOnlineTranslation ?? true;
      autoSyncTranslations.value = settings.resourceNaming.autoSyncTranslations ?? true;
    }
    if (settings.aiConfig) {
      aiProviderType.value = settings.aiConfig.providerType === 'remoteApi' ? 'remoteApi' : 'embeddedLlm';
      aiRemoteApiKey.value = settings.aiConfig.remoteApiKey || '';
      aiRemoteBaseUrl.value = settings.aiConfig.remoteBaseUrl || '';
      aiRemoteModel.value = settings.aiConfig.remoteModel || '';
      aiActiveEmbeddedModel.value = settings.aiConfig.activeEmbeddedModel || '';
      aiMaxRamUsage.value = settings.aiConfig.maxRamUsage || 4096;
      aiUnlockContextSize.value = settings.aiConfig.unlockContextSize || false;
    }
    await loadTranslationStats();
    await refreshTemplatePreview();
  } catch (e) {
    logger.error('Failed to load launcher settings:', e);
  }
}

let isSavingLauncherSettings = false;
let hasPendingLauncherSettingsSave = false;

function buildLauncherSettingsSnapshot(): LauncherSettings {
  const naming = {
    filenameMode: filenameMode.value,
    filenamePreset: filenamePreset.value,
    customTemplate: customTemplate.value,
    renameTargets: renameTargets.value,
    managementDisplayStyle: managementDisplayStyle.value,
    enableOnlineTranslation: enableOnlineTranslation.value,
    autoSyncTranslations: autoSyncTranslations.value,
  };
  return {
    enableInstanceInheritance: enableInstanceInheritance.value,
    downloadSource: downloadSource.value,
    maxConcurrentDownloads: maxConcurrentDownloads.value,
    enableTelemetry: enableTelemetry.value,
    enableGlobalGameSettings: enableGlobalGameSettings.value,
    enableGlobalModCache: enableGlobalModCache.value,
    autoCleanCache: autoCleanCache.value,
    cacheRetentionPolicy: cacheRetentionPolicy.value,
    autoCleanSharedAssets: autoCleanSharedAssets.value,
    enableJvmTuner: enableJvmTuner.value,
    globalMaxMemory: defaultMaxMemory.value,
    uiLanguage: locale.value,
    resourceNaming: naming,
    logLevel: configuredLogLevel.value,
    logRetentionDays: logRetentionDays.value,
    aiConfig: {
      providerType: aiProviderType.value,
      remoteApiKey: aiRemoteApiKey.value,
      remoteBaseUrl: aiRemoteBaseUrl.value,
      remoteModel: aiRemoteModel.value,
      activeEmbeddedModel: aiActiveEmbeddedModel.value,
      maxRamUsage: aiMaxRamUsage.value,
      unlockContextSize: aiUnlockContextSize.value,
    },
  };
}

async function saveLauncherSettings() {
  if (isSavingLauncherSettings) {
    hasPendingLauncherSettingsSave = true;
    return;
  }
  isSavingLauncherSettings = true;
  try {
    do {
      hasPendingLauncherSettingsSave = false;
      const snapshot = buildLauncherSettingsSnapshot();
      await invoke('save_launcher_settings', {
        settings: snapshot,
      });
      notifyResourceNamingUpdated(snapshot.resourceNaming);
    } while (hasPendingLauncherSettingsSave);
  } catch (e) {
    logger.error('Failed to save launcher settings:', e);
    toast.error(t('settings.saveFailed'), getErrorMessage(e));
  } finally {
    isSavingLauncherSettings = false;
  }
}

async function refreshUpdateAvailabilitySilently(channel: 'stable' | 'prerelease') {
  const requestSeq = ++silentUpdateCheckSeq;
  try {
    const update = await invoke<CustomUpdate | null>("check_launcher_update", { channel });
    if (isSettingsUnmounted || requestSeq !== silentUpdateCheckSeq || updateChannel.value !== channel) {
      return;
    }
    setUpdateAvailable(update && update.version !== appVersion.value ? update : null);
  } catch (err) {
    if (isSettingsUnmounted || requestSeq !== silentUpdateCheckSeq) {
      return;
    }
    logger.warn("Failed to silently refresh update availability:", err);
    setUpdateAvailable(null);
  }
}

function changeUpdateChannel(channel: string) {
  const normalizedChannel = normalizeUpdateChannel(channel);
  if (normalizedChannel === updateChannel.value) {
    return;
  }
  updateChannel.value = normalizedChannel;
  localStorage.setItem('updateChannel', normalizedChannel);
  setUpdateAvailable(null);
  void refreshUpdateAvailabilitySilently(normalizedChannel);
}

async function checkForUpdates() {
  isCheckingUpdate.value = true;
  const requestSeq = ++silentUpdateCheckSeq;
  try {
    const channel = normalizeUpdateChannel(localStorage.getItem('updateChannel'));
    const update = await invoke<CustomUpdate | null>("check_launcher_update", { channel });
    if (isSettingsUnmounted || requestSeq !== silentUpdateCheckSeq || updateChannel.value !== channel) {
      return;
    }
    if (update && update.version !== appVersion.value) {
      openUpdateModal(update);
      return;
    }
    setUpdateAvailable(null);
    alert(t('settings.about.upToDate'));
  } catch (err) {
    if (isSettingsUnmounted || requestSeq !== silentUpdateCheckSeq) {
      return;
    }
    logger.error("Failed to check for updates:", err);
    alert(t('settings.about.updateFailed') + err);
  } finally {
    isCheckingUpdate.value = false;
  }
}

// Java management state
const installedJavas = ref<JavaInfo[]>([]);
const isScanningJava = ref(false);
const isDownloadingJava = ref(false);
const downloadingVersion = ref<number | null>(null);
const javaDownloadProgress = ref(0);
const javaDownloadedBytes = ref(0);
const javaTotalBytes = ref(0);
const javaDownloadSpeed = ref("0 B/s");
const customJavaDownloadPath = ref<string>("");
const selectedJavaVersion = ref<number>(21);
const availableJavaVersions = ref<number[]>([8, 11, 17, 21, 23]);
const isFetchingJavaVersions = ref(false);
const isFullDiskScanning = ref(false);
const fullDiskScanPath = ref("");



// Authlib state
const authlibServers = ref<AuthlibServer[]>([]);
const isFetchingAuthlibServers = ref(false);
const newAuthlibUrl = ref("");
const isAddingAuthlibServer = ref(false);

async function loadAuthlibServers(): Promise<void> {
  isFetchingAuthlibServers.value = true;
  try {
    const res = await invoke<AuthlibServer[]>("fetch_authlib_servers");
    authlibServers.value = res || [];
  } catch (err) {
    logger.error("Failed to load authlib servers:", err);
    authlibServers.value = [];
  } finally {
    isFetchingAuthlibServers.value = false;
  }
}

async function addAuthlibServer(): Promise<void> {
  if (!newAuthlibUrl.value.trim()) return;
  isAddingAuthlibServer.value = true;
  try {
    const server = await invoke<AuthlibServer>("add_authlib_server", { url: newAuthlibUrl.value.trim() });
    authlibServers.value = authlibServers.value.filter(s => s.url !== server.url);
    authlibServers.value.push(server);
    trackEvent("Authlib Added", { type: "manual_authlib", api: sanitizeTrackingUrl(newAuthlibUrl.value) });
    newAuthlibUrl.value = "";
  } catch (err) {
    logger.error("Failed to add authlib server:", err);
    trackEvent("Error Occurred", { 
      context: "manual_authlib", 
      error_type: getErrorType(err), 
      api: sanitizeTrackingUrl(newAuthlibUrl.value) 
    });
    alert(`Failed to add Authlib Server: ${getErrorMessage(err)}`);
  } finally {
    isAddingAuthlibServer.value = false;
  }
}

async function removeAuthlibServer(url: string): Promise<void> {
  if (confirm("Are you sure you want to remove this authentication server?")) {
    try {
      await invoke("remove_authlib_server", { url });
      authlibServers.value = authlibServers.value.filter(s => s.url !== url);
    } catch (err) {
      logger.error("Failed to remove authlib server:", err);
    }
  }
}

async function loadSystemMemory(): Promise<void> {
  try {
    systemMemory.value = await invoke<SystemMemoryInfo>("get_system_memory");
    if (!hasCustomMaxMemory.value) {
      defaultMaxMemory.value = systemMemory.value.recommendedMaxMb;
    }
  } catch (err) {
    logger.error("Failed to load system memory:", err);
  }
}

// Java management functions
async function scanLocalJavas(): Promise<void> {
  isScanningJava.value = true;
  try {
    installedJavas.value = await invoke<JavaInfo[]>("scan_local_javas");
  } catch (err) {
    logger.error("Failed to scan Java installations:", err);
  } finally {
    isScanningJava.value = false;
  }
}

async function loadJavaDownloadPath(): Promise<void> {
  try {
    const path = await invoke<string | null>("get_java_download_path");
    customJavaDownloadPath.value = path || "";
  } catch (err) {
    logger.error("Failed to load custom Java download path:", err);
  }
}

async function loadAvailableJavas(): Promise<void> {
  isFetchingJavaVersions.value = true;
  try {
    const versions = await invoke<number[]>("fetch_available_javas");
    if (versions && versions.length > 0) {
      availableJavaVersions.value = versions;
      // If 21 is available, default to it, else default to the newest LTS or first item
      if (!versions.includes(selectedJavaVersion.value)) {
        selectedJavaVersion.value = versions[0];
      }
    }
  } catch (err) {
    logger.error("Failed to fetch available Javas from API:", err);
  } finally {
    isFetchingJavaVersions.value = false;
  }
}

async function chooseJavaDownloadPath(): Promise<void> {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (selected && typeof selected === "string") {
      await invoke("set_java_download_path", { path: selected });
      customJavaDownloadPath.value = selected;
    }
  } catch (err) {
    logger.error("Failed to select directory:", err);
  }
}

async function clearJavaDownloadPath(): Promise<void> {
  try {
    await invoke("set_java_download_path", { path: null });
    customJavaDownloadPath.value = "";
  } catch (err) {
    logger.error("Failed to clear directory:", err);
  }
}

async function addManualJava(): Promise<void> {
  try {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [{ name: "Executable", extensions: ["exe", "bin", ""] }]
    });
    if (selected && typeof selected === "string") {
      const javaInfo = await invoke<JavaInfo>("add_manual_java", { path: selected });
      // Add to list if not already there
      if (!installedJavas.value.some(j => j.path === javaInfo.path)) {
        installedJavas.value.push(javaInfo);
      }
    }
  } catch (err) {
    logger.error("Failed to add manual Java:", err);
    alert(`Failed to add Java: ${getErrorMessage(err)}`);
  }
}

async function removeJava(path: string): Promise<void> {
  const isManaged = path.includes("runtimes") || (customJavaDownloadPath.value && path.includes(customJavaDownloadPath.value));
  const msg = isManaged 
    ? "Are you sure you want to delete this Java installation from disk?"
    : "Remove this Java from the launcher list?";
    
  if (confirm(msg)) {
    try {
      await invoke("remove_java", { path });
      installedJavas.value = installedJavas.value.filter(j => j.path !== path);
    } catch (err) {
      logger.error("Failed to remove Java:", err);
      alert(`Failed to remove Java: ${getErrorMessage(err)}`);
    }
  }
}

async function scanFullDisk(): Promise<void> {
  isFullDiskScanning.value = true;
  fullDiskScanPath.value = "Starting scan...";
  try {
    const unlisten = await listen<any>("java-scan-progress", (event) => {
      const payload = event.payload;
      if (payload.status === "scanning") {
        fullDiskScanPath.value = payload.currentPath;
      } else if (payload.status === "complete") {
        isFullDiskScanning.value = false;
        scanLocalJavas();
        unlisten();
      }
    });
    await invoke("scan_full_disk");
  } catch (err) {
    logger.error("Failed to start full disk scan:", err);
    isFullDiskScanning.value = false;
  }
}

async function downloadJava(majorVersion: number): Promise<void> {
  isDownloadingJava.value = true;
  downloadingVersion.value = majorVersion;
  javaDownloadProgress.value = 0;
  javaDownloadedBytes.value = 0;
  javaTotalBytes.value = 0;
  javaDownloadSpeed.value = "0 B/s";

  let unlisten: (() => void) | null = null;
  try {
    // Listen for download progress
    unlisten = await listen<DownloadProgress>("download-progress", (event) => {
      const payload = event.payload;
      if (payload.total > 0) {
        javaDownloadProgress.value = Math.floor((payload.downloaded / payload.total) * 100);
        javaDownloadedBytes.value = payload.downloaded;
        javaTotalBytes.value = payload.total;
        if (payload.speed !== undefined) {
          javaDownloadSpeed.value = formatSpeed(payload.speed) || "0 B/s";
        }
      }
    });

    const javaInfo = await invoke<JavaInfo>("download_java", { majorVersion });
    installedJavas.value.unshift(javaInfo);
    trackEvent("Java Download Completed", { majorVersion, version: javaInfo.versionString });
    
  } catch (err) {
    logger.error("Failed to download Java:", err);
    trackEvent("Error Occurred", { 
      context: "java_download", 
      error_type: getErrorType(err) 
    });
    alert(`Failed to download Java ${majorVersion}: ${getErrorMessage(err)}`);
  } finally {
    if (unlisten) unlisten();
    isDownloadingJava.value = false;
    downloadingVersion.value = null;
    javaDownloadProgress.value = 0;
  }
}



const languageOptions = [
  { label: 'English', value: 'en' },
  { label: '简体中文', value: 'zh-CN' }
];

const updateChannelOptions = computed(() => [
  { label: t('settings.about.channelStable'), value: 'stable' },
  { label: t('settings.about.channelPrerelease'), value: 'prerelease' }
]);

const downloadSourceOptions = computed(() => [
  { label: t('settings.general.downloadSourceOfficial'), value: 'official' },
  { label: t('settings.general.downloadSourceBmclapi'), value: 'bmclapi' }
]);

const javaVersionOptions = computed(() => isFetchingJavaVersions.value 
  ? [{ label: `${t('settings.java.scanning')}...`, value: 0, disabled: true }] 
  : availableJavaVersions.value.map(v => ({ label: `Java ${v}`, value: v }))
);

// --- General State ---
function changeLanguage(lang: string) {
  locale.value = lang;
  localStorage.setItem('language', lang);
  localStorage.setItem('userSelectedLanguage', 'true');
  invoke('sync_ui_language', { language: lang }).catch((err) => {
    logger.warn('Failed to sync UI language to backend:', err);
  });
}

// --- AI State ---
const aiProviderType = ref<'remoteApi' | 'embeddedLlm'>('embeddedLlm');
const aiRemoteApiKey = ref('');
const aiRemoteBaseUrl = ref('');
const aiRemoteModel = ref('');
const aiActiveEmbeddedModel = ref('');
const aiMaxRamUsage = ref(4096);
const aiUnlockContextSize = ref(false);
const localModels = ref<string[]>([]);
const isFetchingModels = ref(false);

const availableRemoteModels = ref<string[]>([]);
const isTestingApi = ref(false);

async function testRemoteApi() {
  if (!aiRemoteBaseUrl.value) return;
  isTestingApi.value = true;
  try {
    const models = await invoke<string[]>('fetch_remote_models', { baseUrl: aiRemoteBaseUrl.value, apiKey: aiRemoteApiKey.value });
    availableRemoteModels.value = models;

    alert(t('settings.ai.testSuccess', { count: models.length }));
  } catch(e) {
    logger.error("API test failed", e);
    alert(t('settings.ai.testFailed', { error: String(e) }));
  } finally {
    isTestingApi.value = false;
  }
}

const recommendedModels = computed(() => [
  { name: `DeepSeek-R1-Distill-Qwen-1.5B ${t('settings.ai.deepseekHint')}`, filename: 'DeepSeek-R1-Distill-Qwen-1.5B-Q4_K_M.gguf', url: 'https://hf-mirror.com/unsloth/DeepSeek-R1-Distill-Qwen-1.5B-GGUF/resolve/main/DeepSeek-R1-Distill-Qwen-1.5B-Q4_K_M.gguf', size: '1.12 GB', parameters: '1.5B', quantization: 'Q4_K_M', recommendedMinRam: 4096 },
  { name: `Qwen2.5-7B-Instruct ${t('settings.ai.qwenHint')}`, filename: 'qwen2.5-7b-instruct-q4_k_m-00001-of-00002.gguf', urls: [
    { url: 'https://hf-mirror.com/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m-00001-of-00002.gguf', filename: 'qwen2.5-7b-instruct-q4_k_m-00001-of-00002.gguf' },
    { url: 'https://hf-mirror.com/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m-00002-of-00002.gguf', filename: 'qwen2.5-7b-instruct-q4_k_m-00002-of-00002.gguf' }
  ], size: '4.6 GB', parameters: '7B', quantization: 'Q4_K_M', recommendedMinRam: 12288 },
  { name: `Qwen2.5-14B-Instruct ${t('settings.ai.qwen14bHint')}`, filename: 'qwen2.5-14b-instruct-q4_k_m-00001-of-00003.gguf', urls: [
    { url: 'https://hf-mirror.com/Qwen/Qwen2.5-14B-Instruct-GGUF/resolve/main/qwen2.5-14b-instruct-q4_k_m-00001-of-00003.gguf', filename: 'qwen2.5-14b-instruct-q4_k_m-00001-of-00003.gguf' },
    { url: 'https://hf-mirror.com/Qwen/Qwen2.5-14B-Instruct-GGUF/resolve/main/qwen2.5-14b-instruct-q4_k_m-00002-of-00003.gguf', filename: 'qwen2.5-14b-instruct-q4_k_m-00002-of-00003.gguf' },
    { url: 'https://hf-mirror.com/Qwen/Qwen2.5-14B-Instruct-GGUF/resolve/main/qwen2.5-14b-instruct-q4_k_m-00003-of-00003.gguf', filename: 'qwen2.5-14b-instruct-q4_k_m-00003-of-00003.gguf' }
  ], size: '8.5 GB', parameters: '14B', quantization: 'Q4_K_M', recommendedMinRam: 24576 }
]);

type MemoryTag = 'recommended' | 'available' | 'tight' | 'insufficient';

const recommendedModelsWithMemory = computed(() => {
  const totalMb = systemMemory.value.totalMb;
  return recommendedModels.value
    .map(model => {
      let memoryTag: MemoryTag;
      if (totalMb >= model.recommendedMinRam * 1.5) {
        memoryTag = 'recommended';
      } else if (totalMb >= model.recommendedMinRam) {
        memoryTag = 'available';
      } else if (totalMb >= model.recommendedMinRam * 0.75) {
        memoryTag = 'tight';
      } else {
        memoryTag = 'insufficient';
      }
      return { ...model, memoryTag };
    })
    .sort((a, b) => {
      const order: Record<MemoryTag, number> = { recommended: 0, available: 1, tight: 2, insufficient: 3 };
      return order[a.memoryTag] - order[b.memoryTag];
    });
});

const downloadingModels = ref<Record<string, { downloaded: number, total: number, speed: number, parts?: Record<string, {downloaded: number, total: number, speed: number}> }>>({});

function getDownloadPercentage(filename: string) {
  const model = downloadingModels.value[filename];
  if (!model || !model.total) return 0;
  return Math.round((model.downloaded / model.total) * 100);
}



function formatModelName(filename: string) {
  return filename.replace(/-00001-of-\d{5}/, '');
}

async function downloadModel(model: { filename: string, url?: string, urls?: {url: string, filename: string}[] }) {
  if (downloadingModels.value[model.filename]) return;
  downloadingModels.value[model.filename] = { downloaded: 0, total: 1, speed: 0 };
  try {
    let targets: NonNullable<typeof model.urls> = [];
    if (model.urls) {
      targets = model.urls;
    } else if (model.url) {
      targets = [{ url: model.url, filename: model.filename }];
    }
    await invoke('download_model', { targets, mainFilename: model.filename });
  } catch(e) {
    logger.error("Failed to start model download", e);
    delete downloadingModels.value[model.filename];
  }
}

async function loadLocalModels() {
  isFetchingModels.value = true;
  try {
    localModels.value = await invoke<string[]>('list_local_models');
  } catch (err) {
    logger.error('Failed to load local models:', err);
  } finally {
    isFetchingModels.value = false;
  }
}

// Log Center State
const logStore = useLogStore();
const logLevelFilter = ref('ALL');
const logSourceFilter = ref<'ALL' | 'rust' | 'frontend'>('ALL');
const logSearchQuery = ref('');
const logViewMode = ref<'live' | 'history'>('live');
const logFiles = ref<LogFileInfo[]>([]);
const selectedLogFile = ref<string>('');
const historicalLines = ref<string[]>([]);
const isLoadingHistory = ref(false);
const isExportingLogs = ref(false);

const runtimeLevelOptions = computed(() => [
  {
    label: t('settings.logs.levelAuto', {
      level: (
        logStore.logConfig.value.configuredLevel === null
          ? logStore.logConfig.value.effectiveLevel
          : import.meta.env.DEV
            ? 'debug'
            : 'info'
      ).toUpperCase(),
    }),
    value: 'auto',
  },
  { label: 'ERROR', value: 'error' },
  { label: 'WARN', value: 'warn' },
  { label: 'INFO', value: 'info' },
  { label: 'DEBUG', value: 'debug' },
  { label: 'TRACE', value: 'trace' },
]);

const logSourceOptions = computed(() => [
  { label: t('settings.logs.allSources'), value: 'ALL' },
  { label: t('settings.logs.sourceRust'), value: 'rust' },
  { label: t('settings.logs.sourceFrontend'), value: 'frontend' },
]);

const logFileOptions = computed(() =>
  logFiles.value.length === 0
    ? [{ label: t('settings.logs.noFiles'), value: '', disabled: true }]
    : logFiles.value.map((f) => ({
        label: `${f.name} (${formatBytes(f.sizeBytes, 1)})`,
        value: f.name,
      })),
);

const runtimeLogLevelModel = computed<string>({
  get: () => logStore.logConfig.value.configuredLevel ?? 'auto',
  set: (val: string) => {
    const nextLevel = val === 'auto' ? null : (val as LogLevelSetting);
    void logStore
      .setLogLevel(nextLevel)
      .then((updated) => {
        configuredLogLevel.value = updated.configuredLevel;
        logRetentionDays.value = updated.retentionDays;
        toast.success(t('settings.logs.levelUpdated'));
      })
      .catch((err) => {
        logger.error('Failed to update runtime log level:', err);
        toast.error(t('common.operationFailed'), getErrorMessage(err));
      });
  },
});

const filteredLogs = computed(() => {
  let logs = logStore.globalLogs.value;
  if (logSourceFilter.value !== 'ALL') {
    logs = logs.filter((l) => (l.source || 'rust').toLowerCase() === logSourceFilter.value);
  }
  if (logLevelFilter.value !== 'ALL') {
    logs = logs.filter((l) => l.level.toUpperCase() === logLevelFilter.value);
  }
  if (logSearchQuery.value) {
    const q = logSearchQuery.value.toLowerCase();
    logs = logs.filter(
      (l) =>
        l.message.toLowerCase().includes(q) ||
        (l.target && l.target.toLowerCase().includes(q)) ||
        (l.task_id && l.task_id.toLowerCase().includes(q)),
    );
  }
  // Reverse to show latest first
  return [...logs].reverse();
});

const filteredHistoricalLines = computed(() => {
  if (!logSearchQuery.value) {
    return historicalLines.value;
  }
  const q = logSearchQuery.value.toLowerCase();
  return historicalLines.value.filter((line) => line.toLowerCase().includes(q));
});

const clearLogs = () => {
  logStore.clearDisplayLogs();
};

async function loadSelectedLogFile(): Promise<void> {
  if (!selectedLogFile.value) {
    historicalLines.value = [];
    return;
  }
  isLoadingHistory.value = true;
  try {
    historicalLines.value = await logStore.readLogFile(selectedLogFile.value, 500);
  } catch (err) {
    logger.error('Failed to read historical log file:', err);
    toast.error(t('common.operationFailed'), getErrorMessage(err));
    historicalLines.value = [];
  } finally {
    isLoadingHistory.value = false;
  }
}

async function refreshLogFiles(): Promise<void> {
  isLoadingHistory.value = true;
  try {
    const files = await logStore.listLogFiles();
    logFiles.value = files;
    if (files.length === 0) {
      selectedLogFile.value = '';
      historicalLines.value = [];
      return;
    }
    if (!selectedLogFile.value || !files.some((f) => f.name === selectedLogFile.value)) {
      selectedLogFile.value = files[0].name;
    }
    await loadSelectedLogFile();
  } catch (err) {
    logger.error('Failed to list log files:', err);
    toast.error(t('common.operationFailed'), getErrorMessage(err));
  } finally {
    isLoadingHistory.value = false;
  }
}

async function handleOpenLogDirectory(): Promise<void> {
  try {
    await logStore.openLogDirectory();
  } catch (err) {
    logger.error('Failed to open log directory:', err);
    toast.error(t('common.operationFailed'), getErrorMessage(err));
  }
}

async function handleExportDiagnosticLogs(): Promise<void> {
  if (isExportingLogs.value) return;
  try {
    const defaultName = `dawnland-diagnostics-${new Date().toISOString().slice(0, 10)}.zip`;
    const destPath = await save({
      defaultPath: defaultName,
      filters: [{ name: 'Zip Archive', extensions: ['zip'] }],
    });
    if (!destPath || typeof destPath !== 'string') return;
    isExportingLogs.value = true;
    const savedPath = await logStore.exportDiagnosticLogs(destPath, true);
    toast.success(t('settings.logs.exportSuccess'), savedPath);
  } catch (err) {
    logger.error('Failed to export diagnostic logs:', err);
    toast.error(t('settings.logs.exportFailed'), getErrorMessage(err));
  } finally {
    isExportingLogs.value = false;
  }
}

watch(activeTab, (tab) => {
  if (tab === 'logs') {
    void logStore.loadLogConfig().catch((err) => {
      logger.warn('Failed to refresh log config:', err);
    });
    if (logViewMode.value === 'history') {
      void refreshLogFiles();
    }
  }
});

watch(logViewMode, (mode) => {
  if (mode === 'history') {
    void refreshLogFiles();
  }
});

watch(selectedLogFile, (name) => {
  if (name && logViewMode.value === 'history') {
    void loadSelectedLogFile();
  }
});
</script>

<template>
  <div class="flex h-full p-4 gap-4 bg-transparent">
    <!-- Left Sidebar -->
    <DSidebarTabs
      :title="$t('settings.title')"
      :tabs="translatedTabs"
      v-model="activeTab"
    />

    <!-- Right Content Area -->
    <div class="flex-1 relative bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md rounded-xl border border-neutral-200/50 dark:border-zinc-800/50 shadow-sm overflow-y-auto p-6">

    <!-- General Settings Tab -->
    <div v-if="activeTab === 'general'" class="space-y-4">
      <!-- Language Settings -->
      <div class="relative z-50 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div class="flex items-center gap-2.5">
          <Languages :size="18" class="text-primary shrink-0" />
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ $t('settings.general.languageTitle') }}
            <DTooltip :content="$t('settings.general.languageDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
        </div>
        <DSelect 
          :model-value="locale"
          :options="languageOptions"
          @update:model-value="changeLanguage($event as string)"
          class="min-w-[140px]"
        />
      </div>

      <!-- Instance Inheritance Settings -->
      <div class="relative z-30 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div class="flex items-center gap-2.5">
          <Package :size="18" class="text-primary shrink-0" />
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ $t('settings.general.inheritanceTitle') }}
            <DTooltip :content="$t('settings.general.inheritanceDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
        </div>
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="enableInstanceInheritance" @change="saveLauncherSettings" class="sr-only peer">
          <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
        </label>
      </div>

      <!-- Global Game Settings -->
      <div class="relative z-25 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between gap-4 shadow-sm">
        <div class="flex items-center gap-2.5">
          <Settings class="w-[18px] h-[18px] text-primary shrink-0" />
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ $t('settings.general.globalGameSettingsTitle') }}
            <DTooltip :content="$t('settings.general.globalGameSettingsDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
        </div>
        <div class="flex items-center gap-3 shrink-0">
          <DButton v-if="enableGlobalGameSettings" variant="outline" size="sm" @click="openGlobalGameSettings">
            <Edit class="w-4 h-4 mr-1.5" />
            {{ $t('settings.general.editGlobalGameSettings') }}
          </DButton>
          <label class="relative inline-flex items-center cursor-pointer">
            <input type="checkbox" v-model="enableGlobalGameSettings" @change="saveLauncherSettings" class="sr-only peer">
            <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
          </label>
        </div>
      </div>

      <!-- Global Mod Cache -->
      <div class="relative z-24 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md space-y-3.5 shadow-sm">
        <div class="flex items-center justify-between gap-4">
          <div class="flex items-center gap-2.5">
            <RefreshCw class="w-[18px] h-[18px] text-primary shrink-0" />
            <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
              {{ $t('settings.general.globalModCacheTitle') }}
              <DTooltip :content="$t('settings.general.globalModCacheDesc')">
                <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                  <CircleHelp :size="15" />
                </span>
              </DTooltip>
            </h2>
          </div>
          <div class="flex items-center gap-3 shrink-0">
            <DButton
              v-if="enableGlobalModCache"
              variant="outline"
              size="sm"
              @click="cleanGlobalModCache"
              :disabled="isCleaningCache"
            >
              <Loader2 v-if="isCleaningCache" class="w-4 h-4 mr-1.5 animate-spin" />
              <Trash2 v-else class="w-4 h-4 mr-1.5" />
              {{ $t('settings.general.cleanGlobalModCache') }}
            </DButton>
            <label class="relative inline-flex items-center cursor-pointer">
              <input type="checkbox" v-model="enableGlobalModCache" @change="saveLauncherSettings" class="sr-only peer">
              <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
            </label>
          </div>
        </div>

        <div
          v-if="enableGlobalModCache"
          class="pt-3 border-t border-black/5 dark:border-white/5 space-y-3"
        >
          <div class="flex items-center justify-between gap-4">
            <span class="text-sm font-medium flex items-center gap-1.5 text-foreground">
              {{ $t('settings.general.autoCleanCacheTitle') }}
              <DTooltip :content="$t('settings.general.autoCleanCacheDesc')">
                <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                  <CircleHelp :size="14" />
                </span>
              </DTooltip>
            </span>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" v-model="autoCleanCache" @change="saveLauncherSettings" class="sr-only peer">
              <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
            </label>
          </div>

          <div class="flex items-center justify-between gap-4">
            <span class="text-sm font-medium text-foreground">
              {{ $t('settings.general.cacheRetentionPolicyTitle') }}
            </span>
            <DSelect
              v-model="cacheRetentionPolicy"
              :options="cacheRetentionOptions"
              :disabled="!autoCleanCache"
              @update:model-value="saveLauncherSettings"
              class="min-w-[240px]"
            />
          </div>

          <div class="flex items-center justify-between gap-4">
            <span class="text-sm font-medium flex items-center gap-1.5 text-foreground">
              {{ $t('settings.general.autoCleanSharedAssetsTitle') }}
              <DTooltip :content="$t('settings.general.autoCleanSharedAssetsDesc')">
                <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                  <CircleHelp :size="14" />
                </span>
              </DTooltip>
            </span>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" v-model="autoCleanSharedAssets" @change="saveLauncherSettings" class="sr-only peer">
              <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
            </label>
          </div>

          <p
            v-if="lastGcReport && lastGcReport.freedBytes > 0"
            class="text-xs text-muted-foreground pt-1"
          >
            {{
              $t('settings.general.lastCacheCleaned', {
                size: formatBytes(lastGcReport.freedBytes, 2),
                pools: lastGcReport.removedPoolObjects + lastGcReport.removedTmpFiles,
                modCache: lastGcReport.removedModCacheEntries + lastGcReport.removedModIcons,
                versions: lastGcReport.removedVersionCaches,
                shared: lastGcReport.removedAssets + lastGcReport.removedLibraries,
              })
            }}
          </p>
        </div>
      </div>

      <!-- Smart JVM Tuning -->
      <div class="relative z-23 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div class="flex items-center gap-2.5">
          <Gauge :size="18" class="text-primary shrink-0" />
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ $t('settings.general.jvmTunerTitle') }}
            <DTooltip :content="$t('settings.general.jvmTunerDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
        </div>
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="enableJvmTuner" @change="saveLauncherSettings" class="sr-only peer">
          <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
        </label>
      </div>

      <!-- Community Resource Naming & Display Settings -->
      <div class="relative z-[21] rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm space-y-4">
        <div class="flex items-center justify-between gap-4">
          <div class="flex items-center gap-2.5">
            <Languages :size="18" class="text-primary shrink-0" />
            <div>
              <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
                {{ $t('settings.resourceNaming.sectionTitle', '社区资源命名与译名显示') }}
                <DTooltip :content="$t('settings.resourceNaming.sectionDesc', '自动识别并应用社区公认中文译名，支持自定义下载文件名格式与本地列表显示方式。')">
                  <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                    <CircleHelp :size="15" />
                  </span>
                </DTooltip>
              </h2>
              <p v-if="translationStats" class="text-xs text-muted-foreground mt-0.5">
                {{ $t('settings.resourceNaming.dbStatsSummary', { total: translationStats.totalEntries, version: translationStats.snapshotVersion }) }}
              </p>
            </div>
          </div>

          <div class="flex items-center gap-3">
            <DButton v-if="enableOnlineTranslation" variant="outline" size="sm" :disabled="isSyncingTranslations" @click="syncTranslationsNow">
              <Loader2 v-if="isSyncingTranslations" class="w-3.5 h-3.5 mr-1.5 animate-spin" />
              <RefreshCw v-else class="w-3.5 h-3.5 mr-1.5" />
              {{ $t('settings.resourceNaming.syncDbBtn', '同步最新词库') }}
            </DButton>

            <div class="flex items-center gap-2">
              <span class="text-xs font-medium select-none" :class="enableOnlineTranslation ? 'text-primary' : 'text-muted-foreground'">
                {{ enableOnlineTranslation ? $t('settings.resourceNaming.toggleEnabledLabel', '自动重命名已开启') : $t('settings.resourceNaming.toggleDisabledLabel', '已停用') }}
              </span>
              <label class="relative inline-flex items-center cursor-pointer" :title="$t('settings.resourceNaming.toggleTitle', '开启或关闭自动重命名与中文译名功能')">
                <input type="checkbox" v-model="enableOnlineTranslation" @change="saveLauncherSettings" class="sr-only peer">
                <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
              </label>
            </div>
          </div>
        </div>

        <!-- Auto-rename Feature Banner -->
        <div
          class="rounded-md px-3.5 py-2.5 text-xs flex items-start gap-2.5 transition-colors border"
          :class="enableOnlineTranslation 
            ? 'bg-primary/5 border-primary/20 text-foreground' 
            : 'bg-muted/40 border-muted text-muted-foreground'"
        >
          <Sparkles v-if="enableOnlineTranslation" class="w-4 h-4 mt-0.5 shrink-0 text-primary" />
          <Info v-else class="w-4 h-4 mt-0.5 shrink-0 text-muted-foreground" />
          <div class="leading-relaxed">
            <span v-if="enableOnlineTranslation">
              <strong class="font-semibold text-primary mr-1">{{ $t('settings.resourceNaming.autoRenameActiveTitle', '自动重命名已开启：') }}</strong>
              {{ $t('settings.resourceNaming.autoRenameTipEnabled', '新安装的模组、资源包、光影包、数据包及整合包中的对应资源落盘时，将自动按规则规范重命名（包含中文译名），并在实例内展示译名。') }}
            </span>
            <span v-else>
              <strong class="font-semibold mr-1">{{ $t('settings.resourceNaming.autoRenameInactiveTitle', '自动重命名已关闭：') }}</strong>
              {{ $t('settings.resourceNaming.autoRenameTipDisabled', '新安装资源及整合包将保留原始英文文件名；实例管理中的“整理文件名”批量修改等功能也将一并禁用。') }}
            </span>
          </div>
        </div>

        <template v-if="enableOnlineTranslation">
        <!-- Filename Format Mode -->
        <div class="grid grid-cols-1 md:grid-cols-2 gap-3 pt-1">
          <div class="space-y-1.5">
            <label class="text-xs font-medium text-muted-foreground block">
              {{ $t('settings.resourceNaming.formatModeLabel', '文件名重命名规则') }}
            </label>
            <DSelect
              v-model="filenameMode"
              :options="filenameModeOptions"
              @update:model-value="saveLauncherSettings"
            />
          </div>

          <div v-if="filenameMode === 'preset'" class="space-y-1.5">
            <label class="text-xs font-medium text-muted-foreground block">
              {{ $t('settings.resourceNaming.presetLabel', '预设文件名格式') }}
            </label>
            <DSelect
              v-model="filenamePreset"
              :options="filenamePresetOptions"
              @update:model-value="saveLauncherSettings"
            />
          </div>

          <div v-else-if="filenameMode === 'auto'" class="space-y-1.5">
            <label class="text-xs font-medium text-muted-foreground block">
              {{ $t('settings.resourceNaming.autoEffectiveLabel', '当前自动生效规则') }}
            </label>
            <div class="h-10 px-3 rounded-md border border-neutral-200 dark:border-zinc-800 bg-neutral-50/70 dark:bg-zinc-900/70 flex items-center text-xs font-mono text-foreground">
              {{ effectivePreviewTemplate() }}
            </div>
          </div>
        </div>

        <!-- Custom Template Editor + Placeholder Chips -->
        <div v-if="filenameMode === 'custom'" class="space-y-2.5">
          <div class="space-y-1">
            <label class="text-xs font-medium text-muted-foreground block">
              {{ $t('settings.resourceNaming.customTemplateLabel', '自定义占位符模板') }}
            </label>
            <DInput
              v-model="customTemplate"
              @change="saveLauncherSettings"
              placeholder="[{zh}] {stem}{ext}"
              class="font-mono text-xs"
            />
          </div>
          <div class="flex flex-wrap gap-1.5">
            <DButton
              v-for="item in PLACEHOLDER_TOKENS"
              :key="item.token"
              variant="outline"
              size="sm"
              class="h-6 px-2 text-[11px] font-mono"
              @click="appendPlaceholderToken(item.token)"
            >
              {{ item.token }} · {{ $t(item.labelKey) }}
            </DButton>
          </div>
        </div>

        <!-- Live Preview Box -->
        <div class="rounded-md bg-neutral-100/80 dark:bg-zinc-800/60 border border-neutral-200/80 dark:border-zinc-700/60 p-3 space-y-1 text-xs">
          <div class="flex items-center justify-between gap-2">
            <span class="text-muted-foreground">{{ $t('settings.resourceNaming.previewWithZh', '有译名时预览:') }}</span>
            <span class="font-mono font-medium text-foreground break-all">{{ templatePreview.rendered }}</span>
          </div>
          <div class="flex items-center justify-between gap-2">
            <span class="text-muted-foreground">{{ $t('settings.resourceNaming.previewFallback', '无译名时回退:') }}</span>
            <span class="font-mono text-muted-foreground break-all">{{ templatePreview.fallbackRendered }}</span>
          </div>
          <div v-if="templatePreview.warnings.length > 0" class="text-[11px] text-amber-600 dark:text-amber-400 pt-1">
            {{ templatePreview.warnings.map(w => $t(`settings.resourceNaming.warn.${w}`)).join(' · ') }}
          </div>
        </div>

        <!-- Management Display Style & Scope -->
        <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
          <div class="space-y-1.5">
            <label class="text-xs font-medium text-muted-foreground block">
              {{ $t('settings.resourceNaming.displayStyleLabel', '资源管理显示样式') }}
            </label>
            <DSelect
              v-model="managementDisplayStyle"
              :options="managementDisplayOptions"
              @update:model-value="saveLauncherSettings"
            />
          </div>

          <div class="space-y-1.5">
            <label class="text-xs font-medium text-muted-foreground block">
              {{ $t('settings.resourceNaming.targetsLabel', '自动重命名适用资源类型') }}
            </label>
            <DMultiSelect
              v-model="renameTargets"
              :options="renameTargetOptions"
              @update:model-value="saveLauncherSettings"
            />
          </div>
        </div>
        </template>
      </div>

      <!-- Download Source Settings -->
      <div class="relative z-20 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div class="flex items-center gap-2.5">
          <Download :size="18" class="text-primary shrink-0" />
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ $t('settings.general.downloadSourceTitle') }}
            <DTooltip :content="$t('settings.general.downloadSourceDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
        </div>
        <DSelect 
          v-model="downloadSource"
          :options="downloadSourceOptions"
          @update:model-value="saveLauncherSettings"
          class="min-w-[140px]"
        />
      </div>

      <!-- Concurrent Downloads Settings -->
      <div class="relative z-10 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
        <div class="flex items-center justify-between mb-3">
          <div class="flex items-center gap-2.5">
            <Download :size="18" class="text-primary shrink-0" />
            <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
              {{ $t('settings.general.concurrentDownloadsTitle') }}
              <DTooltip :content="$t('settings.general.concurrentDownloadsDesc')">
                <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                  <CircleHelp :size="15" />
                </span>
              </DTooltip>
            </h2>
          </div>
          <span class="text-sm font-mono text-primary font-semibold">{{ maxConcurrentDownloads }}</span>
        </div>
        <div class="space-y-1.5">
          <input
            v-model.number="maxConcurrentDownloads"
            @change="saveLauncherSettings"
            type="range"
            min="1"
            max="128"
            step="1"
            class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-primary"
          />
          <div class="flex justify-between text-xs text-muted-foreground">
            <span>1</span>
            <span>128</span>
          </div>
        </div>
      </div>

      <!-- Telemetry Settings -->
      <div class="relative z-5 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div class="flex items-center gap-2.5">
          <Shield :size="18" class="text-primary shrink-0" />
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ $t('settings.general.telemetryTitle') }}
            <DTooltip :content="$t('settings.general.telemetryDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
        </div>
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="enableTelemetry" @change="saveLauncherSettings" class="sr-only peer">
          <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white dark:peer-checked:after:border-zinc-900 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white dark:peer-checked:after:bg-zinc-900 after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
        </label>
      </div>

      <!-- Global Memory Settings -->
      <div class="relative z-0 rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ $t('settings.general.memoryTitle') }}
            <DTooltip :content="$t('settings.general.recommended', { recommended: systemMemory.recommendedMaxMb })">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
          <span class="text-sm font-mono text-primary font-semibold">{{ defaultMaxMemory }} MB</span>
        </div>
        <div class="space-y-1.5">
          <input
            v-model.number="defaultMaxMemory"
            type="range"
            min="512"
            :max="systemMemory.totalMb"
            step="512"
            class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-primary"
            @change="saveLauncherSettings"
          />
          <div class="flex justify-between text-xs text-muted-foreground">
            <span>512 MB</span>
            <span>{{ $t('settings.general.system') }}: {{ systemMemory.totalMb }} MB</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Java Management Tab -->
    <div v-if="activeTab === 'java'" class="space-y-6">
      <div class="rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-lg font-semibold">{{ $t('settings.java.title') }}</h2>
          <div class="flex gap-2">
            <button
              class="flex items-center gap-2 rounded-lg bg-secondary px-3 py-1.5 text-sm font-medium hover:bg-secondary/80 disabled:opacity-50"
              :disabled="isFullDiskScanning"
              @click="scanFullDisk"
            >
              <Loader2 v-if="isFullDiskScanning" :size="14" class="animate-spin" />
              <Search v-else :size="14" />
              {{ isFullDiskScanning ? $t('settings.java.scanning') : $t('settings.java.fullScan') }}
            </button>
            <button
              class="flex items-center gap-2 rounded-lg bg-secondary px-3 py-1.5 text-sm font-medium hover:bg-secondary/80"
              @click="addManualJava"
            >
              <Plus :size="14" />
              {{ $t('settings.java.addLocal') }}
            </button>
            <button
              class="flex items-center gap-2 rounded-lg bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
              :disabled="isScanningJava"
              @click="async () => { await invoke('clear_java_cache'); await scanLocalJavas(); }"
            >
              <Loader2 v-if="isScanningJava" :size="14" class="animate-spin" />
              <Coffee v-else :size="14" />
              {{ isScanningJava ? $t('settings.java.scanning') : $t('settings.java.refresh') }}
            </button>
          </div>
        </div>

      <!-- Scan Progress -->
      <div v-if="isFullDiskScanning" class="mb-4 text-xs text-muted-foreground flex items-center gap-2">
        <Loader2 :size="12" class="animate-spin" />
        <span class="truncate">{{ fullDiskScanPath }}</span>
      </div>

      <!-- Custom Download Path -->
      <div class="mb-4 space-y-1">
        <label class="text-sm font-medium">{{ $t('settings.java.customPath') }}</label>
        <div class="flex gap-2">
          <DInput
            v-model="customJavaDownloadPath"
            readonly
            :placeholder="$t('settings.java.defaultPath')"
            class="flex-1"
          />
          <button
            class="rounded-lg bg-secondary px-3 py-1.5 text-sm font-medium hover:bg-secondary/80"
            @click="chooseJavaDownloadPath"
          >
            <FolderOpen :size="14" />
          </button>
          <button
            v-if="customJavaDownloadPath"
            class="rounded-lg bg-red-100 text-red-600 px-3 py-1.5 text-sm font-medium hover:bg-red-200 dark:bg-red-900/30 dark:hover:bg-red-900/50"
            @click="clearJavaDownloadPath"
          >
            <Trash2 :size="14" />
          </button>
        </div>
      </div>

      <!-- Download Java Section -->
      <div class="mb-4 p-3 bg-muted/30 rounded-lg">
        <p class="text-sm text-muted-foreground mb-3">{{ $t('settings.java.download') }}</p>
        <div class="flex items-center gap-2">
          <DSelect 
            v-model="selectedJavaVersion"
            :options="javaVersionOptions"
            :disabled="isFetchingJavaVersions || isDownloadingJava"
          />
          <button
            class="flex items-center gap-1.5 px-4 py-1.5 text-sm font-medium bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50"
            :disabled="isDownloadingJava"
            @click="downloadJava(selectedJavaVersion)"
          >
            <Loader2 v-if="isDownloadingJava" :size="14" class="animate-spin" />
            <Download v-else :size="14" />
            {{ $t('settings.java.downloadBtn') }}
          </button>
        </div>
        <!-- Download progress -->
        <div v-if="isDownloadingJava" class="mt-3">
          <div class="flex items-center justify-between text-sm mb-1">
            <span>{{ $t('settings.java.downloading', { version: downloadingVersion }) }}</span>
            <span>{{ javaDownloadProgress }}%</span>
          </div>
          <div class="w-full h-2 bg-gray-200 rounded-full dark:bg-zinc-700">
            <div
              class="h-full bg-blue-500 rounded-full transition-all"
              :style="{ width: `${javaDownloadProgress}%` }"
            ></div>
          </div>
          <div class="flex items-center justify-between text-xs text-muted-foreground mt-1">
            <span>{{ formatBytes(javaDownloadedBytes) }} / {{ formatBytes(javaTotalBytes) }}</span>
            <span class="font-mono tabular-nums">{{ javaDownloadSpeed }}</span>
          </div>
        </div>
      </div>

      <!-- Installed Java List -->
      <div class="space-y-2">
        <p class="text-sm font-medium">{{ $t('settings.java.installed') }} ({{ installedJavas.length }})</p>
        <div v-if="installedJavas.length === 0" class="text-sm text-muted-foreground py-2">
          {{ $t('settings.java.noInstalled') }}
        </div>
        <div
          v-for="java in installedJavas"
          :key="java.path"
          class="flex items-center justify-between p-3 border rounded-lg bg-muted/20"
        >
          <div class="flex items-center gap-3 overflow-hidden">
            <Coffee class="h-5 w-5 text-orange-500 shrink-0" />
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <p class="text-sm font-medium">Java {{ java.majorVersion }} ({{ java.vendor }})</p>
                <span v-if="java.isOpenJ9" class="inline-flex items-center rounded-full bg-yellow-100 px-2 py-0.5 text-[10px] font-medium text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300">OpenJ9</span>
                <span v-else-if="java.isGraalvm" class="inline-flex items-center rounded-full bg-blue-100 px-2 py-0.5 text-[10px] font-medium text-blue-800 dark:bg-blue-900/40 dark:text-blue-300">GraalVM</span>
                <span v-else class="inline-flex items-center rounded-full bg-emerald-100 px-2 py-0.5 text-[10px] font-medium text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300">HotSpot</span>
              </div>
              <p class="text-xs text-muted-foreground truncate" :title="java.path">{{ java.path }}</p>
            </div>
          </div>
          <div class="flex items-center gap-4 shrink-0 pl-3">
            <span class="text-xs text-muted-foreground hidden sm:block">{{ java.versionString }}</span>
            <button
              class="p-1.5 text-muted-foreground hover:text-red-500 hover:bg-red-50 rounded-md transition-colors dark:hover:bg-red-950"
              @click="removeJava(java.path)"
              title="Remove Java"
            >
              <Trash2 :size="16" />
            </button>
          </div>
        </div>
        </div>
      </div>
    </div>

    <!-- Authlib Management Tab -->
    <div v-if="activeTab === 'authlib'" class="space-y-6">
      <div class="rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
        <h2 class="text-lg font-semibold mb-4">{{ $t('settings.authlib.title') }}</h2>
        
        <!-- Add Server -->
        <div class="mb-6 space-y-1">
          <label class="text-sm font-medium">{{ $t('settings.authlib.addServer') }}</label>
          <div class="flex gap-2">
            <DInput
              v-model="newAuthlibUrl"
              :placeholder="$t('settings.authlib.addServerPlaceholder')"
              class="flex-1"
              @keyup.enter="addAuthlibServer"
            />
            <button
              class="flex items-center gap-1.5 rounded-lg bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
              :disabled="isAddingAuthlibServer || !newAuthlibUrl.trim()"
              @click="addAuthlibServer"
            >
              <Loader2 v-if="isAddingAuthlibServer" :size="14" class="animate-spin" />
              <Plus v-else :size="14" />
              {{ $t('settings.authlib.addBtn') }}
            </button>
          </div>
        </div>

        <!-- Servers List -->
        <div class="space-y-2">
          <p class="text-sm font-medium">{{ $t('settings.authlib.addedServers') }} ({{ authlibServers.length }})</p>
          <div v-if="isFetchingAuthlibServers" class="py-4 flex justify-center">
            <Loader2 class="animate-spin text-muted-foreground" :size="24" />
          </div>
          <div v-else-if="authlibServers.length === 0" class="text-sm text-muted-foreground py-2">
            {{ $t('settings.authlib.noServers') }}
          </div>
          <div
            v-else
            v-for="server in authlibServers"
            :key="server.url"
            class="flex items-center justify-between p-3 border rounded-lg bg-muted/20"
          >
            <div class="min-w-0">
              <p class="text-sm font-medium">{{ server.name }}</p>
              <p class="text-xs text-muted-foreground truncate mt-0.5" :title="server.url">{{ server.url }}</p>
            </div>
            <button
              class="p-1.5 text-muted-foreground hover:text-red-500 hover:bg-red-50 rounded-md transition-colors dark:hover:bg-red-950 shrink-0 ml-2"
              @click="removeAuthlibServer(server.url)"
              title="Remove Server"
            >
              <Trash2 :size="16" />
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- AI Settings Tab -->
    <div v-if="activeTab === 'ai'" class="space-y-6">
      <div class="rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
        <h2 class="text-lg font-semibold mb-4">{{ $t('settings.ai.title') }}</h2>
        
        <div class="space-y-4">
          <div>
            <label class="text-sm font-medium">{{ $t('settings.ai.provider') }}</label>
            <DSelect
              v-model="aiProviderType"
              :options="[{ label: $t('settings.ai.embedded'), value: 'embeddedLlm' }, { label: $t('settings.ai.remote'), value: 'remoteApi' }]"
              @update:model-value="saveLauncherSettings"
            />
          </div>

          <template v-if="aiProviderType === 'embeddedLlm'">
            <div class="p-4 bg-muted/30 rounded-lg space-y-4">
              <div class="flex items-center justify-between mb-2">
                <label class="text-sm font-medium">{{ $t('settings.ai.activeModel') }}</label>
                <button
                  class="flex items-center gap-2 rounded-lg bg-secondary px-3 py-1.5 text-xs font-medium hover:bg-secondary/80 disabled:opacity-50"
                  :disabled="isFetchingModels"
                  @click="loadLocalModels"
                >
                  <Loader2 v-if="isFetchingModels" :size="12" class="animate-spin" />
                  <Search v-else :size="12" />
                  {{ $t('settings.java.refresh') }}
                </button>
              </div>
              <DSelect
                v-model="aiActiveEmbeddedModel"
                :options="localModels.map(m => ({ label: formatModelName(m), value: m }))"
                @update:model-value="saveLauncherSettings"
              />
              <p v-if="localModels.length === 0" class="text-xs text-muted-foreground">{{ $t('settings.ai.noModels') }}</p>
              
              <div class="space-y-2 mt-4">
                <div class="flex items-center justify-between">
                  <label class="text-sm font-medium">{{ $t('settings.ai.maxRam') }}</label>
                  <span class="text-sm font-mono text-primary">{{ aiMaxRamUsage }} MB</span>
                </div>
                <input
                  v-model.number="aiMaxRamUsage"
                  type="range"
                  min="2048"
                  :max="systemMemory.totalMb"
                  step="512"
                  class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-blue-500"
                  @change="saveLauncherSettings"
                />
              </div>
            </div>

            <!-- Unlock Context Size Option -->
            <div class="relative rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
              <div class="flex items-center gap-2.5">
                <span class="i-lucide-unlock w-4 h-4 text-orange-500 shrink-0"></span>
                <h3 class="text-sm font-semibold flex items-center gap-1.5 text-foreground">
                  {{ $t('settings.ai.unlockContext') }}
                  <DTooltip :content="$t('settings.ai.unlockContextDesc')">
                    <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                      <CircleHelp :size="14" />
                    </span>
                  </DTooltip>
                </h3>
              </div>
              <label class="relative inline-flex items-center cursor-pointer">
                <input type="checkbox" v-model="aiUnlockContextSize" @change="saveLauncherSettings" class="sr-only peer">
                <div class="w-11 h-6 bg-zinc-200 peer-focus:outline-none rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-zinc-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-zinc-600 peer-checked:bg-primary"></div>
              </label>
            </div>

            <!-- Model Hub -->
            <div class="p-4 bg-muted/30 rounded-lg space-y-4">
              <div class="flex items-center justify-between">
                <h3 class="text-sm font-medium">{{ $t('settings.ai.modelHub') }}</h3>
                <span class="text-xs text-muted-foreground">{{ $t('settings.ai.systemMemoryInfo') }}: {{ Math.round(systemMemory.totalMb / 1024) }} GB</span>
              </div>
              <div class="space-y-3">
                <div v-for="model in recommendedModelsWithMemory" :key="model.filename"
                  class="flex flex-col gap-2 p-3 rounded-lg border transition-colors"
                  :class="{
                    'bg-green-50/60 dark:bg-green-900/15 border-green-200 dark:border-green-800/50': model.memoryTag === 'recommended',
                    'bg-blue-50/50 dark:bg-blue-900/10 border-blue-200 dark:border-blue-800/40': model.memoryTag === 'available',
                    'bg-amber-50/50 dark:bg-amber-900/10 border-amber-200 dark:border-amber-800/40': model.memoryTag === 'tight',
                    'bg-red-50/50 dark:bg-red-900/10 border-red-200 dark:border-red-800/40': model.memoryTag === 'insufficient'
                  }">
                  <div class="flex items-center justify-between">
                    <div class="flex items-center gap-2 min-w-0">
                      <p class="text-sm font-medium truncate">{{ model.name }}</p>
                      <span v-if="model.memoryTag === 'recommended'"
                        class="shrink-0 inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-semibold rounded-full bg-green-100 text-green-700 dark:bg-green-900/50 dark:text-green-400">
                        {{ $t('settings.ai.recommendedForSystem') }}
                      </span>
                      <span v-else-if="model.memoryTag === 'tight'"
                        class="shrink-0 inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-semibold rounded-full bg-amber-100 text-amber-700 dark:bg-amber-900/50 dark:text-amber-400">
                        {{ $t('settings.ai.memoryTight') }}
                      </span>
                      <span v-else-if="model.memoryTag === 'insufficient'"
                        class="shrink-0 inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-semibold rounded-full bg-red-100 text-red-700 dark:bg-red-900/50 dark:text-red-400">
                        {{ $t('settings.ai.memoryInsufficient') }}
                      </span>
                    </div>
                    <button
                      v-if="!localModels.includes(model.filename) && !downloadingModels[model.filename]"
                      @click="downloadModel(model)"
                      class="shrink-0 flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors"
                    >
                      <Download :size="12" /> {{ $t('settings.ai.download') }}
                    </button>
                    <span v-else-if="localModels.includes(model.filename)" class="shrink-0 text-xs font-medium text-green-600 bg-green-100 dark:bg-green-900/40 dark:text-green-400 px-2 py-1 rounded-md">
                      {{ $t('settings.ai.installed') }}
                    </span>
                  </div>
                  <div class="flex items-center gap-2 text-xs text-muted-foreground">
                    <span>{{ model.size }}</span>
                    <span class="text-zinc-300 dark:text-zinc-600">·</span>
                    <span>{{ model.parameters }}</span>
                    <span class="text-zinc-300 dark:text-zinc-600">·</span>
                    <span>{{ model.quantization }}</span>
                    <span class="text-zinc-300 dark:text-zinc-600">·</span>
                    <span>{{ $t('settings.ai.minRamRequired') }} {{ Math.round(model.recommendedMinRam / 1024) }} GB</span>
                  </div>
                  <!-- Progress bar -->
                  <div v-if="downloadingModels[model.filename]" class="w-full">
                    <div class="flex justify-between text-xs mb-1">
                      <span class="font-mono tabular-nums">{{ $t('settings.ai.downloading') }} ({{ formatSpeed(downloadingModels[model.filename].speed) || '0 B/s' }})</span>
                      <span>{{ getDownloadPercentage(model.filename) }}%</span>
                    </div>
                    <div class="w-full h-1.5 bg-gray-200 rounded-full dark:bg-zinc-700 overflow-hidden">
                      <div class="h-full bg-blue-500 rounded-full transition-all" :style="{ width: `${getDownloadPercentage(model.filename)}%` }"></div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </template>

          <template v-if="aiProviderType === 'remoteApi'">
            <div class="space-y-4 p-4 bg-muted/30 rounded-lg">
              <div>
                <label class="text-sm font-medium">Base URL</label>
                <DInput v-model="aiRemoteBaseUrl" placeholder="https://api.openai.com/v1" @blur="saveLauncherSettings" />
              </div>
              <div>
                <label class="text-sm font-medium">API Key</label>
                <DInput v-model="aiRemoteApiKey" type="password" placeholder="sk-..." @blur="saveLauncherSettings" />
              </div>
              
              <div class="pt-2">
                <button
                  @click="testRemoteApi"
                  :disabled="isTestingApi || !aiRemoteBaseUrl"
                  class="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground font-medium rounded-lg hover:bg-primary/90 disabled:opacity-50 transition-colors"
                >
                  <span v-if="isTestingApi" class="i-lucide-loader-2 w-4 h-4 animate-spin"></span>
                  <span v-else class="i-lucide-zap w-4 h-4"></span>
                  {{ isTestingApi ? $t('settings.ai.testingApi') : $t('settings.ai.testApi') }}
                </button>
              </div>
              
              <div class="mt-4">
                <label class="text-sm font-medium">{{ $t('settings.ai.selectModel') }}</label>
                <DCombobox
                  v-model="aiRemoteModel"
                  :options="availableRemoteModels.map(m => ({ label: m, value: m }))"
                  @blur="saveLauncherSettings"
                  :placeholder="$t('settings.ai.modelPlaceholder')"
                />
              </div>
            </div>
          </template>
        </div>
      </div>
    </div>

    <!-- Logs Tab -->
    <div v-if="activeTab === 'logs'" class="h-full flex flex-col">
      <div class="flex flex-wrap items-start justify-between gap-4 mb-4 shrink-0">
        <div>
          <h2 class="text-lg font-bold">{{ $t('settings.tabs.logs') }}</h2>
          <p class="text-sm text-muted-foreground">{{ $t('settings.logs.desc') }}</p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <div class="flex items-center gap-2">
            <span class="text-xs text-muted-foreground whitespace-nowrap">{{ $t('settings.logs.runtimeLevelLabel') }}</span>
            <DSelect
              v-model="runtimeLogLevelModel"
              :options="runtimeLevelOptions"
              class="w-36"
            />
          </div>
          <DButton @click="handleOpenLogDirectory" variant="outline" size="sm" class="gap-1.5">
            <FolderOpen class="w-4 h-4" /> {{ $t('settings.logs.openDir') }}
          </DButton>
          <DButton @click="handleExportDiagnosticLogs" variant="outline" size="sm" :disabled="isExportingLogs" class="gap-1.5">
            <Loader2 v-if="isExportingLogs" class="w-4 h-4 animate-spin" />
            <Download v-else class="w-4 h-4" />
            {{ $t('settings.logs.exportZip') }}
          </DButton>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-3 mb-4 shrink-0">
        <div class="flex items-center gap-1 rounded-lg border bg-card/40 p-1">
          <DButton
            :variant="logViewMode === 'live' ? 'primary' : 'ghost'"
            size="sm"
            @click="logViewMode = 'live'"
          >
            {{ $t('settings.logs.liveMode') }}
          </DButton>
          <DButton
            :variant="logViewMode === 'history' ? 'primary' : 'ghost'"
            size="sm"
            @click="logViewMode = 'history'"
          >
            {{ $t('settings.logs.historyMode') }}
          </DButton>
        </div>

        <DInput v-model="logSearchQuery" :placeholder="$t('settings.logs.search')" class="flex-1 min-w-[200px]" :icon="Search" />

        <template v-if="logViewMode === 'live'">
          <DSelect
            v-model="logSourceFilter"
            :options="logSourceOptions"
            class="w-36"
          />
          <DSelect
            v-model="logLevelFilter"
            :options="[
              { label: $t('settings.logs.allLevels'), value: 'ALL' },
              { label: 'ERROR', value: 'ERROR' },
              { label: 'WARN', value: 'WARN' },
              { label: 'INFO', value: 'INFO' },
              { label: 'DEBUG', value: 'DEBUG' },
              { label: 'TRACE', value: 'TRACE' }
            ]"
            class="w-32"
          />
          <DButton @click="clearLogs" variant="danger" size="sm" class="gap-1.5">
            <Trash2 class="w-4 h-4" /> {{ $t('settings.logs.clear') }}
          </DButton>
        </template>

        <template v-else>
          <DSelect
            v-model="selectedLogFile"
            :options="logFileOptions"
            :placeholder="$t('settings.logs.selectFile')"
            class="w-64"
          />
          <DButton @click="refreshLogFiles" variant="outline" size="sm" :disabled="isLoadingHistory" class="gap-1.5">
            <RefreshCw :class="['w-4 h-4', { 'animate-spin': isLoadingHistory }]" />
            {{ $t('settings.logs.refreshFiles') }}
          </DButton>
        </template>
      </div>

      <div v-if="logViewMode === 'live'" class="bg-card/50 border rounded-lg p-4 space-y-1.5 flex-1 overflow-y-auto relative">
        <div v-if="filteredLogs.length === 0" class="absolute inset-0 flex items-center justify-center text-muted-foreground">
          {{ $t('settings.logs.noLogs') }}
        </div>
        <div
          v-for="(log, i) in filteredLogs"
          :key="log.seq ?? `${log.timestamp}-${i}`"
          class="font-mono text-xs sm:text-sm break-all leading-relaxed"
        >
          <span class="text-muted-foreground mr-2">[{{ log.timestamp }}]</span>
          <span :class="{
            'text-blue-500': log.level.toUpperCase() === 'INFO',
            'text-yellow-500': log.level.toUpperCase() === 'WARN',
            'text-red-500': log.level.toUpperCase() === 'ERROR',
            'text-gray-500': log.level.toUpperCase() === 'DEBUG',
            'text-slate-400': log.level.toUpperCase() === 'TRACE',
          }" class="font-bold mr-2">[{{ log.level.toUpperCase() }}]</span>
          <span class="text-cyan-600 dark:text-cyan-400 mr-2">[{{ log.source === 'frontend' ? `FE:${log.target}` : log.target }}]</span>
          <span v-if="log.task_id" class="text-purple-500 mr-2">[{{ log.task_id }}]</span>
          <span class="text-foreground/90">{{ log.message }}</span>
        </div>
      </div>

      <div v-else class="bg-card/50 border rounded-lg p-4 space-y-1 flex-1 overflow-y-auto relative">
        <div v-if="isLoadingHistory" class="absolute inset-0 flex items-center justify-center text-muted-foreground">
          <Loader2 class="w-5 h-5 animate-spin" />
        </div>
        <div v-else-if="filteredHistoricalLines.length === 0" class="absolute inset-0 flex items-center justify-center text-muted-foreground">
          {{ logFiles.length === 0 ? $t('settings.logs.noFiles') : $t('settings.logs.noLogs') }}
        </div>
        <div
          v-else
          v-for="(line, i) in filteredHistoricalLines"
          :key="i"
          class="font-mono text-xs break-all text-foreground/90 whitespace-pre-wrap"
        >{{ line }}</div>
      </div>
    </div>

    <!-- About Tab -->
    <div v-if="activeTab === 'about'" class="h-full">
      <div class="rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex flex-col items-center justify-center text-center shadow-sm min-h-full">
        <div class="w-16 h-16 bg-primary/10 rounded-2xl flex items-center justify-center mb-3 text-primary">
          <!-- Temporary logo placeholder -->
          <Package :size="34" />
        </div>
        <h2 class="text-2xl font-bold">Dawnland Launcher</h2>
        <p class="text-sm text-muted-foreground mt-1 mb-4">{{ $t('settings.about.desc') }}</p>
        
        <div class="grid grid-cols-2 gap-3 w-full max-w-md text-left">
          <div class="p-3.5 rounded-lg bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-100 dark:border-zinc-800">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase mb-1">{{ $t('settings.about.version') }}</h3>
            <p class="font-mono text-sm">v{{ appVersion }}</p>
          </div>
          <div class="p-3.5 rounded-lg bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-100 dark:border-zinc-800">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase mb-1">{{ $t('settings.about.arch') }}</h3>
            <p class="text-sm">Tauri v2 + Vue 3</p>
          </div>
        </div>

        <div class="mt-5 pt-5 border-t border-neutral-200 dark:border-zinc-800 w-full flex flex-col gap-2.5 max-w-md">
          <a href="https://github.com/DawnLandTeam/dawnland-launcher" target="_blank" class="flex items-center justify-between p-3 rounded-lg border hover:bg-muted/50 transition-colors group">
            <span class="text-sm font-medium">{{ $t('settings.about.github') }}</span>
            <span class="text-xs text-muted-foreground group-hover:text-primary transition-colors">{{ $t('settings.about.visitRepo') }} &rarr;</span>
          </a>
          <a href="https://github.com/DawnLandTeam/dawnland-launcher/issues" target="_blank" class="flex items-center justify-between p-3 rounded-lg border hover:bg-muted/50 transition-colors group">
            <span class="text-sm font-medium">{{ $t('settings.about.reportBug') }}</span>
            <span class="text-xs text-muted-foreground group-hover:text-primary transition-colors">{{ $t('settings.about.submitIssue') }} &rarr;</span>
          </a>
          <a href="https://github.com/DawnLandTeam/dawnland-launcher/issues/new" target="_blank" class="flex items-center justify-between p-3 rounded-lg border hover:bg-muted/50 transition-colors group">
            <span class="text-sm font-medium">{{ $t('settings.about.featureReq') }}</span>
            <span class="text-xs text-muted-foreground group-hover:text-primary transition-colors">{{ $t('settings.about.suggestFeature') }} &rarr;</span>
          </a>

          <!-- Update Channel Settings -->
          <div
            data-testid="about-update-channel"
            class="flex items-center justify-between gap-3 px-3 py-2 rounded-lg border bg-neutral-50/70 dark:bg-zinc-800/40 text-left"
          >
            <div class="flex items-center gap-2 min-w-0">
              <Download class="w-4 h-4 text-primary shrink-0" />
              <span class="text-sm font-medium text-foreground flex items-center gap-1.5">
                {{ $t('settings.about.updateChannelTitle') }}
                <DTooltip :content="$t('settings.about.updateChannelDesc')">
                  <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                    <CircleHelp :size="14" />
                  </span>
                </DTooltip>
              </span>
            </div>
            <DSelect
              :model-value="updateChannel"
              :options="updateChannelOptions"
              @update:model-value="changeUpdateChannel($event as string)"
              class="min-w-[168px] shrink-0"
            />
          </div>
          
          <button @click="checkForUpdates" :disabled="isCheckingUpdate" class="relative flex items-center justify-between p-3 rounded-lg border hover:bg-primary/10 hover:border-primary/30 transition-colors group text-left">
            <div class="flex items-center gap-2">
              <Download class="w-4 h-4 text-primary" />
              <span class="text-sm font-medium text-primary">{{ $t('settings.about.checkUpdates') }}</span>
            </div>
            
            <span v-if="hasUpdateAvailable" class="absolute -top-1 -right-1 flex h-3 w-3">
              <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-red-400 opacity-75"></span>
              <span class="relative inline-flex rounded-full h-3 w-3 bg-red-500"></span>
            </span>
            <span class="text-xs text-muted-foreground">
              <Loader2 v-if="isCheckingUpdate" class="w-4 h-4 animate-spin" />
              <span v-else>&rarr;</span>
            </span>
          </button>
        </div>
        
        <p class="text-xs text-muted-foreground mt-5">
          {{ $t('settings.about.footer') }}
        </p>
      </div>
    </div>
    
    </div>
  </div>
  
  <!-- Global Game Settings Modal -->
  <div v-if="showGlobalGameSettingsModal" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" @click.self="showGlobalGameSettingsModal = false">
    <div class="bg-white dark:bg-zinc-900 p-6 rounded-xl shadow-xl max-w-2xl w-full flex flex-col gap-4 border border-zinc-200 dark:border-zinc-800">
      <div class="flex items-center justify-between">
        <h2 class="text-lg font-semibold flex items-center gap-2">
          <Settings class="w-5 h-5 text-primary" />
          {{ $t('settings.general.globalGameSettingsModalTitle') }}
        </h2>
        <button @click="showGlobalGameSettingsModal = false" class="text-muted-foreground hover:text-foreground">
          <X :size="20" />
        </button>
      </div>
      <p class="text-sm text-muted-foreground">
        {{ $t('settings.general.globalGameSettingsModalDesc') }}
      </p>
      <textarea 
        v-model="globalGameSettingsContent" 
        class="w-full h-64 p-3 rounded-md bg-zinc-100 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 font-mono text-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/50 text-foreground"
        spellcheck="false"
      ></textarea>
      <div class="flex justify-end gap-3 mt-2">
        <DButton variant="ghost" @click="showGlobalGameSettingsModal = false">
          {{ $t('common.cancel') }}
        </DButton>
        <DButton @click="saveGlobalGameSettingsContent" :disabled="isSavingGlobalSettings">
          <span v-if="isSavingGlobalSettings" class="animate-spin mr-2">
            <RefreshCw class="w-4 h-4" />
          </span>
          {{ $t('common.save') }}
        </DButton>
      </div>
    </div>
  </div>
</template>