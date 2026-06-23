<script setup lang="ts">
import { useAttrs } from 'vue';

defineProps<{
  modelValue?: string | number;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const attrs = useAttrs();

const handleInput = (event: Event) => {
  const target = event.target as HTMLInputElement;
  emit('update:modelValue', target.value);
};
</script>

<template>
  <input
    :value="modelValue"
    @input="handleInput"
    v-bind="attrs"
    class="flex h-10 w-full rounded-md px-3 py-2 text-sm bg-white dark:bg-zinc-900 border border-neutral-300 dark:border-zinc-700 text-neutral-900 dark:text-zinc-100 placeholder:text-neutral-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-emerald-500 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
  />
</template>
