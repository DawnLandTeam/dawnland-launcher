<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { useTaskStore } from '../composables/useTaskStore';
import { X, Clock, Loader2, CheckCircle, Pause, XCircle, Ban, ArrowRight } from '@lucide/vue';
import { getTaskName } from '../types/task';

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

const task = computed(() => {
  if (!taskStore.selectedTaskId.value) return null;
  return taskStore.tasks.value.find(t => t.id === taskStore.selectedTaskId.value) || null;
});

const isCancelable = computed(() => {
  if (!task.value) return false;
  return ['Pending', 'Running', 'Paused'].includes(task.value.status);
});

function handleClose() {
  taskStore.closeTaskDetail();
}

function handleCancel() {
  if (task.value && isCancelable.value) {
    taskStore.cancelTask(task.value.id);
  }
}



function getSubTaskPercentage(sub: any) {
  if (sub.status === 'Completed') return 100;
  if (sub.total === 0) return 0;
  return Math.min(100, Math.round((sub.current / sub.total) * 100));
}
</script>

<template>
  <Transition
    enter-active-class="transition-all duration-300 ease-out"
    enter-from-class="opacity-0 translate-x-[-16px] scale-95"
    enter-to-class="opacity-100 translate-x-0 scale-100"
    leave-active-class="transition-all duration-200 ease-in"
    leave-from-class="opacity-100 translate-x-0 scale-100"
    leave-to-class="opacity-0 translate-x-[-16px] scale-95"
  >
    <div
      v-if="taskStore.isTaskDetailOpen.value && task"
      class="task-detail-modal fixed bottom-4 left-[400px] w-96 max-h-[80vh] flex flex-col rounded-2xl bg-white/80 dark:bg-black/80 backdrop-blur-xl border border-black/10 dark:border-white/10 shadow-2xl z-[100]"
    >
      <!-- Header -->
      <div class="flex flex-col gap-1 p-4 border-b border-black/10 dark:border-white/10 shrink-0 relative">
        <div class="flex items-start justify-between pr-6">
          <h3 class="font-semibold text-neutral-800 dark:text-neutral-200 leading-tight">
            {{ getTaskName(task.task_type) }}
          </h3>
          <!-- Status Icon -->
          <div class="shrink-0 flex items-center justify-center">
            <Clock v-if="task.status === 'Pending'" class="w-5 h-5 text-neutral-400" />
            <Loader2 v-else-if="task.status === 'Running'" class="w-5 h-5 text-emerald-400 animate-spin" />
            <Pause v-else-if="task.status === 'Paused'" class="w-5 h-5 text-yellow-400" />
            <CheckCircle v-else-if="task.status === 'Completed'" class="w-5 h-5 text-emerald-500" />
            <XCircle v-else-if="task.status === 'Failed'" class="w-5 h-5 text-red-500" />
            <Ban v-else-if="task.status === 'Cancelled'" class="w-5 h-5 text-orange-400" />
          </div>
        </div>
        <p v-if="task.status === 'Failed' || task.status === 'Cancelled'" class="text-xs text-red-500 dark:text-red-400 line-clamp-2 mt-1">
          {{ task.error }}
        </p>
        <div v-else-if="!task.progress.sub_tasks || task.progress.sub_tasks.length === 0" class="flex items-center gap-2 mt-1">
          <span class="text-xs text-neutral-500 dark:text-neutral-400 line-clamp-2">
            {{ task.progress.detail }}
          </span>
          <span v-if="task.progress.speed > 0" class="shrink-0 text-xs font-mono text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-1.5 py-0.5 rounded">
            {{ formatSpeed(task.progress.speed) }}
          </span>
        </div>
        <div v-else-if="task.progress.speed > 0" class="flex items-center mt-1">
          <span class="text-xs font-mono text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-1.5 py-0.5 rounded">
            {{ formatSpeed(task.progress.speed) }}
          </span>
        </div>
        
        <button 
          @click="handleClose"
          class="absolute top-4 right-4 p-1.5 rounded-lg text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-200 hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Sub-Tasks List -->
      <div class="flex-1 overflow-y-auto p-4 space-y-4 minimal-scrollbar">
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
              <!-- Icon based on subtask status -->
              <Clock v-if="sub.status === 'Pending'" class="w-3.5 h-3.5 text-neutral-400 shrink-0" />
              <Loader2 v-else-if="sub.status === 'Running'" class="w-3.5 h-3.5 text-emerald-400 animate-spin shrink-0" />
              <CheckCircle v-else-if="sub.status === 'Completed'" class="w-3.5 h-3.5 text-emerald-500 shrink-0" />
              <XCircle v-else-if="sub.status === 'Failed'" class="w-3.5 h-3.5 text-red-500 shrink-0" />
              <ArrowRight v-else class="w-3.5 h-3.5 text-neutral-400 shrink-0" />
              
              <span class="truncate">{{ getSubTaskName(sub) }}</span>
            </span>
            <span class="text-xs font-mono text-neutral-500 shrink-0" v-if="sub.status === 'Running' || sub.status === 'Completed'">
              <span v-if="sub.status === 'Running' && sub.current === 0 && sub.total === 100" class="text-emerald-500 animate-pulse">
                {{ $t('task.installing') }}
              </span>
              <span v-else>
                {{ getSubTaskPercentage(sub) }}%
              </span>
            </span>
          </div>
          
          <!-- Progress bar (only for running tasks) -->
          <div v-if="sub.status === 'Running'" class="w-full bg-black/10 dark:bg-white/10 rounded-full h-1 overflow-hidden">
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
  </Transition>
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
