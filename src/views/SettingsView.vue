<script setup lang="ts">
import { ref, shallowRef, onMounted, onActivated, watch, onUnmounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from '@tauri-apps/plugin-dialog';
import DInput from '../components/ui/DInput.vue';
import DSidebarTabs, { type SidebarTab } from '../components/ui/DSidebarTabs.vue';
import { Loader2, Download, Coffee, Trash2, FolderOpen, Plus, Search, Package, Languages, Settings, Shield, Info } from "@lucide/vue";
import { useI18n } from 'vue-i18n';
import DSelect from '../components/ui/DSelect.vue';
import { useRoute, useRouter } from "vue-router";
import UpdaterModal from "../components/UpdaterModal.vue";
import { getVersion } from "@tauri-apps/api/app";
import { setUpdateAvailable, hasUpdateAvailable, type CustomUpdate, parseUpdateData } from "../composables/useUpdate";
import { trackEvent, getErrorType, sanitizeTrackingUrl } from "../utils/analytics";
import { getErrorMessage } from "../utils/error";
import { normalizeUpdateChannel, getUpdateChannelQuery } from "../utils/updateChannel";
import { toast } from "../composables/useToast";

const route = useRoute();
const router = useRouter();

// App version state
const appVersion = ref('0.0.0');

onMounted(async () => {
  trackEvent("Settings Viewed");
  try {
    appVersion.value = await getVersion();
  } catch (err) {
    console.error("Failed to get app version:", err);
  }

  window.addEventListener('authlib-servers-updated', loadAuthlibServers);
});

onUnmounted(() => {
  window.removeEventListener('authlib-servers-updated', loadAuthlibServers);
});

onActivated(async () => {
  await loadLauncherSettings();
  await loadAuthlibServers();
  await loadSystemMemory();
  await scanLocalJavas();
  await loadJavaDownloadPath();
  await loadAvailableJavas();
});

const tabs = [
  { id: 'general', name: 'settings.tabs.general', icon: Settings },
  { id: 'java', name: 'settings.tabs.java', icon: Coffee },
  { id: 'authlib', name: 'settings.authlib.tab', icon: Shield },
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
      if (['general', 'java', 'authlib', 'about'].includes(newTab as string)) {
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
}

interface AuthlibServer {
  url: string;
  name: string;
}

// Global settings

const systemMemory = ref<SystemMemoryInfo>({ totalMb: 8192, recommendedMaxMb: 4096 });
const defaultMaxMemory = ref(4096);

// Updater state
const isCheckingUpdate = ref(false);
const showUpdaterModal = ref(false);
const updateInfo = shallowRef<CustomUpdate | null>(null);
const updateChannel = ref(normalizeUpdateChannel(localStorage.getItem('updateChannel')));

// Launcher Settings state
const enableInstanceInheritance = ref(false);
const downloadSource = ref<'official' | 'bmclapi'>('official');
const maxConcurrentDownloads = ref(32);
const enableTelemetry = ref(false);

async function loadLauncherSettings() {
  try {
    const settings = await invoke<any>('load_launcher_settings');
    enableInstanceInheritance.value = settings.enableInstanceInheritance;
    downloadSource.value = settings.downloadSource === 'bmclapi' ? 'bmclapi' : 'official';
    maxConcurrentDownloads.value = settings.maxConcurrentDownloads || 32;
    enableTelemetry.value = settings.enableTelemetry === true;
    if (settings.globalMaxMemory) {
      defaultMaxMemory.value = settings.globalMaxMemory;
    }
  } catch (e) {
    console.error('Failed to load launcher settings:', e);
  }
}

async function saveLauncherSettings() {
  try {
    await invoke('save_launcher_settings', {
      settings: {
        enableInstanceInheritance: enableInstanceInheritance.value,
        downloadSource: downloadSource.value,
        maxConcurrentDownloads: maxConcurrentDownloads.value,
        enableTelemetry: enableTelemetry.value,
        globalMaxMemory: defaultMaxMemory.value
      }
    });
  } catch (e) {
    console.error('Failed to save launcher settings:', e);
    toast.error(t('settings.saveFailed', '保存设置失败'), getErrorMessage(e));
  }
}

function changeUpdateChannel(channel: string) {
  const normalizedChannel = normalizeUpdateChannel(channel);
  updateChannel.value = normalizedChannel;
  localStorage.setItem('updateChannel', normalizedChannel);
}

async function checkForUpdates() {
  isCheckingUpdate.value = true;
  try {
    const targetOS = navigator.userAgent.includes("Windows") ? "windows-standalone" : "linux-standalone";
    const baseUrl = import.meta.env.VITE_WEB_BACKEND_URL || 'http://localhost:3030';
    const channel = getUpdateChannelQuery();
    const res = await fetch(`${baseUrl}/api/launcher/update/${targetOS}/${appVersion.value}${channel}`);
    if (res.status === 200) {
      const data = await res.json();
      const update = parseUpdateData(data, targetOS);
      if (update && update.version !== appVersion.value) {
        updateInfo.value = update;
        showUpdaterModal.value = true;
        setUpdateAvailable(update);
        return;
      }
    }
    setUpdateAvailable(null);
    alert(t('settings.about.upToDate'));
  } catch (err) {
    console.error("Failed to check for updates:", err);
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

function formatBytes(bytes: number) {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

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
    console.error("Failed to load authlib servers:", err);
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
    console.error("Failed to add authlib server:", err);
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
      console.error("Failed to remove authlib server:", err);
    }
  }
}

async function loadSystemMemory(): Promise<void> {
  try {
    systemMemory.value = await invoke<SystemMemoryInfo>("get_system_memory");
    defaultMaxMemory.value = systemMemory.value.recommendedMaxMb;
  } catch (err) {
    console.error("Failed to load system memory:", err);
  }
}

// Java management functions
async function scanLocalJavas(): Promise<void> {
  isScanningJava.value = true;
  try {
    installedJavas.value = await invoke<JavaInfo[]>("scan_local_javas");
  } catch (err) {
    console.error("Failed to scan Java installations:", err);
  } finally {
    isScanningJava.value = false;
  }
}

async function loadJavaDownloadPath(): Promise<void> {
  try {
    const path = await invoke<string | null>("get_java_download_path");
    customJavaDownloadPath.value = path || "";
  } catch (err) {
    console.error("Failed to load custom Java download path:", err);
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
    console.error("Failed to fetch available Javas from API:", err);
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
    console.error("Failed to select directory:", err);
  }
}

async function clearJavaDownloadPath(): Promise<void> {
  try {
    await invoke("set_java_download_path", { path: null });
    customJavaDownloadPath.value = "";
  } catch (err) {
    console.error("Failed to clear directory:", err);
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
    console.error("Failed to add manual Java:", err);
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
      console.error("Failed to remove Java:", err);
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
    console.error("Failed to start full disk scan:", err);
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
    let lastDownloaded = 0;
    let lastTime = performance.now();

    // Listen for download progress
    unlisten = await listen<DownloadProgress>("download-progress", (event) => {
      const payload = event.payload;
      if (payload.total > 0) {
        javaDownloadProgress.value = Math.floor((payload.downloaded / payload.total) * 100);
        javaDownloadedBytes.value = payload.downloaded;
        javaTotalBytes.value = payload.total;

        const now = performance.now();
        const timeDiff = (now - lastTime) / 1000;
        if (timeDiff >= 0.5) {
          const bytesDiff = payload.downloaded - lastDownloaded;
          const speed = bytesDiff / timeDiff;
          javaDownloadSpeed.value = `${formatBytes(speed)}/s`;
          lastDownloaded = payload.downloaded;
          lastTime = now;
        }
      }
    });

    const javaInfo = await invoke<JavaInfo>("download_java", { majorVersion });
    installedJavas.value.unshift(javaInfo);
    trackEvent("Java Download Completed", { majorVersion, version: javaInfo.versionString });
    
  } catch (err) {
    console.error("Failed to download Java:", err);
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


const { t, locale } = useI18n();

const languageOptions = [
  { label: 'English', value: 'en' },
  { label: '简体中文', value: 'zh-CN' }
];

const updateChannelOptions = computed(() => [
  { label: t('settings.general.channelStable'), value: 'stable' },
  { label: t('settings.general.channelPrerelease'), value: 'prerelease' }
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
}
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
    <div v-if="activeTab === 'general'" class="space-y-6">
      <!-- Language Settings -->
      <div class="relative z-50 rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div>
          <h2 class="text-lg font-semibold flex items-center gap-2">
            <Languages :size="20" class="text-primary" />
            {{ $t('settings.general.languageTitle') }}
          </h2>
          <p class="text-sm text-muted-foreground mt-1">{{ $t('settings.general.languageDesc') }}</p>
        </div>
        <DSelect 
          :model-value="locale"
          :options="languageOptions"
          @update:model-value="changeLanguage($event as string)"
          class="min-w-[120px]"
        />
      </div>

      <!-- Update Channel Settings -->
      <div class="relative z-40 rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div>
          <h2 class="text-lg font-semibold flex items-center gap-2">
            <Download :size="20" class="text-primary" />
            {{ $t('settings.general.updateChannelTitle') }}
          </h2>
          <p class="text-sm text-muted-foreground mt-1">{{ $t('settings.general.updateChannelDesc') }}</p>
        </div>
        <DSelect 
          :model-value="updateChannel"
          :options="updateChannelOptions"
          @update:model-value="changeUpdateChannel($event as string)"
          class="min-w-[120px]"
        />
      </div>

      <!-- Instance Inheritance Settings -->
      <div class="relative z-30 rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div>
          <h2 class="text-lg font-semibold flex items-center gap-2">
            <Package :size="20" class="text-primary" />
            {{ $t('settings.general.inheritanceTitle') }}
          </h2>
          <p class="text-sm text-muted-foreground mt-1">{{ $t('settings.general.inheritanceDesc') }}</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="enableInstanceInheritance" @change="saveLauncherSettings" class="sr-only peer">
          <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
        </label>
      </div>

      <!-- Download Source Settings -->
      <div class="relative z-20 rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div>
          <h2 class="text-lg font-semibold flex items-center gap-2">
            <Download :size="20" class="text-primary" />
            {{ $t('settings.general.downloadSourceTitle') }}
          </h2>
          <p class="text-sm text-muted-foreground mt-1">{{ $t('settings.general.downloadSourceDesc') }}</p>
        </div>
        <DSelect 
          v-model="downloadSource"
          :options="downloadSourceOptions"
          @update:model-value="saveLauncherSettings"
          class="min-w-[120px]"
        />
      </div>

      <!-- Concurrent Downloads Settings -->
      <div class="relative z-10 rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
        <h2 class="mb-4 text-lg font-semibold flex items-center gap-2">
          <Download :size="20" class="text-primary" />
          {{ $t('settings.general.concurrentDownloadsTitle', '最大并发下载数') }}
        </h2>
        <div class="space-y-3">
          <div class="flex items-center justify-between">
            <label class="text-sm font-medium">{{ $t('settings.general.concurrentDownloadsDesc', '允许的最大同时下载任务数量') }}</label>
            <span class="text-sm font-mono text-primary">{{ maxConcurrentDownloads }}</span>
          </div>
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
      <div class="relative z-5 rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md flex items-center justify-between shadow-sm">
        <div>
          <h2 class="text-lg font-semibold flex items-center gap-2">
            <Shield :size="20" class="text-primary" />
            {{ $t('settings.general.telemetryTitle', '遥测数据收集') }}
          </h2>
          <p class="text-sm text-muted-foreground mt-1">{{ $t('settings.general.telemetryDesc', '允许收集纯匿名的使用数据，帮助我们持续改善启动器体验') }}</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="enableTelemetry" @change="saveLauncherSettings" class="sr-only peer">
          <div class="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/30 dark:peer-focus:ring-primary/80 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
        </label>
      </div>

      <!-- Global Memory Settings -->
      <div class="relative z-0 rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
        <h2 class="mb-4 text-lg font-semibold">{{ $t('settings.general.memoryTitle') }}</h2>
        <div class="space-y-3">
          <div class="flex items-center justify-between">
            <label class="text-sm font-medium">{{ $t('settings.general.maxMemory') }}</label>
            <span class="text-sm font-mono text-primary">{{ defaultMaxMemory }} MB</span>
          </div>
          <input
            v-model.number="defaultMaxMemory"
            type="range"
            min="512"
            :max="systemMemory.totalMb"
            step="512"
            class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-blue-500"
            @change="saveLauncherSettings"
          />
          <div class="flex justify-between text-xs text-muted-foreground">
            <span>512 MB</span>
            <span>{{ $t('settings.general.system') }}: {{ systemMemory.totalMb }} MB</span>
          </div>
          <p class="text-xs text-muted-foreground">
            {{ $t('settings.general.recommended', { recommended: systemMemory.recommendedMaxMb }) }}
          </p>
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
            <span>{{ javaDownloadSpeed }}</span>
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

    <!-- About Tab -->
    <div v-if="activeTab === 'about'" class="h-full">
      <div class="rounded-lg border border-white/20 bg-white/60 p-4 dark:bg-zinc-900/60 backdrop-blur-md flex flex-col items-center justify-center text-center shadow-sm min-h-full">
        <div class="w-20 h-20 bg-primary/10 rounded-2xl flex items-center justify-center mb-4 text-primary">
          <!-- Temporary logo placeholder -->
          <Package :size="40" />
        </div>
        <h2 class="text-2xl font-bold">Dawnland Launcher</h2>
        <p class="text-sm text-muted-foreground mt-1 mb-6">{{ $t('settings.about.desc') }}</p>
        
        <div class="grid grid-cols-2 gap-4 w-full max-w-md text-left">
          <div class="p-4 rounded-lg bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-100 dark:border-zinc-800">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase mb-1">{{ $t('settings.about.version') }}</h3>
            <p class="font-mono text-sm">v{{ appVersion }}</p>
          </div>
          <div class="p-4 rounded-lg bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-100 dark:border-zinc-800">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase mb-1">{{ $t('settings.about.arch') }}</h3>
            <p class="text-sm">Tauri v2 + Vue 3</p>
          </div>
        </div>

        <div class="mt-8 pt-6 border-t border-neutral-200 dark:border-zinc-800 w-full flex flex-col gap-3 max-w-md">
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
        
        <p class="text-xs text-muted-foreground mt-8">
          {{ $t('settings.about.footer') }}
        </p>
      </div>
    </div>
    
    <UpdaterModal v-model:open="showUpdaterModal" :update-info="updateInfo" />
    </div>
  </div>
</template>