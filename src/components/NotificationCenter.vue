<script setup lang="ts">
import { notificationStore } from '../composables/useNotificationStore';
import { onClickOutside } from '@vueuse/core';
import { Trash2, X, CheckCircle2, AlertCircle, Info } from '@lucide/vue';
import { ref, computed } from 'vue';

const notifications = computed(() => notificationStore.notifications.value);
const canClear = computed(() => notifications.value.some(n => n.status === 'read'));
const centerRef = ref(null);

onClickOutside(centerRef, () => {
  if (notificationStore.isCenterOpen.value) {
    notificationStore.toggleCenter();
  }
}, {
  ignore: ['.notification-toggle']
});

const formatTime = (ts: number) => {
  const d = new Date(ts);
  return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
};
</script>

<template>
  <Transition
    enter-active-class="transition-all duration-300 ease-out"
    enter-from-class="opacity-0 -translate-y-4 scale-95"
    enter-to-class="opacity-100 translate-y-0 scale-100"
    leave-active-class="transition-all duration-200 ease-in"
    leave-from-class="opacity-100 translate-y-0 scale-100"
    leave-to-class="opacity-0 -translate-y-4 scale-95"
  >
    <div 
      v-if="notificationStore.isCenterOpen.value"
      ref="centerRef"
      class="fixed right-4 top-10 w-80 max-h-[60vh] flex flex-col rounded-2xl bg-white/60 dark:bg-black/40 backdrop-blur-xl border border-black/10 dark:border-white/10 shadow-2xl z-[100]"
    >
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-black/10 dark:border-white/10 shrink-0">
        <h3 class="font-semibold text-neutral-800 dark:text-neutral-200">通知中心</h3>
        <div class="flex items-center gap-2">
          <button 
            v-if="canClear"
            @click="notificationStore.clearExpired()"
            class="p-1.5 rounded-lg text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-200 hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
            title="清除已读通知"
          >
            <Trash2 class="w-4 h-4" />
          </button>
          <button 
            @click="notificationStore.toggleCenter()"
            class="p-1.5 rounded-lg text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-200 hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
          >
            <X class="w-4 h-4" />
          </button>
        </div>
      </div>

      <!-- List -->
      <div class="flex-1 overflow-y-auto p-2 pb-4 space-y-2 minimal-scrollbar">
        <div v-if="notifications.length === 0" class="py-8 text-center text-sm text-neutral-500 dark:text-neutral-400">
          暂无通知
        </div>
        <div 
          v-for="notif in notifications" 
          :key="notif.id"
          class="group relative flex items-start gap-3 p-3 rounded-xl bg-black/5 dark:bg-white/5 border border-black/10 dark:border-white/10 transition-all hover:bg-black/10 dark:hover:bg-white/10"
        >
          <!-- Unread indicator -->
          <div v-if="notif.status === 'unread'" class="absolute -top-1 -left-1 w-2.5 h-2.5 bg-red-500 rounded-full border-2 border-white dark:border-black"></div>

          <!-- Icon -->
          <div class="shrink-0 mt-0.5">
            <CheckCircle2 v-if="notif.type === 'success'" class="w-5 h-5 text-emerald-500" />
            <AlertCircle v-else-if="notif.type === 'error'" class="w-5 h-5 text-red-500" />
            <Info v-else class="w-5 h-5 text-blue-500" />
          </div>

          <!-- Content -->
          <div class="flex-1 min-w-0">
            <div class="flex justify-between items-start gap-2">
              <h4 class="text-sm font-medium text-neutral-800 dark:text-neutral-200 break-words pr-2 leading-tight">{{ notif.title }}</h4>
              <span class="text-[10px] text-neutral-400 shrink-0 mt-0.5">{{ formatTime(notif.timestamp) }}</span>
            </div>
            <p v-if="notif.description" class="text-xs text-neutral-500 dark:text-neutral-400 mt-1 break-words">
              {{ notif.description }}
            </p>
          </div>

          <!-- Dismiss button -->
          <button 
            @click="notificationStore.removeNotification(notif.id)"
            class="shrink-0 p-1 -mr-1 -mt-1 rounded-md opacity-0 group-hover:opacity-100 hover:bg-black/10 dark:hover:bg-white/10 text-neutral-400 transition-all absolute right-2 bottom-2 bg-white/50 dark:bg-black/50 backdrop-blur-sm"
            title="删除"
          >
            <X class="w-3.5 h-3.5" />
          </button>
        </div>
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
.group:hover .opacity-0 {
  opacity: 1 !important;
}
</style>
