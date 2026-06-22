<script setup lang="ts">
import { useAttrs } from 'vue';

defineProps<{
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost' | 'outline';
  size?: 'sm' | 'md' | 'lg' | 'icon';
  disabled?: boolean;
}>();

const attrs = useAttrs();
</script>

<template>
  <button
    :disabled="disabled"
    v-bind="attrs"
    class="inline-flex items-center justify-center rounded-md text-sm font-medium transition-colors bg-emerald-600 hover:bg-emerald-700 text-white shadow-sm disabled:opacity-50 disabled:cursor-not-allowed h-10 px-4 py-2"
  >
    <slot></slot>
  </button>
</template>
