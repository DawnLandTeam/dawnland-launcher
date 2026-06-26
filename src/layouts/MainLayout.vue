<script setup lang="ts">
import CustomTitleBar from "../components/CustomTitleBar.vue";
import Sidebar from "../components/Sidebar.vue";
import { useRoute } from "vue-router";

const route = useRoute();
</script>

<template>
  <div class="relative flex h-full flex-col text-neutral-900 dark:text-zinc-100 overflow-hidden">
    <!-- Global Background Image -->
    <div class="absolute inset-0 z-0">
      <img src="/minecraft_bg.png" class="w-full h-full object-cover" />
      <div class="absolute inset-0 bg-white/30 dark:bg-black/60 backdrop-blur-sm"></div>
    </div>
    
    <!-- App Content Layout -->
    <div class="relative z-10 flex h-full flex-col">
      <CustomTitleBar />
      <div class="flex flex-1 overflow-hidden">
        <Sidebar />
        <main class="flex-1 overflow-y-auto">
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