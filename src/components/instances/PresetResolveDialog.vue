<script setup lang="ts">
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../composables/useToast';
import {
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '../ui/dialog';
import { Loader2, CheckCircle2, XCircle } from '@lucide/vue';

const props = defineProps<{
  open?: boolean;
  instanceId: string;
  presetName: string;
  assetType: string;
  resolvedData: any;
}>();

const emit = defineEmits(['update:open', 'close']);
const isSubmitting = ref(false);

const submit = async () => {
  isSubmitting.value = true;
  try {
    await invoke('task_create', {
      taskType: 'install-preset',
      payload: {
        preset_name: props.presetName,
        asset_type: props.assetType,
        instance_id: props.instanceId,
        mods: props.resolvedData.resolved_mods
      }
    });

    toast.success('预设应用任务已添加，请在任务列表查看进度');
    emit('update:open', false);
    emit('close');
  } catch (err) {
    toast.error('应用预设失败: ' + err);
  } finally {
    isSubmitting.value = false;
  }
};
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-[500px]">
    <div class="space-y-1.5">
      <DialogTitle>应用预设确认</DialogTitle>
      <DialogDescription>
        根据当前实例的版本和加载器，我们为您匹配了以下资源。
      </DialogDescription>
    </div>

    <div class="py-4 space-y-4 max-h-[60vh] overflow-y-auto pr-2 minimal-scrollbar">
      <div v-if="resolvedData.resolved_mods.length > 0" class="space-y-2">
        <h4 class="text-sm font-medium text-emerald-600 flex items-center gap-2">
          <CheckCircle2 class="w-4 h-4" /> 匹配成功 ({{ resolvedData.resolved_mods.length }})
        </h4>
        <div class="bg-emerald-50/50 dark:bg-emerald-900/10 border border-emerald-100 dark:border-emerald-900/30 rounded-lg p-3 space-y-2">
          <div v-for="rm in resolvedData.resolved_mods" :key="rm.project_id" class="text-sm">
            <span class="font-medium text-neutral-800 dark:text-neutral-200">{{ rm.project_name }}</span>
            <span class="text-xs text-neutral-500 ml-2">{{ rm.filename }}</span>
          </div>
        </div>
      </div>

      <div v-if="resolvedData.failed_mods.length > 0" class="space-y-2">
        <h4 class="text-sm font-medium text-red-600 flex items-center gap-2">
          <XCircle class="w-4 h-4" /> 无法匹配此版本 ({{ resolvedData.failed_mods.length }})
        </h4>
        <div class="bg-red-50/50 dark:bg-red-900/10 border border-red-100 dark:border-red-900/30 rounded-lg p-3 space-y-2">
          <div v-for="fm in resolvedData.failed_mods" :key="fm.project_id" class="text-sm">
            <span class="font-medium text-neutral-800 dark:text-neutral-200">{{ fm.name }}</span>
            <span class="text-xs text-neutral-500 ml-2">ID: {{ fm.project_id }}</span>
          </div>
        </div>
      </div>
      
      <div v-if="resolvedData.resolved_mods.length === 0" class="py-4 text-center text-sm text-neutral-500">
        没有成功匹配任何模组，预设可能不兼容当前实例。
      </div>
    </div>

    <div class="flex justify-end gap-2 mt-4 pt-4 border-t border-neutral-200 dark:border-zinc-800">
      <button @click="emit('update:open', false); emit('close')" class="px-4 py-2 text-sm font-medium border border-neutral-200 dark:border-zinc-700 hover:bg-neutral-50 dark:hover:bg-zinc-800 rounded-md transition-colors">
        取消
      </button>
      <button 
        @click="submit" 
        :disabled="isSubmitting || resolvedData.resolved_mods.length === 0" 
        class="px-4 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-md text-sm font-medium transition-colors flex items-center gap-2 disabled:opacity-50"
      >
        <Loader2 v-if="isSubmitting" class="w-4 h-4 animate-spin" />
        开始下载
      </button>
    </div>
  </DialogContent>
</template>
