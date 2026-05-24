<script setup lang="ts">
const props = defineProps<{
  class?: string;
}>();
</script>

<template>
  <p :class="['text-sm text-muted-foreground', props.class]">
    <slot />
  </p>
</template>
