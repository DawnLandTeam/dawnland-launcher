<script setup lang="ts">
import { ref, shallowRef, onMounted, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import MainLayout from "./layouts/MainLayout.vue";
import UpdaterModal from "./components/UpdaterModal.vue";
import { setUpdateAvailable, type CustomUpdate } from "./composables/useUpdate";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "vue-i18n";
import { useTaskStore } from "./composables/useTaskStore";
import TaskCenter from "./components/TaskCenter.vue";
import NotificationCenter from "./components/NotificationCenter.vue";
import Toaster from "./components/Toaster.vue";
import DeepLinkReceiveModal, { type DeepLinkData } from "./components/DeepLinkReceiveModal.vue";
import { toast } from "./composables/useToast";
import { parseDeepLinkUrl } from "./utils/deepLink";

const isUpdateModalOpen = ref(false);
const updateInfo = shallowRef<CustomUpdate | null>(null);
const showDeepLinkModal = ref(false);
const incomingLinkData = ref<DeepLinkData | null>(null);
const { locale, t } = useI18n();
const taskStore = useTaskStore();
const router = useRouter();

let unlistenDeepLink: (() => void) | null = null;

onMounted(async () => {
  // Initialize task center
  await taskStore.init();

  // Deep Link listener
  unlistenDeepLink = await onOpenUrl((urls) => {
    for (const urlStr of urls) {
      const parsedData = parseDeepLinkUrl(urlStr);
      if (parsedData) {
        incomingLinkData.value = parsedData;
        showDeepLinkModal.value = true;
      } else {
        console.warn("Received invalid or unrecognized deep link:", urlStr);
      }
    }
  });

  // Show window
  getCurrentWindow().show().catch(err => console.error("Failed to show window:", err));

  // Async precise locale detection from Rust
  if (localStorage.getItem('userSelectedLanguage') !== 'true') {
    try {
      const sysLocale = await invoke<string | null>("get_system_locale");
      if (sysLocale) {
        const detected = sysLocale.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
        if (locale.value !== detected) {
          locale.value = detected;
          localStorage.setItem('language', detected);
        }
      }
    } catch (e) {
      console.warn("Failed to get system locale from Rust:", e);
    }
  }

  // Delay the update check slightly to ensure network and plugins are fully initialized
  setTimeout(async () => {
    try {
      const currentVersion = await getVersion();
      const targetOS = navigator.userAgent.includes("Windows") ? "windows-standalone" : "linux-standalone";
      const baseUrl = import.meta.env.VITE_WEB_BACKEND_URL || 'http://localhost:3030';
      const res = await fetch(`${baseUrl}/api/launcher/update/${targetOS}/${currentVersion}`);
      if (res.status === 200) {
        const data = await res.json();
        if (data.version && data.version !== currentVersion) {
          console.log(`Update available: ${data.version}`);
          const update = { version: data.version, body: data.notes || '' };
          updateInfo.value = update;
          isUpdateModalOpen.value = true;
          setUpdateAvailable(update);
        }
      }
    } catch (error) {
      console.error("Failed to check for updates on startup:", error);
    }
  }, 2000);

  document.addEventListener('dragenter', handleDrag, true);
  document.addEventListener('dragover', handleDrag, true);
  document.addEventListener('drop', handleDrop, true);
});

// Handle Deep Link Confirmation
const handleDeepLinkConfirm = async (data: DeepLinkData) => {
  if (data.type === 'modpack') {
    const { projectId, source, versionId, name } = data.payload;
    toast.info(t('deepLink.fetching', '正在获取整合包详情...'));
    try {
      const fetchVersions = source === 'modrinth' ? 'get_modrinth_modpack_versions' : 'get_curseforge_modpack_versions';
      const versions: any = await invoke(fetchVersions, { projectId });
      let targetVersion = versions.find((v: any) => v.id.toString() === versionId);
      if (!targetVersion) targetVersion = versions.find((v: any) => v.name === versionId);
      if (!targetVersion) targetVersion = versions.find((v: any) => v.name.includes(versionId));
      
      if (targetVersion) {
        const rawName = name || targetVersion.name || targetVersion.displayName || 'Shared Modpack';
        const finalName = rawName.replace(/[<>:"/\\|?*\x00-\x1F]/g, '_');
        
        await invoke("download_and_install_online_modpack", {
          url: targetVersion.download_url,
          instanceName: finalName,
          source: source,
          projectId: projectId,
          versionId: versionId,
          isUpdate: false
        });
        toast.success(t('deepLink.installStarted', '已开始安装整合包'), t('deepLink.installStartedDesc', '请在任务中心查看进度'));
      } else {
        toast.error(t('deepLink.installFailed', '安装失败'), t('deepLink.versionNotFound', '未找到对应的整合包版本'));
      }
    } catch (e) {
      toast.error(t('deepLink.installFailed', '安装失败'), String(e));
    }
  } else if (data.type === 'authlib') {
    invoke("add_authlib_server", { url: data.payload.url })
      .then(() => {
        window.dispatchEvent(new CustomEvent('authlib-servers-updated'));
        alert(t('settings.authlib.addSuccess', { url: data.payload.url }));
      })
      .catch(err => {
        alert(t('settings.authlib.addFailed', { error: String(err) }));
      });
  } else if (data.type === 'server') {
    router.push({
      path: '/servers',
      query: { view_id: data.payload.id }
    });
  }
};

const handleDrag = (e: DragEvent) => {
  e.preventDefault();
  e.stopPropagation();
  if (e.dataTransfer) {
    e.dataTransfer.dropEffect = 'copy';
  }
};

const handleDrop = async (e: DragEvent) => {
  e.preventDefault();
  e.stopPropagation();
  
  let text = e.dataTransfer?.getData('text/plain');
  if (!text) text = e.dataTransfer?.getData('text/html');
  if (!text) text = e.dataTransfer?.getData('text/uri-list');

  if (text) {
    // Regex search to find the authlib string even if it's wrapped in HTML or other text
    const match = text.match(/authlib-injector:yggdrasil-server:([^\s"']+)/);
    if (match) {
      const url = decodeURIComponent(match[1]);
      try {
        await invoke("add_authlib_server", { url: url.trim() });
        window.dispatchEvent(new CustomEvent('authlib-servers-updated'));
        alert(t('settings.authlib.addSuccess', { url }));
      } catch (err) {
        alert(t('settings.authlib.addFailed', { error: String(err) }));
      }
    }
  }
};

onUnmounted(() => {
  if (unlistenDeepLink) {
    unlistenDeepLink();
  }
  document.removeEventListener('dragenter', handleDrag, true);
  document.removeEventListener('dragover', handleDrag, true);
  document.removeEventListener('drop', handleDrop, true);
});
</script>

<template>
  <MainLayout />
  <UpdaterModal v-model:open="isUpdateModalOpen" :update-info="updateInfo" />
  <TaskCenter />
  <NotificationCenter />
  <DeepLinkReceiveModal v-model:open="showDeepLinkModal" :data="incomingLinkData" @confirm="handleDeepLinkConfirm" />
  <Toaster />
</template>