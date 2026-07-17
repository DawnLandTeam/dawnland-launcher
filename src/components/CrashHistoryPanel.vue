<script setup lang="ts">
import { ref, onMounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useToast } from "../composables/useToast";
import { AlertTriangle, Trash2, ChevronRight, FileText } from "@lucide/vue";
import DButton from "../components/ui/DButton.vue";

const { t } = useI18n();
const toast = useToast();

const props = defineProps<{
  instanceId?: string;
}>();

interface CrashHistoryEntry {
  id: number;
  instanceId: string;
  instanceName: string | null;
  exitCode: number | null;
  crashSummary: string;
  aiCause: string | null;
  aiSolution: string | null;
  aiActions: string | null;
  createdAt: number;
}

const history = ref<CrashHistoryEntry[]>([]);
const loading = ref(false);
const expandedId = ref<number | null>(null);

async function loadHistory() {
  loading.value = true;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    history.value = await invoke<CrashHistoryEntry[]>('get_crash_history', {
      instanceId: props.instanceId || null,
    });
  } catch (e: any) {
    toast.error(e?.message || String(e));
  } finally {
    loading.value = false;
  }
}

async function deleteEntry(id: number) {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke('delete_crash_history', { id });
    history.value = history.value.filter(e => e.id !== id);
    if (expandedId.value === id) expandedId.value = null;
    toast.success(t('crash.history.deleted'));
  } catch (e: any) {
    toast.error(e?.message || String(e));
  }
}

async function clearAll() {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke('clear_crash_history', { instanceId: props.instanceId || null });
    history.value = [];
    expandedId.value = null;
    toast.success(t('crash.history.cleared'));
  } catch (e: any) {
    toast.error(e?.message || String(e));
  }
}

function toggleExpand(id: number) {
  expandedId.value = expandedId.value === id ? null : id;
}

function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString();
}

onMounted(() => {
  loadHistory();
});

watch(() => props.instanceId, () => {
  loadHistory();
});
</script>

<template>
  <div class="flex-1 h-full w-full flex flex-col min-h-0 bg-white/40 dark:bg-zinc-900/40">
    <!-- Header -->
    <div class="px-6 py-4 border-b border-neutral-200/50 dark:border-zinc-800/50 flex-shrink-0 flex items-center justify-between">
      <h3 class="text-lg font-semibold flex items-center gap-2">
        <AlertTriangle class="w-5 h-5 text-amber-500" />
        {{ $t('instances.crashHistory') }}
      </h3>
      <DButton
        v-if="history.length > 0"
        variant="danger"
        size="sm"
        @click="clearAll"
      >
        <Trash2 class="w-4 h-4 mr-1.5" />
        {{ $t('crash.history.clearAll') }}
      </DButton>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-y-auto p-6 minimal-scrollbar">
      <!-- Loading -->
      <div v-if="loading" class="flex justify-center items-center py-12">
        <div class="w-8 h-8 border-4 border-primary border-t-transparent rounded-full animate-spin"></div>
      </div>

      <!-- Empty state -->
      <div v-else-if="history.length === 0" class="flex flex-col items-center justify-center py-16 text-center text-muted-foreground">
        <AlertTriangle class="w-16 h-16 mb-4 opacity-20" />
        <p class="text-lg font-medium">{{ $t('crash.history.empty') }}</p>
        <p class="text-sm mt-1">{{ $t('crash.history.emptyDesc') }}</p>
      </div>

      <!-- History list -->
      <div v-else class="grid gap-3">
        <div
          v-for="entry in history"
          :key="entry.id"
          class="rounded-lg border bg-card text-card-foreground shadow-sm transition-all hover:shadow-md overflow-hidden"
        >
          <!-- Row header -->
          <div
            class="flex items-center gap-3 p-3 cursor-pointer hover:bg-zinc-50 dark:hover:bg-zinc-800/50 transition-colors"
            @click="toggleExpand(entry.id)"
          >
            <div class="w-9 h-9 rounded-lg bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center flex-shrink-0">
              <AlertTriangle class="w-4.5 h-4.5 text-amber-600 dark:text-amber-400" />
            </div>

            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium truncate">
                {{ entry.aiCause || (entry.crashSummary.substring(0, 80) + '...') }}
              </p>
              <div class="flex items-center gap-2 text-xs text-muted-foreground mt-0.5">
                <span>{{ formatTime(entry.createdAt) }}</span>
                <template v-if="entry.exitCode != null">
                  <span class="text-zinc-300 dark:text-zinc-600">·</span>
                  <span>{{ $t('crash.exitCode') }}: {{ entry.exitCode }}</span>
                </template>
                <template v-if="entry.instanceName">
                  <span class="text-zinc-300 dark:text-zinc-600">·</span>
                  <span>{{ entry.instanceName }}</span>
                </template>
              </div>
            </div>

            <button
              class="inline-flex items-center gap-1.5 px-2.5 py-1.5 text-xs font-medium text-muted-foreground hover:text-red-600 dark:hover:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-md transition-colors flex-shrink-0"
              @click.stop="deleteEntry(entry.id)"
            >
              <Trash2 class="w-3.5 h-3.5" />
              {{ $t('common.delete') }}
            </button>

            <ChevronRight
              class="w-4 h-4 text-muted-foreground transition-transform flex-shrink-0"
              :class="{ 'rotate-90': expandedId === entry.id }"
            />
          </div>

          <!-- Expanded detail -->
          <div
            v-if="expandedId === entry.id"
            class="p-4 border-t border-neutral-200/50 dark:border-zinc-800/50 space-y-3 bg-zinc-50/50 dark:bg-zinc-800/30"
          >
            <!-- AI Cause -->
            <div v-if="entry.aiCause" class="p-3 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800/40 rounded-lg">
              <h5 class="text-xs font-bold text-blue-700 dark:text-blue-300 mb-1 uppercase tracking-wider flex items-center gap-1.5">
                <span class="i-lucide-search w-3.5 h-3.5"></span>
                {{ $t('crash.ai.cause') }}
              </h5>
              <p class="text-sm text-blue-900 dark:text-blue-100 leading-relaxed">{{ entry.aiCause }}</p>
            </div>

            <!-- AI Solution -->
            <div v-if="entry.aiSolution" class="p-3 bg-emerald-50 dark:bg-emerald-900/20 border border-emerald-200 dark:border-emerald-800/40 rounded-lg">
              <h5 class="text-xs font-bold text-emerald-700 dark:text-emerald-300 mb-1 uppercase tracking-wider flex items-center gap-1.5">
                <span class="i-lucide-lightbulb w-3.5 h-3.5"></span>
                {{ $t('crash.ai.solution') }}
              </h5>
              <p class="text-sm text-emerald-900 dark:text-emerald-100 leading-relaxed">{{ entry.aiSolution }}</p>
            </div>

            <!-- Crash log summary -->
            <div v-if="entry.crashSummary" class="rounded-lg border border-neutral-200/80 dark:border-zinc-700/80 overflow-hidden">
              <button
                class="w-full flex items-center gap-2 px-3 py-2 text-xs text-muted-foreground hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
                @click.stop
              >
                <FileText class="w-3.5 h-3.5" />
                {{ $t('crash.viewLog') }}
              </button>
              <pre class="px-3 pb-3 text-xs text-zinc-600 dark:text-zinc-400 bg-zinc-100/80 dark:bg-zinc-900/50 p-2 rounded-b-lg overflow-x-auto whitespace-pre-wrap break-words max-h-40 overflow-y-auto border-t border-neutral-200/50 dark:border-zinc-800/50">{{ entry.crashSummary }}</pre>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
