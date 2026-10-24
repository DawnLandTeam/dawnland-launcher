<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import defaultIcon from "../assets/minecraft-icon.png";
import { getIconPathRevision } from "../composables/useInstanceIconUpdates";

/**
 * Displays an instance icon cached on disk, falling back to the default
 * Minecraft icon when no icon is available or it fails to load.
 * Renders a single root `<img>` so caller-supplied classes (width, height,
 * border-radius, shrink, etc.) always fall through cleanly.
 */
const props = withDefaults(
  defineProps<{
    iconPath?: string | null;
    alt?: string;
  }>(),
  { iconPath: null, alt: "" },
);

const loadFailed = ref(false);

const revision = computed(() => getIconPathRevision(props.iconPath));

watch(
  [() => props.iconPath, revision],
  () => {
    loadFailed.value = false;
  },
);

const customSrc = computed(() => {
  if (!props.iconPath || loadFailed.value) return null;
  const base = convertFileSrc(props.iconPath);
  const rev = revision.value;
  if (rev === undefined) return base;
  const sep = base.includes("?") ? "&" : "?";
  return `${base}${sep}v=${rev}`;
});

const resolvedSrc = computed(() => customSrc.value ?? defaultIcon);

function handleError() {
  if (customSrc.value) {
    loadFailed.value = true;
  }
}
</script>

<template>
  <img
    :src="resolvedSrc"
    :alt="alt"
    class="object-cover select-none"
    draggable="false"
    @error="handleError"
  />
</template>
