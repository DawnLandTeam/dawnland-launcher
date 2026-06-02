<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { 
  User, Trash2, Plus, UserPlus, Loader2, WifiOff, Globe, MonitorCheck
} from "@lucide/vue";
import { DialogContent, DialogTitle } from "../components/ui/dialog";
import { useRouter } from "vue-router";

const router = useRouter();
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../components/ui/alert-dialog";

interface Account {
  id: string;
  username: string;
  accountType: "offline" | "microsoft" | "authlib";
  accessToken?: string;
  refreshToken?: string;
  authlibServerName?: string;
}

interface LoginInitResponse {
  userCode: string;
  deviceCode: string;
  verificationUri: string;
  message: string;
}

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

// Modal state
const showAddAccountModal = ref(false);
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
function openAddAccountModal(): void {
  showAddAccountModal.value = true;
  selectedAccountType.value = "microsoft";
  newUsername.value = "";
  loginError.value = null;
  microsoftLoginData.value = null;
  isLoggingInMicrosoft.value = false;
  
  // Reset authlib form and fetch initial meta if authlib is selected or on open
  authlibUsername.value = "";
  authlibPassword.value = "";
  loadAuthlibServers();
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

// Add authlib account
async function addAuthlibAccount(): Promise<void> {
  if (!authlibUsername.value.trim() || !authlibPassword.value.trim() || !authlibUrl.value.trim()) return;

  isAddingAuthlib.value = true;
  loginError.value = null;

  try {
    await invoke("add_authlib_account", { 
      url: authlibUrl.value.trim(),
      username: authlibUsername.value.trim(),
      password: authlibPassword.value
    });
    authlibUsername.value = "";
    authlibPassword.value = "";
    await loadAccounts();
    closeAddAccountModal();
    await emit("accounts-updated");
  } catch (err) {
    loginError.value = typeof err === "string" ? err : String(err);
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
    if (!accounts.value) accounts.value = [];
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

function isAuthlibAccount(account: Account): boolean {
  return account.accountType === "authlib";
}

onMounted(() => {
  loadAccounts();
});
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
        @click="selectedAccountType = 'offline'; openAddAccountModal()"
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
                <span class="text-xs text-neutral-500">外置登录</span>
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
            <input
              v-model="newUsername"
              type="text"
              :placeholder="$t('accounts.enterUsername')"
              class="w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 dark:border-zinc-700 dark:bg-zinc-800"
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
            <!-- Not logging in -->
            <div v-if="!isLoggingInMicrosoft && !microsoftLoginData">
              <button
                class="w-full flex items-center justify-center gap-2 rounded-lg bg-emerald-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-emerald-500"
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
            <div v-else class="flex items-center justify-center gap-2 py-2 text-sm text-neutral-500">
              <Loader2 :size="16" class="animate-spin" />
              {{ $t('accounts.preparing') }}
            </div>
          </div>

          <!-- Authlib Account Form -->
          <div v-else-if="selectedAccountType === 'authlib'" class="mt-4 space-y-3">
            <div v-if="!authlibServers || authlibServers.length === 0" class="flex flex-col items-center justify-center p-6 border-2 border-dashed border-neutral-300 dark:border-zinc-700 rounded-lg bg-neutral-50 dark:bg-zinc-800/50">
              <p class="text-sm text-neutral-500 mb-4 text-center">暂无已添加的认证服务器，<br>请先前往设置页面进行添加。</p>
              <button 
                class="bg-primary text-primary-foreground text-sm font-medium px-4 py-2 rounded-lg hover:bg-primary/90 transition-colors" 
                @click="closeAddAccountModal(); router.push({ path: '/settings', query: { tab: 'authlib' } })"
              >
                前往设置管理
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
                  <a v-if="authlibMeta.meta.links.homepage" :href="authlibMeta.meta.links.homepage" target="_blank" class="text-blue-500 hover:underline">主页</a>
                  <a v-if="authlibMeta.meta.links.register" :href="authlibMeta.meta.links.register" target="_blank" class="text-blue-500 hover:underline">注册</a>
                  <a v-if="authlibMeta.meta.links.register && authlibMeta.meta.links.register.includes('/register')" :href="authlibMeta.meta.links.register.replace('/register', '/forgot')" target="_blank" class="text-blue-500 hover:underline">忘记密码</a>
                </div>
              </div>
              
              <div class="space-y-1">
                <label class="text-sm font-medium">认证服务器</label>
                <select
                  v-model="authlibUrl"
                  class="w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-primary/50 dark:border-zinc-700 dark:bg-zinc-800"
                  @change="fetchAuthlibMeta"
                >
                  <option v-for="server in authlibServers" :key="server.url" :value="server.url">
                    {{ server.name }}
                  </option>
                </select>
              </div>
              <div class="space-y-1">
                <label class="text-sm font-medium">Email / Username</label>
                <input
                  v-model="authlibUsername"
                  type="text"
                  class="w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 dark:border-zinc-700 dark:bg-zinc-800"
                />
              </div>
              <div class="space-y-1">
                <label class="text-sm font-medium">Password</label>
                <input
                  v-model="authlibPassword"
                  type="password"
                  class="w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-900 dark:text-white placeholder:text-neutral-400 dark:placeholder:text-neutral-500 dark:border-zinc-700 dark:bg-zinc-800"
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
                {{ isAddingAuthlib ? $t('accounts.adding') : $t('accounts.add') }}
              </button>
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
          <p class="text-sm mb-4" v-html="$t('accounts.deleteConfirm', { name: deletingAccountName })">
          </p>
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