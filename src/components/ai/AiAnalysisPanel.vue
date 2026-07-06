<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { Zap } from "@lucide/vue";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { useTaskStore } from "../../composables/useTaskStore";

const { locale, t } = useI18n();
const router = useRouter();
const taskStore = useTaskStore();

const props = defineProps<{
  logContext: string;
  contextType: "crash" | "task";
  taskId?: string;
  versionId?: string; // Optional context for search-mod and goto instance-mods
}>();

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
    
    // Normalize keys to lowercase to handle model variations (e.g., "Cause" instead of "cause")
    const normalized: any = {};
    for (const key in parsed) {
      if (Object.prototype.hasOwnProperty.call(parsed, key)) {
        normalized[key.toLowerCase()] = parsed[key];
      }
    }
    
    if (normalized.cause && normalized.solution) {
      return normalized as AiResponse;
    }
  } catch (e) {
    console.warn("Failed to parse AI JSON response:", e);
    return null;
  }
  return null;
});

const renderedAiResult = computed(() => {
  if (!aiAnalysisResult.value) return '';
  const rawHtml = marked.parse(aiAnalysisResult.value) as string;
  return DOMPurify.sanitize(rawHtml);
});

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
      
      executeAction({ type: action, payload });
    }
  }
}

function executeAction(action: { type: string, payload: string }) {
  if (action.type === 'search-mod') {
    router.push({ path: '/downloads', query: { q: action.payload, tab: 'mod', instanceId: props.versionId } });
  } else if (action.type === 'goto') {
    if (action.payload === 'settings-java') {
      router.push({ path: '/settings', query: { tab: 'java' } });
    } else if (action.payload === 'instance-mods' && props.versionId) {
      router.push({ path: `/instances/${props.versionId}`, query: { tab: 'mods' } });
    }
  } else if (action.type === 'retry-task' && props.taskId) {
    taskStore.retryTask(props.taskId);
  }
}

function getActionLabel(action: { type: string, payload: string, label?: string }) {
  if (action.type === 'search-mod') {
    return t('crash.actions.searchMod', { mod: action.payload });
  } else if (action.type === 'goto') {
    if (action.payload === 'settings-java') return t('crash.actions.gotoJava');
    if (action.payload === 'instance-mods') return t('crash.actions.gotoMods');
  } else if (action.type === 'retry-task') {
    return t('task.actions.retryTask', '重试任务 (Retry Task)');
  }
  return action.label || action.type;
}

async function analyzeWithAi() {
  if (!props.logContext) {
    aiError.value = "没有提供有效的日志上下文进行分析 (No valid log context provided).";
    return;
  }
  
  isAnalyzing.value = true;
  aiError.value = null;
  
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    // Passing contextType alongside the log. The backend route decides the prompt.
    const result = await invoke<string>("analyze_crash", { 
      crashLog: props.logContext, 
      language: locale.value,
      context_type: props.contextType 
    });
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
  <div class="mb-2 mt-2">
    <div v-if="aiAnalysisResult" class="p-4 bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-700/50 rounded-xl shadow-inner">
      <h4 class="font-semibold flex items-center gap-2 text-blue-800 dark:text-blue-300 mb-2">
        <span class="i-lucide-bot w-5 h-5"></span> {{ $t('crash.ai.resultTitle', 'AI 智能分析结果') }}
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
</template>
