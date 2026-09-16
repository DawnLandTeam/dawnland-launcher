<script setup lang="ts">
import { ref, watch, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { DialogContent, DialogTitle } from './ui/dialog';
import { Loader2, ExternalLink, RefreshCw, Upload } from '@lucide/vue';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { SkinViewer, WalkingAnimation } from 'skinview3d';
import type { Account, AccountTextures } from '../types';
import DButton from './ui/DButton.vue';
import { useI18n } from 'vue-i18n';
import { getErrorMessage } from '../utils/error';
import { STEVE_SKIN_BASE64, getProxiedImageBase64 } from '../utils/steve';

const props = defineProps<{
  account: Account | null;
  show: boolean;
}>();

const emit = defineEmits(['update:show', 'textures-updated']);
const { t } = useI18n();

const skinContainer = ref<HTMLElement | null>(null);
const isLoading = ref(false);
const isUploading = ref(false);
const isReauthRequired = ref(false);
let viewer: SkinViewer | null = null;
const errorMsg = ref('');

function close() {
  emit('update:show', false);
}

async function initViewer() {
  if (!skinContainer.value) return;
  
  if (viewer) {
    viewer.dispose();
  }
  
  viewer = new SkinViewer({
    canvas: document.createElement('canvas'),
    width: 200,
    height: 300,
  });
  
  let skinUrl = props.account?.textures?.skinUrl || STEVE_SKIN_BASE64;
  if (skinUrl !== STEVE_SKIN_BASE64) {
    try {
      skinUrl = await getProxiedImageBase64(invoke, skinUrl);
    } catch(e) {
      console.warn("Proxy failed, using direct url");
    }
  }

  viewer.loadSkin(skinUrl).catch(e => {
    console.warn("Failed to load skin:", e);
    if (skinUrl !== STEVE_SKIN_BASE64) {
      viewer?.loadSkin(STEVE_SKIN_BASE64).catch(() => {});
    }
  });
  
  if (props.account?.textures?.capeUrl) {
    let capeUrl = props.account.textures.capeUrl;
    try {
      capeUrl = await getProxiedImageBase64(invoke, capeUrl);
    } catch(e) {}
    viewer.loadCape(capeUrl).catch(e => console.warn("Failed to load cape:", e));
  }
  
  viewer.animation = new WalkingAnimation();
  viewer.autoRotate = true;
  viewer.autoRotateSpeed = 0.5;
  
  skinContainer.value.innerHTML = '';
  skinContainer.value.appendChild(viewer.canvas);
}

async function fetchTextures() {
  if (!props.account) return;
  isLoading.value = true;
  errorMsg.value = '';
  isReauthRequired.value = false;
  try {
    const textures = await invoke<AccountTextures>('fetch_account_textures', { accountId: props.account.id });
    emit('textures-updated', { ...props.account, textures });
    if (viewer) {
      let skinUrl = textures.skinUrl || STEVE_SKIN_BASE64;
      if (textures.skinUrl) {
        try {
          skinUrl = await getProxiedImageBase64(invoke, skinUrl, true);
        } catch(e) {}
      }

      viewer.loadSkin(skinUrl).catch(e => {
        console.warn("Failed to load updated skin:", e);
        if (skinUrl !== STEVE_SKIN_BASE64) {
          viewer?.loadSkin(STEVE_SKIN_BASE64).catch(() => {});
        }
      });
      
      if (textures.capeUrl) {
        let capeUrl = textures.capeUrl;
        try {
          capeUrl = await getProxiedImageBase64(invoke, capeUrl, true);
        } catch(e) {}
        viewer.loadCape(capeUrl).catch(e => console.warn("Failed to load updated cape:", e));
      } else {
        viewer.resetCape();
      }
    }
  } catch (e: any) {
    if (typeof e === 'object' && e?.code === 'MICROSOFT_REAUTH_REQUIRED' || e === 'MICROSOFT_REAUTH_REQUIRED') {
      isReauthRequired.value = true;
    }
    errorMsg.value = getErrorMessage(e);
  } finally {
    isLoading.value = false;
  }
}

async function reLogin() {
  if (!props.account || props.account.accountType !== 'microsoft') return;
  isLoading.value = true;
  errorMsg.value = '';
  try {
    const updatedAccount = await invoke<Account>('login_microsoft_oauth');
    emit('textures-updated', updatedAccount);
    isReauthRequired.value = false;
    await fetchTextures();
  } catch (e: any) {
    errorMsg.value = getErrorMessage(e);
  } finally {
    isLoading.value = false;
  }
}

async function uploadSkin() {
  if (!props.account) return;
  if (props.account.accountType === 'microsoft') {
    const selected = await openDialog({
      filters: [{ name: 'Image', extensions: ['png'] }],
      multiple: false,
    });
    
    if (typeof selected === 'string') {
      isUploading.value = true;
      errorMsg.value = '';
      try {
        await invoke('upload_microsoft_skin', {
          accountId: props.account.id,
          skinPath: selected,
          variant: 'classic' // hardcode variant as classic for now
        });
        await fetchTextures();
      } catch (e: any) {
        if (typeof e === 'object' && e?.code === 'MICROSOFT_REAUTH_REQUIRED' || e === 'MICROSOFT_REAUTH_REQUIRED') {
          isReauthRequired.value = true;
        }
        errorMsg.value = getErrorMessage(e);
      } finally {
        isUploading.value = false;
      }
    }
  } else if (props.account.accountType === 'authlib') {
    // Open homepage
    if (props.account.authlibUrl) {
      try {
        const meta = await invoke<any>('get_authlib_meta', { url: props.account.authlibUrl });
        if (meta?.meta?.links?.homepage) {
          openUrl(meta.meta.links.homepage);
        } else {
          openUrl(props.account.authlibUrl);
        }
      } catch (e) {
        openUrl(props.account.authlibUrl);
      }
    }
  }
}

watch(() => props.show, (newVal) => {
  if (newVal && props.account) {
    nextTick(() => {
      initViewer();
    });
  } else {
    if (viewer) {
      viewer.dispose();
      viewer = null;
    }
  }
});
</script>

<template>
  <DialogContent :open="show" @update:open="!$event && close()" class="max-w-md p-6 bg-white dark:bg-zinc-900 border border-white/20 dark:border-zinc-800 shadow-xl rounded-2xl">
    <div class="flex flex-col items-center gap-4">
      <DialogTitle class="text-xl font-bold">{{ account?.username }}</DialogTitle>
      
      <div 
        ref="skinContainer" 
        class="w-48 h-72 bg-neutral-100/50 dark:bg-zinc-800/50 rounded-xl flex items-center justify-center overflow-hidden relative shadow-inner border border-neutral-200 dark:border-zinc-700"
      >
        <Loader2 v-if="!viewer" class="animate-spin text-neutral-400" />
      </div>

      <div class="flex gap-3 w-full mt-2">
        <DButton
          v-if="isReauthRequired"
          variant="primary"
          class="w-full flex items-center justify-center gap-2 bg-red-600 hover:bg-red-700 text-white"
          :disabled="isLoading"
          @click="reLogin"
        >
          <RefreshCw :size="16" :class="{ 'animate-spin': isLoading }" />
          {{ t('accounts.reLogin', '重新登录') }}
        </DButton>

        <template v-else>
          <DButton
            variant="secondary"
            class="flex-1 flex items-center justify-center gap-2"
            :disabled="isLoading || isUploading"
            @click="fetchTextures"
          >
            <RefreshCw :size="16" :class="{ 'animate-spin': isLoading }" />
            {{ t('accounts.refreshSkin', '刷新外观') }}
          </DButton>

          <DButton
            v-if="account?.accountType === 'microsoft' || account?.accountType === 'authlib'"
            variant="primary"
            class="flex-1 flex items-center justify-center gap-2"
            :disabled="isLoading || isUploading"
            @click="uploadSkin"
          >
            <Loader2 v-if="isUploading" :size="16" class="animate-spin" />
            <Upload v-else-if="account?.accountType === 'microsoft'" :size="16" />
            <ExternalLink v-else :size="16" />
            {{ account?.accountType === 'microsoft' ? t('accounts.changeSkin', '更换皮肤') : t('accounts.website', '前往官网') }}
          </DButton>
        </template>
      </div>

      <p v-if="errorMsg" class="text-sm text-red-500 text-center mt-2 break-all">{{ errorMsg }}</p>
    </div>
  </DialogContent>
</template>
