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
          @click="close"
        />
        
        <!-- Content with solid background -->
        <div
          class="relative z-10 w-full max-w-2xl gap-4 border bg-white dark:bg-zinc-900 p-6 shadow-xl rounded-lg max-h-[85vh] overflow-y-auto pointer-events-auto"
          :class="props.class"
        >
          <slot />
          
          <button
            class="absolute right-4 top-4 rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
            @click="close"
          >
            <X class="h-4 w-4" />
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