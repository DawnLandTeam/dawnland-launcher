<script setup lang="ts">
import { X } from "@lucide/vue";

const props = defineProps<{
  open?: boolean;
  class?: string;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
}>();

function close() {
  emit("update:open", false);
}
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
      <div v-if="open" class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
        <!-- Frosted glass backdrop -->
        <div 
          class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"
        />
        
        <!-- Content with solid background -->
        <div
          role="dialog"
          aria-modal="true"
          class="relative z-10 w-full max-w-2xl gap-4 border bg-white dark:bg-zinc-900 p-4 shadow-xl rounded-lg max-h-[85vh] overflow-y-auto pointer-events-auto"
          :class="props.class"
        >
          <slot />
          
          <button
            class="absolute right-4 top-4 rounded-sm opacity-70 hover:opacity-100 transition-opacity"
            @click="close"
          >
            <X class="h-4 w-4 text-neutral-900 dark:text-white" />
            <span class="sr-only">Close</span>
          </button>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
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