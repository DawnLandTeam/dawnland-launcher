<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { X, Copy, AlertTriangle, Zap } from "@lucide/vue";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { marked } from "marked";
import DOMPurify from "dompurify";

const { locale, t } = useI18n();
const router = useRouter();

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

const isAnalyzing = ref(false);
const aiAnalysisResult = ref<string | null>(null);
const aiError = ref<string | null>(null);

interface AiResponse {
  cause: string;
  solution: string;
  actions?: { label: string; type: string; payload: string }[];
}

const parsedAiResult = computed<AiResponse | null>(() => {
  if (!aiAnalysisResult.value) return null;
  try {
    let jsonStr = aiAnalysisResult.value.trim();
    if (jsonStr.startsWith("```json")) {
      jsonStr = jsonStr.replace(/^```json\n?/, '').replace(/```$/, '').trim();
    } else if (jsonStr.startsWith("```")) {
      jsonStr = jsonStr.replace(/^```\n?/, '').replace(/```$/, '').trim();
    }
    
    // Support DeepSeek R1 reasoning models by stripping out <think> blocks
    jsonStr = jsonStr.replace(/<think>[\s\S]*?<\/think>/g, '').trim();
    
    // Find first '{' and last '}'
    const startIndex = jsonStr.indexOf('{');
    const endIndex = jsonStr.lastIndexOf('}');
    if (startIndex !== -1 && endIndex !== -1) {
      jsonStr = jsonStr.substring(startIndex, endIndex + 1);
    }
    
    const parsed = JSON.parse(jsonStr);
    if (parsed.cause && parsed.solution) {
      return parsed;
    }
  } catch (e) {
    return null;
  }
  return null;
});

const renderedAiResult = computed(() => {
  if (!aiAnalysisResult.value) return '';
  // parse() can return string | Promise<string> depending on config, cast to string is safe here with default sync config
  const rawHtml = marked.parse(aiAnalysisResult.value) as string;
  return DOMPurify.sanitize(rawHtml);
});

function close() {
  aiAnalysisResult.value = null;
  aiError.value = null;
  emit("update:open", false);
}

function handleAiResultClick(e: MouseEvent) {
  const target = e.target as HTMLElement;
  const anchor = target.closest('a');
  if (anchor) {
    const href = anchor.getAttribute('href');
    if (href && href.startsWith('#action:')) {
      e.preventDefault();
      const parts = href.substring(8).split(':');
      const action = parts[0];
      const payload = decodeURIComponent(parts.slice(1).join(':'));
      
      close();
      if (action === 'search-mod') {
        router.push({ path: '/downloads', query: { q: payload, tab: 'mod', instanceId: props.versionId } });
      } else if (action === 'goto') {
        if (payload === 'settings-java') {
          router.push({ path: '/settings', query: { tab: 'java' } });
        } else if (payload === 'instance-mods') {
          router.push({ path: `/instances/${props.versionId}`, query: { tab: 'mods' } });
        }
      }
    }
  }
}

function executeAction(action: { type: string, payload: string }) {
  close();
  if (action.type === 'search-mod') {
    router.push({ path: '/downloads', query: { q: action.payload, tab: 'mod', instanceId: props.versionId } });
  } else if (action.type === 'goto') {
    if (action.payload === 'settings-java') {
      router.push({ path: '/settings', query: { tab: 'java' } });
    } else if (action.payload === 'instance-mods') {
      router.push({ path: `/instances/${props.versionId}`, query: { tab: 'mods' } });
    }
  }
}

function getActionLabel(action: { type: string, payload: string, label?: string }) {
  if (action.type === 'search-mod') {
    return t('crash.actions.searchMod', { mod: action.payload });
  } else if (action.type === 'goto') {
    if (action.payload === 'settings-java') return t('crash.actions.gotoJava');
    if (action.payload === 'instance-mods') return t('crash.actions.gotoMods');
  }
  return action.label || action.type;
}

async function analyzeWithAi() {
  isAnalyzing.value = true;
  aiError.value = null;
  
  const logToAnalyze = props.crashReport || formattedLogs.value;
  
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const result = await invoke<string>("analyze_crash", { crashLog: logToAnalyze, language: locale.value });
    aiAnalysisResult.value = result;
  } catch (err: any) {
    console.error("AI Analysis failed:", err);
    aiError.value = typeof err === 'object' && err !== null && 'message' in err
      ? err.message
      : String(err);
  } finally {
    isAnalyzing.value = false;
  }
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

const isDownloadingEngine = ref(false);
const engineDownloadStatus = ref('');

async function downloadEngine() {
  isDownloadingEngine.value = true;
  engineDownloadStatus.value = '正在下载... (0%)';
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke('download_engine');
  } catch(e: any) {
    aiError.value = typeof e === 'object' && e !== null && 'message' in e ? e.message : String(e);
    isDownloadingEngine.value = false;
  }
}

let unlistenProgress: any;
let unlistenComplete: any;
let unlistenError: any;

onMounted(async () => {
  unlistenProgress = await listen<any>('download-progress', (event) => {
    const { taskId, downloaded, total, error } = event.payload;
    if (taskId === 'engine_zip') {
      if (error) {
        aiError.value = `引擎下载出错: ${error}`;
        isDownloadingEngine.value = false;
      } else {
        const pct = Math.round((downloaded / total) * 100) || 0;
        engineDownloadStatus.value = `正在下载... (${pct}%)`;
      }
    }
  });
  
  unlistenComplete = await listen<any>('engine-download-complete', () => {
    isDownloadingEngine.value = false;
    analyzeWithAi(); // Auto retry analysis after download
  });
  
  unlistenError = await listen<any>('engine-download-error', (event) => {
    isDownloadingEngine.value = false;
    aiError.value = `引擎下载失败: ${event.payload}`;
  });
});

onUnmounted(() => {
  if (unlistenProgress) unlistenProgress();
  if (unlistenComplete) unlistenComplete();
  if (unlistenError) unlistenError();
});
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
          <div class="mb-2 mt-2">
            <div v-if="aiAnalysisResult" class="p-4 bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-700/50 rounded-xl shadow-inner">
              <h4 class="font-semibold flex items-center gap-2 text-blue-800 dark:text-blue-300 mb-2">
                <span class="i-lucide-bot w-5 h-5"></span> {{ $t('crash.ai.resultTitle', 'AI 崩溃分析结果') }}
              </h4>
              
              <div v-if="parsedAiResult" class="space-y-3">
                <div>
                  <h5 class="text-xs font-bold text-blue-900/70 dark:text-blue-100/70 mb-1 uppercase tracking-wider">{{ $t('crash.ai.cause', '原因分析') }}</h5>
                  <p class="text-sm text-blue-900 dark:text-blue-100 leading-relaxed">{{ parsedAiResult.cause }}</p>
                </div>
                <div>
                  <h5 class="text-xs font-bold text-blue-900/70 dark:text-blue-100/70 mb-1 uppercase tracking-wider">{{ $t('crash.ai.solution', '解决方案') }}</h5>
                  <p class="text-sm text-blue-900 dark:text-blue-100 leading-relaxed">{{ parsedAiResult.solution }}</p>
                </div>
                
                <div v-if="parsedAiResult.actions && parsedAiResult.actions.length > 0" class="flex flex-wrap gap-2 mt-3 pt-3 border-t border-blue-200/50 dark:border-blue-700/30">
                  <button
                    v-for="action in parsedAiResult.actions"
                    :key="action.payload + action.type"
                    @click="executeAction(action)"
                    class="flex items-center gap-1.5 px-3 py-1.5 bg-blue-100 hover:bg-blue-200 dark:bg-blue-800/50 dark:hover:bg-blue-700 text-blue-700 dark:text-blue-100 text-xs font-medium rounded-lg transition-colors shadow-sm outline-none focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50"
                  >
                    <Zap class="w-3.5 h-3.5" />
                    {{ getActionLabel(action) }}
                  </button>
                </div>
              </div>
              <div v-else class="prose prose-sm dark:prose-invert prose-blue max-w-none text-blue-900 dark:text-blue-100" v-html="renderedAiResult" @click="handleAiResultClick"></div>
            </div>
            <div v-else-if="aiError" class="p-4 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-700/50 rounded-lg">
              <h4 class="font-semibold text-red-800 dark:text-red-300 mb-1">{{ $t('crash.ai.error', 'AI 分析失败') }}</h4>
              <p class="text-sm text-red-600 dark:text-red-400">{{ aiError }}</p>
              
              <div v-if="aiError.includes('llama-server')" class="mt-3">
                <button @click="downloadEngine" :disabled="isDownloadingEngine" class="px-3 py-1.5 bg-red-100 dark:bg-red-800 text-red-700 dark:text-red-200 rounded-md text-sm font-medium hover:bg-red-200 transition-colors">
                  <span v-if="isDownloadingEngine" class="i-lucide-loader-2 w-4 h-4 animate-spin inline-block mr-1 align-text-bottom"></span>
                  {{ isDownloadingEngine ? engineDownloadStatus : $t('crash.ai.downloadEngine', '下载 AI 推理引擎 (仅需一次)') }}
                </button>
              </div>
              <button v-else @click="analyzeWithAi" class="mt-2 text-sm underline text-red-600">{{ $t('crash.ai.retry', '重试') }}</button>
            </div>
            <div v-else class="flex justify-end">
              <button
                @click="analyzeWithAi"
                :disabled="isAnalyzing"
                class="flex items-center gap-2 px-5 py-2.5 bg-gradient-to-r from-blue-600 to-indigo-600 text-white font-medium rounded-xl hover:from-blue-500 hover:to-indigo-500 disabled:opacity-50 disabled:from-zinc-500 disabled:to-zinc-500 shadow-lg shadow-blue-500/20 transition-all hover:scale-[1.02] active:scale-95"
              >
                <span v-if="isAnalyzing" class="i-lucide-loader-2 w-4 h-4 animate-spin"></span>
                <span v-else class="i-lucide-bot w-4 h-4"></span>
                {{ isAnalyzing ? $t('crash.ai.analyzing', 'AI 正在深度分析...') : $t('crash.ai.analyze', '一键 AI 智能分析') }}
              </button>
            </div>
          </div>
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