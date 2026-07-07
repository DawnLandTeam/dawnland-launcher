<script setup lang="ts">
import { ref, computed } from "vue";
import { AlertTriangle, Copy, X } from "@lucide/vue";
import AiAnalysisPanel from "./ai/AiAnalysisPanel.vue";

const props = defineProps<{
  open: boolean;
  exitCode: number;
  versionId: string;
  logs: string[];
  isOpenJ9?: boolean;
  crashReport?: string | null;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
}>();

const copied = ref(false);

// Get all provided logs
const crashLogs = computed(() => {
  return props.logs || [];
});

const formattedLogs = computed(() => {
  return crashLogs.value.join("\n");
});

function close() {
  emit("update:open", false);
}

async function copyLogs() {
  try {
    await navigator.clipboard.writeText(formattedLogs.value);
    copied.value = true;
    setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch (e) {
    console.error("Failed to copy logs:", e);
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
      <div
        v-if="open"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <!-- Frosted glass backdrop -->
        <div
          class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"
        />

        <!-- Crash Report Content -->
        <div
          class="relative z-10 w-full max-w-3xl gap-4 border border-white/20 dark:border-zinc-800/50 bg-white/80 dark:bg-zinc-950/80 backdrop-blur-xl p-4 shadow-2xl rounded-2xl max-h-[85vh] overflow-hidden flex flex-col pointer-events-auto transition-all"
        >
          <!-- Header -->
          <div class="flex items-center justify-between pb-4 border-b border-black/5 dark:border-white/5 -mx-4 px-4 pt-2">
            <div class="flex items-center gap-3">
              <AlertTriangle class="h-6 w-6 text-red-600" />
              <div>
                <h3 class="font-bold text-lg text-red-600">{{ $t('crash.title') }}</h3>
                <p class="text-sm text-muted-foreground">
                  {{ versionId }} · {{ $t('crash.exitCode') }}: {{ exitCode }}
                </p>
              </div>
            </div>
            <button
              @click="close"
              class="text-muted-foreground hover:text-foreground"
            >
              <X class="h-5 w-5" />
            </button>
          </div>

          <!-- OpenJ9 Compatibility Warning -->
          <div v-if="isOpenJ9" class="mx-6 mt-4 p-4 bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700/50 rounded-lg flex items-start gap-3 text-yellow-800 dark:text-yellow-200">
            <AlertTriangle class="h-5 w-5 shrink-0 mt-0.5" />
            <div class="space-y-1">
              <h4 class="font-semibold">{{ $t('crash.openj9Title', 'OpenJ9 兼容性警告 (OpenJ9 Compatibility Warning)') }}</h4>
              <p class="text-sm opacity-90">{{ $t('crash.openj9Desc', '游戏在使用 OpenJ9 虚拟机时崩溃。OpenJ9 与部分 Minecraft 版本及 Mod（特别是 Forge）存在已知的兼容性问题。我们强烈建议您前往“设置 -> Java 管理”中，使用 HotSpot 架构的 Java（如 Eclipse Temurin 或 Microsoft Build of OpenJDK）。') }}</p>
            </div>
          </div>

          <!-- Crash Log Display -->
          <div class="flex-1 mt-4 mb-2 overflow-hidden flex flex-col min-h-0">
            <div class="flex items-center justify-between mb-2">
              <label class="text-sm font-medium">{{ $t('crash.viewLog') }}</label>
              <button
                @click="copyLogs"
                class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium border rounded-md hover:bg-muted transition-colors"
              >
                <Copy class="h-3.5 w-3.5" />
                {{ copied ? $t('crash.copied', 'Copied!') : $t('crash.copyLogs', 'Copy Logs') }}
              </button>
            </div>
            <textarea
              readonly
              :value="formattedLogs"
              class="crash-log-scrollbar flex-1 w-full px-3 py-2 font-mono text-xs bg-black text-green-400 rounded-lg resize-none border-0 focus:ring-0"
              style="min-height: 150px; max-height: 200px;"
            />
          </div>

          <!-- AI Analysis Section -->
          <AiAnalysisPanel
            :log-context="crashReport || formattedLogs"
            context-type="crash"
            :version-id="versionId"
          />
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.crash-log-scrollbar::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}
.crash-log-scrollbar::-webkit-scrollbar-track {
  background: transparent;
  margin-top: 4px;
  margin-bottom: 4px;
}
.crash-log-scrollbar::-webkit-scrollbar-thumb {
  background-color: #3f3f46;
  border-radius: 9999px;
  border: 2px solid black; /* Creates padding effect against the black background */
}
.crash-log-scrollbar::-webkit-scrollbar-thumb:hover {
  background-color: #52525b;
}

.dialog-enter-active,
.dialog-leave-active {
  transition: opacity 150ms ease;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}

.dialog-enter-active .relative,
.dialog-leave-active .relative {
  transition: transform 150ms ease, opacity 150ms ease;
}

.dialog-enter-from .relative {
  transform: scale(0.95);
  opacity: 0;
}

.dialog-leave-to .relative {
  transform: scale(0.95);
  opacity: 0;
}
</style>