<script setup lang="ts">
import { ref, computed, onMounted, watch, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { 
  User, Trash2, Plus, UserPlus, Loader2, WifiOff, Globe, MonitorCheck
} from "@lucide/vue";
import { DialogContent, DialogTitle } from "../components/ui/dialog";
import { useRouter, useRoute } from "vue-router";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../components/ui/alert-dialog";
import QrcodeVue from 'qrcode.vue';
import { getErrorType, trackEvent, sanitizeTrackingUrl } from "../utils/analytics";
import DInput from "../components/ui/DInput.vue";
import DSelect from "../components/ui/DSelect.vue";
import { getErrorMessage } from "../utils/error";

import { Account, LoginInitResponse } from "../types";

interface YggdrasilMetaLinks {
  homepage?: string;
  register?: string;
}

interface YggdrasilMeta {
  serverName?: string;
  links?: YggdrasilMetaLinks;
}

interface YggdrasilRootResponse {
  meta?: YggdrasilMeta;
}

interface AuthlibServer {
  url: string;
  name: string;
}

interface YggdrasilProfile {
  id: string;
  name: string;
}

interface AuthlibAuthResult {
  accessToken: string;
  clientToken: string;
  availableProfiles: YggdrasilProfile[];
  authlibServerName?: string;
}

type AccountType = "offline" | "microsoft" | "authlib";

// State
const accounts = ref<Account[]>([]);
const newUsername = ref("");
const authlibUrl = ref("");
const authlibUsername = ref("");
const authlibPassword = ref("");
const authlibMeta = ref<YggdrasilRootResponse | null>(null);
const authlibServers = ref<AuthlibServer[]>([]);
const isFetchingMeta = ref(false);
const isAddingOffline = ref(false);
const isAddingAuthlib = ref(false);
const isLoggingInMicrosoft = ref(false);
const microsoftLoginData = ref<LoginInitResponse | null>(null);
const loginError = ref<string | null>(null);
const deviceCode = ref("");
const showDeviceCodeFlow = ref(false);
const authlibServerOptions = computed(() => authlibServers.value.map(server => ({
  label: server.name,
  value: server.url
})));

const authlibProfiles = ref<YggdrasilProfile[] | null>(null);
const selectedAuthlibProfiles = ref<string[]>([]);
const tempAuthData = ref<AuthlibAuthResult | null>(null);

function isProfileAlreadyAdded(profile: YggdrasilProfile): boolean {
  return accounts.value.some(a => 
    a.accountType === 'authlib' && 
    a.id.replace(/-/g, '').toLowerCase() === profile.id.replace(/-/g, '').toLowerCase()
  );
}

const allProfilesAdded = computed(() => {
  if (!authlibProfiles.value || authlibProfiles.value.length === 0) return false;
  return authlibProfiles.value.every(p => isProfileAlreadyAdded(p));
});

// Modal state
const showAddAccountModal = ref(false);
const router = useRouter();
const route = useRoute();

const selectedAccountType = ref<AccountType>("microsoft");
const showDeleteDialog = ref(false);
const deletingAccountId = ref("");
const deletingAccountName = ref("");

// Load accounts on mount
async function loadAccounts(): Promise<void> {
  try {
    const res = await invoke<Account[]>("get_accounts");
    accounts.value = res || [];
  } catch (err) {
    console.error("Failed to load accounts:", err);
    accounts.value = [];
  }
}

// Open add account modal
function openAddAccountModal(type?: AccountType): void {
  showAddAccountModal.value = true;
  if (type) {
    selectedAccountType.value = type;
  }
  newUsername.value = "";
  loginError.value = null;
  microsoftLoginData.value = null;
  isLoggingInMicrosoft.value = false;
  
  // Reset authlib form and fetch initial meta if authlib is selected or on open
  authlibUsername.value = "";
  authlibPassword.value = "";
  authlibProfiles.value = null;
  selectedAuthlibProfiles.value = [];
  tempAuthData.value = null;
  loadAuthlibServers();
}

// Close add account modal
function closeAddAccountModal(): void {
  showAddAccountModal.value = false;
  newUsername.value = "";
  loginError.value = null;
  microsoftLoginData.value = null;
  isLoggingInMicrosoft.value = false;
  showDeviceCodeFlow.value = false;
  authlibProfiles.value = null;
  selectedAuthlibProfiles.value = [];
  tempAuthData.value = null;
}

// Add offline account
async function addOfflineAccount(): Promise<void> {
  if (!newUsername.value.trim()) return;

  isAddingOffline.value = true;
  loginError.value = null;

  try {
    await invoke("add_offline_account", { username: newUsername.value.trim() });
    newUsername.value = "";
    trackEvent("Account Added", { type: "offline" });
    await loadAccounts();
    closeAddAccountModal();
    // Notify other views to refresh accounts
    await emit("accounts-updated");
  } catch (err) {
    loginError.value = getErrorMessage(err);
  } finally {
    isAddingOffline.value = false;
  }
}

// Add authlib account
async function addAuthlibAccount(): Promise<void> {
  if (!authlibUsername.value.trim() || !authlibPassword.value.trim() || !authlibUrl.value.trim()) return;

  isAddingAuthlib.value = true;
  loginError.value = null;

  try {
    const authResult = await invoke<AuthlibAuthResult>("authenticate_authlib_user", { 
      url: authlibUrl.value.trim(),
      username: authlibUsername.value.trim(),
      password: authlibPassword.value
    });
    
    tempAuthData.value = authResult;
    authlibProfiles.value = authResult.availableProfiles;
    selectedAuthlibProfiles.value = authResult.availableProfiles
      .filter(p => !isProfileAlreadyAdded(p))
      .map(p => p.id);
    
  } catch (err) {
    trackEvent("Login Failed", { 
      type: "authlib", 
      error_type: getErrorType(err), 
      api: sanitizeTrackingUrl(authlibUrl.value) 
    });
    loginError.value = getErrorMessage(err);
  } finally {
    isAddingAuthlib.value = false;
  }
}

async function saveAuthlibAccounts(): Promise<void> {
  if (!tempAuthData.value || !authlibProfiles.value || selectedAuthlibProfiles.value.length === 0) return;

  isAddingAuthlib.value = true;
  loginError.value = null;

  try {
    const profilesToSave = authlibProfiles.value.filter(p => selectedAuthlibProfiles.value.includes(p.id));
    
    await invoke("save_authlib_accounts", { 
      url: authlibUrl.value.trim(),
      selectedProfiles: profilesToSave,
      accessToken: tempAuthData.value.accessToken,
      clientToken: tempAuthData.value.clientToken,
      authlibServerName: tempAuthData.value.authlibServerName,
      authlibEmail: authlibUsername.value.trim()
    });
    
    authlibUsername.value = "";
    authlibPassword.value = "";
    authlibProfiles.value = null;
    tempAuthData.value = null;
    selectedAuthlibProfiles.value = [];
    
    trackEvent("Account Added", { type: "authlib", api: sanitizeTrackingUrl(authlibUrl.value) });
    await loadAccounts();
    closeAddAccountModal();
    await emit("accounts-updated");
  } catch (err) {
    trackEvent("Login Failed", { 
      type: "authlib", 
      error_type: getErrorType(err), 
      api: sanitizeTrackingUrl(authlibUrl.value),
      phase: "save"
    });
    loginError.value = getErrorMessage(err);
  } finally {
    isAddingAuthlib.value = false;
  }
}

// Fetch authlib metadata
async function fetchAuthlibMeta(): Promise<void> {
  if (!authlibUrl.value || !authlibUrl.value.trim()) return;
  isFetchingMeta.value = true;
  try {
    const meta = await invoke<YggdrasilRootResponse>("get_authlib_meta", { url: authlibUrl.value.trim() });
    authlibMeta.value = meta;
  } catch (err) {
    console.error("Failed to fetch authlib meta:", err);
    authlibMeta.value = null;
  } finally {
    isFetchingMeta.value = false;
  }
}

async function loadAuthlibServers(): Promise<void> {
  try {
    const res = await invoke<AuthlibServer[]>("fetch_authlib_servers");
    authlibServers.value = res || [];
    if (authlibServers.value.length > 0) {
      if (!authlibServers.value.some(s => s.url === authlibUrl.value)) {
        authlibUrl.value = authlibServers.value[0].url;
      }
      fetchAuthlibMeta();
    } else {
      authlibUrl.value = "";
      authlibMeta.value = null;
    }
  } catch (err) {
    console.error("Failed to load authlib servers:", err);
    authlibServers.value = [];
  }
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

// Seamless Microsoft login
async function startSeamlessMicrosoftLogin(): Promise<void> {
  isLoggingInMicrosoft.value = true;
  loginError.value = null;
  microsoftLoginData.value = null;

  try {
    const account = await invoke<Account>("login_microsoft_oauth");
    if (!accounts.value) accounts.value = [];
    accounts.value.push(account);
    trackEvent("Account Added", { type: "microsoft", flow: "seamless" });
    isLoggingInMicrosoft.value = false;
    closeAddAccountModal();
    // Notify other views to refresh accounts
    await emit("accounts-updated");
  } catch (err) {
    trackEvent("Login Failed", { type: "microsoft", flow: "seamless", error_type: getErrorType(err) });
    loginError.value = getErrorMessage(err);
    isLoggingInMicrosoft.value = false;
  }
}

// Start Microsoft login (Device Code Fallback)
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
    trackEvent("Login Failed", { type: "microsoft", flow: "device_code_init", error_type: getErrorType(err) });
    loginError.value = getErrorMessage(err);
    isLoggingInMicrosoft.value = false;
  }
}

// Poll for Microsoft token
async function pollMicrosoftToken(code: string): Promise<void> {
  try {
    const account = await invoke<Account>("poll_microsoft_token", { deviceCode: code });
    if (!accounts.value) accounts.value = [];
    accounts.value.push(account);
    trackEvent("Account Added", { type: "microsoft", flow: "device_code" });
    microsoftLoginData.value = null;
    isLoggingInMicrosoft.value = false;
    closeAddAccountModal();
    // Notify other views to refresh accounts
    await emit("accounts-updated");
  } catch (err) {
    const errorMsg = getErrorMessage(err);
    if (errorMsg.includes("authorization_pending")) {
      setTimeout(() => pollMicrosoftToken(code), 5000);
    } else if (errorMsg.includes("expired_token") || errorMsg.includes("cancellation")) {
      trackEvent("Login Failed", { type: "microsoft", flow: "device_code", error_type: "DeviceCodeError", error_msg: errorMsg });
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

function isAuthlibAccount(account: Account): boolean {
  return account.accountType === "authlib";
}

onMounted(async () => {
  trackEvent("Accounts Viewed");
  await loadAccounts();
  window.addEventListener('authlib-servers-updated', loadAuthlibServers);
});

onUnmounted(() => {
  window.removeEventListener('authlib-servers-updated', loadAuthlibServers);
});

watch(
  () => route.query.addAuthlib,
  async (newVal) => {
    if (newVal) {
      const url = newVal as string;
      
      if (authlibServers.value.length === 0) {
        await loadAuthlibServers();
      }
      
      const exists = authlibServers.value.some(s => s.url === url);
      if (!exists) {
        try {
          await invoke("add_authlib_server", { url });
          trackEvent("Authlib Added", { type: "manual_authlib", api: sanitizeTrackingUrl(url) });
          await loadAuthlibServers();
        } catch (err) {
          console.error("Failed to auto-add authlib server:", err);
          trackEvent("Error Occurred", { 
            context: "manual_authlib", 
            error_type: getErrorType(err), 
            api: sanitizeTrackingUrl(url) 
          });
        }
      }

      selectedAccountType.value = "authlib";
      authlibUrl.value = url;
      openAddAccountModal("authlib");
      
      // Clean up the query so it doesn't trigger again on subsequent visits
      router.replace({ query: { ...route.query, addAuthlib: undefined } });
    }
  },
  { immediate: true }
);
</script>

<template>
  <div class="flex h-full flex-col p-4 gap-4 overflow-y-auto">
    <!-- Header -->
    <div>
      <h1 class="text-2xl font-bold">{{ $t('accounts.title') }}</h1>
      <p class="text-sm text-neutral-500 mt-1">{{ $t('accounts.desc') }}</p>
    </div>

    <!-- Accounts Grid Header -->
    <div class="flex items-center justify-between">
      <h2 class="text-lg font-semibold text-neutral-900 dark:text-white">{{ $t('accounts.saved', { count: accounts?.length || 0 }) }}</h2>
      <button
        class="flex items-center gap-2 rounded-lg bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
        @click="openAddAccountModal('microsoft')"
      >
        <Plus :size="16" />
        {{ $t('accounts.add') }}
      </button>
    </div>

    <!-- Accounts Grid -->
    <div class="flex-1">
      <div v-if="!accounts || accounts.length === 0" class="flex flex-col items-center justify-center py-12 px-6 text-center max-w-md mx-auto mt-12 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-xl border border-white/40 dark:border-zinc-800 rounded-3xl shadow-sm">
        <div class="flex h-16 w-16 items-center justify-center rounded-2xl bg-white/80 dark:bg-zinc-800 shadow-sm border border-white/50 dark:border-zinc-700 mb-5">
          <User :size="32" class="text-neutral-700 dark:text-neutral-300" />
        </div>
        <p class="text-xl font-semibold text-neutral-900 dark:text-white mb-2">{{ $t('accounts.noAccounts') }}</p>
        <p class="text-sm text-neutral-600 dark:text-neutral-400 font-medium">{{ $t('accounts.noAccountsDesc') }}</p>
      </div>

      <div v-else class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <div
          v-for="account in accounts"
          :key="account.id"
          class="relative rounded-xl border border-white/20 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md p-4 transition-all hover:border-primary/50 hover:bg-white/80 dark:hover:bg-zinc-900/80 shadow-sm"
        >
          <!-- Account Info -->
          <div class="flex items-start gap-3">
            <div
              class="flex h-12 w-12 shrink-0 items-center justify-center rounded-full text-lg font-bold"
              :class="{
                'bg-emerald-100 text-emerald-600 dark:bg-emerald-900/30 dark:text-emerald-400': isMsaAccount(account),
                'bg-purple-100 text-purple-600 dark:bg-purple-900/30 dark:text-purple-400': isAuthlibAccount(account),
                'bg-neutral-100 text-neutral-600 dark:bg-zinc-800 dark:text-zinc-400': !isMsaAccount(account) && !isAuthlibAccount(account)
              }"
            >
              {{ account.username.charAt(0).toUpperCase() }}
            </div>
            <div class="flex-1 min-w-0">
              <p class="font-medium truncate">{{ account.username }}</p>
              <div class="flex items-center gap-1.5 mt-1">
                <MonitorCheck v-if="isMsaAccount(account)" :size="14" class="text-green-500" />
                <Globe v-else-if="isAuthlibAccount(account)" :size="14" class="text-purple-500" />
                <WifiOff v-else :size="14" class="text-neutral-400 dark:text-neutral-500" />
                <span class="text-xs text-neutral-500 dark:text-neutral-400 truncate">
                  {{ isMsaAccount(account) ? $t('accounts.microsoft') : (isAuthlibAccount(account) ? ($t('accounts.authlib') + (account.authlibServerName ? ` - ${account.authlibServerName}` : '')) : $t('accounts.offline')) }}
                </span>
              </div>
            </div>
          </div>

          <!-- Action Buttons -->
          <div class="flex gap-2 mt-4">
            <button
              class="w-full flex items-center justify-center gap-1.5 rounded-lg border border-red-200 bg-red-50 px-3 py-1.5 text-sm text-red-600 hover:bg-red-100 dark:border-red-900/30 dark:bg-red-900/20 dark:text-red-400 dark:hover:bg-red-900/30 transition-colors"
              @click="confirmDeleteAccount(account)"
            >
              <Trash2 :size="14" />
              {{ $t('accounts.delete') }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Add Account Modal -->
    <DialogContent :open="showAddAccountModal" @update:open="!$event && closeAddAccountModal()" class="max-w-md p-4">
      <!-- Header -->
      <div class="flex items-center justify-between mb-4">
        <DialogTitle>{{ $t('accounts.add') }}</DialogTitle>
      </div>

          <!-- Account Type Selection -->
          <div class="space-y-3">
            <label class="text-sm font-medium">{{ $t('accounts.type') }}</label>
            <div class="flex gap-3">
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
              <button
                class="flex-1 flex flex-col items-center gap-2 rounded-lg border-2 p-4 transition-all"
                :class="selectedAccountType === 'authlib' 
                  ? 'border-purple-500 bg-purple-50 dark:bg-purple-900/30' 
                  : 'border-neutral-200 dark:border-zinc-700 hover:border-purple-300'"
                @click="selectedAccountType = 'authlib'"
              >
                <Globe :size="24" :class="selectedAccountType === 'authlib' ? 'text-purple-600 dark:text-purple-400' : 'text-neutral-500'" />
                <span class="text-sm font-medium" :class="selectedAccountType === 'authlib' ? 'text-purple-700 dark:text-purple-300' : ''">Authlib</span>
                <span class="text-xs text-neutral-500">{{ $t('accounts.authlibTab') }}</span>
              </button>
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
            </div>
          </div>

          <!-- Offline Account Form -->
          <div v-if="selectedAccountType === 'offline'" class="mt-4 space-y-3">
            <label class="text-sm font-medium">{{ $t('accounts.username') }}</label>
            <DInput
              v-model="newUsername"
              :placeholder="$t('accounts.enterUsername')"
              @keyup.enter="addOfflineAccount"
            />
            <button
              class="w-full flex items-center justify-center gap-2 rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
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
            <!-- Default view: One-click Login -->
            <div v-if="!isLoggingInMicrosoft && !microsoftLoginData && !showDeviceCodeFlow" class="space-y-3">
              <button
                class="w-full flex items-center justify-center gap-2 rounded-lg bg-emerald-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-emerald-500 disabled:opacity-50"
                :disabled="isLoggingInMicrosoft"
                @click="startSeamlessMicrosoftLogin"
              >
                <Loader2 v-if="isLoggingInMicrosoft" :size="16" class="animate-spin" />
                <UserPlus v-else :size="16" />
                {{ isLoggingInMicrosoft ? $t('accounts.preparing') : $t('accounts.loginWithMs') }}
              </button>
              
              <button
                class="w-full text-sm text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300 transition-colors"
                @click="showDeviceCodeFlow = true"
              >
                {{ $t('accounts.useDeviceCode') }}
              </button>
            </div>

            <!-- Seamless logging in -->
            <div v-else-if="isLoggingInMicrosoft && !microsoftLoginData && !showDeviceCodeFlow" class="flex flex-col items-center justify-center gap-3 py-4">
              <Loader2 :size="24" class="animate-spin text-emerald-500" />
              <span class="text-sm text-neutral-600 dark:text-neutral-400">{{ $t('accounts.waitingForBrowser') }}</span>
            </div>

            <!-- Device Code Flow (Fallback) Initial -->
            <div v-else-if="showDeviceCodeFlow && !microsoftLoginData" class="space-y-3">
              <button
                class="w-full flex items-center justify-center gap-2 rounded-lg bg-emerald-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-emerald-500"
                @click="startMicrosoftLogin"
              >
                <MonitorCheck :size="16" />
                {{ $t('accounts.fetchingDeviceCode') }}
              </button>
              <button
                class="w-full text-sm text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300 transition-colors"
                @click="showDeviceCodeFlow = false"
              >
                {{ $t('accounts.backToOneClick') }}
              </button>
            </div>

            <!-- Logging in with Device Code - show QR code -->
            <div v-else-if="microsoftLoginData" class="flex flex-col items-center space-y-4 py-2">
              <p class="text-sm text-center text-neutral-600 dark:text-zinc-400">
                {{ $t('accounts.scanQrCode') }}<a :href="microsoftLoginData.verificationUri + '?otc=' + microsoftLoginData.userCode" target="_blank" class="text-indigo-600 hover:underline">{{ $t('accounts.verificationLink') }}</a>
              </p>
              
              <div class="bg-white p-2 rounded-xl">
                <qrcode-vue :value="microsoftLoginData.verificationUri + '?otc=' + microsoftLoginData.userCode" :size="180" level="M" />
              </div>

              <div class="flex flex-col items-center">
                <span class="text-xs text-neutral-500 mb-1">{{ $t('accounts.enterCode') }}</span>
                <div class="flex items-center gap-2">
                  <span class="text-2xl font-mono font-bold tracking-wider text-indigo-600 dark:text-indigo-400">
                    {{ microsoftLoginData.userCode }}
                  </span>
                  <button
                    class="rounded p-1 text-neutral-500 hover:bg-neutral-100 dark:hover:bg-zinc-800"
                    title="复制验证码"
                    @click="copyCode"
                  >
                    <User :size="16" />
                  </button>
                </div>
              </div>

              <div class="flex items-center justify-center gap-2 py-2 text-sm text-emerald-600 dark:text-emerald-400">
                <Loader2 :size="16" class="animate-spin" />
                {{ $t('accounts.waitingForAuth') }}...
              </div>

              <button
                class="w-full rounded-lg bg-neutral-200 px-3 py-2 text-sm text-neutral-700 hover:bg-neutral-300 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
                @click="cancelMicrosoftLogin"
              >
                {{ $t('accounts.cancel') }}
              </button>
            </div>
          </div>

          <!-- Authlib Account Form -->
          <div v-else-if="selectedAccountType === 'authlib'" class="mt-4 space-y-3">
            <div v-if="!authlibServers || authlibServers.length === 0" class="flex flex-col items-center justify-center p-6 border-2 border-dashed border-neutral-300 dark:border-zinc-700 rounded-lg bg-neutral-50 dark:bg-zinc-800/50">
              <p class="text-sm text-neutral-500 mb-4 text-center whitespace-pre-wrap">{{ $t('accounts.noAuthlibServers') }}</p>
              <button 
                class="bg-primary text-primary-foreground text-sm font-medium px-4 py-2 rounded-lg hover:bg-primary/90 transition-colors" 
                @click="closeAddAccountModal(); router.push({ path: '/settings', query: { tab: 'authlib' } })"
              >
                {{ $t('accounts.goToSettings') }}
              </button>
            </div>
            
            <template v-else>
              <div class="flex items-center justify-between px-1">
                <div class="flex items-center gap-2">
                  <Loader2 v-if="isFetchingMeta" :size="14" class="animate-spin text-muted-foreground" />
                  <span v-else-if="authlibMeta?.meta?.serverName" class="text-sm text-neutral-600 dark:text-zinc-400">
                    {{ authlibMeta.meta.serverName }}
                  </span>
                </div>
                <div class="flex items-center gap-2 text-xs" v-if="authlibMeta?.meta?.links">
                  <a v-if="authlibMeta.meta.links.homepage" :href="authlibMeta.meta.links.homepage" target="_blank" class="text-blue-500 hover:underline">{{ $t('accounts.homePage') }}</a>
                  <a v-if="authlibMeta.meta.links.register" :href="authlibMeta.meta.links.register" target="_blank" class="text-blue-500 hover:underline">{{ $t('accounts.register') }}</a>
                  <a v-if="authlibMeta.meta.links.register && authlibMeta.meta.links.register.includes('/register')" :href="authlibMeta.meta.links.register.replace('/register', '/forgot')" target="_blank" class="text-blue-500 hover:underline">{{ $t('accounts.forgotPassword') }}</a>
                </div>
              </div>
              
              <div class="space-y-1">
                <label class="text-sm font-medium">{{ $t('accounts.authServer') }}</label>
                <DSelect
                  v-model="authlibUrl"
                  :options="authlibServerOptions"
                  @update:model-value="fetchAuthlibMeta"
                  class="w-full"
                />
              </div>
              <!-- Login Form -->
              <div v-if="!authlibProfiles" class="space-y-3">
                <div class="space-y-1">
                  <label class="text-sm font-medium">{{ $t('accounts.emailOrUsername') }}</label>
                  <DInput
                    v-model="authlibUsername"
                  />
                </div>
                <div class="space-y-1">
                  <label class="text-sm font-medium">{{ $t('accounts.password') }}</label>
                  <DInput
                    v-model="authlibPassword"
                    type="password"
                    @keyup.enter="addAuthlibAccount"
                  />
                </div>
                <button
                  class="w-full flex items-center justify-center gap-2 rounded-lg bg-purple-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-purple-500 disabled:cursor-not-allowed disabled:opacity-50"
                  :disabled="isAddingAuthlib || !authlibUsername.trim() || !authlibPassword.trim() || !authlibUrl.trim()"
                  @click="addAuthlibAccount"
                >
                  <Loader2 v-if="isAddingAuthlib" :size="16" class="animate-spin" />
                  <Globe v-else :size="16" />
                  {{ isAddingAuthlib ? $t('accounts.adding') : $t('accounts.login') }}
                </button>
              </div>

              <!-- Character Selection Form -->
              <div v-else class="space-y-3">
                <p class="text-sm font-medium">{{ $t('accounts.selectCharacters') }}</p>

                <div v-if="allProfilesAdded" class="p-3 mb-2 rounded-lg bg-neutral-100 dark:bg-zinc-800 border border-neutral-200 dark:border-zinc-700">
                  <p class="text-sm text-center text-neutral-500 dark:text-neutral-400">{{ $t('accounts.allCharactersAdded') }}</p>
                </div>

                <div class="max-h-48 overflow-y-auto space-y-1.5 border border-neutral-200 dark:border-zinc-700 rounded-lg p-2 bg-white/50 dark:bg-zinc-900/50">
                  <label v-for="profile in authlibProfiles" :key="profile.id" 
                    class="flex items-center gap-3 p-2 rounded-lg transition-colors"
                    :class="isProfileAlreadyAdded(profile) ? 'opacity-60 cursor-not-allowed bg-neutral-50 dark:bg-zinc-800/50' : 'hover:bg-neutral-100 dark:hover:bg-zinc-800 cursor-pointer'"
                  >
                    <input type="checkbox" :value="profile.id" v-model="selectedAuthlibProfiles" 
                      :disabled="isProfileAlreadyAdded(profile)"
                      class="rounded text-purple-600 focus:ring-purple-500 dark:border-zinc-600 dark:bg-zinc-800 disabled:opacity-50" 
                    />
                    <div class="flex items-center gap-2">
                      <div class="flex h-8 w-8 items-center justify-center rounded-md bg-purple-100 text-purple-600 dark:bg-purple-900/30 dark:text-purple-400 font-bold text-xs"
                        :class="isProfileAlreadyAdded(profile) ? 'grayscale' : ''">
                        {{ profile.name.charAt(0).toUpperCase() }}
                      </div>
                      <span class="text-sm font-medium" :class="isProfileAlreadyAdded(profile) ? 'text-neutral-500 dark:text-neutral-500' : 'text-neutral-900 dark:text-neutral-100'">
                        {{ profile.name }}
                        <span v-if="isProfileAlreadyAdded(profile)" class="ml-2 text-xs font-normal text-neutral-400">({{ $t('accounts.added') }})</span>
                      </span>
                    </div>
                  </label>
                </div>
                
                <div class="flex gap-2 pt-1">
                  <button 
                    class="flex-1 rounded-lg bg-neutral-200 px-3 py-2 text-sm text-neutral-700 hover:bg-neutral-300 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700 transition-colors"
                    @click="authlibProfiles = null; selectedAuthlibProfiles = []; tempAuthData = null;"
                  >
                    {{ $t('accounts.back') }}
                  </button>
                  <button
                    class="flex-1 flex items-center justify-center gap-2 rounded-lg bg-purple-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-purple-500 disabled:cursor-not-allowed disabled:opacity-50"
                    :disabled="isAddingAuthlib || selectedAuthlibProfiles.length === 0"
                    @click="saveAuthlibAccounts"
                  >
                    <Loader2 v-if="isAddingAuthlib" :size="16" class="animate-spin" />
                    <Plus v-else :size="16" />
                    {{ isAddingAuthlib ? $t('accounts.saving') : $t('accounts.confirmAdd') }}
                  </button>
                </div>
              </div>
            </template>
          </div>

          <!-- Error Display -->
          <div v-if="loginError" class="mt-3 rounded-lg bg-red-900/40 px-3 py-2 text-sm text-red-400">
            {{ loginError }}
          </div>
    </DialogContent>

    <!-- Delete Confirmation Dialog -->
    <AlertDialog :open="showDeleteDialog" @update:open="showDeleteDialog = $event" class="max-w-sm p-4">
      <div class="flex items-center gap-3 mb-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-full bg-red-100 dark:bg-red-900/30">
          <AlertCircle :size="20" class="text-red-600 dark:text-red-400" />
        </div>
        <div>
          <AlertDialogTitle class="font-semibold text-neutral-900 dark:text-white">{{ $t('accounts.deleteTitle') }}</AlertDialogTitle>
          <AlertDialogDescription class="text-sm text-muted-foreground">{{ $t('accounts.deleteUndone') }}</AlertDialogDescription>
        </div>
      </div>
          <i18n-t keypath="accounts.deleteConfirm" tag="p" class="text-sm mb-4">
            <template #name>
              <strong>{{ deletingAccountName }}</strong>
            </template>
          </i18n-t>
          <div class="flex justify-end gap-2">
            <button
              class="px-3 py-1.5 text-sm font-medium border rounded-lg hover:bg-muted transition-colors"
              @click="showDeleteDialog = false"
            >
              {{ $t('accounts.cancel') }}
            </button>
            <button
              class="flex items-center gap-2 px-3 py-1.5 text-sm font-medium bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors"
              @click="removeAccount"
            >
              <Trash2 :size="14" />
              {{ $t('accounts.delete') }}
            </button>
          </div>
    </AlertDialog>
  </div>
</template>