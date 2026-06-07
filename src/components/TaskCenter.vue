<script setup lang="ts">
import { computed, ref } from 'vue';
import { useTaskStore } from '../composables/useTaskStore';
import { onClickOutside } from '@vueuse/core';
import TaskItem from './TaskItem.vue';
import { Trash2, X } from '@lucide/vue';

const taskStore = useTaskStore();
const tasks = computed(() => taskStore.tasks.value);

const canClear = computed(() => {
  return tasks.value.some(t => ['Completed', 'Failed', 'Cancelled'].includes(t.status));
});

const taskCenterRef = ref(null);

onClickOutside(taskCenterRef, () => {
  if (taskStore.isTaskCenterOpen.value) {
    taskStore.toggleTaskCenter();
  }
}, {
  ignore: ['.task-center-toggle']
});
</script>

<template>
  <Transition
    enter-active-class="transition-all duration-300 ease-out"
    enter-from-class="opacity-0 translate-y-4 scale-95"
    enter-to-class="opacity-100 translate-y-0 scale-100"
    leave-active-class="transition-all duration-200 ease-in"
    leave-from-class="opacity-100 translate-y-0 scale-100"
    leave-to-class="opacity-0 translate-y-4 scale-95"
  >
    <div 
      v-if="taskStore.isTaskCenterOpen.value"
      ref="taskCenterRef"
      class="fixed left-[72px] bottom-4 w-80 max-h-[60vh] flex flex-col rounded-2xl bg-white/60 dark:bg-black/40 backdrop-blur-xl border border-black/10 dark:border-white/10 shadow-2xl z-[100]"
    >
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-black/10 dark:border-white/10 shrink-0">
        <h3 class="font-semibold text-neutral-800 dark:text-neutral-200">Task Center</h3>
        <div class="flex items-center gap-2">
          <button 
            v-if="canClear"
            @click="taskStore.clearHistory()"
            class="p-1.5 rounded-lg text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-200 hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
            title="Clear History"
          >
            <Trash2 class="w-4 h-4" />
          </button>
          <button 
            @click="taskStore.toggleTaskCenter()"
            class="p-1.5 rounded-lg text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-200 hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
          >
            <X class="w-4 h-4" />
          </button>
        </div>
      </div>

      <!-- Task List -->
      <div class="flex-1 overflow-y-auto p-2 space-y-2 minimal-scrollbar">
        <div v-if="tasks.length === 0" class="py-8 text-center text-sm text-neutral-500 dark:text-neutral-400">
          No task history
        </div>
        <TaskItem 
          v-for="task in tasks" 
          :key="task.id" 
          :task="task" 
        />
      </div>
    </div>
  </Transition>
</template>

<style scoped>
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
