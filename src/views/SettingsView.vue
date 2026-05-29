<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { Loader2, Download, Coffee, Trash2, FolderOpen, Plus, Search, Package, Languages } from "@lucide/vue";
import { useI18n } from "vue-i18n";

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
}

interface DownloadProgress {
  taskId: string;
  downloaded: number;
  total: number;
}

// Global settings
const activeTab = ref<'general' | 'java' | 'about'>('general');
const systemMemory = ref<SystemMemoryInfo>({ totalMb: 8192, recommendedMaxMb: 4096 });
const defaultMaxMemory = ref(4096);

// Java management state
const installedJavas = ref<JavaInfo[]>([]);
const isScanningJava = ref(false);
const isDownloadingJava = ref(false);
const downloadingVersion = ref<number | null>(null);
const javaDownloadProgress = ref(0);
const customJavaDownloadPath = ref<string>("");
const selectedJavaVersion = ref<number>(21);
const availableJavaVersions = [8, 11, 17, 21, 22, 23];
const isFullDiskScanning = ref(false);
const fullDiskScanPath = ref("");

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
    alert(`Failed to add Java: ${err}`);
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
      alert(`Failed to remove Java: ${err}`);
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

  try {
    // Listen for download progress
    const unlisten = await listen<DownloadProgress>("download-progress", (event) => {
      const payload = event.payload;
      if (payload.total > 0) {
        javaDownloadProgress.value = Math.round((payload.downloaded / payload.total) * 100);
      }
    });

    const javaInfo = await invoke<JavaInfo>("download_java", { majorVersion });
    installedJavas.value.unshift(javaInfo);
    
    unlisten();
  } catch (err) {
    console.error("Failed to download Java:", err);
    alert(`Failed to download Java ${majorVersion}: ${err}`);
  } finally {
    isDownloadingJava.value = false;
    downloadingVersion.value = null;
    javaDownloadProgress.value = 0;
  }
}

onMounted(() => {
  loadSystemMemory();
  loadJavaDownloadPath();
  scanLocalJavas();
});

const { locale } = useI18n();

function changeLanguage(lang: string) {
  locale.value = lang;
  localStorage.setItem('language', lang);
}
</script>

<template>
  <div class="flex h-full flex-col p-6 gap-6 overflow-y-auto">
    <div>
      <h1 class="text-2xl font-bold">{{ $t('settings.title') }}</h1>
      <p class="text-sm text-neutral-500 mt-1">{{ $t('settings.desc') }}</p>
    </div>

    <!-- Tabs Navigation -->
    <div class="flex border-b border-neutral-200 dark:border-zinc-800">
      <button
        class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
        :class="activeTab === 'general' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground'"
        @click="activeTab = 'general'"
      >
        {{ $t('settings.tabs.general') }}
      </button>
      <button
        class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
        :class="activeTab === 'java' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground'"
        @click="activeTab = 'java'"
      >
        {{ $t('settings.tabs.java') }}
      </button>
      <button
        class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
        :class="activeTab === 'about' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground'"
        @click="activeTab = 'about'"
      >
        {{ $t('settings.tabs.about') }}
      </button>
    </div>

    <!-- General Settings Tab -->
    <div v-if="activeTab === 'general'" class="space-y-6">
      <!-- Language Settings -->
      <div class="rounded-lg border border-neutral-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900 flex items-center justify-between">
        <div>
          <h2 class="text-lg font-semibold flex items-center gap-2">
            <Languages :size="20" class="text-primary" />
            {{ $t('settings.general.languageTitle') }}
          </h2>
          <p class="text-sm text-muted-foreground mt-1">{{ $t('settings.general.languageDesc') }}</p>
        </div>
        <select 
          :value="locale"
          @change="changeLanguage(($event.target as HTMLSelectElement).value)"
          class="rounded-md border border-neutral-300 bg-transparent px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/50 dark:border-zinc-700 min-w-[120px]"
        >
          <option value="en">English</option>
          <option value="zh-CN">简体中文</option>
        </select>
      </div>

      <!-- Global Memory Settings -->
      <div class="rounded-lg border border-neutral-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900">
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
      <div class="rounded-lg border border-neutral-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900">
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
          <input
            v-model="customJavaDownloadPath"
            type="text"
            readonly
            :placeholder="$t('settings.java.defaultPath')"
            class="flex-1 rounded-md border border-neutral-300 bg-neutral-50 px-3 py-1.5 text-sm text-neutral-500 dark:border-zinc-700 dark:bg-zinc-800/50"
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
          <select 
            v-model="selectedJavaVersion"
            class="rounded-md border border-neutral-300 bg-transparent px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/50 dark:border-zinc-700"
          >
            <option v-for="v in availableJavaVersions" :key="v" :value="v">Java {{ v }}</option>
          </select>
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
              <p class="text-sm font-medium">Java {{ java.majorVersion }} ({{ java.vendor }})</p>
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

    <!-- About Tab -->
    <div v-if="activeTab === 'about'" class="space-y-6">
      <div class="rounded-lg border border-neutral-200 bg-white p-6 dark:border-zinc-800 dark:bg-zinc-900 flex flex-col items-center text-center">
        <div class="w-20 h-20 bg-primary/10 rounded-2xl flex items-center justify-center mb-4 text-primary">
          <!-- Temporary logo placeholder -->
          <Package :size="40" />
        </div>
        <h2 class="text-2xl font-bold">Dawnland Launcher</h2>
        <p class="text-sm text-muted-foreground mt-1 mb-6">{{ $t('settings.about.desc') }}</p>
        
        <div class="grid grid-cols-2 gap-4 w-full max-w-md text-left">
          <div class="p-4 rounded-lg bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-100 dark:border-zinc-800">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase mb-1">{{ $t('settings.about.version') }}</h3>
            <p class="font-mono text-sm">v0.1.0-alpha</p>
          </div>
          <div class="p-4 rounded-lg bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-100 dark:border-zinc-800">
            <h3 class="text-xs font-semibold text-muted-foreground uppercase mb-1">{{ $t('settings.about.arch') }}</h3>
            <p class="text-sm">Tauri v2 + Vue 3</p>
          </div>
        </div>

        <div class="mt-8 pt-6 border-t border-neutral-200 dark:border-zinc-800 w-full flex flex-col gap-3 max-w-md">
          <a href="https://github.com/yourusername/dawnland-launcher" target="_blank" class="flex items-center justify-between p-3 rounded-lg border hover:bg-muted/50 transition-colors group">
            <span class="text-sm font-medium">{{ $t('settings.about.github') }}</span>
            <span class="text-xs text-muted-foreground group-hover:text-primary transition-colors">Visit Repository &rarr;</span>
          </a>
          <a href="https://github.com/yourusername/dawnland-launcher/issues" target="_blank" class="flex items-center justify-between p-3 rounded-lg border hover:bg-muted/50 transition-colors group">
            <span class="text-sm font-medium">{{ $t('settings.about.reportBug') }}</span>
            <span class="text-xs text-muted-foreground group-hover:text-primary transition-colors">Submit Issue &rarr;</span>
          </a>
          <a href="https://github.com/yourusername/dawnland-launcher/issues/new" target="_blank" class="flex items-center justify-between p-3 rounded-lg border hover:bg-muted/50 transition-colors group">
            <span class="text-sm font-medium">{{ $t('settings.about.featureReq') }}</span>
            <span class="text-xs text-muted-foreground group-hover:text-primary transition-colors">Suggest Feature &rarr;</span>
          </a>
        </div>
        
        <p class="text-xs text-muted-foreground mt-8">
          {{ $t('settings.about.footer') }}
        </p>
      </div>
    </div>
  </div>
</template>