<script setup lang="ts">
import type { Component } from 'vue';
import { computed } from 'vue';

export interface SidebarTab {
  id: string;
  name: string;
  icon?: Component;
  disabled?: boolean;
  group?: string;
  hasDot?: boolean;
  action?: () => void;
}

const props = defineProps<{
  title?: string;
  tabs: SidebarTab[];
  modelValue: string;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const selectTab = (tab: SidebarTab) => {
  if (tab.disabled) return;
  if (tab.action) {
    tab.action();
    return;
  }
  emit('update:modelValue', tab.id);
};

const groupedTabs = computed(() => {
  const groups: { name: string; tabs: SidebarTab[] }[] = [];
  const ungrouped: SidebarTab[] = [];
  const groupMap = new Map<string, SidebarTab[]>();
  
  props.tabs.forEach(tab => {
    if (tab.group) {
      if (!groupMap.has(tab.group)) {
        groupMap.set(tab.group, []);
      }
      groupMap.get(tab.group)!.push(tab);
    } else {
      ungrouped.push(tab);
    }
  });

  if (ungrouped.length > 0) {
    groups.push({ name: '', tabs: ungrouped });
  }

  groupMap.forEach((tabs, name) => {
    groups.push({ name, tabs });
  });

  return groups;
});
</script>

<template>
  <div class="w-56 shrink-0 flex flex-col gap-1 p-4 min-h-full rounded-xl border border-neutral-200/50 dark:border-zinc-800/50 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm">
    <div v-if="title" class="mb-4">
      <h2 class="text-lg font-bold px-3">{{ title }}</h2>
    </div>
    
    <template v-for="(group, idx) in groupedTabs" :key="idx">
      <div v-if="group.name" class="mt-3 mb-1 px-3 text-xs font-semibold text-neutral-500 dark:text-zinc-400 uppercase tracking-wider">
        {{ group.name }}
      </div>
      
      <button
        v-for="tab in group.tabs"
        :key="tab.id"
        @click="selectTab(tab)"
        :disabled="tab.disabled"
        class="flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-all w-full text-left"
        :class="[
          tab.disabled 
            ? 'opacity-50 cursor-not-allowed text-neutral-400 dark:text-zinc-600' 
            : (modelValue === tab.id 
                ? 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/50 dark:text-indigo-300 shadow-sm' 
                : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50')
        ]"
      >
        <component v-if="tab.icon" :is="tab.icon" class="w-4 h-4 shrink-0" />
        <span class="truncate">{{ tab.name }}</span>
        <span v-if="tab.hasDot" class="relative flex h-2 w-2 ml-auto shrink-0">
          <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-red-400 opacity-75"></span>
          <span class="relative inline-flex rounded-full h-2 w-2 bg-red-500"></span>
        </span>
      </button>
    </template>
  </div>
</template>
