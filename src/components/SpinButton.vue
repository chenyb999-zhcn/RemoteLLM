<script setup lang="ts">
import { NButton, NSpin } from "naive-ui";

// 按钮 + 右侧常显转圈槽：转圈在按钮外部右侧，槽位固定尺寸（始终占位），
// 出现/消失不改变按钮与其它元素的位置
defineOptions({ inheritAttrs: false });
defineProps<{ loading?: boolean }>();
</script>

<template>
  <span class="spin-btn">
    <n-button v-bind="$attrs" :class="{ 'is-busy': loading }">
      <slot />
    </n-button>
    <span class="spin-btn__slot">
      <n-spin v-if="loading" size="small" />
    </span>
  </span>
</template>

<style scoped>
.spin-btn {
  display: inline-flex;
  align-items: center;
}
.spin-btn__slot {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  margin-left: 4px;
  flex-shrink: 0;
}
</style>
