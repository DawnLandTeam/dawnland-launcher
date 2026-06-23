<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { useTaskStore } from '../composables/useTaskStore';
import { Clock, Loader2, CheckCircle, Pause, XCircle, Ban, ArrowRight } from '@lucide/vue';
import { getTaskName, type TaskState } from '../types/task';

const props = defineProps<{
  task: TaskState;
}>();

const { t, te } = useI18n();
const taskStore = useTaskStore();

function getSubTaskName(sub: any) {
  const key = `task.sub.${sub.key}`;
  return te(key) ? t(key) : sub.name;
}

function formatSpeed(speed: number | undefined) {
  if (speed === undefined || speed === 0) return '';
  if (speed > 1024 * 1024) return `${(speed / 1024 / 1024).toFixed(1)} MB/s`;
  if (speed > 1024) return `${(speed / 1024).toFixed(1)} KB/s`;
  return `${speed} B/s`;
}

const isCancelable = computed(() => {
  return ['Pending', 'Running', 'Paused'].includes(props.task.status);
});

function handleCancel() {
  if (isCancelable.value) {
    taskStore.cancelTask(props.task.id);
  }
}

function getSubTaskPercentage(sub: any) {
  if (sub.status === 'Completed') return 100;
  if (sub.total === 0) return 0;
  return Math.min(100, Math.floor((sub.current / sub.total) * 100));
}
</script>

<template>
  <div class="flex flex-col h-full rounded-lg bg-muted/30 overflow-hidden">
    <!-- Header -->
    <div class="flex flex-col gap-1 p-4 shrink-0 relative border-b border-black/5 dark:border-white/5">
      <div class="flex items-start justify-between">
        <h3 class="font-semibold text-neutral-800 dark:text-neutral-200 leading-tight">
          {{ getTaskName(task.task_type) }}
        </h3>
        <!-- Status Icon -->
        <div class="shrink-0 flex items-center justify-center gap-3">
          <span v-if="task.progress.speed > 0" class="shrink-0 text-xs font-mono text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-1.5 py-0.5 rounded">
            {{ formatSpeed(task.progress.speed) }}
          </span>
          <Clock v-if="task.status === 'Pending'" class="w-5 h-5 text-neutral-400" />
          <Loader2 v-else-if="task.status === 'Running'" class="w-5 h-5 text-emerald-400 animate-spin" />
          <Pause v-else-if="task.status === 'Paused'" class="w-5 h-5 text-yellow-400" />
          <CheckCircle v-else-if="task.status === 'Completed'" class="w-5 h-5 text-emerald-500" />
          <XCircle v-else-if="task.status === 'Failed'" class="w-5 h-5 text-red-500" />
          <Ban v-else-if="task.status === 'Cancelled'" class="w-5 h-5 text-orange-400" />
        </div>
      </div>
      <p v-if="task.status === 'Failed' || task.status === 'Cancelled'" class="text-xs text-red-500 dark:text-red-400 line-clamp-2 mt-1">
        {{ task.error === 'Task cancelled' ? $t('task.cancelledError') : task.error }}
      </p>
      <div v-else-if="!task.progress.sub_tasks || task.progress.sub_tasks.length === 0" class="flex items-center gap-2 mt-1">
        <span class="text-xs text-neutral-500 dark:text-neutral-400 line-clamp-2">
          {{ task.progress.detail }}
        </span>
        
      </div>
      
    </div>

    <!-- Sub-Tasks List -->
    <div class="flex-1 overflow-y-auto p-4 space-y-4 minimal-scrollbar min-h-[200px]">
      <div v-if="!task.progress.sub_tasks || task.progress.sub_tasks.length === 0" class="py-8 text-center text-sm text-neutral-500 dark:text-neutral-400">
        {{ $t('task.noSubTasks') }}
      </div>
      
      <div 
        v-for="sub in task.progress.sub_tasks" 
        :key="sub.key"
        class="flex flex-col gap-1.5"
        :class="{'opacity-50': sub.status === 'Pending'}"
      >
        <div class="flex justify-between items-center text-sm">
          <span class="flex items-center gap-2 text-neutral-700 dark:text-neutral-300 font-medium truncate">
            <CheckCircle v-if="sub.status === 'Completed'" class="w-3.5 h-3.5 text-emerald-500 shrink-0" />
            <XCircle v-else-if="sub.status === 'Failed' || (sub.status === 'Running' && task.status === 'Failed')" class="w-3.5 h-3.5 text-red-500 shrink-0" />
            <Ban v-else-if="task.status === 'Cancelled'" class="w-3.5 h-3.5 text-orange-400 dark:text-orange-500 shrink-0" />
            <Clock v-else-if="sub.status === 'Pending' && task.status !== 'Failed'" class="w-3.5 h-3.5 text-neutral-400 shrink-0" />
            <Loader2 v-else-if="sub.status === 'Running' && task.status !== 'Failed'" class="w-3.5 h-3.5 text-emerald-400 animate-spin shrink-0" />
            <ArrowRight v-else class="w-3.5 h-3.5 text-neutral-400 shrink-0" />
            
            <span class="truncate">{{ getSubTaskName(sub) }}</span>
          </span>
          <span class="text-xs font-mono text-neutral-500 shrink-0" v-if="(sub.status === 'Running' || sub.status === 'Completed') && task.status !== 'Cancelled' && task.status !== 'Failed'">
            <span v-if="sub.status === 'Running' && sub.current === 0 && sub.total === 100" class="text-emerald-500 animate-pulse">
              {{ $t('task.installing') }}
            </span>
            <span v-else>
              {{ getSubTaskPercentage(sub) }}%
            </span>
          </span>
        </div>
        
        <div v-if="sub.status === 'Running' && task.status !== 'Cancelled' && task.status !== 'Failed'" class="w-full bg-black/10 dark:bg-white/10 rounded-full h-1 overflow-hidden">
          <div 
            v-if="sub.current > 0 || sub.total === 0"
            class="h-full rounded-full transition-all duration-300 relative bg-emerald-400"
            :style="{ width: `${getSubTaskPercentage(sub)}%` }"
          >
            <div 
              class="absolute inset-0 bg-[length:1rem_1rem]"
              style="background-image: linear-gradient(45deg, rgba(255,255,255,0.15) 25%, transparent 25%, transparent 50%, rgba(255,255,255,0.15) 50%, rgba(255,255,255,0.15) 75%, transparent 75%, transparent); animation: progress-stripes 1s linear infinite;"
            ></div>
          </div>
          <div v-else class="h-full rounded-full bg-emerald-400/50 indeterminate-progress"></div>
        </div>
      </div>
    </div>

    <!-- Footer Action -->
    <div v-if="isCancelable" class="p-4 border-t border-black/10 dark:border-white/10 shrink-0">
      <button 
        @click="handleCancel"
        class="w-full py-2 rounded-lg bg-red-500/10 text-red-600 dark:text-red-400 hover:bg-red-500/20 font-medium text-sm transition-colors"
      >
        {{ $t('task.cancel') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
@keyframes progress-stripes {
  from { background-position: 1rem 0; }
  to { background-position: 0 0; }
}

.minimal-scrollbar::-webkit-scrollbar {
  width: 4px;
}
.minimal-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.minimal-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(150, 150, 150, 0.3);
  border-radius: 4px;
}
:global(.dark) .minimal-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
}
</style>
