<script setup lang="ts">
import { computed } from "vue";
import { useDark } from "@vueuse/core";
import CustomTitleBar from "../components/CustomTitleBar.vue";
import Sidebar from "../components/Sidebar.vue";
import { useRoute } from "vue-router";
import { useBackgroundSettings } from "../composables/useBackgroundSettings";

const route = useRoute();
const isDark = useDark();
const {
  backgroundState,
  resolvedBackgroundSrc,
  fitObjectClass,
  getEffectiveOverlayOpacity,
  handleBackgroundImageError,
} = useBackgroundSettings();

const effectiveOverlayOpacity = computed(() =>
  getEffectiveOverlayOpacity(backgroundState.value.overlayOpacity, isDark.value),
);

const blurOverlayStyle = computed(() => {
  const blur = backgroundState.value.blurPx;
  const filterValue = blur > 0 ? `blur(${blur}px)` : "none";
  return {
    backdropFilter: filterValue,
    WebkitBackdropFilter: filterValue,
  };
});

const tintOverlayStyle = computed(() => ({
  opacity: effectiveOverlayOpacity.value / 100,
}));
</script>

<template>
  <div class="relative flex h-full flex-col text-neutral-900 dark:text-zinc-100 overflow-hidden">
    <!-- Global Background Image -->
    <div class="absolute inset-0 z-0 overflow-hidden bg-neutral-100 dark:bg-zinc-950">
      <img
        :src="resolvedBackgroundSrc"
        alt=""
        :class="['w-full h-full select-none transition-opacity duration-300', fitObjectClass]"
        draggable="false"
        @error="handleBackgroundImageError"
      />
      <div
        class="absolute inset-0 pointer-events-none"
        :style="blurOverlayStyle"
      ></div>
      <div
        class="absolute inset-0 pointer-events-none bg-white dark:bg-black transition-opacity duration-200"
        :style="tintOverlayStyle"
      ></div>
    </div>
    
    <!-- App Content Layout -->
    <div class="relative z-10 flex h-full flex-col">
      <CustomTitleBar />
      <div class="flex flex-1 overflow-hidden">
        <Sidebar />
        <main class="flex-1 overflow-y-auto overflow-x-hidden no-scrollbar">
          <router-view v-slot="{ Component }">
            <keep-alive exclude="InstanceManagementView">
              <component :is="Component" :key="route.path" />
            </keep-alive>
          </router-view>
        </main>
      </div>
    </div>
  </div>
</template>