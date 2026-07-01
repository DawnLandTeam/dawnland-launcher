<script setup lang="ts">
import { ref, computed } from 'vue';
import type { Component } from 'vue';
import { onClickOutside } from '@vueuse/core';
import { ChevronDown, Check, Search } from '@lucide/vue';

export interface SelectOption {
  label: string;
  value: string | number;
  disabled?: boolean;
  group?: string;
  icon?: Component;
}

const props = defineProps<{
  options: SelectOption[];
  modelValue: (string | number)[];
  placeholder?: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: (string | number)[]): void;
}>();

const isOpen = ref(false);
const targetRef = ref(null);
const searchQuery = ref('');

onClickOutside(targetRef, () => {
  isOpen.value = false;
});

const toggleOpen = () => {
  if (props.disabled) return;
  isOpen.value = !isOpen.value;
  if (isOpen.value) {
    searchQuery.value = ''; // clear search on open
  }
};

const toggleOption = (option: SelectOption) => {
  if (option.disabled) return;
  const newSet = new Set(props.modelValue);
  if (newSet.has(option.value)) {
    newSet.delete(option.value);
  } else {
    newSet.add(option.value);
  }
  emit('update:modelValue', Array.from(newSet));
};

const filteredOptions = computed(() => {
  if (!searchQuery.value) return props.options;
  const lowerQuery = searchQuery.value.toLowerCase();
  return props.options.filter(o => o.label.toLowerCase().includes(lowerQuery));
});

const groupedOptions = computed(() => {
  const groups: { name: string; options: SelectOption[] }[] = [];
  const ungrouped: SelectOption[] = [];
  const groupMap = new Map<string, SelectOption[]>();
  
  filteredOptions.value.forEach(opt => {
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

const selectedLabels = computed(() => {
  if (props.modelValue.length === 0) return props.placeholder || '请选择';
  return props.modelValue
    .map(val => props.options.find(o => o.value === val)?.label)
    .filter(Boolean)
    .join(', ');
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
      <span class="truncate flex-1 min-w-0 text-left block w-full pr-2" :title="selectedLabels">{{ selectedLabels }}</span>
      <ChevronDown class="h-4 w-4 opacity-50 shrink-0" />
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
        class="absolute z-50 mt-1 max-h-80 min-w-full w-max left-1/2 -translate-x-1/2 max-w-[90vw] sm:max-w-md overflow-hidden flex flex-col rounded-md border border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-950 p-1 text-base shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none sm:text-sm"
      >
        <!-- Search Input -->
        <div class="p-1 pb-2 border-b border-neutral-100 dark:border-zinc-800 shrink-0">
          <div class="relative flex items-center">
            <Search class="absolute left-2.5 h-4 w-4 text-neutral-500 dark:text-zinc-400" />
            <input
              type="text"
              v-model="searchQuery"
              :placeholder="$t('common.searchPlaceholder')"
              class="flex h-8 w-full rounded-md border border-neutral-300 dark:border-zinc-700 bg-transparent px-3 py-1 pl-9 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-emerald-500"
            />
          </div>
        </div>

        <div class="overflow-y-auto flex-1 pt-1">
          <div v-for="(group, idx) in groupedOptions" :key="idx">
            <div v-if="group.name" class="px-2 py-1.5 text-xs font-semibold text-neutral-500 dark:text-zinc-400">
              {{ group.name }}
            </div>
            
            <div
              v-for="option in group.options"
              :key="option.value"
              @click="toggleOption(option)"
              role="option"
              :aria-selected="modelValue.includes(option.value)"
              :aria-disabled="option.disabled ? 'true' : undefined"
              class="relative flex w-full cursor-default select-none items-center rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none transition-colors"
              :class="[
                option.disabled 
                  ? 'opacity-50 cursor-not-allowed text-neutral-500 dark:text-zinc-500' 
                  : 'cursor-pointer hover:bg-neutral-100 hover:text-neutral-900 dark:hover:bg-zinc-800 dark:hover:text-zinc-50 text-neutral-900 dark:text-zinc-100',
              ]"
            >
              <span class="absolute left-2 flex h-3.5 w-3.5 items-center justify-center">
                <Check v-if="modelValue.includes(option.value)" class="h-4 w-4 text-emerald-600 dark:text-emerald-500" />
              </span>
              <component v-if="option.icon" :is="option.icon" class="h-4 w-4 shrink-0 opacity-70 mr-2" />
              <span class="truncate" :title="option.label">{{ option.label }}</span>
            </div>
          </div>
          <div v-if="filteredOptions.length === 0" class="py-6 text-center text-sm text-neutral-500">
            无匹配选项
          </div>
        </div>
      </div>
    </transition>
  </div>
</template>
