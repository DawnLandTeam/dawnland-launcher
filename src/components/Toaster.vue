<script setup lang="ts">
import { notificationStore } from '../composables/useNotificationStore';
import { X, CheckCircle2, AlertCircle, Info } from '@lucide/vue';
import { computed } from 'vue';

const popups = computed(() => notificationStore.notifications.value.filter(n => n.isPopup));
</script>

<template>
  <!-- Fixed container for toasts, high z-index to stay above everything -->
  <div class="fixed top-12 right-4 z-[9999] flex flex-col gap-2 w-full max-w-sm pointer-events-none">
    <TransitionGroup 
      enter-active-class="transition-all duration-300 ease-out"
      enter-from-class="opacity-0 -translate-y-4 scale-95"
      enter-to-class="opacity-100 translate-y-0 scale-100"
      leave-active-class="transition-all duration-200 ease-in"
      leave-from-class="opacity-100 scale-100"
      leave-to-class="opacity-0 scale-95"
    >
      <div 
        v-for="toast in popups" 
        :key="toast.id"
        class="pointer-events-auto flex items-start gap-3 p-4 rounded-xl shadow-lg border backdrop-blur-md"
        :class="[
          toast.type === 'success' ? 'bg-emerald-500/10 border-emerald-500/20 text-emerald-600 dark:text-emerald-400' :
          toast.type === 'error' ? 'bg-red-500/10 border-red-500/20 text-red-600 dark:text-red-400' :
          'bg-white/80 border-black/5 dark:bg-black/80 dark:border-white/10 text-neutral-800 dark:text-neutral-200'
        ]"
      >
        <!-- Icon -->
        <div class="shrink-0 mt-0.5">
          <CheckCircle2 v-if="toast.type === 'success'" class="w-5 h-5" />
          <AlertCircle v-else-if="toast.type === 'error'" class="w-5 h-5" />
          <Info v-else class="w-5 h-5" />
        </div>

        <!-- Content -->
        <div class="flex-1 min-w-0 break-words">
          <h4 class="text-sm font-semibold">{{ toast.title }}</h4>
          <p v-if="toast.description" class="text-xs opacity-80 mt-1">
            {{ toast.description }}
          </p>
        </div>

        <!-- Close button -->
        <button 
          @click="notificationStore.dismissPopup(toast.id)"
          class="shrink-0 p-1 -mr-2 -mt-1 rounded-md opacity-50 hover:opacity-100 hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>
