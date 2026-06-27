<script setup lang="ts">
import { ref, shallowRef, computed, nextTick, onMounted, onDeactivated, onActivated, watch } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useTaskStore } from "../../composables/useTaskStore";
import TaskDetailView from "../TaskDetailView.vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { trackEvent, getErrorType } from "../../utils/analytics";
import { getErrorMessage } from "../../utils/error";
import { toast } from "../../composables/useToast";
import { useI18n } from "vue-i18n";
import { Package, UploadCloud, Loader2, Search, Download, User, Calendar } from "@lucide/vue";
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from "../../components/ui/alert-dialog";
import { DialogContent, DialogTitle, DialogDescription } from "../../components/ui/dialog";
import DInput from "../ui/DInput.vue";
import DSelect from "../ui/DSelect.vue";

const { t } = useI18n();
const router = useRouter();
const taskStore = useTaskStore();
const currentTaskId = ref<string | null>(null);
const currentTask = computed(() => {
  if (!currentTaskId.value) return null;
  return taskStore.tasks.value.find((t: any) => t.id === currentTaskId.value) || null;
});
const route = useRoute();

const props = defineProps<{
  updateId?: string;
  updateProjectId?: string;
  updateSource?: string;
  updateCurrentVersion?: string;
  isModalUpdate?: boolean;
}>();

const emit = defineEmits(['cancel-update']);

// Modes: 'online' or 'local'
const installMode = ref<'online' | 'local'>('online');

const isUpdate = computed(() => !!props.updateId || (!!route.query.update_id && !route.query.server_id));

const zipPath = ref("");
const onlineUrl = ref("");
const selectedVersionName = ref<string | null>(null);
const instanceName = ref("");
const isInstalling = ref(false);
const showSuccessModal = ref(false);
const showCancelConfirmModal = ref(false);
const isCanceling = ref(false);

// --- Online Search State ---
const searchQuery = ref('');
const source = ref('curseforge'); // 'modrinth' or 'curseforge'
const sourceOptions = [
  { label: 'CurseForge', value: 'curseforge' },
  { label: 'Modrinth', value: 'modrinth' }
];
const isSearching = ref(false);
const modpacks = shallowRef<any[]>([]);

const selectedModpack = ref<any>(null);
const showVersionsModal = ref(false);
const isFetchingVersions = ref(false);
const modpackVersions = shallowRef<any[]>([]);
const instanceNameInput = ref('');

// --- Install Progress State ---
const currentPhase = ref("");
const statusMessage = ref("");
const completedMods = ref(new Set<string>());
const totalMods = ref(0);
const currentFile = ref("");
const forgeLogs = ref<string[]>([]);




const logContainer = ref<HTMLElement | null>(null);
const activeDownloads = ref(new Map<string, any>());



const lastProcessedQueryStr = ref("");

const initializeView = async () => {
  const currentQueryStr = JSON.stringify(route.query);
  if (lastProcessedQueryStr.value === currentQueryStr && !props.isModalUpdate) {
    return;
  }
  lastProcessedQueryStr.value = currentQueryStr;

  // Restore if task is already running
  if (currentTaskId.value) {
     const task = taskStore.tasks.value.find((t: any) => t.id === currentTaskId.value);
     if (task && (task.status === 'Running' || task.status === 'Pending')) {
        isInstalling.value = true;
        return;
     }
  }

  const activeTask = taskStore.tasks.value.find((t: any) => 
    (t.status === 'Running' || t.status === 'Pending') && (t.type === 'install_modpack' || t.type === 'update_modpack' || t.type === 'install_server_modpack')
  );
  if (activeTask) {
    currentTaskId.value = activeTask.id;
    isInstalling.value = true;
    return;
  }

  // Clear previous installation state in case user returns from a cancelled installation
  zipPath.value = "";
  onlineUrl.value = "";
  selectedVersionName.value = null;
  instanceName.value = "";
  isInstalling.value = false;

  if (props.updateId) {
    instanceName.value = props.updateId;
    installMode.value = 'online';
    searchQuery.value = props.updateId;
    source.value = props.updateSource || 'curseforge';
    
    if (props.updateProjectId) {
      const dummyModpack = {
        project_id: props.updateProjectId,
        title: props.updateId,
        source: source.value
      };
      try {
        await openVersionsModal(dummyModpack);
      } catch (err) {
        toast.error(t('common.error', 'Error'), getErrorMessage(err));
        showVersionsModal.value = false;
        isInstalling.value = false;
        emit('cancel-update');
      }
    }
    return;
  }

  if (route.query.update_id) {
    instanceName.value = route.query.update_id as string;
    
    if (route.query.zip) {
      installMode.value = 'local';
      zipPath.value = route.query.zip as string;
    } else if (route.query.online_url) {
      // Direct install from a backend server ZIP URL
      installMode.value = 'online';
      onlineUrl.value = route.query.online_url as string;
      
      // Auto-start installation for server zip
      nextTick(() => {
        installModpack();
      });
    } else {
      installMode.value = 'online';
      searchQuery.value = route.query.update_id as string;
      const sourceQuery = route.query.source as string;
      if (sourceQuery === 'modrinth' || sourceQuery === 'curseforge') {
        source.value = sourceQuery;
      }
      
      if (route.query.project_id) {
        const dummyModpack = {
          project_id: route.query.project_id as string,
          title: route.query.update_id as string,
          source: source.value
        };
        
        if (route.query.version_id) {
          // If we have a specific version ID, fetch versions and auto-install
          isSearching.value = true;
          const fetchVersions = source.value === 'modrinth' ? 'get_modrinth_modpack_versions' : 'get_curseforge_modpack_versions';
          invoke(fetchVersions, { projectId: dummyModpack.project_id })
            .then((versions: any) => {
              let targetVersion = versions.find((v: any) => v.id.toString() === route.query.version_id);
              if (!targetVersion) targetVersion = versions.find((v: any) => v.name === route.query.version_id);
              if (!targetVersion) targetVersion = versions.find((v: any) => typeof v.name === 'string' && v.name.includes(route.query.version_id as string));

              if (targetVersion) {
                onlineUrl.value = targetVersion.download_url;
                selectedVersionName.value = targetVersion.name;
                nextTick(() => {
                  installModpack();
                });
              } else {
                // Fallback to versions modal if not found
                openVersionsModal(dummyModpack);
              }
            })
            .catch((e) => {
              console.error(e);
              openVersionsModal(dummyModpack);
            })
            .finally(() => {
              isSearching.value = false;
            });
        } else {
          // Open versions modal if no specific version
          openVersionsModal(dummyModpack);
        }
      } else {
        isSearching.value = true;
        modpacks.value = [];
        invoke(source.value === 'modrinth' ? 'search_modrinth_modpacks' : 'search_curseforge_modpacks', { query: searchQuery.value })
          .then((res: any) => {
            modpacks.value = res;
            if (res && res.length > 0) {
              openVersionsModal(res[0]);
            }
          })
          .catch(console.error)
          .finally(() => {
            isSearching.value = false;
          });
      }
    }
  } else {
    // Default to fetching trending modpacks if not an update
    searchModpacks();
  }
};

onMounted(() => {
  initializeView();
});

onActivated(() => {
  initializeView();
});

watch(() => route.query, () => {
  if (route.path === '/modpack-install') {
    initializeView();
  }
}, { deep: true });

onDeactivated(() => {
  searchQuery.value = "";
  modpacks.value = [];
  lastProcessedQueryStr.value = "";
});



listen("install-progress", (e: any) => {
  if (e.payload.phase) {
    currentPhase.value = e.payload.phase;
  }

  if (e.payload.phase === "downloading" && e.payload.totalTasks) {
    totalMods.value = e.payload.totalTasks;
    completedMods.value.clear();
  } else if (e.payload.phase === "running_processors") {
    totalMods.value = 0;
  } else if (e.payload.phase === "complete") {
    totalMods.value = 0;
    forgeLogs.value = [];
  }

  if (e.payload.currentFile) {
    if (e.payload.phase === "running_processors") {
      statusMessage.value = "Running Forge Installer...";
      forgeLogs.value.push(e.payload.currentFile);
      if (forgeLogs.value.length > 500) {
        forgeLogs.value.shift();
      }
      nextTick(() => {
        if (logContainer.value) {
          logContainer.value.scrollTop = logContainer.value.scrollHeight;
        }
      });
    } else {
      statusMessage.value = "Installing dependency: " + e.payload.currentFile;
    }
  } else if (e.payload.phase && e.payload.phase !== "complete" && e.payload.phase !== "running_processors") {
    statusMessage.value = "Installing dependency...";
  }

  if (e.payload.completedFile) {
    completedMods.value.add(e.payload.completedFile);
    currentFile.value = e.payload.completedFile;
  }
});

listen("download-progress", (e: any) => {
  const p = e.payload;
  if (p.completed) {
    activeDownloads.value.delete(p.taskId);
    if (!p.error) {
      completedMods.value.add(p.taskId);
      if (p.fileName) {
        currentFile.value = p.fileName;
      }
    }
  } else {
    activeDownloads.value.set(p.taskId, p);
    if (p.fileName) {
      currentFile.value = p.fileName;
    }
  }
});

/* const progressPercent = computed(() => {
  if (totalMods.value === 0) return 0;
  return Math.floor((completedMods.value.size / totalMods.value) * 100);
}); */

// --- Actions ---

const searchModpacks = async () => {
  isSearching.value = true;
  modpacks.value = [];
  try {
    if (source.value === 'modrinth') {
      modpacks.value = await invoke('search_modrinth_modpacks', { query: searchQuery.value });
    } else {
      modpacks.value = await invoke('search_curseforge_modpacks', { query: searchQuery.value });
    }
  } catch (error) {
    console.error("Failed to search modpacks:", error);
  } finally {
    isSearching.value = false;
  }
};

const openVersionsModal = async (modpack: any) => {
  selectedModpack.value = modpack;
  if (isUpdate.value) {
    instanceNameInput.value = instanceName.value;
  } else {
    instanceNameInput.value = modpack.title.replace(/[^a-zA-Z0-9_ -]/g, '_');
  }
  showVersionsModal.value = true;
  isFetchingVersions.value = true;
  modpackVersions.value = [];
  
  try {
    if (modpack.source === 'modrinth') {
      modpackVersions.value = await invoke('get_modrinth_modpack_versions', { projectId: modpack.project_id });
    } else {
      modpackVersions.value = await invoke('get_curseforge_modpack_versions', { projectId: modpack.project_id });
    }
  } catch (error) {
    console.error("Failed to fetch modpack versions:", error);
    throw error;
  } finally {
    isFetchingVersions.value = false;
  }
};

const getVersionUpgradeStatus = (index: number) => {
  let cv = "";
  if (props.isModalUpdate) {
    cv = String(props.updateCurrentVersion || "");
  } else {
    cv = String(route.query.current_version || "");
  }
  cv = cv.trim();

  if (!cv || cv === "undefined" || cv === "null") {
    return { text: t('modpacks.installBtn'), disabled: false, class: 'bg-emerald-600 hover:bg-emerald-700' };
  }
  
  const cvLower = cv.toLowerCase();
  const currentVersionIndex = modpackVersions.value.findIndex(v => {
    const vName = String(v.name || "");
    const vId = String(v.id || "");
    
    if (vName === cv || vId === cv || (vName && vName.toLowerCase().includes(cvLower))) {
      return true;
    }
    
    // Fuzzy match: extract the semver part (e.g., "1.7.31" from "v1.7.31-CF" and "COBBLEVERSE 1.7.31 [CF]")
    const extractVer = (s: string) => {
      const match = s.match(/\d+(?:\.\d+)+/);
      return match ? match[0] : "";
    };
    
    const cvVer = extractVer(cv);
    const vNameVer = extractVer(vName);
    
    if (cvVer && vNameVer && cvVer === vNameVer) {
      return true;
    }
    
    return false;
  });
  
  if (currentVersionIndex === -1) {
    return { text: t('modpacks.installBtn'), disabled: false, class: 'bg-emerald-600 hover:bg-emerald-700' };
  }
  
  if (index < currentVersionIndex) return { text: t('modpack.upgrade'), disabled: false, class: 'bg-blue-600 hover:bg-blue-700' };
  if (index === currentVersionIndex) return { text: t('modpack.reinstall'), disabled: false, class: 'bg-amber-600 hover:bg-amber-700' };
  return { text: t('modpack.outdated'), disabled: true, class: 'bg-gray-500' };
};

const selectOnlineVersion = (version: any) => {
  if (!instanceNameInput.value.trim()) {
    alert("请输入实例名称 / Please enter an instance name");
    return;
  }
  
  // Set installation parameters
  onlineUrl.value = version.download_url;
  installMode.value = 'online';
  instanceName.value = instanceNameInput.value;
  selectedVersionName.value = version.name;
  showVersionsModal.value = false;
  
  // Start installation automatically
  installModpack();
};

const selectZip = async () => {
  try {
    const selected = await open({
      filters: [{ name: "Modpack Archives", extensions: ["zip", "mrpack"] }],
    });
    
    if (selected && typeof selected === 'string') {
      zipPath.value = selected;
      
      // Auto-extract name if not set
      if (!instanceName.value) {
        try {
          const manifestName = await invoke('read_modpack_name', { zipPath: selected });
          if (manifestName && typeof manifestName === 'string') {
            instanceName.value = String(manifestName).replace(/[<>:"/\\|?*]/g, "").trim();
          }
        } catch (e) {
          console.warn("Failed to read modpack name from manifest:", e);
          const match = selected.match(/[\\\/]([^\\\/]+)\.(zip|mrpack)$/i);
          if (match) {
            instanceName.value = match[1];
          }
        }
      }
    }
  } catch (err) {
    console.error("Failed to open dialog", err);
  }
};

const installModpack = async () => {
  if (isInstalling.value) return;
  if (!zipPath.value && !onlineUrl.value) return;
  if (!instanceName.value) return;

  isInstalling.value = true;
  completedMods.value.clear();
  forgeLogs.value = [];
  totalMods.value = 0;
  currentPhase.value = "starting";
  statusMessage.value = "Starting installation...";

  try {
    // Event tracking removed: trackEvent("modpack_install_started", { type: onlineUrl.value ? "online" : "local", isUpdate: isUpdate.value });
    if (onlineUrl.value) {
      console.log("Invoking download_and_install_online_modpack...");
      currentTaskId.value = await invoke<string>("download_and_install_online_modpack", {
        url: onlineUrl.value,
        instanceName: instanceName.value,
        projectId: selectedModpack.value?.project_id || route.query.project_id || null,
        isUpdate: isUpdate.value,
      });
    } else {
      console.log("Invoking install_modpack...");
      currentTaskId.value = await invoke<string>("install_modpack", {
        zipPath: zipPath.value,
        instanceName: instanceName.value,
        isUpdate: isUpdate.value,
        projectId: null,
      });
    }
    // Bind to server if applicable (we wait for bind to submit, wait, bind happens AFTER install?)
    // Actually, bind_instance_to_server expects the instance to exist. Wait! If the install is running in the background, the instance folder might not be fully ready.
    // However, bind_instance_to_server just writes to `servers.json` in the app data. So it's fine to do it immediately.
    if (route.query.server_id) {
      console.log("Binding instance to server...");
      await invoke("bind_instance_to_server", {
        instanceId: instanceName.value,
        serverId: String(route.query.server_id),
        packVersionId: route.query.version_id ? String(route.query.version_id) : null,
        packFileName: route.query.pack_file_name ? String(route.query.pack_file_name) : null,
      });
    }
    
    console.log("Installation task submitted successfully.");
    let finalName = selectedModpack.value ? selectedModpack.value.title || selectedModpack.value.name : "Unknown Modpack";
    if (!selectedModpack.value && zipPath.value) {
      try {
        finalName = await invoke<string>("get_modpack_name", { zipPath: zipPath.value });
      } catch (e) {
        console.warn("Could not read manifest name, falling back to filename", e);
        finalName = zipPath.value.split(/[/\\]/).pop() || "Unknown Modpack";
      }
    }
    trackEvent("Modpack Install Completed", { name: finalName, projectId: selectedModpack.value?.project_id || route.query.project_id || null });
  } catch (error) {
    console.error("Installation failed:", error);
    trackEvent("Error Occurred", { context: "modpack_install", error_type: getErrorType(error) });
    statusMessage.value = `Installation failed: ${getErrorMessage(error)}`;
    isInstalling.value = false;
    currentTaskId.value = null;
    
    if (getErrorMessage(error).includes("cancelled by user") && route.query.server_id) {
      router.push("/servers");
    }
  }
};

const cancelInstallation = async () => {
  showCancelConfirmModal.value = false;
  isCanceling.value = true;
  try {
    await invoke("cancel_installation");
  } catch (e) {
    console.error("Failed to cancel installation:", e);
    isCanceling.value = false;
  }
};

const handleSuccessConfirm = () => {
  showSuccessModal.value = false;
  router.push("/instances");
};

const formatDate = (dateString: string) => {
  if (!dateString) return 'Unknown';
  return new Date(dateString).toLocaleDateString();
};
</script>

<template>
  <div class="h-full flex flex-col mx-auto w-full" :class="isInstalling ? 'max-w-2xl py-10 px-6' : 'p-4 space-y-6 overflow-hidden'">
    
    <!-- Absolute positioned floating button for installing state -->
    <div v-if="isInstalling && currentTask && ['Completed', 'Failed', 'Cancelled'].includes(currentTask.status)" class="absolute top-6 right-6 z-50">
      <button
        @click="() => { isInstalling = false; currentTaskId = null; initializeView(); }"
        class="flex items-center gap-2 px-6 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md transition-colors text-sm font-medium shadow-sm"
      >
        {{ currentTask.status === 'Completed' ? t('install.finish') : t('install.back') }}
      </button>
    </div>
    
    <!-- Unified Header -->
    <div class="flex items-center justify-between shrink-0 mb-2">
      <div class="flex items-center gap-3">
        <Package class="w-8 h-8 text-emerald-600" />
        <div>
          <h1 class="text-2xl font-bold text-gray-900 dark:text-white">
            {{ isUpdate ? t('install.updateModpackTitle', 'Update Modpack') : t('install.modpackTitle', 'Install Modpack') }}
          </h1>
          <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
            {{ isUpdate ? t('install.updateModpackDesc', 'Update your instance using a newer modpack archive.') : t('install.installModpackDesc', '从在线资源库搜索整合包，或从本地上传压缩包进行安装。') }}
          </p>
        </div>
      </div>
      
      <!-- Mode Selector -->
      <div v-if="!isInstalling" class="flex p-1 bg-gray-100 dark:bg-gray-900 rounded-lg w-fit shrink-0">
        <button 
          @click="installMode = 'online'"
          class="px-6 py-2 rounded-md text-sm font-medium transition-all"
          :class="installMode === 'online' ? 'bg-white dark:bg-gray-800 text-emerald-600 shadow-sm' : 'text-gray-500 hover:text-gray-700 dark:hover:text-gray-300'"
        >
          {{ t('modpacks.searchOnline') }}
        </button>
        <button 
          @click="installMode = 'local'"
          class="px-6 py-2 rounded-md text-sm font-medium transition-all"
          :class="installMode === 'local' ? 'bg-white dark:bg-gray-800 text-emerald-600 shadow-sm' : 'text-gray-500 hover:text-gray-700 dark:hover:text-gray-300'"
        >
          {{ t('modpacks.uploadLocal') }}
        </button>
      </div>
    </div>
    <!-- ONLINE MODE UI -->
    <template v-if="!isInstalling && installMode === 'online' && !props.isModalUpdate">
      <!-- Search Controls -->
      <div class="flex gap-4 items-center bg-white dark:bg-zinc-950 p-4 rounded-xl border border-neutral-200 dark:border-zinc-800 shadow-sm shrink-0">
        <div class="relative flex-1">
          <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-neutral-500 dark:text-zinc-400" />
          <DInput 
            v-model="searchQuery" 
            :placeholder="t('modpacks.searchPlaceholder')" 
            class="!pl-10"
            @keydown.enter="searchModpacks"
          />
        </div>
        
        <DSelect 
          v-model="source" 
          :options="sourceOptions"
          @update:model-value="searchModpacks"
          :disabled="isSearching || isInstalling"
          class="w-[180px]"
        />
        
        <button 
          @click="searchModpacks" 
          :disabled="isSearching" 
          class="inline-flex items-center justify-center rounded-md text-sm font-medium transition-colors bg-emerald-600 hover:bg-emerald-700 text-white dark:bg-emerald-600 dark:hover:bg-emerald-700 h-10 px-4 py-2 min-w-[100px] shadow-sm disabled:opacity-50"
        >
          <Loader2 v-if="isSearching" class="h-4 w-4 animate-spin mr-2" />
          <Search v-else class="h-4 w-4 mr-2" />
          {{ t('modpacks.searchBtn') }}
        </button>
      </div>

      <!-- Results Grid -->
      <div class="flex-1 overflow-y-auto pr-2 pb-4 relative rounded-xl">
        <!-- Loading Overlay -->
        <div v-if="isSearching" class="absolute inset-0 flex flex-col items-center justify-center bg-white/40 dark:bg-zinc-900/40 backdrop-blur-[2px] z-10 rounded-xl transition-all duration-300">
          <Loader2 class="h-10 w-10 animate-spin text-emerald-600 dark:text-emerald-500 mb-4 drop-shadow-sm" />
          <p class="text-sm text-neutral-600 dark:text-neutral-400 font-medium animate-pulse">{{ t('install.loading', '加载中...') }}</p>
        </div>

        <div v-if="modpacks.length === 0 && !isSearching" class="h-full flex flex-col items-center justify-center text-neutral-500 dark:text-zinc-400">
          <Package class="h-16 w-16 mb-4 opacity-20" />
          <p>{{ t('modpacks.searchHint') }}</p>
        </div>
        
        <div v-else class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6" :class="{ 'opacity-50 pointer-events-none transition-opacity duration-300': isSearching }">
          <div 
            v-for="modpack in modpacks" 
            :key="modpack.project_id"
            class="rounded-xl border border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 text-neutral-900 dark:text-zinc-100 shadow-sm group overflow-hidden flex flex-col cursor-pointer transition-all duration-300 hover:shadow-xl hover:-translate-y-1 hover:border-emerald-500/50 backdrop-blur-sm"
            @click="openVersionsModal(modpack)"
          >
            <!-- Cover/Header Image Area -->
            <div class="h-32 bg-neutral-100 dark:bg-zinc-800 relative overflow-hidden flex items-center justify-center shrink-0">
              <img 
                v-if="modpack.icon_url" 
                :src="modpack.icon_url" 
                class="w-full h-full object-cover opacity-80 group-hover:opacity-100 group-hover:scale-105 transition-all duration-500"
                alt="Cover"
              />
              <Package v-else class="h-12 w-12 text-neutral-400/30 dark:text-zinc-500/30" />
              <div class="absolute inset-0 bg-gradient-to-t from-white/90 dark:from-zinc-900/90 to-transparent"></div>
              
              <div class="absolute bottom-3 left-4 flex gap-2">
                <div class="inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold transition-colors border-transparent bg-white/80 text-neutral-900 dark:bg-zinc-900/80 dark:text-zinc-100 backdrop-blur-md shadow-sm">
                  {{ modpack.source === 'modrinth' ? 'Modrinth' : 'CurseForge' }}
                </div>
              </div>
            </div>
            
            <div class="flex flex-col space-y-1.5 p-6 pt-4 pb-2 shrink-0">
              <h3 class="text-2xl font-semibold leading-none tracking-tight line-clamp-1 text-lg group-hover:text-emerald-500 transition-colors">
                {{ modpack.title }}
              </h3>
              <p class="text-sm text-neutral-500 dark:text-zinc-400 flex items-center gap-4 text-xs mt-1.5">
                <span class="flex items-center gap-1.5 font-medium">
                  <User class="h-3.5 w-3.5" /> {{ modpack.author }}
                </span>
                <span class="flex items-center gap-1.5">
                  <Download class="h-3.5 w-3.5" /> {{ (modpack.downloads / 1000).toFixed(1) }}k
                </span>
              </p>
            </div>
            
            <div class="p-6 pt-0 text-sm text-muted-foreground line-clamp-2 flex-1">
              {{ modpack.description || '无介绍' }}
            </div>
            
            <div class="p-4 pt-0 mt-auto flex flex-wrap gap-1.5">
              <div v-for="loader in modpack.loaders.slice(0, 3)" :key="loader" class="inline-flex items-center rounded-full border px-2.5 py-0.5 font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 text-foreground text-[10px] uppercase">
                {{ loader }}
              </div>
              <div v-if="modpack.loaders.length > 3" class="inline-flex items-center rounded-full border px-2.5 py-0.5 font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 text-foreground text-[10px]">
                +{{ modpack.loaders.length - 3 }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- LOCAL MODE UI -->
    <template v-else-if="!isInstalling && installMode === 'local'">
      <div class="max-w-2xl w-full mx-auto space-y-8 mt-4">
        <!-- Upload Zone -->
        <div
          class="border-2 border-dashed border-gray-300 dark:border-gray-700 rounded-xl p-10 flex flex-col items-center justify-center text-center cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors"
          @click="selectZip"
        >
          <UploadCloud class="w-12 h-12 text-gray-400 mb-4" />
          <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-1">
            {{ zipPath ? t('install.fileSelected', 'File Selected') : t('install.selectFile', 'Select Modpack Archive') }}
          </h3>
          <p class="text-sm text-gray-500 max-w-sm overflow-hidden text-ellipsis whitespace-nowrap">
            {{ zipPath || t('install.supportedFormats', 'Supports .zip and .mrpack formats') }}
          </p>
        </div>

        <!-- Form -->
        <div class="space-y-6">
          <div>
            <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              {{ t('install.instanceName', 'Instance Name') }}
            </label>
            <DInput
              v-model="instanceName"
              :disabled="isUpdate"
              :placeholder="t('modpacks.defaultInstanceName')"
            />
          </div>

          <div v-if="isUpdate" class="p-4 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800/50 rounded-lg flex items-start gap-3">
            <div class="mt-0.5 text-yellow-600 dark:text-yellow-400 font-bold">!</div>
            <div>
              <h4 class="text-sm font-semibold text-yellow-800 dark:text-yellow-300">{{ t('install.updateNoticeTitle', 'Update Notice') }}</h4>
              <p class="text-sm text-yellow-700 dark:text-yellow-400/80 mt-1">
                {{ t('install.updateNoticeDesc', 'Updating will automatically clean up outdated modpack mods and apply the new ones. Don\'t worry, your manually installed mods will be preserved.') }}
              </p>
            </div>
          </div>

          <div class="flex justify-end pt-4">
            <button
              class="px-8 py-3 bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg font-medium transition-colors disabled:opacity-50"
              :disabled="!zipPath || !instanceName"
              @click="installModpack"
            >
              {{ t('install.installButton', 'Install Local Archive') }}
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- INSTALLING STATE UI -->
    <template v-else-if="isInstalling">
      <div v-if="currentTask" class="mt-4 w-full max-w-4xl mx-auto">
        <TaskDetailView :task="currentTask" />
      </div>
    </template>
    
    <!-- Modpack Versions Modal -->
    <DialogContent 
      :open="showVersionsModal" 
      @update:open="val => { showVersionsModal = val; if (!val && props.isModalUpdate) emit('cancel-update'); }"
      class="max-w-4xl max-h-[85vh] flex flex-col overflow-hidden bg-white dark:bg-zinc-950 border-neutral-200 dark:border-zinc-800 shadow-2xl p-6 text-neutral-900 dark:text-zinc-100"
    >
        <div class="flex flex-col space-y-1.5 text-center sm:text-left shrink-0">
          <DialogTitle class="text-2xl flex items-center gap-3">
            <img v-if="selectedModpack?.icon_url" :src="selectedModpack.icon_url" class="h-8 w-8 rounded-md" />
            {{ selectedModpack?.title }}
          </DialogTitle>
          <DialogDescription class="mt-2 line-clamp-2 text-muted-foreground text-sm">
            {{ selectedModpack?.description }}
          </DialogDescription>
        </div>

        <div class="flex flex-col gap-4 py-4 overflow-hidden">
          <div class="flex items-center gap-4 bg-secondary/30 p-4 rounded-xl border border-white/5">
            <div class="flex-1">
              <label class="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1.5 block">
                {{ t('modpacks.instanceNamePrefix') }}
              </label>
              <DInput 
                v-model="instanceNameInput" 
                :disabled="isUpdate"
                :placeholder="t('install.instanceNamePlaceholder', '输入安装后的游戏实例名称...')" 
              />
            </div>
          </div>

          <div class="flex-1 overflow-hidden border rounded-xl bg-background/50 flex flex-col">
            <!-- Table Header -->
            <div class="grid grid-cols-12 gap-4 p-3 bg-secondary/50 text-xs font-semibold text-muted-foreground uppercase tracking-wider border-b shrink-0 relative">
              <div class="col-span-4 pl-2">{{ t('modpacks.packVersion') }}</div>
              <div class="col-span-2">{{ t('modpacks.gameVersion') }}</div>
              <div class="col-span-2">{{ t('modpacks.loader') }}</div>
              <div class="col-span-2">{{ t('modpacks.publishDate') }}</div>
              <div class="col-span-2 text-right pr-2">{{ t('modpacks.actions') }}</div>
            </div>
            
            <!-- Table Body -->
            <div class="overflow-y-auto flex-1 p-2 space-y-1 relative min-h-[200px]">
              <div v-if="isFetchingVersions" class="absolute inset-0 flex flex-col items-center justify-center bg-background/80 backdrop-blur-sm z-10">
                <Loader2 class="h-8 w-8 animate-spin text-emerald-500 mb-4" />
                <p class="text-sm text-muted-foreground font-medium animate-pulse">{{ t('install.loading', '加载中...') }}</p>
              </div>
              
              <div v-else-if="modpackVersions.length === 0" class="flex flex-col items-center justify-center h-full text-muted-foreground opacity-60">
                <Package class="h-12 w-12 mb-3" />
                <p>{{ t('modpacks.noVersions', '该整合包暂无可用版本') }}</p>
              </div>

              <div 
                v-for="(version, index) in modpackVersions" 
                :key="version.id"
                class="grid grid-cols-12 gap-4 p-3 items-center hover:bg-secondary/60 rounded-lg transition-colors group border border-transparent hover:border-white/5"
              >
                <div class="col-span-4 font-medium pl-2 flex items-center gap-2 line-clamp-1" :title="version.name">
                  <span class="truncate">{{ version.name }}</span>
                  <span v-if="getVersionUpgradeStatus(index).text === t('modpack.reinstall')" class="shrink-0 inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300">
                    {{ t('modpack.currentVersion') }}
                  </span>
                </div>
                <div class="col-span-2">
                  <div class="inline-flex items-center rounded-full border px-2.5 py-0.5 font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 text-foreground font-mono bg-background text-xs">
                    {{ version.mc_version.split(',')[0] }}
                  </div>
                </div>
                <div class="col-span-2 flex gap-1 flex-wrap">
                  <div class="inline-flex items-center rounded-full border px-2.5 py-0.5 font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80 text-[10px] capitalize">
                    {{ version.loaders[0] || 'Unknown' }}
                  </div>
                </div>
                <div class="col-span-2 flex items-center gap-2 text-xs text-muted-foreground">
                  <Calendar class="h-3.5 w-3.5 opacity-50" />
                  {{ formatDate(version.date) }}
                </div>
                <div class="col-span-2 flex justify-end pr-2">
                  <button 
                    class="inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 text-white h-9 px-3 w-full shadow-sm hover:shadow-md transition-all"
                    :class="getVersionUpgradeStatus(index).class"
                    :disabled="getVersionUpgradeStatus(index).disabled"
                    @click="selectOnlineVersion(version)"
                  >
                    {{ getVersionUpgradeStatus(index).text }}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </DialogContent>
    <!-- Success Modal -->
    <AlertDialog :open="showSuccessModal" @update:open="val => { if (!val) handleSuccessConfirm() }">
      <div class="p-2">
        <AlertDialogTitle class="text-xl font-bold text-gray-900 dark:text-white mb-2">
          {{ t('install.successTitle', 'Installation Complete') }}
        </AlertDialogTitle>
        <AlertDialogDescription class="text-gray-600 dark:text-gray-300 mb-6">
          {{ t('install.successDesc', 'The modpack has been successfully installed. You can now find it in your Instances page.') }}
        </AlertDialogDescription>
        <div class="flex justify-end gap-3 mt-4">
          <button
            class="px-5 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg font-medium transition-colors"
            @click="handleSuccessConfirm"
          >
            {{ t('common.confirm', 'OK') }}
          </button>
        </div>
      </div>
    </AlertDialog>

    <!-- Cancel Confirmation Modal -->
    <AlertDialog :open="showCancelConfirmModal" @update:open="val => showCancelConfirmModal = val">
      <div class="p-2">
        <AlertDialogTitle class="text-xl font-bold text-gray-900 dark:text-white mb-2">
          {{ t('install.cancelConfirmTitle', 'Cancel Installation') }}
        </AlertDialogTitle>
        <AlertDialogDescription class="text-gray-600 dark:text-gray-300 mb-6">
          {{ t('install.cancelConfirmDesc', 'Are you sure you want to cancel the installation? This will clean up all downloaded and extracted files for this instance.') }}
        </AlertDialogDescription>
        <div class="flex justify-end gap-3 mt-4">
          <button
            class="px-5 py-2 text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg font-medium transition-colors"
            @click="showCancelConfirmModal = false"
          >
            {{ t('common.cancel', 'Cancel') }}
          </button>
          <button
            class="px-5 py-2 bg-red-600 hover:bg-red-700 text-white rounded-lg font-medium transition-colors"
            @click="cancelInstallation"
          >
            {{ t('common.confirm', 'Yes, Cancel') }}
          </button>
        </div>
      </div>
    </AlertDialog>
  </div>
</template>
