<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from 'vue';

export interface TooltipProps {
  content?: string;
  placement?: 'top' | 'bottom' | 'left' | 'right';
  delay?: number;
  disabled?: boolean;
  maxWidth?: string;
}

const props = withDefaults(defineProps<TooltipProps>(), {
  content: '',
  placement: 'top',
  delay: 100,
  disabled: false,
  maxWidth: 'max-w-xs',
});

const isVisible = ref(false);
const isPositioned = ref(false);
const triggerRef = ref<HTMLElement | null>(null);
const tooltipRef = ref<HTMLElement | null>(null);
const coords = ref({ top: 0, left: 0 });

let timer: ReturnType<typeof setTimeout> | undefined;

const calculatePosition = () => {
  if (!triggerRef.value || !tooltipRef.value) return;

  const triggerRect = triggerRef.value.getBoundingClientRect();
  const tooltipRect = tooltipRef.value.getBoundingClientRect();
  const offset = 8;
  const viewportPadding = 8;

  const winWidth = typeof window !== 'undefined' ? (window.innerWidth || 1024) : 1024;
  const winHeight = typeof window !== 'undefined' ? (window.innerHeight || 768) : 768;

  let top = 0;
  let left = 0;
  let activePlacement = props.placement;

  // Decide position based on preferred placement with viewport bounds fallback
  if (activePlacement === 'top') {
    top = triggerRect.top - tooltipRect.height - offset;
    left = triggerRect.left + (triggerRect.width - tooltipRect.width) / 2;

    // Flip to bottom if clipping top
    if (top < viewportPadding) {
      activePlacement = 'bottom';
      top = triggerRect.bottom + offset;
    }
  } else if (activePlacement === 'bottom') {
    top = triggerRect.bottom + offset;
    left = triggerRect.left + (triggerRect.width - tooltipRect.width) / 2;

    // Flip to top if clipping bottom
    if (top + tooltipRect.height > winHeight - viewportPadding) {
      top = triggerRect.top - tooltipRect.height - offset;
    }
  } else if (activePlacement === 'left') {
    left = triggerRect.left - tooltipRect.width - offset;
    top = triggerRect.top + (triggerRect.height - tooltipRect.height) / 2;

    if (left < viewportPadding) {
      left = triggerRect.right + offset;
    }
  } else if (activePlacement === 'right') {
    left = triggerRect.right + offset;
    top = triggerRect.top + (triggerRect.height - tooltipRect.height) / 2;

    if (left + tooltipRect.width > winWidth - viewportPadding) {
      left = triggerRect.left - tooltipRect.width - offset;
    }
  }

  // Clamp horizontal coordinate inside viewport
  left = Math.max(viewportPadding, Math.min(left, winWidth - tooltipRect.width - viewportPadding));
  // Clamp vertical coordinate inside viewport
  top = Math.max(viewportPadding, Math.min(top, winHeight - tooltipRect.height - viewportPadding));

  coords.value = {
    top: Math.round(top),
    left: Math.round(left),
  };
};

const show = () => {
  if (props.disabled) return;
  if (timer) clearTimeout(timer);

  timer = setTimeout(async () => {
    isVisible.value = true;
    isPositioned.value = false;
    await nextTick();
    calculatePosition();
    isPositioned.value = true;
  }, props.delay);
};

const hide = () => {
  if (timer) {
    clearTimeout(timer);
    timer = undefined;
  }
  isVisible.value = false;
  isPositioned.value = false;
};

const handleWindowUpdate = () => {
  if (isVisible.value) {
    calculatePosition();
  }
};

onMounted(() => {
  window.addEventListener('resize', handleWindowUpdate, { passive: true });
  window.addEventListener('scroll', handleWindowUpdate, { capture: true, passive: true });
});

onUnmounted(() => {
  if (timer) clearTimeout(timer);
  window.removeEventListener('resize', handleWindowUpdate);
  window.removeEventListener('scroll', handleWindowUpdate, true);
});
</script>

<template>
  <span
    ref="triggerRef"
    class="inline-flex items-center"
    @mouseenter="show"
    @mouseleave="hide"
    @focusin="show"
    @focusout="hide"
  >
    <slot />
  </span>

  <Teleport to="body">
    <transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="opacity-0 scale-95"
      enter-to-class="opacity-100 scale-100"
      leave-active-class="transition duration-100 ease-in"
      leave-from-class="opacity-100 scale-100"
      leave-to-class="opacity-0 scale-95"
    >
      <div
        v-if="isVisible"
        ref="tooltipRef"
        role="tooltip"
        :class="[
          'fixed z-[9999] pointer-events-none rounded-lg px-3 py-2 text-xs font-normal leading-relaxed text-zinc-100 dark:text-zinc-100 bg-zinc-900/95 dark:bg-zinc-800/95 backdrop-blur-md border border-white/10 dark:border-white/10 shadow-xl break-words',
          maxWidth,
          isPositioned ? 'opacity-100' : 'opacity-0'
        ]"
        :style="{
          top: `${coords.top}px`,
          left: `${coords.left}px`,
        }"
      >
        <slot name="content">
          {{ content }}
        </slot>
      </div>
    </transition>
  </Teleport>
</template>
