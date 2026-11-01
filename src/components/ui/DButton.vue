<script setup lang="ts">
import { computed, useAttrs } from 'vue';
import { cn } from '../../lib/utils';

const props = withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary' | 'danger' | 'ghost' | 'outline';
    size?: 'sm' | 'md' | 'lg' | 'icon';
    disabled?: boolean;
    class?: string;
  }>(),
  {
    variant: 'primary',
    size: 'md',
    disabled: false,
  }
);

const attrs = useAttrs();

const variantClasses = computed(() => {
  switch (props.variant) {
    case 'secondary':
      return 'bg-secondary hover:bg-secondary/80 text-secondary-foreground shadow-sm';
    case 'danger':
      return 'bg-red-600 hover:bg-red-700 text-white shadow-sm';
    case 'outline':
      return 'border border-border bg-transparent hover:bg-secondary text-foreground shadow-sm';
    case 'ghost':
      return 'hover:bg-secondary text-foreground';
    case 'primary':
    default:
      return 'bg-emerald-600 hover:bg-emerald-700 text-white shadow-sm';
  }
});

const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm':
      return 'h-8 rounded-md px-3 text-xs';
    case 'lg':
      return 'h-11 rounded-md px-8 text-base';
    case 'icon':
      return 'h-10 w-10 p-0';
    case 'md':
    default:
      return 'h-10 px-4 py-2 text-sm';
  }
});
</script>

<template>
  <button
    :disabled="disabled"
    v-bind="attrs"
    :class="cn(
      'inline-flex items-center justify-center rounded-md font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed',
      variantClasses,
      sizeClasses,
      props.class,
      attrs.class as string
    )"
  >
    <slot></slot>
  </button>
</template>
