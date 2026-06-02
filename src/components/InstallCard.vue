<template>
  <div
    class="relative flex flex-col items-center justify-center p-5 border rounded-xl transition-all duration-200 select-none"
    :class="[
      isDisabled ? 'bg-gray-50 border-gray-200 cursor-not-allowed opacity-75' : 'bg-white dark:bg-zinc-800 border-gray-200 dark:border-zinc-700 hover:shadow-md cursor-pointer',
      isSelected ? 'border-primary/50 dark:border-primary/50' : ''
    ]"
    @click="handleCardClick"
  >
    <div class="h-12 flex items-center justify-center mb-3">
      <img v-if="iconUrl" :src="iconUrl" class="max-h-full max-w-full object-contain" :alt="title" />
      <div v-else class="w-10 h-10 bg-gray-200 dark:bg-zinc-700 rounded-md"></div>
    </div>

    <span class="text-sm font-medium text-gray-800 dark:text-gray-200 mb-1">{{ title }}</span>

    <div class="h-10 flex flex-col items-center justify-center w-full">
      <span v-if="isDisabled" class="text-xs text-gray-500 dark:text-gray-400 text-center px-2">
        {{ conflictReason }}
      </span>

      <div v-else-if="isSelected" class="flex flex-col items-center w-full">
        <span class="text-xs text-gray-600 dark:text-gray-300 mb-1.5 font-medium truncate w-full text-center px-2">{{ version }}</span>
        <div class="flex items-center gap-6 text-gray-600 dark:text-gray-400">
          <button @click.stop="$emit('remove')" class="hover:text-red-500 transition-colors p-1" title="取消安装">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
          </button>
          <button @click.stop="$emit('change')" class="hover:text-primary transition-colors p-1" title="更换版本">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
          </button>
        </div>
      </div>

      <div v-else class="flex flex-col items-center">
        <span class="text-xs text-gray-500 dark:text-gray-400 mb-1">{{ t('install.uninstalled') }}</span>
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-gray-500 dark:text-gray-400"><path d="M5 12h14"/><path d="m12 5 7 7-7 7"/></svg>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

const { t } = useI18n();

interface Props {
  title: string;
  iconUrl?: string;
  status: 'pending' | 'selected' | 'disabled';
  version?: string;
  conflictReason?: string;
}

const props = defineProps<Props>();
const emit = defineEmits<{ (e: 'click'): void; (e: 'remove'): void; (e: 'change'): void; }>();

const isDisabled = computed(() => props.status === 'disabled');
const isSelected = computed(() => props.status === 'selected');

const handleCardClick = () => {
  if (!isDisabled.value && !isSelected.value) emit('click');
};
</script>
