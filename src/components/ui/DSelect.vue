<script setup lang="ts">
import { ref, computed, watch, nextTick, onUnmounted } from 'vue';
import type { Component } from 'vue';
import { onClickOutside } from '@vueuse/core';
import { ChevronDown, Search } from '@lucide/vue';
import InstanceIcon from '../InstanceIcon.vue';

export interface SelectOption {
  label: string;
  value: string | number;
  disabled?: boolean;
  group?: string;
  icon?: Component;
  iconPath?: string | null;
  showInstanceIcon?: boolean;
  sublabel?: string;
}

interface DropdownStyles {
  top?: string;
  bottom?: string;
  left: string;
  width?: string;
  maxHeight: string;
}

const DROPDOWN_MAX_HEIGHT = 256;
const DROPDOWN_OFFSET = 4;
const VIEWPORT_PADDING = 8;

const props = defineProps<{
  options: SelectOption[];
  modelValue?: string | number | null;
  placeholder?: string;
  disabled?: boolean;
  searchable?: boolean;
  searchPlaceholder?: string;
  emptyText?: string;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | number): void;
}>();

const isOpen = ref(false);
const openUpward = ref(false);
const targetRef = ref<HTMLElement | null>(null);
const triggerRef = ref<HTMLButtonElement | null>(null);
const dropdownRef = ref<HTMLElement | null>(null);
const searchInputRef = ref<HTMLInputElement | null>(null);
const searchQuery = ref('');
const dropdownStyles = ref<DropdownStyles>({
  top: '0px',
  left: '0px',
  maxHeight: `${DROPDOWN_MAX_HEIGHT}px`,
});

onClickOutside(
  targetRef,
  () => {
    isOpen.value = false;
  },
  {
    ignore: [dropdownRef],
  },
);

const updatePosition = () => {
  const anchor = triggerRef.value ?? targetRef.value;
  if (!anchor) return;

  const rect = anchor.getBoundingClientRect();
  const winWidth = typeof window !== 'undefined' ? window.innerWidth || 1024 : 1024;
  const winHeight = typeof window !== 'undefined' ? window.innerHeight || 768 : 768;

  const spaceBelow = Math.max(0, winHeight - rect.bottom - DROPDOWN_OFFSET - VIEWPORT_PADDING);
  const spaceAbove = Math.max(0, rect.top - DROPDOWN_OFFSET - VIEWPORT_PADDING);

  const shouldOpenUpward = spaceBelow < DROPDOWN_MAX_HEIGHT && spaceAbove > spaceBelow;
  openUpward.value = shouldOpenUpward;

  const availableVertical = shouldOpenUpward ? spaceAbove : spaceBelow;
  const maxHeight = Math.min(DROPDOWN_MAX_HEIGHT, Math.max(48, availableVertical));

  const maxViewportWidth = Math.max(0, winWidth - VIEWPORT_PADDING * 2);
  const measuredWidth = rect.width > 0 ? Math.min(rect.width, maxViewportWidth) : 0;
  const clampedLeft =
    measuredWidth > 0
      ? Math.max(VIEWPORT_PADDING, Math.min(rect.left, winWidth - measuredWidth - VIEWPORT_PADDING))
      : Math.max(VIEWPORT_PADDING, rect.left);

  dropdownStyles.value = {
    top: shouldOpenUpward ? undefined : `${Math.round(rect.bottom + DROPDOWN_OFFSET)}px`,
    bottom: shouldOpenUpward ? `${Math.round(winHeight - rect.top + DROPDOWN_OFFSET)}px` : undefined,
    left: `${Math.round(clampedLeft)}px`,
    width: measuredWidth > 0 ? `${Math.round(measuredWidth)}px` : undefined,
    maxHeight: `${Math.round(maxHeight)}px`,
  };
};

const handleWindowScroll = (event: Event) => {
  if (!isOpen.value) return;
  if (dropdownRef.value && event.target instanceof Node && dropdownRef.value.contains(event.target)) {
    return;
  }
  updatePosition();
};

const attachListeners = () => {
  if (typeof window === 'undefined') return;
  window.addEventListener('resize', updatePosition, { passive: true });
  window.addEventListener('scroll', handleWindowScroll, { capture: true, passive: true });
};

const detachListeners = () => {
  if (typeof window === 'undefined') return;
  window.removeEventListener('resize', updatePosition);
  window.removeEventListener('scroll', handleWindowScroll, true);
};

watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) {
      isOpen.value = false;
    }
  },
);

watch(isOpen, async (open) => {
  if (open) {
    searchQuery.value = '';
    updatePosition();
    attachListeners();
    await nextTick();
    updatePosition();
    if (props.searchable) {
      searchInputRef.value?.focus({ preventScroll: true });
    }
  } else {
    detachListeners();
    searchQuery.value = '';
  }
});

onUnmounted(() => {
  detachListeners();
});

const toggleOpen = () => {
  if (props.disabled) return;
  if (!isOpen.value) {
    updatePosition();
  }
  isOpen.value = !isOpen.value;
};

const selectOption = (option: SelectOption) => {
  if (option.disabled) return;
  emit('update:modelValue', option.value);
  isOpen.value = false;
};

const filteredOptions = computed(() => {
  if (!props.searchable || !searchQuery.value.trim()) {
    return props.options;
  }
  const q = searchQuery.value.trim().toLowerCase();
  return props.options.filter(
    (opt) =>
      opt.label.toLowerCase().includes(q) ||
      (opt.sublabel && opt.sublabel.toLowerCase().includes(q)) ||
      String(opt.value).toLowerCase().includes(q),
  );
});

const groupedOptions = computed(() => {
  const groups: { name: string; options: SelectOption[] }[] = [];
  const ungrouped: SelectOption[] = [];

  const groupMap = new Map<string, SelectOption[]>();

  filteredOptions.value.forEach((opt) => {
    if (opt.group) {
      if (!groupMap.has(opt.group)) {
        groupMap.set(opt.group, []);
      }
      groupMap.get(opt.group)!.push(opt);
    } else {
      ungrouped.push(opt);
    }
  });

  if (ungrouped.length > 0) {
    groups.push({ name: '', options: ungrouped });
  }

  groupMap.forEach((opts, name) => {
    groups.push({ name, options: opts });
  });

  return groups;
});

const selectedOption = computed(() =>
  props.options.find((o) => o.value === props.modelValue),
);

const selectedLabel = computed(() => {
  return selectedOption.value
    ? selectedOption.value.label
    : props.placeholder || 'Please select...';
});

const selectedIcon = computed(() => {
  return selectedOption.value ? selectedOption.value.icon : undefined;
});
</script>

<template>
  <div class="relative" ref="targetRef">
    <button
      ref="triggerRef"
      type="button"
      @click="toggleOpen"
      :disabled="disabled"
      :aria-expanded="isOpen"
      class="flex h-10 w-full items-center justify-between rounded-md border border-neutral-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm text-neutral-900 dark:text-zinc-100 placeholder:text-neutral-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-emerald-500 disabled:cursor-not-allowed disabled:opacity-50 transition-colors"
    >
      <div class="flex items-center gap-2 flex-1 min-w-0" :title="selectedLabel">
        <InstanceIcon
          v-if="selectedOption && (selectedOption.showInstanceIcon || selectedOption.iconPath !== undefined)"
          :icon-path="selectedOption.iconPath"
          :alt="selectedOption.label"
          class="h-5 w-5 rounded shrink-0"
        />
        <component v-else-if="selectedIcon" :is="selectedIcon" class="h-4 w-4 shrink-0 opacity-70" />
        <span class="truncate text-left font-medium">{{ selectedLabel }}</span>
        <span
          v-if="selectedOption?.sublabel"
          class="text-xs text-neutral-400 dark:text-zinc-500 truncate shrink-0"
        >
          ({{ selectedOption.sublabel }})
        </span>
      </div>
      <ChevronDown class="h-4 w-4 opacity-50 shrink-0 ml-2" />
    </button>

    <Teleport to="body">
      <transition
        enter-active-class="transition duration-100 ease-out"
        enter-from-class="transform scale-95 opacity-0"
        enter-to-class="transform scale-100 opacity-100"
        leave-active-class="transition duration-75 ease-in"
        leave-from-class="transform scale-100 opacity-100"
        leave-to-class="transform scale-95 opacity-0"
      >
        <div
          v-if="isOpen"
          ref="dropdownRef"
          data-dselect-dropdown
          :style="dropdownStyles"
          :class="[
            'fixed z-[300] min-w-[8rem] overflow-hidden flex flex-col rounded-md border border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-950 p-1 text-base shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none sm:text-sm',
            openUpward ? 'origin-bottom' : 'origin-top',
          ]"
        >
          <!-- Optional Search Input -->
          <div
            v-if="searchable"
            class="flex items-center gap-2 px-2.5 py-1.5 mb-1 border-b border-neutral-100 dark:border-zinc-800/80"
            @click.stop
          >
            <Search class="h-3.5 w-3.5 text-neutral-400 dark:text-zinc-500 shrink-0" />
            <input
              ref="searchInputRef"
              v-model="searchQuery"
              type="text"
              :placeholder="searchPlaceholder || 'Search...'"
              class="w-full bg-transparent text-xs text-neutral-900 dark:text-zinc-100 placeholder:text-neutral-400 dark:placeholder:text-zinc-500 focus:outline-none"
            />
          </div>

          <div class="overflow-y-auto overflow-x-hidden custom-scrollbar flex-1 w-full">
            <div
              v-if="groupedOptions.length === 0"
              class="px-3 py-4 text-center text-xs text-neutral-400 dark:text-zinc-500"
            >
              {{ emptyText || 'No matching options' }}
            </div>
            <div v-for="(group, idx) in groupedOptions" :key="idx">
              <div v-if="group.name" class="px-2 py-1.5 text-xs font-semibold text-neutral-500 dark:text-zinc-400">
                {{ group.name }}
              </div>

              <div
                v-for="option in group.options"
                :key="option.value"
                @click="selectOption(option)"
                role="option"
                :aria-selected="modelValue === option.value"
                :aria-disabled="option.disabled ? 'true' : undefined"
                class="relative flex w-full cursor-default select-none items-center gap-2.5 rounded-sm py-1.5 px-2.5 text-sm outline-none transition-colors"
                :class="[
                  option.disabled
                    ? 'opacity-50 cursor-not-allowed text-neutral-500 dark:text-zinc-500'
                    : modelValue === option.value
                      ? 'cursor-pointer bg-neutral-100 dark:bg-zinc-800 font-medium text-emerald-600 dark:text-emerald-500'
                      : 'cursor-pointer hover:bg-neutral-100 hover:text-neutral-900 dark:hover:bg-zinc-800 dark:hover:text-zinc-50 text-neutral-900 dark:text-zinc-100',
                ]"
              >
                <InstanceIcon
                  v-if="option.showInstanceIcon || option.iconPath !== undefined"
                  :icon-path="option.iconPath"
                  :alt="option.label"
                  class="h-5 w-5 rounded shrink-0"
                />
                <component v-else-if="option.icon" :is="option.icon" class="h-4 w-4 shrink-0 opacity-70" />
                <span class="truncate flex-1" :title="option.label">{{ option.label }}</span>
                <span
                  v-if="option.sublabel"
                  class="text-[11px] text-neutral-400 dark:text-zinc-500 shrink-0 font-normal"
                >
                  {{ option.sublabel }}
                </span>
              </div>
            </div>
          </div>
          <slot name="append"></slot>
        </div>
      </transition>
    </Teleport>
  </div>
</template>
