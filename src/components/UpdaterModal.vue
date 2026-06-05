<script setup lang="ts">
import { ref, computed } from "vue";
import { Download, Rocket, X, Loader2, CheckCircle2 } from "@lucide/vue";
import { Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface UpdateProgressPayload {
  event: string;
  data?: {
    contentLength?: number;
    chunkLength?: number;
  };
}

const props = defineProps<{
  open: boolean;
  updateInfo: Update | null;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
}>();

const isDownloading = ref(false);
const downloadProgress = ref(0);
const downloadedBytes = ref(0);
const totalBytes = ref(0);
const isFinished = ref(false);
const error = ref<string | null>(null);

function close() {
  if (isDownloading.value) return; // Prevent close during download
  emit("update:open", false);
}

// Convert markdown-ish body or just plain text to lines
const releaseNotes = computed(() => {
  if (!props.updateInfo?.body) return [];
  return props.updateInfo.body.split('\n').filter(line => line.trim().length > 0);
});

function formatBytes(bytes: number) {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

async function startUpdate() {
  if (!props.updateInfo || isDownloading.value) return;
  
  isDownloading.value = true;
  error.value = null;
  downloadProgress.value = 0;
  
  try {
    let downloaded = 0;
    let contentLength = 0;
    
    const isPortable = await invoke<boolean>("is_portable_version");
    
    if (isPortable) {
      // Custom portable updater bypass
      const unlisten = await listen<UpdateProgressPayload>("portable-update-progress", (event) => {
        const payload = event.payload;
        switch (payload.event) {
          case 'Started':
            contentLength = payload.data?.contentLength || 0;
            totalBytes.value = contentLength;
            break;
          case 'Progress':
            downloaded += payload.data?.chunkLength || 0;
            downloadedBytes.value = downloaded;
            if (contentLength > 0) {
              downloadProgress.value = Math.floor((downloaded / contentLength) * 100);
            }
            break;
          case 'Finished':
            isFinished.value = true;
            break;
        }
      });
      
      await invoke("update_portable_version", { version: props.updateInfo.version });
      unlisten();
    } else {
      // Standard Tauri Updater for MSIs and NSIS installers
      await props.updateInfo.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            contentLength = event.data.contentLength || 0;
            totalBytes.value = contentLength;
            break;
          case 'Progress':
            downloaded += event.data.chunkLength;
            downloadedBytes.value = downloaded;
            if (contentLength > 0) {
              downloadProgress.value = Math.floor((downloaded / contentLength) * 100);
            }
            break;
          case 'Finished':
            isFinished.value = true;
            break;
        }
      });
    }
    
    // Update successful, restart the app
    await relaunch();
    
  } catch (err) {
    error.value = typeof err === 'string' ? err : String(err);
    isDownloading.value = false;
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
      <div v-if="open && updateInfo" class="fixed inset-0 z-[100] flex items-center justify-center pointer-events-none">
        <!-- Backdrop -->
        <div class="absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm pointer-events-auto transition-opacity" @click="close" />
        
        <!-- Modal Card -->
        <div class="relative z-10 w-full max-w-md bg-white/90 dark:bg-zinc-900/90 backdrop-blur-xl border border-white/20 dark:border-zinc-800/50 rounded-3xl shadow-2xl flex flex-col pointer-events-auto overflow-hidden">
          
          <!-- Subtle Glow Background -->
          <div class="absolute top-0 left-1/2 -translate-x-1/2 w-full h-32 bg-primary/20 dark:bg-primary/10 blur-[50px] rounded-full pointer-events-none"></div>

          <!-- Close button -->
          <button @click="close" :disabled="isDownloading" class="absolute top-4 right-4 z-20 p-1.5 rounded-full text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-zinc-800 transition-colors disabled:opacity-0">
            <X class="w-5 h-5" />
          </button>
          
          <div class="px-8 pt-10 pb-8 text-center relative z-10">
            <!-- Icon -->
            <div class="mx-auto w-16 h-16 bg-gradient-to-br from-primary/10 to-primary/5 dark:from-primary/20 dark:to-primary/5 border border-primary/20 rounded-2xl flex items-center justify-center shadow-inner mb-5 relative">
              <div class="absolute inset-0 bg-primary/10 blur-md rounded-2xl"></div>
              <Rocket class="w-8 h-8 text-primary relative z-10 drop-shadow-sm" />
            </div>
            
            <!-- Title & Version -->
            <h2 class="text-2xl font-bold text-neutral-900 dark:text-white mb-2 tracking-tight">{{ $t('updater.title') }}</h2>
            <div class="inline-flex items-center justify-center px-3 py-1 rounded-full bg-primary/10 border border-primary/20 text-primary text-sm font-semibold tracking-wide mb-6 shadow-sm">
              v{{ updateInfo.version }}
            </div>
            
            <!-- Release Notes -->
            <div class="text-left bg-white/50 dark:bg-black/20 rounded-2xl p-5 mb-6 border border-black/5 dark:border-white/5 max-h-52 overflow-y-auto custom-scrollbar shadow-inner backdrop-blur-sm">
              <h3 class="text-sm font-bold text-neutral-800 dark:text-neutral-200 mb-3 flex items-center gap-2">
                <span class="w-1 h-4 bg-primary rounded-full"></span>
                {{ $t('updater.releaseNotes') }}
              </h3>
              <ul class="space-y-2">
                <li v-for="(line, i) in releaseNotes" :key="i" class="text-sm text-neutral-600 dark:text-neutral-400 flex items-start gap-2.5 leading-relaxed">
                  <span class="text-primary/60 mt-1.5 shrink-0 text-[10px]">●</span>
                  <span>{{ line }}</span>
                </li>
                <li v-if="releaseNotes.length === 0" class="text-sm text-neutral-500 italic">{{ $t('updater.noNotes') }}</li>
              </ul>
            </div>
            
            <!-- Progress Section -->
            <div v-if="isDownloading" class="mb-6 bg-white/50 dark:bg-black/20 p-5 rounded-2xl border border-black/5 dark:border-white/5 backdrop-blur-sm shadow-inner">
              <div class="flex justify-between text-sm mb-3 font-semibold">
                <span class="text-neutral-800 dark:text-neutral-200">
                  {{ isFinished ? $t('updater.preparingRestart') : $t('updater.downloading') }}
                </span>
                <span class="text-primary">{{ downloadProgress }}%</span>
              </div>
              <div class="w-full bg-neutral-200 dark:bg-zinc-800 rounded-full h-2.5 overflow-hidden mb-2.5 shadow-inner">
                <div 
                  class="bg-primary h-full rounded-full transition-all duration-300 ease-out relative overflow-hidden"
                  :style="{ width: `${downloadProgress}%` }"
                >
                  <div class="absolute inset-0 bg-white/20 w-full h-full animate-[shimmer_2s_infinite]"></div>
                </div>
              </div>
              <div class="flex justify-between text-xs font-medium text-neutral-500 dark:text-neutral-400">
                <span>{{ formatBytes(downloadedBytes) }} / {{ totalBytes > 0 ? formatBytes(totalBytes) : $t('updater.unknownSize') }}</span>
              </div>
            </div>
            
            <!-- Error Alert -->
            <div v-if="error" class="mb-6 p-4 bg-red-50/80 dark:bg-red-950/30 border border-red-200 dark:border-red-900/50 rounded-2xl text-left backdrop-blur-sm">
              <p class="text-sm font-bold text-red-600 dark:text-red-400 mb-1 flex items-center gap-2">
                <X class="w-4 h-4" /> {{ $t('updater.updateFailed') }}
              </p>
              <p class="text-xs text-red-500 dark:text-red-400/80 break-words leading-relaxed">{{ error }}</p>
            </div>
            
            <!-- Action Buttons -->
            <div class="flex gap-3 mt-2">
              <button 
                @click="close" 
                :disabled="isDownloading"
                class="flex-1 py-3 px-4 text-sm font-semibold bg-neutral-100 hover:bg-neutral-200 text-neutral-700 dark:bg-zinc-800/80 dark:hover:bg-zinc-700 dark:text-neutral-300 rounded-xl transition-all disabled:opacity-50 disabled:cursor-not-allowed shadow-sm"
              >
                {{ $t('updater.remindLater') }}
              </button>
              <button 
                @click="startUpdate" 
                :disabled="isDownloading"
                class="flex-[2] flex items-center justify-center gap-2 py-3 px-4 text-sm font-bold bg-primary text-primary-foreground rounded-xl hover:bg-primary/90 transition-all shadow-md hover:shadow-lg disabled:opacity-70 disabled:cursor-not-allowed"
              >
                <Loader2 v-if="isDownloading && !isFinished" class="w-4 h-4 animate-spin" />
                <CheckCircle2 v-else-if="isFinished" class="w-4 h-4" />
                <Download v-else class="w-4 h-4" />
                {{ isDownloading ? (isFinished ? $t('updater.restarting') : $t('updater.updating')) : $t('updater.updateNow') }}
              </button>
            </div>
            
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.dialog-enter-active,
.dialog-leave-active {
  transition: opacity 250ms ease;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}

.dialog-enter-active .relative,
.dialog-leave-active .relative {
  transition: transform 300ms cubic-bezier(0.175, 0.885, 0.32, 1.275), opacity 250ms ease;
}

.dialog-enter-from .relative {
  transform: scale(0.9) translateY(15px);
  opacity: 0;
}

.dialog-leave-to .relative {
  transform: scale(0.95) translateY(5px);
  opacity: 0;
}

@keyframes shimmer {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(100%); }
}
</style>
