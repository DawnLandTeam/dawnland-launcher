<script setup lang="ts">
import { ref, computed } from 'vue';
import { X, Globe, Users, Copy, Check, MessageSquare, Share2 } from '@lucide/vue';
import { marked } from 'marked';

const props = defineProps<{
  open: boolean;
  server: any; // ServerInfo
}>();

const emit = defineEmits(['update:open']);
import { useI18n } from 'vue-i18n';
import { toast } from '../composables/useToast';
const { t } = useI18n();

const copiedIp = ref(false);
const copiedShareLink = ref(false);

const close = () => {
  emit('update:open', false);
};

const copyIp = async () => {
  if (!props.server?.ip) return;
  try {
    const textToCopy = props.server.port ? `${props.server.ip}:${props.server.port}` : props.server.ip;
    await navigator.clipboard.writeText(textToCopy);
    copiedIp.value = true;
    setTimeout(() => {
      copiedIp.value = false;
    }, 2000);
  } catch (err) {
    console.error("Failed to write to clipboard:", err);
    toast.error('无法复制 IP 到剪贴板，请检查浏览器权限。');
  }
};

const shareServer = async () => {
  if (!props.server?.id) return;
  const rawLink = `dlml://server/view?id=${encodeURIComponent(props.server.id)}`;
  const backendUrl = import.meta.env.VITE_WEB_BACKEND_URL || 'https://api.dawnland.cn';
  const b64 = btoa(unescape(encodeURIComponent(rawLink)));
  const link = `${backendUrl}/link?b64=${b64}`;
  
  try {
    await navigator.clipboard.writeText(link);
    copiedShareLink.value = true;
    setTimeout(() => {
      copiedShareLink.value = false;
    }, 2000);
  } catch (err) {
    console.error("Failed to write to clipboard:", err);
    toast.error('无法复制链接到剪贴板，请检查浏览器权限。');
  }
};

const tags = computed(() => {
  if (!props.server?.tags) return [];
  return props.server.tags.split(',').map((t: string) => t.trim()).filter((t: string) => t.length > 0);
});

const renderedDescription = computed(() => {
  if (!props.server?.description) return '<p class="text-muted-foreground text-sm italic">' + t('servers.details.noDescription') + '</p>';
  return marked(props.server.description, { breaks: true }) as string;
});
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
      <!-- Backdrop -->
      <div class="absolute inset-0 bg-black/50 backdrop-blur-sm pointer-events-auto" @click="close"></div>
      
      <!-- Modal Content -->
      <div class="relative z-10 w-full max-w-3xl max-h-[85vh] flex flex-col bg-white dark:bg-zinc-900 border border-neutral-200 dark:border-zinc-800 shadow-2xl rounded-xl pointer-events-auto overflow-hidden">
        
        <!-- Header -->
        <div class="flex items-center justify-between p-6 border-b border-neutral-200 dark:border-zinc-800 bg-neutral-50 dark:bg-zinc-900/50">
          <div class="flex items-center gap-4">
            <img v-if="server?.iconUrl" :src="server.iconUrl" alt="Icon" class="w-16 h-16 rounded-xl shadow-sm object-cover bg-white dark:bg-black" />
            <div v-else class="w-16 h-16 rounded-xl bg-neutral-200 dark:bg-zinc-800 flex items-center justify-center">
              <Globe class="w-8 h-8 text-neutral-400" />
            </div>
            
            <div>
              <h2 class="text-2xl font-bold text-neutral-900 dark:text-white">{{ server?.name || 'Unknown Server' }}</h2>
              <div class="flex items-center gap-2 mt-1">
                <span class="inline-flex items-center rounded-full bg-blue-100 px-2 py-0.5 text-xs font-semibold text-blue-800 dark:bg-blue-900/30 dark:text-blue-300">
                  MC {{ server?.version }}
                </span>
                <span class="inline-flex items-center rounded-full bg-neutral-100 px-2 py-0.5 text-xs font-semibold text-neutral-800 dark:bg-zinc-800 dark:text-neutral-300">
                  {{ server?.serverType === 'vanilla' ? t('servers.details.vanilla') : server?.serverType === 'modded' ? t('servers.details.modded') : t('servers.details.custom', '自定义 (Custom)') }}
                </span>
                <span v-if="server?.loaderType" class="inline-flex items-center rounded-full bg-emerald-100 px-2 py-0.5 text-xs font-semibold text-emerald-800 dark:bg-emerald-900/30 dark:text-emerald-300">
                  {{ server.loaderType }}
                </span>
              </div>
            </div>
          </div>
          <div class="flex items-center gap-2">
            <button @click="shareServer" class="p-2 text-neutral-500 hover:text-primary dark:text-neutral-400 dark:hover:text-primary transition-colors rounded-lg hover:bg-neutral-200 dark:hover:bg-zinc-800" title="分享此服务器 (Share Server)">
              <Check v-if="copiedShareLink" class="w-6 h-6 text-green-500" />
              <Share2 v-else class="w-6 h-6" />
            </button>
            <button @click="close" class="p-2 text-neutral-500 hover:text-neutral-900 dark:text-neutral-400 dark:hover:text-white transition-colors rounded-lg hover:bg-neutral-200 dark:hover:bg-zinc-800">
              <X class="w-6 h-6" />
            </button>
          </div>
        </div>

        <!-- Body -->
        <div class="flex-1 overflow-y-auto p-6 flex flex-col gap-6 custom-scrollbar">
          
          <!-- Tags -->
          <div v-if="tags.length > 0" class="flex flex-wrap gap-2">
            <span v-for="(tag, idx) in tags" :key="idx" class="px-3 py-1 bg-primary/10 text-primary rounded-full text-sm font-medium border border-primary/20">
              {{ tag }}
            </span>
          </div>

          <!-- Basic Info Box -->
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <!-- IP & Connection -->
            <div class="p-4 bg-neutral-50 dark:bg-zinc-800/50 rounded-xl border border-neutral-100 dark:border-zinc-800 flex flex-col justify-center">
              <p class="text-sm font-medium text-neutral-500 dark:text-neutral-400 mb-1">{{ t('servers.details.serverAddress') }}</p>
              <div class="flex items-center gap-2">
                <code class="text-lg font-mono font-bold text-neutral-900 dark:text-white select-all">{{ server?.ip }}:{{ server?.port }}</code>
                <button @click="copyIp" class="p-1.5 text-neutral-500 hover:text-primary dark:text-neutral-400 dark:hover:text-primary transition-colors rounded-md hover:bg-neutral-200 dark:hover:bg-zinc-700">
                  <Check v-if="copiedIp" class="w-4 h-4 text-green-500" />
                  <Copy v-else class="w-4 h-4" />
                </button>
              </div>
            </div>

            <!-- MOTD & Auth -->
            <div class="p-4 bg-neutral-50 dark:bg-zinc-800/50 rounded-xl border border-neutral-100 dark:border-zinc-800 flex flex-col justify-center">
              <p class="text-sm font-medium text-neutral-500 dark:text-neutral-400 mb-1">{{ t('servers.details.auth') }}</p>
              <div class="text-neutral-900 dark:text-white font-medium truncate" :title="server?.motd">{{ server?.motd || '-' }}</div>
              <div class="text-xs text-neutral-500 dark:text-neutral-400 mt-1">
                {{ server?.authType === 'microsoft' ? t('servers.details.authMicrosoft') : server?.authType === 'offline' ? t('servers.details.authOffline', '验证方式: 离线 (Offline)') : t('servers.details.authAuthlib') }}
              </div>
            </div>
          </div>

          <!-- Contact Info -->
          <div v-if="server?.contactGroup || server?.contactOwner" class="p-4 bg-blue-50 dark:bg-blue-900/10 border border-blue-100 dark:border-blue-900/30 rounded-xl">
            <h3 class="text-sm font-bold text-blue-900 dark:text-blue-300 mb-3 uppercase tracking-wider">{{ t('servers.details.contact') }}</h3>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div v-if="server?.contactGroup" class="flex items-start gap-3">
                <MessageSquare class="w-5 h-5 text-blue-500 mt-0.5 shrink-0" />
                <div class="min-w-0">
                  <p class="text-sm font-medium text-neutral-900 dark:text-white">{{ t('servers.details.communityGroup') }}</p>
                  <p class="text-sm text-neutral-600 dark:text-neutral-400 select-all truncate">{{ server.contactGroup }}</p>
                </div>
              </div>
              <div v-if="server?.contactOwner" class="flex items-start gap-3">
                <Users class="w-5 h-5 text-blue-500 mt-0.5 shrink-0" />
                <div class="min-w-0">
                  <p class="text-sm font-medium text-neutral-900 dark:text-white">{{ t('servers.details.ownerContact') }}</p>
                  <p class="text-sm text-neutral-600 dark:text-neutral-400 select-all truncate">{{ server.contactOwner }}</p>
                </div>
              </div>
            </div>
          </div>

          <!-- Description (Markdown Rendered) -->
          <div class="flex flex-col gap-2">
            <h3 class="text-lg font-bold text-neutral-900 dark:text-white border-b border-neutral-200 dark:border-zinc-800 pb-2">{{ t('servers.details.description') }}</h3>
            <div class="prose dark:prose-invert max-w-none text-sm leading-relaxed" v-safe-html="renderedDescription"></div>
          </div>

        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
/* Simple styling for the rendered markdown content */
:deep(.prose) {
  --tw-prose-body: #3f3f46;
  --tw-prose-headings: #18181b;
  --tw-prose-links: #3b82f6;
  --tw-prose-bold: #18181b;
}

@media (prefers-color-scheme: dark) {
  :deep(.prose) {
    --tw-prose-body: #d4d4d8;
    --tw-prose-headings: #ffffff;
    --tw-prose-links: #60a5fa;
    --tw-prose-bold: #ffffff;
  }
}

:deep(.prose p) {
  margin-top: 0.5em;
  margin-bottom: 0.5em;
}

:deep(.prose h1), :deep(.prose h2), :deep(.prose h3) {
  margin-top: 1em;
  margin-bottom: 0.5em;
  font-weight: 700;
}

:deep(.prose ul), :deep(.prose ol) {
  margin-top: 0.5em;
  margin-bottom: 0.5em;
  padding-left: 1.5em;
}

:deep(.prose ul) {
  list-style-type: disc;
}

:deep(.prose ol) {
  list-style-type: decimal;
}

:deep(.prose img) {
  max-width: 100%;
  border-radius: 0.5rem;
  margin-top: 1rem;
  margin-bottom: 1rem;
}
</style>
