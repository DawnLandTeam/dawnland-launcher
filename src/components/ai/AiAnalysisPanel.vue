<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { Zap } from "@lucide/vue";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { useTaskStore } from "../../composables/useTaskStore";
import { toast } from "../../composables/useToast";

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
const streamingText = ref<string>('');
const analysisPhase = ref<'idle' | 'streaming' | 'done' | 'error'>('idle');

let unlistenChunk: any;
let unlistenStreamError: any;
const streamErrorOccurred = ref(false);

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

async function executeAction(action: { type: string, payload: string }) {
  if (action.type === 'search-mod' || action.type === 'install-dependency') {
    router.push({ path: '/downloads', query: { q: action.payload, tab: 'mod', instanceId: props.versionId } });
  } else if (action.type === 'goto' || action.type === 'switch-java') {
    const target = action.type === 'switch-java' ? 'settings-java' : action.payload;
    if (target === 'settings-java') {
      router.push({ path: '/settings', query: { tab: 'java' } });
    } else if (target === 'instance-mods' && props.versionId) {
      router.push({ path: `/instances/${props.versionId}`, query: { tab: 'mods' } });
    }
  } else if (action.type === 'disable-mod') {
    if (!props.versionId) {
      toast.error(t('crash.actions.noInstance'));
      return;
    }
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const disabled = await invoke<string[]>('disable_mod_by_name', {
        versionId: props.versionId,
        modName: action.payload
      });
      toast.success(t('crash.actions.disableModSuccess', { mods: disabled.join(', ') }));
    } catch (err: any) {
      const msg = typeof err === 'object' && err !== null && 'message' in err ? err.message : String(err);
      toast.error(t('crash.actions.disableModFailed', { msg }));
    }
  } else if (action.type === 'retry-task' && props.taskId) {
    taskStore.retryTask(props.taskId);
  }
}

function getActionLabel(action: { type: string, payload: string, label?: string }) {
  if (action.type === 'search-mod') {
    return t('crash.actions.searchMod', { mod: action.payload });
  } else if (action.type === 'install-dependency') {
    return t('crash.actions.installDependency', { dep: action.payload });
  } else if (action.type === 'disable-mod') {
    return t('crash.actions.disableMod', { mod: action.payload });
  } else if (action.type === 'goto' || action.type === 'switch-java') {
    if (action.type === 'switch-java' || action.payload === 'settings-java') return t('crash.actions.gotoJava');
    if (action.payload === 'instance-mods') return t('crash.actions.gotoMods');
  } else if (action.type === 'retry-task') {
    return t('task.actions.retryTask');
  }
  return action.label || action.type;
}

async function analyzeWithAi() {
  if (!props.logContext) {
    aiError.value = t('crash.ai.noLogContext');
    return;
  }

  isAnalyzing.value = true;
  aiError.value = null;
  aiAnalysisResult.value = null;
  streamingText.value = '';
  streamErrorOccurred.value = false;
  analysisPhase.value = 'streaming';

  // Listen for streaming chunks forwarded from the Rust SSE reader.
  unlistenChunk = await listen<string>('ai-analysis-chunk', (event) => {
    streamingText.value += event.payload;
  });

  unlistenStreamError = await listen<string>('ai-analysis-error', (event) => {
    analysisPhase.value = 'error';
    aiError.value = event.payload;
    streamErrorOccurred.value = true;
  });

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    // The command streams progress via events and returns the full text on completion.
    const result = await invoke<string>("analyze_crash", {
      crashLog: props.logContext,
      language: locale.value,
      context_type: props.contextType
    });
    aiAnalysisResult.value = result;
    if (!streamErrorOccurred.value) {
      analysisPhase.value = 'done';
      // Persist to crash history (crash context only, requires instance id)
      if (props.versionId && props.contextType === 'crash') {
        saveCrashHistory(result).catch((e) => {
          console.warn('Failed to save crash history:', e);
        });
      }
    }
  } catch (err: any) {
    console.error("AI Analysis failed:", err);
    // Only set error if the stream-error listener hasn't already handled it.
    if (!streamErrorOccurred.value) {
      aiError.value = typeof err === 'object' && err !== null && 'message' in err
        ? err.message
        : String(err);
      analysisPhase.value = 'error';
    }
  } finally {
    if (unlistenChunk) { unlistenChunk(); unlistenChunk = null; }
    if (unlistenStreamError) { unlistenStreamError(); unlistenStreamError = null; }
    isAnalyzing.value = false;
  }
}

async function saveCrashHistory(aiResult: string) {
  let aiCause: string | undefined;
  let aiSolution: string | undefined;
  let aiActions: string | undefined;
  try {
    let jsonStr = aiResult.trim()
      .replace(/^```json\n?/, '').replace(/```$/, '').trim()
      .replace(/<think>[\s\S]*?<\/think>/g, '').trim();
    const startIndex = jsonStr.indexOf('{');
    const endIndex = jsonStr.lastIndexOf('}');
    if (startIndex !== -1 && endIndex !== -1) {
      jsonStr = jsonStr.substring(startIndex, endIndex + 1);
    }
    const parsed = JSON.parse(jsonStr);
    const normalized: any = {};
    for (const key in parsed) {
      if (Object.prototype.hasOwnProperty.call(parsed, key)) {
        normalized[key.toLowerCase()] = parsed[key];
      }
    }
    aiCause = normalized.cause;
    aiSolution = normalized.solution;
    aiActions = normalized.actions ? JSON.stringify(normalized.actions) : undefined;
  } catch {
    aiCause = aiResult.substring(0, 500);
  }

  const { invoke } = await import("@tauri-apps/api/core");
  await invoke('save_crash_history', {
    input: {
      instanceId: props.versionId,
      crashSummary: props.logContext.substring(0, 2000),
      aiCause,
      aiSolution,
      aiActions,
    }
  });
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
  if (unlistenChunk) unlistenChunk();
  if (unlistenStreamError) unlistenStreamError();
});
</script>

<template>
  <div class="mb-2 mt-2">
    <div v-if="aiAnalysisResult" class="p-4 bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-700/50 rounded-xl shadow-inner">
      <h4 class="font-semibold flex items-center gap-2 text-blue-800 dark:text-blue-300 mb-2">
        <span class="i-lucide-bot w-5 h-5"></span> {{ $t('crash.ai.resultTitle') }}
      </h4>
      
      <div v-if="parsedAiResult" class="space-y-3">
        <div>
          <h5 class="text-xs font-bold text-blue-900/70 dark:text-blue-100/70 mb-1 uppercase tracking-wider">{{ $t('crash.ai.cause') }}</h5>
          <p class="text-sm text-blue-900 dark:text-blue-100 leading-relaxed">{{ parsedAiResult.cause }}</p>
        </div>
        <div>
          <h5 class="text-xs font-bold text-blue-900/70 dark:text-blue-100/70 mb-1 uppercase tracking-wider">{{ $t('crash.ai.solution') }}</h5>
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

    <div v-else-if="analysisPhase === 'streaming'" class="p-4 bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-700/50 rounded-xl shadow-inner">
      <h4 class="font-semibold flex items-center gap-2 text-blue-800 dark:text-blue-300 mb-2">
        <span class="i-lucide-loader-2 w-5 h-5 animate-spin"></span> {{ $t('crash.ai.analyzing') }}
      </h4>
      <div class="text-sm text-blue-900 dark:text-blue-100 leading-relaxed whitespace-pre-wrap break-words min-h-[2rem]">
        {{ streamingText }}<span class="inline-block w-2 h-4 bg-blue-500 animate-pulse ml-0.5 align-text-bottom"></span>
      </div>
    </div>

    <div v-else-if="aiError" class="p-4 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-700/50 rounded-lg">
      <h4 class="font-semibold text-red-800 dark:text-red-300 mb-1">{{ $t('crash.ai.error') }}</h4>
      <p class="text-sm text-red-600 dark:text-red-400">{{ aiError }}</p>
      
      <div v-if="aiError.includes('llama-server')" class="mt-3">
        <button @click="downloadEngine" :disabled="isDownloadingEngine" class="px-3 py-1.5 bg-red-100 dark:bg-red-800 text-red-700 dark:text-red-200 rounded-md text-sm font-medium hover:bg-red-200 transition-colors">
          <span v-if="isDownloadingEngine" class="i-lucide-loader-2 w-4 h-4 animate-spin inline-block mr-1 align-text-bottom"></span>
          {{ isDownloadingEngine ? engineDownloadStatus : $t('crash.ai.downloadEngine') }}
        </button>
      </div>
      <button v-else @click="analyzeWithAi" class="mt-2 text-sm underline text-red-600">{{ $t('crash.ai.retry') }}</button>
    </div>
    
    <div v-else class="flex justify-end">
      <button
        @click="analyzeWithAi"
        :disabled="isAnalyzing"
        class="flex items-center gap-2 px-5 py-2.5 bg-gradient-to-r from-blue-600 to-indigo-600 text-white font-medium rounded-xl hover:from-blue-500 hover:to-indigo-500 disabled:opacity-50 disabled:from-zinc-500 disabled:to-zinc-500 shadow-lg shadow-blue-500/20 transition-all hover:scale-[1.02] active:scale-95"
      >
        <span v-if="isAnalyzing" class="i-lucide-loader-2 w-4 h-4 animate-spin"></span>
        <span v-else class="i-lucide-bot w-4 h-4"></span>
        {{ isAnalyzing ? $t('crash.ai.analyzing') : $t('crash.ai.analyze') }}
      </button>
    </div>
  </div>
</template>
