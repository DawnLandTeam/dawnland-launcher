<script setup lang="ts">
import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../composables/useToast';
import { useI18n } from 'vue-i18n';
import {
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '../ui/dialog';
import DInput from '../ui/DInput.vue';
import DSelect from '../ui/DSelect.vue';
import { Loader2 } from '@lucide/vue';

const props = defineProps<{
  open?: boolean;
  assetType: string;
  source: string;
  projectId: string;
  projectName: string;
}>();

const emit = defineEmits(['update:open', 'close']);

const { t } = useI18n();

const isSubmitting = ref(false);
const isNewPreset = ref(false);
const existingPresets = ref<{label: string, value: string}[]>([]);
const selectedPreset = ref('');
const newPresetName = ref('');

const loadPresets = async () => {
  try {
    const assets = await invoke<any[]>('get_asset_presets', { assetType: props.assetType });
    existingPresets.value = assets.map(a => {
      const name = a.filename.replace(/\.json$/, '');
      return { label: name, value: name };
    });
    if (existingPresets.value.length > 0) {
      selectedPreset.value = existingPresets.value[0].value;
      isNewPreset.value = false;
    } else {
      isNewPreset.value = true;
    }
  } catch (err) {
    console.error(err);
  }
};

watch(() => props.assetType, () => {
  loadPresets();
}, { immediate: true });

watch(() => props.open, (newVal) => {
  if (newVal) {
    loadPresets();
    newPresetName.value = '';
  }
});

const submit = async () => {
  if (isNewPreset.value && !newPresetName.value) {
    toast.error(t('addToPreset.emptyName'));
    return;
  }
  const presetName = isNewPreset.value ? newPresetName.value : selectedPreset.value;
  
  isSubmitting.value = true;
  try {
    await invoke('add_mod_to_preset', {
      presetName,
      assetType: props.assetType,
      source: props.source,
      projectId: props.projectId,
      projectName: props.projectName
    });
    toast.success(t('addToPreset.success'));
    emit('update:open', false);
    emit('close');
  } catch(err) {
    toast.error(t('addToPreset.failed', { error: String(err) }));
  } finally {
    isSubmitting.value = false;
  }
};
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-[400px]">
    <div class="space-y-1.5">
      <DialogTitle>{{ t('addToPreset.title') }}</DialogTitle>
      <DialogDescription>
        {{ t('addToPreset.desc', { name: projectName }) }}
      </DialogDescription>
    </div>

    <div class="py-4 space-y-4">
      <div v-if="existingPresets.length > 0" class="flex gap-4 mb-4">
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="radio" :value="false" v-model="isNewPreset" class="text-emerald-600 focus:ring-emerald-600" />
          <span class="text-sm">{{ t('addToPreset.existing') }}</span>
        </label>
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="radio" :value="true" v-model="isNewPreset" class="text-emerald-600 focus:ring-emerald-600" />
          <span class="text-sm">{{ t('addToPreset.new') }}</span>
        </label>
      </div>

      <div v-if="!isNewPreset && existingPresets.length > 0" class="space-y-2">
        <DSelect v-model="selectedPreset" :options="existingPresets" class="w-full" />
      </div>
      
      <div v-if="isNewPreset || existingPresets.length === 0" class="space-y-2">
        <DInput v-model="newPresetName" :placeholder="t('addToPreset.placeholder')" class="w-full" />
      </div>
    </div>

    <div class="flex justify-end gap-2 mt-4 pt-4 border-t border-neutral-200 dark:border-zinc-800">
      <button @click="emit('update:open', false); emit('close')" class="px-4 py-2 text-sm font-medium border border-neutral-200 dark:border-zinc-700 hover:bg-neutral-50 dark:hover:bg-zinc-800 rounded-md transition-colors">
        {{ t('addToPreset.cancel') }}
      </button>
      <button @click="submit" :disabled="isSubmitting" class="px-4 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md text-sm font-medium transition-colors flex items-center gap-2 disabled:opacity-50">
        <Loader2 v-if="isSubmitting" class="w-4 h-4 animate-spin" />
        {{ t('addToPreset.confirm') }}
      </button>
    </div>
  </DialogContent>
</template>
