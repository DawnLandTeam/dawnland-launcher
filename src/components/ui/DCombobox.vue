<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import type { Component } from 'vue';
import { onClickOutside } from '@vueuse/core';
import { ChevronDown } from '@lucide/vue';

export interface ComboboxOption {
  label: string;
  value: string | number;
  disabled?: boolean;
  group?: string;
  icon?: Component;
}

const props = defineProps<{
  options: ComboboxOption[];
  modelValue?: string | number | null;
  placeholder?: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | number): void;
  (e: 'blur'): void;
}>();

const isOpen = ref(false);
const targetRef = ref(null);
const inputValue = ref(props.modelValue ? String(props.modelValue) : '');

watch(() => props.modelValue, (newVal) => {
  inputValue.value = newVal ? String(newVal) : '';
});

onClickOutside(targetRef, () => {
  isOpen.value = false;
  emit('blur');
});

const toggleOpen = () => {
  if (props.disabled) return;
  isOpen.value = !isOpen.value;
};

const selectOption = (option: ComboboxOption) => {
  if (option.disabled) return;
  inputValue.value = String(option.value);
  emit('update:modelValue', option.value);
  isOpen.value = false;
  emit('blur');
};

const handleInput = (event: Event) => {
  const target = event.target as HTMLInputElement;
  inputValue.value = target.value;
  emit('update:modelValue', target.value);
  isOpen.value = true;
};

// Simple filtering for options based on input
const filteredGroups = computed(() => {
  const query = inputValue.value.toLowerCase();
  
  const groups: { name: string; options: ComboboxOption[] }[] = [];
  const ungrouped: ComboboxOption[] = [];
  const groupMap = new Map<string, ComboboxOption[]>();
  
  props.options.forEach(opt => {
    if (query && !String(opt.label).toLowerCase().includes(query) && !String(opt.value).toLowerCase().includes(query)) {
      return;
    }
    
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
</script>

<template>
  <div class="relative" ref="targetRef">
    <div class="relative flex items-center">
      <input
        type="text"
        :value="inputValue"
        @input="handleInput"
        @focus="isOpen = true"
        :placeholder="placeholder"
        :disabled="disabled"
        class="flex h-10 w-full rounded-md border border-neutral-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm text-neutral-900 dark:text-zinc-100 placeholder:text-neutral-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-emerald-500 disabled:cursor-not-allowed disabled:opacity-50 transition-colors pr-10"
      />
      <button
        type="button"
        @click="toggleOpen"
        :disabled="disabled"
        class="absolute right-0 top-0 bottom-0 px-3 flex items-center justify-center text-neutral-500 hover:text-neutral-700 dark:hover:text-zinc-300 outline-none"
      >
        <ChevronDown class="h-4 w-4 opacity-50 shrink-0" />
      </button>
    </div>

    <transition
      enter-active-class="transition duration-100 ease-out"
      enter-from-class="transform scale-95 opacity-0"
      enter-to-class="transform scale-100 opacity-100"
      leave-active-class="transition duration-75 ease-in"
      leave-from-class="transform scale-100 opacity-100"
      leave-to-class="transform scale-95 opacity-0"
    >
      <div
        v-if="isOpen && filteredGroups.length > 0"
        class="absolute z-50 mt-1 max-h-60 min-w-full w-max left-1/2 -translate-x-1/2 overflow-hidden flex flex-col rounded-md border border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-950 p-1 text-base shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none sm:text-sm max-w-[90vw] sm:max-w-md"
      >
        <div class="overflow-y-auto flex-1 w-full">
        <div v-for="(group, idx) in filteredGroups" :key="idx">
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
