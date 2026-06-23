<script setup lang="ts">
import { ref, computed } from 'vue';
import type { Component } from 'vue';
import { onClickOutside } from '@vueuse/core';
import { ChevronDown } from '@lucide/vue';

export interface SelectOption {
  label: string;
  value: string | number;
  disabled?: boolean;
  group?: string;
  icon?: Component;
}

const props = defineProps<{
  options: SelectOption[];
  modelValue?: string | number | null;
  placeholder?: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | number): void;
}>();

const isOpen = ref(false);
const targetRef = ref(null);

onClickOutside(targetRef, () => {
  isOpen.value = false;
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

const groupedOptions = computed(() => {
  const groups: { name: string; options: SelectOption[] }[] = [];
  const ungrouped: SelectOption[] = [];
  
  const groupMap = new Map<string, SelectOption[]>();
  
  props.options.forEach(opt => {
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

const selectedLabel = computed(() => {
  const opt = props.options.find(o => o.value === props.modelValue);
  return opt ? opt.label : props.placeholder || '请选择';
});

const selectedIcon = computed(() => {
  const opt = props.options.find(o => o.value === props.modelValue);
  return opt ? opt.icon : undefined;
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
        <component v-if="selectedIcon" :is="selectedIcon" class="h-4 w-4 shrink-0 opacity-70" />
        <span class="truncate block w-full text-left">{{ selectedLabel }}</span>
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
        class="absolute z-50 mt-1 max-h-60 min-w-full w-max left-1/2 -translate-x-1/2 overflow-hidden flex flex-col rounded-md border border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-950 p-1 text-base shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none sm:text-sm max-w-[90vw] sm:max-w-md"
      >
        <div class="overflow-y-auto flex-1 w-full">
        <div v-for="(group, idx) in groupedOptions" :key="idx">
          <div v-if="group.name" class="px-2 py-1.5 text-xs font-semibold text-neutral-500 dark:text-zinc-400">
            {{ group.name }}
          </div>
          
          <div
            v-for="option in group.options"
            :key="option.value"
            @click="selectOption(option)"
            class="relative flex w-full cursor-default select-none items-center gap-2 rounded-sm py-1.5 pl-2 pr-8 text-sm outline-none transition-colors"
            :class="[
              option.disabled 
                ? 'opacity-50 cursor-not-allowed text-neutral-500 dark:text-zinc-500' 
                : 'cursor-pointer hover:bg-neutral-100 hover:text-neutral-900 dark:hover:bg-zinc-800 dark:hover:text-zinc-50 text-neutral-900 dark:text-zinc-100',
              modelValue === option.value ? 'bg-neutral-100 dark:bg-zinc-800 font-medium text-emerald-600 dark:text-emerald-500' : ''
            ]"
          >
            <component v-if="option.icon" :is="option.icon" class="h-4 w-4 shrink-0 opacity-70" />
            <span class="truncate" :title="option.label">{{ option.label }}</span>
          </div>
        </div>
        </div>
        <slot name="append"></slot>
      </div>
    </transition>
  </div>
</template>
