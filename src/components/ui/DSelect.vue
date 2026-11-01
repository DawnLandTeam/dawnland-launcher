<script setup lang="ts">
import { ref, computed, watch, nextTick } from 'vue';
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
const targetRef = ref<HTMLElement | null>(null);
const searchInputRef = ref<HTMLInputElement | null>(null);
const searchQuery = ref('');

onClickOutside(targetRef, () => {
  isOpen.value = false;
});

watch(isOpen, async (open) => {
  if (open) {
    searchQuery.value = '';
    if (props.searchable) {
      await nextTick();
      searchInputRef.value?.focus();
    }
  } else {
    searchQuery.value = '';
  }
});

const toggleOpen = () => {
  if (props.disabled) return;
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
      type="button"
      @click="toggleOpen"
      :disabled="disabled"
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
        class="absolute z-50 mt-1 max-h-64 left-0 min-w-full w-full overflow-hidden flex flex-col rounded-md border border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-950 p-1 text-base shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none sm:text-sm"
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
                  : 'cursor-pointer hover:bg-neutral-100 hover:text-neutral-900 dark:hover:bg-zinc-800 dark:hover:text-zinc-50 text-neutral-900 dark:text-zinc-100',
                modelValue === option.value ? 'bg-neutral-100 dark:bg-zinc-800 font-medium text-emerald-600 dark:text-emerald-500' : ''
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
  </div>
</template>
