<script setup lang="ts">
import { inject } from "vue";
import { DROPDOWN_CLOSE_KEY } from "./shared";

const props = defineProps<{
  destructive?: boolean;
  disabled?: boolean;
}>();

const close = inject(DROPDOWN_CLOSE_KEY, () => {});

function handleClick() {
  if (props.disabled) return;
  close();
}
</script>

<template>
  <button
    class="flex w-full cursor-pointer select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground disabled:pointer-events-none disabled:opacity-50"
    :class="destructive ? 'text-red-600 hover:bg-red-100 hover:text-red-700 focus:bg-red-100 focus:text-red-700 dark:text-red-400 dark:hover:bg-red-950 dark:hover:text-red-300' : ''"
    :disabled="disabled"
    @click="handleClick"
  >
    <slot />
  </button>
</template>
