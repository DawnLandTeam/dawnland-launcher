<script setup lang="ts">
import CustomTitleBar from "../components/CustomTitleBar.vue";
import Sidebar from "../components/Sidebar.vue";
</script>

<template>
  <div class="flex h-full flex-col bg-neutral-50 dark:bg-zinc-900 text-neutral-900 dark:text-zinc-100">
    <CustomTitleBar />
    <div class="flex flex-1 overflow-hidden">
      <Sidebar />
      <main class="flex-1 overflow-y-auto p-4">
        <router-view v-slot="{ Component }">
          <keep-alive>
            <component :is="Component" />
          </keep-alive>
        </router-view>
      </main>
    </div>
  </div>
</template>