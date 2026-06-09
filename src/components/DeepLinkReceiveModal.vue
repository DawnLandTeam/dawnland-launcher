<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { DialogContent, DialogTitle, DialogDescription } from './ui/dialog';
import { Package, Globe, KeyRound } from '@lucide/vue';

export type DeepLinkType = 'modpack' | 'server' | 'authlib';

export interface DeepLinkData {
  type: DeepLinkType;
  payload: Record<string, string>;
}

const props = defineProps<{
  open: boolean;
  data: DeepLinkData | null;
}>();

const emit = defineEmits<{
  'update:open': [value: boolean];
  'confirm': [data: DeepLinkData];
}>();

const { t } = useI18n();

const iconComponent = computed(() => {
  if (!props.data) return Package;
  switch (props.data.type) {
    case 'modpack': return Package;
    case 'server': return Globe;
    case 'authlib': return KeyRound;
    default: return Package;
  }
});

const title = computed(() => {
  if (!props.data) return '';
  switch (props.data.type) {
    case 'modpack': return t('deepLink.modpackTitle', '收到整合包分享');
    case 'server': return t('deepLink.serverTitle', '收到服务器分享');
    case 'authlib': return t('deepLink.authlibTitle', '收到第三方登录分享');
    default: return '';
  }
});

const description = computed(() => {
  if (!props.data) return '';
  switch (props.data.type) {
    case 'modpack': 
      const mpName = props.data.payload.name || t('deepLink.modpack', '整合包');
      return t('deepLink.modpackDesc', { name: mpName }, `是否要安装分享的整合包：${mpName}？`);
    case 'server': 
      return t('deepLink.serverDesc', '是否要查看该分享的服务器详情？');
    case 'authlib': 
      return t('deepLink.authlibDesc', { url: props.data.payload.url }, `是否要添加此第三方外置登录服务？\n${props.data.payload.url}`);
    default: return '';
  }
});

const confirmText = computed(() => {
  if (!props.data) return '';
  switch (props.data.type) {
    case 'modpack': return t('deepLink.modpackConfirm', '确认安装');
    case 'server': return t('deepLink.serverConfirm', '查看服务器');
    case 'authlib': return t('deepLink.authlibConfirm', '确认添加');
    default: return t('common.confirm', '确认');
  }
});

function handleOpenChange(val: boolean) {
  emit('update:open', val);
}

function handleConfirm() {
  if (props.data) {
    emit('confirm', props.data);
  }
  emit('update:open', false);
}
</script>

<template>
  <DialogContent :open="open" @update:open="handleOpenChange" class="sm:max-w-[425px]">
    <div class="flex flex-col space-y-1.5 text-center sm:text-left">
      <div class="flex items-center gap-4 mb-2">
        <div class="p-3 rounded-full bg-primary/10 text-primary">
          <component :is="iconComponent" class="w-6 h-6" />
        </div>
        <DialogTitle class="text-xl">{{ title }}</DialogTitle>
      </div>
      <DialogDescription class="text-base mt-4 whitespace-pre-wrap leading-relaxed text-neutral-600 dark:text-neutral-300">
        {{ description }}
      </DialogDescription>
    </div>
    
    <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2 mt-6">
      <button class="inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 border border-input bg-background hover:bg-neutral-100 dark:hover:bg-zinc-800 h-10 px-4 py-2 mt-2 sm:mt-0" @click="handleOpenChange(false)">
        {{ $t('common.cancel', '取消') }}
      </button>
      <button class="inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 bg-primary text-primary-foreground hover:bg-primary/90 h-10 px-4 py-2" @click="handleConfirm">
        {{ confirmText }}
      </button>
    </div>
  </DialogContent>
</template>
