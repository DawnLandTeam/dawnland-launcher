<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { User, Trash2, Plus, UserPlus, Loader2, X, WifiOff, Download, Coffee } from "@lucide/vue";

interface Account {
  id: string;
  username: string;
  accountType: "offline" | "microsoft";
  accessToken?: string;
  refreshToken?: string;
}

interface LoginInitResponse {
  userCode: string;
  deviceCode: string;
  verificationUri: string;
  message: string;
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
}

interface DownloadProgress {
  taskId: string;
  downloaded: number;
  total: number;
}

type AccountType = "offline" | "microsoft";

const accounts = ref<Account[]>([]);
const newUsername = ref("");
const isAddingOffline = ref(false);
const isLoggingInMicrosoft = ref(false);
const microsoftLoginData = ref<LoginInitResponse | null>(null);
const loginError = ref<string | null>(null);
const deviceCode = ref("");

// Modal state
const showAddAccountModal = ref(false);
const selectedAccountType = ref<AccountType>("offline");

// Global settings
const systemMemory = ref<SystemMemoryInfo>({ totalMb: 8192, recommendedMaxMb: 4096 });
const defaultMaxMemory = ref(4096);

// Java management state
const installedJavas = ref<JavaInfo[]>([]);
const isScanningJava = ref(false);
const isDownloadingJava = ref(false);
const downloadingVersion = ref<number | null>(null);
const javaDownloadProgress = ref(0);

// Load accounts on mount
async function loadAccounts(): Promise<void> {
  try {
    accounts.value = await invoke<Account[]>("get_accounts");
  } catch (err) {
    console.error("Failed to load accounts:", err);
  }
}

// Open add account modal
function openAddAccountModal(): void {
  showAddAccountModal.value = true;
  selectedAccountType.value = "offline";
  newUsername.value = "";
  loginError.value = null;
  microsoftLoginData.value = null;
  isLoggingInMicrosoft.value = false;
}

// Close add account modal
function closeAddAccountModal(): void {
  showAddAccountModal.value = false;
  newUsername.value = "";
  loginError.value = null;
  microsoftLoginData.value = null;
  isLoggingInMicrosoft.value = false;
}

// Add offline account
async function addOfflineAccount(): Promise<void> {
  if (!newUsername.value.trim()) return;

  isAddingOffline.value = true;
  loginError.value = null;

  try {
    await invoke("add_offline_account", { username: newUsername.value.trim() });
    newUsername.value = "";
    await loadAccounts();
    closeAddAccountModal();
  } catch (err) {
    loginError.value = typeof err === "string" ? err : String(err);
  } finally {
    isAddingOffline.value = false;
  }
}

// Remove account
async function removeAccount(id: string): Promise<void> {
  try {
    await invoke("remove_account", { id });
    await loadAccounts();
  } catch (err) {
    console.error("Failed to remove account:", err);
  }
}

// Start Microsoft login
async function startMicrosoftLogin(): Promise<void> {
  isLoggingInMicrosoft.value = true;
  loginError.value = null;
  microsoftLoginData.value = null;

  try {
    const response = await invoke<LoginInitResponse>("start_microsoft_login");
    microsoftLoginData.value = response;
    deviceCode.value = response.userCode;

    // Start polling for token - use the long device_code, not the short user_code
    pollMicrosoftToken(response.deviceCode);
  } catch (err) {
    loginError.value = typeof err === "string" ? err : String(err);
    isLoggingInMicrosoft.value = false;
  }
}

// Poll for Microsoft token
async function pollMicrosoftToken(code: string): Promise<void> {
  try {
    const account = await invoke<Account>("poll_microsoft_token", { deviceCode: code });
    accounts.value.push(account);
    microsoftLoginData.value = null;
    isLoggingInMicrosoft.value = false;
    closeAddAccountModal();
  } catch (err) {
    const errorMsg = typeof err === "string" ? err : String(err);
    // Check if it's a pending error (user hasn't entered code yet)
    if (errorMsg.includes("authorization_pending")) {
      // Keep polling
      setTimeout(() => pollMicrosoftToken(code), 5000);
    } else if (errorMsg.includes("expired_token") || errorMsg.includes("cancellation")) {
      // User cancelled or expired
      loginError.value = errorMsg;
      microsoftLoginData.value = null;
      isLoggingInMicrosoft.value = false;
    } else {
      // Other error - show but keep trying for now
      console.error("Poll error:", errorMsg);
      setTimeout(() => pollMicrosoftToken(code), 5000);
    }
  }
}

function copyCode(): void {
  navigator.clipboard.writeText(deviceCode.value);
}

function cancelMicrosoftLogin(): void {
  microsoftLoginData.value = null;
  isLoggingInMicrosoft.value = false;
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
  loadAccounts();
  loadSystemMemory();
  scanLocalJavas();
});
</script>

<template>
  <div class="flex h-full flex-col p-6 gap-6 overflow-y-auto">
    <div>
      <h1 class="text-2xl font-bold">Settings</h1>
      <p class="text-sm text-neutral-500 mt-1">Manage accounts and global settings</p>
    </div>

    <!-- Global Memory Settings -->
    <div class="rounded-lg border border-neutral-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900">
      <h2 class="mb-4 text-lg font-semibold">Default Memory Settings</h2>
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium">Default Max Memory</label>
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
          <span>System: {{ systemMemory.totalMb }} MB</span>
        </div>
        <p class="text-xs text-muted-foreground">
          This will be used as the default memory for new instances. Recommended: {{ systemMemory.recommendedMaxMb }} MB (1/3 of system RAM)
        </p>
</div>
    </div>

    <!-- Java Management -->
    <div class="rounded-lg border border-neutral-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900">
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">Java Management</h2>
        <button
          class="flex items-center gap-2 rounded-lg bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          :disabled="isScanningJava"
          @click="scanLocalJavas"
        >
          <Loader2 v-if="isScanningJava" :size="14" class="animate-spin" />
          <Coffee v-else :size="14" />
          {{ isScanningJava ? 'Scanning...' : 'Refresh' }}
        </button>
      </div>

      <!-- Download Java Section -->
      <div class="mb-4 p-3 bg-muted/30 rounded-lg">
        <p class="text-sm text-muted-foreground mb-3">Download Java from Adoptium:</p>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="version in [8, 11, 17, 21]"
            :key="version"
            class="flex items-center gap-1.5 px-3 py-1.5 text-sm border rounded-lg hover:bg-primary/10 hover:border-primary/50 transition-colors disabled:opacity-50"
            :disabled="isDownloadingJava"
            @click="downloadJava(version)"
          >
            <Download :size="14" />
            Java {{ version }}
          </button>
        </div>
        <!-- Download progress -->
        <div v-if="isDownloadingJava" class="mt-3">
          <div class="flex items-center justify-between text-sm mb-1">
            <span>Downloading Java {{ downloadingVersion }}...</span>
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
        <p class="text-sm font-medium">Installed Java ({{ installedJavas.length }})</p>
        <div v-if="installedJavas.length === 0" class="text-sm text-muted-foreground py-2">
          No Java installations found. Click a version above to download.
        </div>
        <div
          v-for="java in installedJavas"
          :key="java.path"
          class="flex items-center justify-between p-3 border rounded-lg bg-muted/20"
        >
          <div class="flex items-center gap-3">
            <Coffee class="h-5 w-5 text-orange-500" />
            <div>
              <p class="text-sm font-medium">Java {{ java.majorVersion }} ({{ java.vendor }})</p>
              <p class="text-xs text-muted-foreground truncate max-w-xs">{{ java.path }}</p>
            </div>
          </div>
          <span class="text-xs text-muted-foreground">{{ java.versionString }}</span>
        </div>
      </div>
    </div>

    <!-- Account List Header -->
    <div class="flex items-center justify-between">
      <h2 class="text-lg font-semibold">Accounts ({{ accounts.length }})</h2>
      <button
        class="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
        @click="openAddAccountModal"
      >
        <Plus :size="16" />
        Add Account
      </button>
    </div>

    <!-- Account List -->
    <div class="flex flex-1 flex-col gap-3 overflow-y-auto">
      <div
        v-for="account in accounts"
        :key="account.id"
        class="flex items-center justify-between rounded-lg border border-neutral-200 bg-white p-4 dark:border-zinc-800 dark:bg-zinc-900"
      >
        <div class="flex items-center gap-3">
          <div
            class="flex h-10 w-10 items-center justify-center rounded-full"
            :class="account.accountType === 'microsoft' ? 'bg-emerald-100 text-emerald-600 dark:bg-emerald-900/30 dark:text-emerald-400' : 'bg-neutral-100 text-neutral-600 dark:bg-zinc-800 dark:text-zinc-400'"
          >
            <User :size="20" />
          </div>
          <div>
            <p class="font-medium text-neutral-900 dark:text-zinc-100">
              {{ account.username }}
            </p>
            <p class="text-xs text-neutral-500">
              {{ account.accountType === 'microsoft' ? 'Microsoft Account' : 'Offline Account' }}
            </p>
          </div>
        </div>
        <button
          class="rounded p-2 text-neutral-400 transition-colors hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-900/20"
          title="Remove account"
          @click="removeAccount(account.id)"
        >
          <Trash2 :size="18" />
        </button>
      </div>

      <p v-if="accounts.length === 0" class="text-center text-sm text-neutral-400">
        No accounts added yet
      </p>
    </div>

    <!-- Add Account Modal -->
    <Teleport to="body">
      <div
        v-if="showAddAccountModal"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <div
          class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"
          @click="closeAddAccountModal"
        />
        <div class="relative z-10 w-full max-w-md gap-4 border bg-white dark:bg-zinc-900 p-6 shadow-xl rounded-lg pointer-events-auto">
          <!-- Header -->
          <div class="flex items-center justify-between mb-4">
            <h3 class="text-lg font-semibold">Add Account</h3>
            <button
              class="rounded p-1 text-neutral-500 hover:bg-neutral-100 dark:hover:bg-zinc-800"
              @click="closeAddAccountModal"
            >
              <X :size="20" />
            </button>
          </div>

          <!-- Account Type Selection -->
          <div class="space-y-3">
            <label class="text-sm font-medium">Account Type</label>
            <div class="flex gap-3">
              <button
                class="flex-1 flex flex-col items-center gap-2 rounded-lg border-2 p-4 transition-colors"
                :class="selectedAccountType === 'offline' ? 'border-primary bg-primary/10' : 'border-neutral-200 dark:border-zinc-700 hover:border-primary/50'"
                @click="selectedAccountType = 'offline'"
              >
                <WifiOff :size="24" class="text-neutral-600 dark:text-zinc-400" />
                <span class="text-sm font-medium">Offline</span>
                <span class="text-xs text-neutral-500">Play without account</span>
              </button>
              <button
                class="flex-1 flex flex-col items-center gap-2 rounded-lg border-2 p-4 transition-colors"
                :class="selectedAccountType === 'microsoft' ? 'border-primary bg-primary/10' : 'border-neutral-200 dark:border-zinc-700 hover:border-primary/50'"
                @click="selectedAccountType = 'microsoft'"
              >
                <UserPlus :size="24" class="text-emerald-600 dark:text-emerald-400" />
                <span class="text-sm font-medium">Microsoft</span>
                <span class="text-xs text-neutral-500">Play online</span>
              </button>
            </div>
          </div>

          <!-- Offline Account Form -->
          <div v-if="selectedAccountType === 'offline'" class="mt-4 space-y-3">
            <label class="text-sm font-medium">Username</label>
            <input
              v-model="newUsername"
              type="text"
              placeholder="Enter username..."
              class="w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-800"
              @keyup.enter="addOfflineAccount"
            />
            <button
              class="w-full flex items-center justify-center gap-2 rounded-lg bg-primary px-4 py-2.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="isAddingOffline || !newUsername.trim()"
              @click="addOfflineAccount"
            >
              <Loader2 v-if="isAddingOffline" :size="16" class="animate-spin" />
              <Plus v-else :size="16" />
              {{ isAddingOffline ? 'Adding...' : 'Add Account' }}
            </button>
          </div>

          <!-- Microsoft Login Form -->
          <div v-else-if="selectedAccountType === 'microsoft'" class="mt-4 space-y-3">
            <!-- Not logging in -->
            <div v-if="!isLoggingInMicrosoft && !microsoftLoginData">
              <button
                class="w-full flex items-center justify-center gap-2 rounded-lg bg-emerald-600 px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-emerald-500"
                @click="startMicrosoftLogin"
              >
                <UserPlus :size="16" />
                Login with Microsoft
              </button>
            </div>

            <!-- Logging in - show device code -->
            <div v-else-if="microsoftLoginData" class="space-y-3">
              <p class="text-sm text-neutral-600 dark:text-zinc-400">
                {{ microsoftLoginData.message }}
              </p>
              <div class="flex items-center gap-2">
                <span class="text-2xl font-mono font-bold tracking-wider text-indigo-600 dark:text-indigo-400">
                  {{ microsoftLoginData.userCode }}
                </span>
                <button
                  class="rounded p-1 text-neutral-500 hover:bg-neutral-100 dark:hover:bg-zinc-800"
                  title="Copy code"
                  @click="copyCode"
                >
                  <User :size="16" />
                </button>
              </div>
              <a
                :href="microsoftLoginData.verificationUri"
                target="_blank"
                class="text-sm text-indigo-600 hover:underline dark:text-indigo-400"
              >
                Open verification page →
              </a>
              <button
                class="w-full rounded-lg bg-neutral-200 px-3 py-2 text-sm text-neutral-700 hover:bg-neutral-300 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
                @click="cancelMicrosoftLogin"
              >
                Cancel
              </button>
            </div>

            <!-- Waiting for poll -->
            <div v-else class="flex items-center justify-center gap-2 py-4 text-sm text-neutral-500">
              <Loader2 :size="16" class="animate-spin" />
              Preparing login...
            </div>
          </div>

          <!-- Error Display -->
          <div v-if="loginError" class="mt-3 rounded-lg bg-red-900/40 px-3 py-2 text-sm text-red-400">
            {{ loginError }}
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>