<script setup lang="ts">
import { X } from "@lucide/vue";

defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
}>();

function onBackdropClick() {
  emit("update:open", false);
}
</script>

<template>
  <Teleport to="body">
    <Transition name="alert-dialog">
      <div v-if="open" class="fixed inset-0 z-50 flex items-center justify-center">
        <!-- Backdrop -->
        <div
          class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"
          @click="onBackdropClick"
        />
        <!-- Panel -->
        <div
          class="relative z-10 w-full max-w-md border bg-white dark:bg-zinc-900 p-4 shadow-xl rounded-lg pointer-events-auto"
        >
          <!-- Close button -->
          <button
            class="absolute right-4 top-4 rounded-sm opacity-70 transition-opacity hover:opacity-100 text-neutral-900 dark:text-white"
            @click="emit('update:open', false)"
          >
            <X class="h-4 w-4" />
            <span class="sr-only">Close</span>
          </button>

          <slot />
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.alert-dialog-enter-active,
.alert-dialog-leave-active {
  transition: opacity 150ms ease;
}
.alert-dialog-enter-from,
.alert-dialog-leave-to {
  opacity: 0;
}
.alert-dialog-enter-active .relative,
.alert-dialog-leave-active .relative {
  transition: transform 150ms ease, opacity 150ms ease;
}
.alert-dialog-enter-from .relative {
  transform: scale(0.95);
  opacity: 0;
}
.alert-dialog-leave-to .relative {
  transform: scale(0.95);
  opacity: 0;
}
</style>
