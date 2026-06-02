<script setup lang="ts">
const props = defineProps<{
  class?: string;
}>();
</script>

<template>
  <h3 :class="['text-lg font-semibold', props.class]">
    <slot />
  </h3>
</template>
