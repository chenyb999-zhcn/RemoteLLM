<script setup lang="ts">
import { ref, watch, nextTick, onMounted, onBeforeUnmount } from "vue";

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
// 智能跟底：仅当用户停留在底部附近时才自动滚到底；
// 用户上翻查看历史时暂停跟底，滚回底部后自动恢复
const stickToBottom = ref(true);
const STICK_THRESHOLD = 40;

function isAtBottom(node: HTMLElement): boolean {
  return node.scrollTop + node.clientHeight >= node.scrollHeight - STICK_THRESHOLD;
}

function onScroll() {
  if (el.value) stickToBottom.value = isAtBottom(el.value);
}

watch(
  () => props.text,
  async () => {
    if (!props.autoScroll) return;
    await nextTick();
    if (el.value && stickToBottom.value) {
      el.value.scrollTop = el.value.scrollHeight;
    }
  },
  { immediate: true }
);

onMounted(() => {
  if (el.value) {
    el.value.addEventListener("scroll", onScroll);
    // 首次挂载强制跟底一次
    if (props.autoScroll) el.value.scrollTop = el.value.scrollHeight;
  }
});

onBeforeUnmount(() => {
  el.value?.removeEventListener("scroll", onScroll);
});
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
