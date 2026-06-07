<script setup lang="ts">
import { ref, provide, onMounted, onBeforeUnmount } from "vue";
import { DROPDOWN_OPEN_KEY, DROPDOWN_CLOSE_KEY } from "./shared";

const props = withDefaults(defineProps<{
  align?: "start" | "end" | "center";
}>(), {
  align: "start"
});

// --- DropdownMenu root ---
const open = ref(false);

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
}>();

function toggle() {
  open.value = !open.value;
  emit("update:open", open.value);
}

function close() {
  if (open.value) {
    open.value = false;
    emit("update:open", false);
  }
}

provide(DROPDOWN_OPEN_KEY, open);
provide(DROPDOWN_CLOSE_KEY, close);

// Close on outside click
const rootRef = ref<HTMLElement | null>(null);
function onDocClick(e: MouseEvent) {
  if (rootRef.value && !rootRef.value.contains(e.target as Node)) {
    close();
  }
}

onMounted(() => document.addEventListener("mousedown", onDocClick));
onBeforeUnmount(() => document.removeEventListener("mousedown", onDocClick));
</script>

<template>
  <div ref="rootRef" class="relative inline-block">
    <div @click="toggle">
      <slot name="trigger" />
    </div>
    <Transition name="dropdown">
      <div
        v-if="open"
        class="absolute z-50 mt-1 min-w-[10rem] overflow-hidden rounded-xl border border-white/20 dark:border-zinc-800/50 bg-white dark:bg-zinc-900 p-1 text-foreground shadow-xl"
        :class="[
          props.align === 'start' ? 'left-0' : '',
          props.align === 'end' ? 'right-0' : '',
          props.align === 'center' ? 'left-1/2 -translate-x-1/2' : ''
        ]"
      >
        <slot />
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.dropdown-enter-active,
.dropdown-leave-active {
  transition: opacity 100ms ease, transform 100ms ease;
}
.dropdown-enter-from,
.dropdown-leave-to {
  opacity: 0;
  transform: scaleY(0.95) translateY(-2px);
  transform-origin: top;
}
</style>
