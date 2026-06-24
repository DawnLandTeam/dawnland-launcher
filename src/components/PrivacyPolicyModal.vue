<script setup lang="ts">
import { ref } from "vue";
import { ShieldCheck, Check, X } from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "../composables/useToast";
import { getErrorMessage } from "../utils/error";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  "resolved": [];
}>();

const isSaving = ref(false);

async function handleAction(agree: boolean) {
  if (isSaving.value) return;
  isSaving.value = true;
  
  try {
    const settings = await invoke<any>("load_launcher_settings");
    settings.enableTelemetry = agree;
    await invoke("save_launcher_settings", { settings });
  } catch (err) {
    console.error("Failed to save privacy settings:", err);
    toast.error(t('settings.saveFailed', "Failed to save settings"), getErrorMessage(err));
  } finally {
    // Close modal regardless of success or failure
    emit("update:open", false);
    emit("resolved");
    isSaving.value = false;
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
      <div v-if="open" class="fixed inset-0 z-[100] flex items-center justify-center pointer-events-none">
        <!-- Backdrop -->
        <div class="absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm pointer-events-auto transition-opacity" />
        
        <!-- Modal Card -->
        <div class="relative z-10 w-full max-w-md bg-white/90 dark:bg-zinc-900/90 backdrop-blur-xl border border-white/20 dark:border-zinc-800/50 rounded-3xl shadow-2xl flex flex-col pointer-events-auto overflow-hidden">
          
          <!-- Subtle Glow Background -->
          <div class="absolute top-0 left-1/2 -translate-x-1/2 w-full h-32 bg-primary/20 dark:bg-primary/10 blur-[50px] rounded-full pointer-events-none"></div>
          
          <div class="px-8 pt-10 pb-8 text-center relative z-10">
            <!-- Icon -->
            <div class="mx-auto w-16 h-16 bg-gradient-to-br from-primary/10 to-primary/5 dark:from-primary/20 dark:to-primary/5 border border-primary/20 rounded-2xl flex items-center justify-center shadow-inner mb-5 relative">
              <div class="absolute inset-0 bg-primary/10 blur-md rounded-2xl"></div>
              <ShieldCheck class="w-8 h-8 text-primary relative z-10 drop-shadow-sm" />
            </div>
            
            <!-- Title -->
            <h2 class="text-2xl font-bold text-neutral-900 dark:text-white mb-6 tracking-tight">{{ $t('privacy.title') }}</h2>
            
            <!-- Content -->
            <div class="text-left bg-white/50 dark:bg-black/20 rounded-2xl p-5 mb-6 border border-black/5 dark:border-white/5 overflow-y-auto custom-scrollbar shadow-inner backdrop-blur-sm">
              <p class="text-sm text-neutral-600 dark:text-neutral-400 whitespace-pre-line leading-relaxed">
                {{ $t('privacy.content') }}
              </p>
            </div>
            
            <!-- Action Buttons -->
            <div class="flex gap-3 mt-2">
              <button 
                @click="handleAction(false)" 
                :disabled="isSaving"
                class="flex-1 py-3 px-4 text-sm font-semibold bg-neutral-100 hover:bg-neutral-200 text-neutral-700 dark:bg-zinc-800/80 dark:hover:bg-zinc-700 dark:text-neutral-300 rounded-xl transition-all disabled:opacity-50 disabled:cursor-not-allowed shadow-sm flex items-center justify-center gap-2"
              >
                <X class="w-4 h-4" />
                {{ $t('privacy.decline') }}
              </button>
              <button 
                @click="handleAction(true)" 
                :disabled="isSaving"
                class="flex-[1.5] flex items-center justify-center gap-2 py-3 px-4 text-sm font-bold bg-primary text-primary-foreground rounded-xl hover:bg-primary/90 transition-all shadow-md hover:shadow-lg disabled:opacity-70 disabled:cursor-not-allowed"
              >
                <Check class="w-4 h-4" />
                {{ $t('privacy.agree') }}
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
  transition: all 0.3s ease;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
  transform: scale(0.95) translateY(10px);
}
</style>
