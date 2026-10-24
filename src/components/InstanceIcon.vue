<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import defaultIcon from "../assets/minecraft-icon.png";

/**
 * Displays an instance icon cached on disk, falling back to the default
 * Minecraft icon when no icon is available or it fails to load.
 * Size and shape are controlled by the caller through `class`.
 */
const props = withDefaults(
  defineProps<{
    iconPath?: string | null;
    alt?: string;
  }>(),
  { iconPath: null, alt: "" },
);

const loadFailed = ref(false);

watch(
  () => props.iconPath,
  () => {
    loadFailed.value = false;
  },
);

const customSrc = computed(() =>
  props.iconPath && !loadFailed.value ? convertFileSrc(props.iconPath) : null,
);
</script>

<template>
  <img
    v-if="customSrc"
    :src="customSrc"
    :alt="alt"
    class="object-cover select-none"
    draggable="false"
    @error="loadFailed = true"
  />
  <img
    v-else
    :src="defaultIcon"
    :alt="alt"
    class="object-cover select-none"
    draggable="false"
  />
</template>
