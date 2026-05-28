<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { 
  User, Trash2, Plus, UserPlus, Loader2, X, WifiOff, 
  MonitorCheck, Check, AlertCircle 
} from "@lucide/vue";

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

type AccountType = "offline" | "microsoft";

// State
const accounts = ref<Account[]>([]);
const activeAccountId = ref<string>("");
const newUsername = ref("");
const isAddingOffline = ref(false);
const isLoggingInMicrosoft = ref(false);
const microsoftLoginData = ref<LoginInitResponse | null>(null);
const loginError = ref<string | null>(null);
const deviceCode = ref("");

// Modal state
const showAddAccountModal = ref(false);
const selectedAccountType = ref<AccountType>("offline");
const showDeleteDialog = ref(false);
const deletingAccountId = ref("");
const deletingAccountName = ref("");

// Load accounts on mount
async function loadAccounts(): Promise<void> {
  try {
    accounts.value = await invoke<Account[]>("get_accounts");
    // Set first account as active if none selected
    if (!activeAccountId.value && accounts.value.length > 0) {
      activeAccountId.value = accounts.value[0].id;
    }
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
    // Notify other views to refresh accounts
    await emit("accounts-updated");
  } catch (err) {
    loginError.value = typeof err === "string" ? err : String(err);
  } finally {
    isAddingOffline.value = false;
  }
}

// Set active account
async function setActiveAccount(id: string): Promise<void> {
  activeAccountId.value = id;
  // TODO: Persist active account preference
}

// Confirm delete account
function confirmDeleteAccount(account: Account): void {
  deletingAccountId.value = account.id;
  deletingAccountName.value = account.username;
  showDeleteDialog.value = true;
}

// Remove account
async function removeAccount(): Promise<void> {
  if (!deletingAccountId.value) return;

  try {
    await invoke("remove_account", { id: deletingAccountId.value });
    // If we removed the active account, clear it
    if (activeAccountId.value === deletingAccountId.value) {
      activeAccountId.value = "";
    }
    await loadAccounts();
    // Notify other views to refresh accounts
    await emit("accounts-updated");
  } catch (err) {
    console.error("Failed to remove account:", err);
  } finally {
    showDeleteDialog.value = false;
    deletingAccountId.value = "";
    deletingAccountName.value = "";
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
    // Notify other views to refresh accounts
    await emit("accounts-updated");
  } catch (err) {
    const errorMsg = typeof err === "string" ? err : String(err);
    if (errorMsg.includes("authorization_pending")) {
      setTimeout(() => pollMicrosoftToken(code), 5000);
    } else if (errorMsg.includes("expired_token") || errorMsg.includes("cancellation")) {
      loginError.value = errorMsg;
      microsoftLoginData.value = null;
      isLoggingInMicrosoft.value = false;
    } else {
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

function isMsaAccount(account: Account): boolean {
  return account.accountType === "microsoft";
}

onMounted(() => {
  loadAccounts();
});
</script>

<template>
  <div class="flex h-full flex-col p-6 gap-6 overflow-y-auto">
    <!-- Header -->
    <div>
      <h1 class="text-2xl font-bold">{{ $t('accounts.title') }}</h1>
      <p class="text-sm text-neutral-500 mt-1">{{ $t('accounts.desc') }}</p>
    </div>

    <!-- Accounts Grid Header -->
    <div class="flex items-center justify-between">
      <h2 class="text-lg font-semibold text-neutral-900 dark:text-white">{{ $t('accounts.saved', { count: accounts.length }) }}</h2>
      <button
        class="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
        @click="selectedAccountType = 'offline'; openAddAccountModal()"
      >
        <Plus :size="16" />
        {{ $t('accounts.add') }}
      </button>
    </div>

    <!-- Accounts Grid -->
    <div class="flex-1">
      <div v-if="accounts.length === 0" class="flex flex-col items-center justify-center py-12 text-center">
        <div class="flex h-16 w-16 items-center justify-center rounded-full bg-muted mb-4">
          <User :size="32" class="text-muted-foreground" />
        </div>
        <p class="text-lg font-medium text-neutral-900 dark:text-white">{{ $t('accounts.noAccounts') }}</p>
        <p class="text-sm text-muted-foreground">{{ $t('accounts.noAccountsDesc') }}</p>
      </div>

      <div v-else class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <div
          v-for="account in accounts"
          :key="account.id"
          class="relative rounded-lg border-2 p-4 transition-all"
          :class="activeAccountId === account.id 
            ? 'border-primary bg-primary/5 dark:bg-primary/10' 
            : 'border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-primary/50'"
        >
          <!-- Active Badge -->
          <div 
            v-if="activeAccountId === account.id"
            class="absolute -top-2.5 -right-2 flex items-center gap-1.5 rounded-full bg-gradient-to-r from-green-500 to-emerald-500 px-2.5 py-1 text-xs font-bold text-white shadow-lg shadow-green-500/30"
          >
            <Check :size="11" stroke-width="3" />
            {{ $t('accounts.active') }}
          </div>

          <!-- Account Info -->
          <div class="flex items-start gap-3">
            <div
              class="flex h-12 w-12 shrink-0 items-center justify-center rounded-full text-lg font-bold"
              :class="isMsaAccount(account) 
                ? 'bg-emerald-100 text-emerald-600 dark:bg-emerald-900/30 dark:text-emerald-400' 
                : 'bg-neutral-100 text-neutral-600 dark:bg-zinc-800 dark:text-zinc-400'"
            >
              {{ account.username.charAt(0).toUpperCase() }}
            </div>
            <div class="flex-1 min-w-0">
              <p class="font-medium truncate">{{ account.username }}</p>
              <div class="flex items-center gap-1.5 mt-1">
                <MonitorCheck v-if="isMsaAccount(account)" :size="14" class="text-green-500" />
                <WifiOff v-else :size="14" class="text-muted-foreground" />
                <span class="text-xs" :class="isMsaAccount(account) ? 'text-green-600 dark:text-green-400' : 'text-muted-foreground'">
                  {{ isMsaAccount(account) ? $t('accounts.microsoft') : $t('accounts.offline') }}
                </span>
              </div>
            </div>
          </div>

          <!-- Action Buttons -->
          <div class="flex gap-2 mt-4">
            <button
              v-if="activeAccountId !== account.id"
              class="flex-1 flex items-center justify-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm hover:bg-muted transition-colors"
              @click="setActiveAccount(account.id)"
            >
              <Check :size="14" />
              {{ $t('accounts.setActive') }}
            </button>
            <button
              class="flex items-center justify-center gap-1.5 rounded-lg border border-red-200 bg-red-50 px-3 py-1.5 text-sm text-red-600 hover:bg-red-100 dark:border-red-900/30 dark:bg-red-900/20 dark:text-red-400 dark:hover:bg-red-900/30 transition-colors"
              @click="confirmDeleteAccount(account)"
            >
              <Trash2 :size="14" />
            </button>
          </div>
        </div>
      </div>
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
            <h3 class="text-lg font-semibold text-neutral-900 dark:text-white">{{ $t('accounts.add') }}</h3>
            <button
              class="rounded p-1 text-neutral-500 hover:bg-neutral-100 dark:hover:bg-zinc-800"
              @click="closeAddAccountModal"
            >
              <X :size="20" class="text-neutral-900 dark:text-white" />
            </button>
          </div>

          <!-- Account Type Selection -->
          <div class="space-y-3">
            <label class="text-sm font-medium">{{ $t('accounts.type') }}</label>
            <div class="flex gap-3">
              <button
                class="flex-1 flex flex-col items-center gap-2 rounded-lg border-2 p-4 transition-all"
                :class="selectedAccountType === 'offline' 
                  ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/30' 
                  : 'border-neutral-200 dark:border-zinc-700 hover:border-blue-300'"
                @click="selectedAccountType = 'offline'"
              >
                <WifiOff :size="24" :class="selectedAccountType === 'offline' ? 'text-blue-600 dark:text-blue-400' : 'text-neutral-500'" />
                <span class="text-sm font-medium" :class="selectedAccountType === 'offline' ? 'text-blue-700 dark:text-blue-300' : ''">{{ $t('accounts.offline') }}</span>
                <span class="text-xs text-neutral-500">{{ $t('accounts.playWithout') }}</span>
              </button>
              <button
                class="flex-1 flex flex-col items-center gap-2 rounded-lg border-2 p-4 transition-all"
                :class="selectedAccountType === 'microsoft' 
                  ? 'border-green-500 bg-green-50 dark:bg-green-900/30' 
                  : 'border-neutral-200 dark:border-zinc-700 hover:border-green-300'"
                @click="selectedAccountType = 'microsoft'"
              >
                <UserPlus :size="24" :class="selectedAccountType === 'microsoft' ? 'text-green-600 dark:text-green-400' : 'text-neutral-500'" />
                <span class="text-sm font-medium" :class="selectedAccountType === 'microsoft' ? 'text-green-700 dark:text-green-300' : ''">{{ $t('accounts.microsoft') }}</span>
                <span class="text-xs text-neutral-500">{{ $t('accounts.playOnline') }}</span>
              </button>
            </div>
          </div>

          <!-- Offline Account Form -->
          <div v-if="selectedAccountType === 'offline'" class="mt-4 space-y-3">
            <label class="text-sm font-medium">{{ $t('accounts.username') }}</label>
            <input
              v-model="newUsername"
              type="text"
              :placeholder="$t('accounts.enterUsername')"
              class="w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 dark:border-zinc-700 dark:bg-zinc-800"
              @keyup.enter="addOfflineAccount"
            />
            <button
              class="w-full flex items-center justify-center gap-2 rounded-lg bg-primary px-4 py-2.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="isAddingOffline || !newUsername.trim()"
              @click="addOfflineAccount"
            >
              <Loader2 v-if="isAddingOffline" :size="16" class="animate-spin" />
              <Plus v-else :size="16" />
              {{ isAddingOffline ? $t('accounts.adding') : $t('accounts.add') }}
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
                {{ $t('accounts.loginWithMs') }}
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
                {{ $t('accounts.openVerify') }}
              </a>
              <button
                class="w-full rounded-lg bg-neutral-200 px-3 py-2 text-sm text-neutral-700 hover:bg-neutral-300 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
                @click="cancelMicrosoftLogin"
              >
                {{ $t('accounts.cancel') }}
              </button>
            </div>

            <!-- Waiting for poll -->
            <div v-else class="flex items-center justify-center gap-2 py-4 text-sm text-neutral-500">
              <Loader2 :size="16" class="animate-spin" />
              {{ $t('accounts.preparing') }}
            </div>
          </div>

          <!-- Error Display -->
          <div v-if="loginError" class="mt-3 rounded-lg bg-red-900/40 px-3 py-2 text-sm text-red-400">
            {{ loginError }}
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Delete Confirmation Dialog -->
    <Teleport to="body">
      <div
        v-if="showDeleteDialog"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <div
          class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"
          @click="showDeleteDialog = false"
        />
        <div class="relative z-10 w-full max-w-sm gap-4 border bg-white dark:bg-zinc-900 p-6 shadow-xl rounded-lg pointer-events-auto">
          <div class="flex items-center gap-3 mb-4">
            <div class="flex h-10 w-10 items-center justify-center rounded-full bg-red-100 dark:bg-red-900/30">
              <AlertCircle :size="20" class="text-red-600 dark:text-red-400" />
            </div>
            <div>
              <h3 class="font-semibold text-neutral-900 dark:text-white">{{ $t('accounts.deleteTitle') }}</h3>
              <p class="text-sm text-muted-foreground">{{ $t('accounts.deleteUndone') }}</p>
            </div>
          </div>
          <p class="text-sm mb-4" v-html="$t('accounts.deleteConfirm', { name: deletingAccountName })">
          </p>
          <div class="flex justify-end gap-2">
            <button
              class="px-4 py-2 text-sm font-medium border rounded-lg hover:bg-muted transition-colors"
              @click="showDeleteDialog = false"
            >
              {{ $t('accounts.cancel') }}
            </button>
            <button
              class="flex items-center gap-2 px-4 py-2 text-sm font-medium bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors"
              @click="removeAccount"
            >
              <Trash2 :size="14" />
              {{ $t('accounts.delete') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>