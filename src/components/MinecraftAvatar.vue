<template>
  <canvas ref="canvasRef" :width="size" :height="size" :class="className"></canvas>
</template>

<script setup lang="ts">
import { ref, watch, onMounted } from 'vue';
import { STEVE_SKIN_BASE64, getProxiedImageBase64 } from '../utils/steve';
import { invoke } from '@tauri-apps/api/core';

const props = withDefaults(defineProps<{
  skinUrl?: string | null;
  size?: number;
  className?: string;
  fallbackUsername?: string;
}>(), {
  skinUrl: null,
  size: 48,
  className: '',
  fallbackUsername: 'Steve',
});

const canvasRef = ref<HTMLCanvasElement | null>(null);

function getDefaultSkin(): string {
  return STEVE_SKIN_BASE64;
}

async function drawAvatar() {
  if (!canvasRef.value) return;
  const ctx = canvasRef.value.getContext('2d');
  if (!ctx) return;
  
  const defaultSkin = getDefaultSkin();
  let finalUrl = defaultSkin;
  
  if (props.skinUrl) {
    // If the URL has no query params, we use the raw URL for cache key
    // But if it's already a full URL we just use it directly
    const rawUrl = props.skinUrl;
    try {
      finalUrl = await getProxiedImageBase64(invoke, rawUrl);
    } catch(e) {
      console.warn("Avatar proxy failed, falling back to default", e);
    }
  }

  const img = new Image();
  img.crossOrigin = 'anonymous';
  
  img.onload = () => {
    // Clear canvas
    ctx.clearRect(0, 0, props.size, props.size);
    // Draw base face
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(img, 8, 8, 8, 8, 0, 0, props.size, props.size);
    // Draw hat/overlay
    ctx.drawImage(img, 40, 8, 8, 8, 0, 0, props.size, props.size);
  };
  
  img.onerror = () => {
    if (img.src !== defaultSkin) {
      img.src = defaultSkin;
    }
  };

  img.src = finalUrl;
}

onMounted(() => {
  drawAvatar();
});

watch([() => props.skinUrl, () => props.fallbackUsername], () => {
  drawAvatar();
});
</script>
