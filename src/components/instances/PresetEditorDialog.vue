<script setup lang="ts">
import { ref, onMounted, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import {
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '../ui/dialog';
import DButton from '../ui/DButton.vue';
import { Trash2, Loader2, Package } from '@lucide/vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';

const props = defineProps<{
  open?: boolean;
  presetName: string;
  assetType: string;
}>();

const emit = defineEmits(['update:open']);

const { t } = useI18n();

interface PresetMod {
  source: string;
  project_id: string;
  name: string;
}

interface OnlinePreset {
  name: string;
  mods: PresetMod[];
}

const loading = ref(false);
const presetData = ref<OnlinePreset | null>(null);

const loadPreset = async () => {
  if (!props.presetName || !props.assetType) return;
  loading.value = true;
  try {
    const result = await invoke<OnlinePreset>('get_preset_details', {
      assetType: props.assetType,
      presetName: props.presetName
    });
    presetData.value = result;
  } catch (err) {
    toast.error(t('common.error', 'Error'), getErrorMessage(err));
    emit('update:open', false);
  } finally {
    loading.value = false;
  }
};

watch(() => props.open, (isOpen) => {
  if (isOpen) {
    loadPreset();
  }
});

onMounted(() => {
  if (props.open) {
    loadPreset();
  }
});

const isUpdatingMods = ref(false);

const removeMod = async (index: number) => {
  if (!presetData.value || isUpdatingMods.value) return;
  
  isUpdatingMods.value = true;
  
  // Optimistically remove from local state
  const previousMods = [...presetData.value.mods];
  presetData.value.mods.splice(index, 1);
  
  try {
    await invoke('update_preset', {
      assetType: props.assetType,
      presetName: props.presetName,
      preset: presetData.value
    });
  } catch (err) {
    // Revert on failure
    presetData.value.mods = previousMods;
    toast.error(t('common.error', 'Error'), getErrorMessage(err));
  } finally {
    isUpdatingMods.value = false;
  }
};
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-[600px] flex flex-col max-h-[85vh]">
    <div class="space-y-1.5 flex-shrink-0">
      <DialogTitle>{{ t('instances.editPresetTitle', { name: presetName.replace('.json', '') }) }}</DialogTitle>
      <DialogDescription>
        {{ t('instances.editPresetDesc', 'Manage items in this preset. Changes are saved automatically.') }}
      </DialogDescription>
    </div>

    <div class="flex-1 overflow-y-auto py-4 minimal-scrollbar min-h-[200px]">
      <div v-if="loading" class="flex items-center justify-center h-full">
        <Loader2 class="w-8 h-8 animate-spin text-primary" />
      </div>
      
      <div v-else-if="presetData">
        <div v-if="presetData.mods.length === 0" class="flex flex-col items-center justify-center py-12 text-muted-foreground">
          <Package class="w-12 h-12 mb-4 opacity-20" />
          <p>{{ t('instances.presetEmpty', 'This preset is currently empty') }}</p>
        </div>
        
        <div v-else class="space-y-2">
          <div 
            v-for="(mod, index) in presetData.mods" 
            :key="mod.project_id + '_' + index"
            class="flex items-center justify-between p-3 rounded-lg border bg-card text-card-foreground shadow-sm"
          >
            <div class="flex-1 min-w-0 pr-4">
              <div class="font-medium text-sm truncate" :title="mod.name">{{ mod.name }}</div>
              <div class="text-xs text-muted-foreground mt-1 flex items-center gap-2">
                <span class="capitalize">{{ mod.source }}</span>
                <span>ID: {{ mod.project_id }}</span>
              </div>
            </div>
            
            <DButton 
              variant="danger" 
              size="sm" 
              class="px-2 flex-shrink-0"
              @click="removeMod(index)"
              :disabled="isUpdatingMods"
              :title="t('instances.removeFromPreset', 'Remove')"
            >
              <Trash2 class="w-4 h-4" />
            </DButton>
          </div>
        </div>
      </div>
    </div>
    
    <div class="flex justify-end pt-4 border-t flex-shrink-0 mt-auto">
      <DButton variant="outline" @click="emit('update:open', false)">
        {{ t('instances.cancel', 'Close') }}
      </DButton>
    </div>
  </DialogContent>
</template>

<style scoped>
.minimal-scrollbar::-webkit-scrollbar {
  width: 4px;
}
.minimal-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.minimal-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(150, 150, 150, 0.3);
  border-radius: 4px;
}
:global(.dark) .minimal-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
}
</style>
