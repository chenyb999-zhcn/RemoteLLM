<script setup lang="ts">
import { ref, watch, nextTick, onMounted, onBeforeUnmount } from "vue";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

const props = withDefaults(
  defineProps<{
    text?: string;
    maxHeight?: string;
    autoScroll?: boolean;
    placeholder?: string;
  }>(),
  { maxHeight: "60vh", autoScroll: true, placeholder: "" }
);

const emit = defineEmits<{
  (e: "reachTop"): void;
}>();

const el = ref<HTMLElement | null>(null);
// 智能跟底：仅当用户停留在底部附近时才自动滚到底；
// 用户上翻查看历史时暂停跟底，滚回底部后自动恢复
const stickToBottom = ref(true);
const STICK_THRESHOLD = 40;
const TOP_THRESHOLD = 30;

// 前置内容插入锚定：记录插入前的 scrollHeight 与 scrollTop，
// 渲染后把 scrollTop 增加内容增量，保持视口不动
let anchor: { height: number; top: number } | null = null;

/** 在文本前置插入内容前调用，保持当前视口不跳动 */
function anchorBeforePrepend() {
  if (el.value) anchor = { height: el.value.scrollHeight, top: el.value.scrollTop };
}
defineExpose({ anchorBeforePrepend });

function isAtBottom(node: HTMLElement): boolean {
  return node.scrollTop + node.clientHeight >= node.scrollHeight - STICK_THRESHOLD;
}

function onScroll() {
  if (!el.value) return;
  stickToBottom.value = isAtBottom(el.value);
  if (el.value.scrollTop <= TOP_THRESHOLD) {
    emit("reachTop");
  }
}

watch(
  () => props.text,
  async () => {
    if (!props.autoScroll) return;
    await nextTick();
    if (!el.value) return;
    if (anchor) {
      // 前置插入：保持视口锚定
      el.value.scrollTop = anchor.top + (el.value.scrollHeight - anchor.height);
      anchor = null;
    } else if (stickToBottom.value) {
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
  <pre ref="el" class="stream-log" :style="{ maxHeight }">{{ text || (placeholder || t("stream.waiting")) }}</pre>
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
