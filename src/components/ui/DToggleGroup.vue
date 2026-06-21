<script setup lang="ts">
export interface ToggleOption {
  label: string;
  value: string | number;
}

const props = defineProps<{
  options: ToggleOption[];
  modelValue?: string | number;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | number): void;
}>();

const selectOption = (value: string | number) => {
  emit('update:modelValue', value);
};
</script>

<template>
  <div class="flex p-1 bg-gray-100 dark:bg-gray-900 rounded-lg w-fit shrink-0">
    <button
      v-for="option in options"
      :key="option.value"
      @click="selectOption(option.value)"
      class="px-6 py-2 rounded-md text-sm font-medium transition-all"
      :class="modelValue === option.value 
        ? 'bg-white dark:bg-gray-800 text-emerald-600 shadow-sm' 
        : 'text-gray-500 hover:text-gray-700 dark:hover:text-gray-300'"
    >
      {{ option.label }}
    </button>
  </div>
</template>
