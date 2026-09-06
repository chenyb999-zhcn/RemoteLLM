<script setup lang="ts">
import { ref, watch, nextTick } from "vue";

const props = withDefaults(
  defineProps<{
    text?: string;
    maxHeight?: string;
    autoScroll?: boolean;
    placeholder?: string;
  }>(),
  { maxHeight: "60vh", autoScroll: true, placeholder: "(等待输出...)" }
);

const el = ref<HTMLElement | null>(null);
watch(
  () => props.text,
  async () => {
    if (!props.autoScroll) return;
    await nextTick();
    if (el.value) el.value.scrollTop = el.value.scrollHeight;
  },
  { immediate: true }
);
</script>

<template>
  <pre ref="el" class="stream-log" :style="{ maxHeight }">{{ text || placeholder }}</pre>
</template>

<style scoped>
.stream-log {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 6px;
  margin: 0;
  overflow-y: auto;
}
</style>
