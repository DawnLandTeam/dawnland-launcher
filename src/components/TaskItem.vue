<script setup lang="ts">
import { computed } from 'vue';
import { Clock, Loader2, Pause, CheckCircle, XCircle, Ban, X, RotateCcw } from '@lucide/vue';
import type { TaskState } from '../types/task';
import { getTaskName } from '../types/task';
import { useTaskStore } from '../composables/useTaskStore';

const props = defineProps<{
  task: TaskState
}>();

const taskStore = useTaskStore();

const percentage = computed(() => {
  if (props.task.status === 'Completed') return 100;
  
  if (props.task.progress.sub_tasks && props.task.progress.sub_tasks.length > 0) {
    let totalProgress = 0;
    for (const sub of props.task.progress.sub_tasks) {
      if (sub.status === 'Completed') {
        totalProgress += sub.weight;
      } else if (sub.status === 'Running' || sub.status === 'Pending') {
        if (sub.total > 0) {
          totalProgress += (sub.current / sub.total) * sub.weight;
        }
      }
    }
    const totalWeight = props.task.progress.sub_tasks.reduce((acc, s) => acc + s.weight, 0);
    if (totalWeight > 0) {
        return Math.min(100, Math.round((totalProgress / totalWeight) * 100));
    }
  }

  const { current, total, step, total_steps } = props.task.progress;
  const safeStep = Math.max(1, step || 1);
  const safeTotalSteps = Math.max(1, total_steps || 1);
  
  const basePercent = ((safeStep - 1) / safeTotalSteps) * 100;
  
  let stepProgress = 0;
  if (total > 0) {
    stepProgress = (current / total) * (100 / safeTotalSteps);
  }
  
  return Math.min(100, Math.round(basePercent + stepProgress));
});

const isCancelable = computed(() => {
  return ['Pending', 'Running', 'Paused'].includes(props.task.status);
});

function handleCancel(e: Event) {
  e.stopPropagation();
  if (isCancelable.value) {
    taskStore.cancelTask(props.task.id);
  }
}

function formatSpeed(speed: number | undefined) {
  if (speed === undefined || speed === 0) return '';
  if (speed > 1024 * 1024) return `${(speed / 1024 / 1024).toFixed(1)} MB/s`;
  if (speed > 1024) return `${(speed / 1024).toFixed(1)} KB/s`;
  return `${speed} B/s`;
}

function handleRetry(e: Event) {
  e.stopPropagation();
  if (props.task.status === 'Failed' || props.task.status === 'Cancelled') {
    taskStore.retryTask(props.task.id);
  }
}
</script>

<template>
  <div 
    class="group relative flex flex-col p-3 rounded-xl bg-black/5 dark:bg-white/5 border border-black/10 dark:border-white/10 transition-all hover:bg-black/10 dark:hover:bg-white/10 cursor-pointer"
    @click="taskStore.openTaskDetail(task.id)"
  >
    <div class="flex items-center justify-between mb-2">
      <div class="flex items-center gap-3 overflow-hidden">
        <!-- Status Icon -->
        <div class="shrink-0 flex items-center justify-center">
          <Clock v-if="task.status === 'Pending'" class="w-5 h-5 text-neutral-400" />
          <Loader2 v-else-if="task.status === 'Running'" class="w-5 h-5 text-emerald-400 animate-spin" />
          <Pause v-else-if="task.status === 'Paused'" class="w-5 h-5 text-yellow-400" />
          <CheckCircle v-else-if="task.status === 'Completed'" class="w-5 h-5 text-emerald-500" />
          <XCircle v-else-if="task.status === 'Failed'" class="w-5 h-5 text-red-500" />
          <Ban v-else-if="task.status === 'Cancelled'" class="w-5 h-5 text-orange-400" />
        </div>

        <!-- Task Title & Detail -->
        <div class="flex flex-col overflow-hidden">
          <span class="text-sm font-medium text-neutral-800 dark:text-neutral-200 truncate">
            {{ getTaskName(task.task_type) }}
            <span v-if="(task.progress.total_steps || 1) > 1" class="text-xs text-neutral-500 dark:text-neutral-400 ml-1 font-normal">
              ({{ task.progress.step || 1 }}/{{ task.progress.total_steps }})
            </span>
          </span>
          <div class="flex items-center gap-2 mt-0.5">
            <span v-if="task.status === 'Failed' || task.status === 'Cancelled'" class="text-xs text-red-500 dark:text-red-400 truncate" :title="task.error || ''">
              {{ task.error === 'Task cancelled' ? $t('task.cancelledError') : task.error }}
            </span>
            <template v-else>
              <span v-if="task.progress.sub_tasks && task.progress.sub_tasks.length > 0" class="text-xs text-neutral-500 dark:text-neutral-400 truncate">
                {{ $t('task.subTasksCompleted', { completed: task.progress.sub_tasks.filter(t => t.status === 'Completed').length, total: task.progress.sub_tasks.length }) }}
              </span>
              <span v-else-if="task.progress.detail" class="text-xs text-neutral-500 dark:text-neutral-400 truncate" :title="task.progress.detail">
                {{ task.progress.detail }}
              </span>
              <span v-if="task.progress.speed > 0" class="shrink-0 text-xs font-mono text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-1.5 py-0.5 rounded">
                {{ formatSpeed(task.progress.speed) }}
              </span>
            </template>
          </div>
        </div>
      </div>

      <!-- Action Buttons -->
      <div class="shrink-0 flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity"
           :class="{'!opacity-100': task.status === 'Failed' || task.status === 'Cancelled'}">
        <button 
          v-if="task.status === 'Failed' || task.status === 'Cancelled'"
          @click="handleRetry"
          class="p-1.5 rounded-md hover:bg-emerald-500/20 text-neutral-500 dark:text-neutral-400 hover:text-emerald-600 dark:hover:text-emerald-400"
          title="Retry Task"
        >
          <RotateCcw class="w-4 h-4" />
        </button>
        <button 
          v-if="isCancelable"
          @click="handleCancel"
          class="p-1.5 rounded-md hover:bg-red-500/20 text-neutral-500 dark:text-neutral-400 hover:text-red-600 dark:hover:text-red-400"
          title="Cancel Task"
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    </div>

    <!-- Progress Bar -->
    <div class="w-full bg-black/10 dark:bg-black/40 rounded-full h-1.5 overflow-hidden">
      <div 
        class="h-full rounded-full transition-all duration-300 relative"
        :class="[
          task.status === 'Completed' ? 'bg-emerald-500' :
          task.status === 'Failed' ? 'bg-red-500' :
          task.status === 'Cancelled' ? 'bg-orange-400' :
          task.status === 'Paused' ? 'bg-yellow-400' :
          'bg-emerald-400 overflow-hidden'
        ]"
        :style="{ width: `${percentage}%` }"
      >
        <!-- Animated stripes for running tasks -->
        <div 
          v-if="task.status === 'Running'"
          class="absolute inset-0 bg-[length:1rem_1rem]"
          style="background-image: linear-gradient(45deg, rgba(255,255,255,0.15) 25%, transparent 25%, transparent 50%, rgba(255,255,255,0.15) 50%, rgba(255,255,255,0.15) 75%, transparent 75%, transparent); animation: progress-stripes 1s linear infinite;"
        ></div>
      </div>
    </div>
  </div>
</template>

<style scoped>
@keyframes progress-stripes {
  from { background-position: 1rem 0; }
  to { background-position: 0 0; }
}
</style>
