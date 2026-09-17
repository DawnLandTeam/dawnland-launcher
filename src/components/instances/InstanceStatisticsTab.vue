<script setup lang="ts">
import { ref, onMounted, watch } from 'vue';
import { Play, Clock, Hash, BarChart } from '@lucide/vue';
import { useStatistics } from '../../composables/useStatistics';

const props = defineProps<{
  instanceId: string;
}>();

const { fetchInstanceStats } = useStatistics();
const stats = ref<any>(null);
const loading = ref(true);

const loadStats = async (id: string) => {
  loading.value = true;
  stats.value = await fetchInstanceStats(id);
  loading.value = false;
};

onMounted(() => loadStats(props.instanceId));
watch(() => props.instanceId, (newId) => loadStats(newId));

const formatTime = (seconds: number) => {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
};
</script>

<template>
  <div class="flex-1 h-full w-full flex flex-col min-h-0 bg-white/40 dark:bg-zinc-900/40">
    <div class="px-6 py-4 border-b border-neutral-200/50 dark:border-zinc-800/50 flex-shrink-0 flex items-center justify-between">
      <h3 class="text-lg font-semibold flex items-center gap-2">
        <BarChart class="w-5 h-5 text-primary" />
        {{ $t('instances.statistics', 'Statistics') }}
      </h3>
    </div>

    <div class="flex-1 overflow-y-auto overflow-x-hidden p-6 flex flex-col gap-4 minimal-scrollbar">
      <div v-if="loading" class="flex justify-center items-center py-12">
        <div class="w-8 h-8 border-4 border-primary border-t-transparent rounded-full animate-spin"></div>
      </div>
      
      <div v-else-if="!stats" class="flex flex-col items-center justify-center p-12 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-xl border border-white/20 dark:border-zinc-800/50 rounded-2xl shadow-sm">
        <Clock class="h-12 w-12 text-muted-foreground mb-4" />
        <h3 class="text-lg font-medium text-foreground">{{ $t('instances.noData', 'No Data') }}</h3>
        <p class="text-sm text-muted-foreground">{{ $t('instances.noDataDesc', 'No play time data recorded yet. Go ahead and start the game!') }}</p>
      </div>

      <div v-else class="grid gap-4 md:grid-cols-3">
        <div class="bg-white/60 dark:bg-zinc-900/60 backdrop-blur-xl border border-white/20 dark:border-zinc-800/50 rounded-2xl shadow-sm p-6 flex flex-col items-center justify-center text-center space-y-2 relative overflow-hidden group">
          <div class="absolute -right-4 -bottom-4 opacity-5 group-hover:scale-110 transition-transform">
            <Clock class="w-32 h-32" />
          </div>
          <Clock class="h-8 w-8 text-primary mb-2" />
          <p class="text-sm font-medium text-muted-foreground">{{ $t('instances.totalPlayTime', 'Total Play Time') }}</p>
          <h3 class="text-3xl font-bold tracking-tight">{{ formatTime(stats.playTimeSeconds || 0) }}</h3>
        </div>

        <div class="bg-white/60 dark:bg-zinc-900/60 backdrop-blur-xl border border-white/20 dark:border-zinc-800/50 rounded-2xl shadow-sm p-6 flex flex-col items-center justify-center text-center space-y-2 relative overflow-hidden group">
          <div class="absolute -right-4 -bottom-4 opacity-5 group-hover:scale-110 transition-transform">
            <Play class="w-32 h-32" />
          </div>
          <Play class="h-8 w-8 text-green-500 mb-2" />
          <p class="text-sm font-medium text-muted-foreground">{{ $t('instances.launchCount', 'Launch Count') }}</p>
          <h3 class="text-3xl font-bold tracking-tight">{{ stats.launchCount || 0 }} {{ $t('instances.times', 'times') }}</h3>
        </div>

        <div class="bg-white/60 dark:bg-zinc-900/60 backdrop-blur-xl border border-white/20 dark:border-zinc-800/50 rounded-2xl shadow-sm p-6 flex flex-col items-center justify-center text-center space-y-2 relative overflow-hidden group">
          <div class="absolute -right-4 -bottom-4 opacity-5 group-hover:scale-110 transition-transform">
            <Hash class="w-32 h-32" />
          </div>
          <Hash class="h-8 w-8 text-purple-500 mb-2" />
          <p class="text-sm font-medium text-muted-foreground">{{ $t('instances.lastPlayed', 'Last Played') }}</p>
          <h3 class="text-xl font-bold tracking-tight mt-2">
            {{ stats.lastPlayedAt ? new Date(stats.lastPlayedAt * 1000).toLocaleDateString() : $t('instances.unknown', 'Unknown') }}
          </h3>
        </div>
      </div>
    </div>
  </div>
</template>
