<script setup lang="ts">
import { computed } from "vue";

/**
 * 纯 SVG 指针表：240° 弧，0 → 左下、满量程 → 右下。
 * 值变化时指针 0.6s 缓动转动；颜色随区间（ok/warn/danger）。
 */
const props = withDefaults(
  defineProps<{
    label: string;
    value: number | null;
    /** 满量程（默认 100） */
    max?: number;
    unit?: string;
    /** 警告阈值（占满量程比例 0-1） */
    warn?: number;
    /** 危险阈值（占满量程比例 0-1） */
    danger?: number;
    digits?: number;
  }>(),
  { max: 100, unit: "", warn: 0.75, danger: 0.9, digits: 0 }
);

const SIZE = 112;
const CX = SIZE / 2; // 56
const CY = SIZE / 2 + 8; // 64，圆心下移给底部数值留空间
const R = 42;
const SWEEP = 240;
const START = -120; // 0° 值对应的角度（12 点方向为 0，顺时针为正）

function pt(angleDeg: number, r: number): [number, number] {
  const a = (angleDeg * Math.PI) / 180;
  return [CX + r * Math.sin(a), CY - r * Math.cos(a)];
}

/** 从 fromFrac 到 toFrac（0-1 比例）的弧路径，顺时针 */
function arcPath(fromFrac: number, toFrac: number, r: number): string {
  const a1 = START + fromFrac * SWEEP;
  const a2 = START + toFrac * SWEEP;
  const [x1, y1] = pt(a1, r);
  const [x2, y2] = pt(a2, r);
  const large = a2 - a1 > 180 ? 1 : 0;
  return `M ${x1.toFixed(2)} ${y1.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x2.toFixed(2)} ${y2.toFixed(2)}`;
}

const frac = computed(() => {
  if (props.value == null || props.max <= 0) return 0;
  return Math.max(0, Math.min(1, props.value / props.max));
});

const zone = computed<"ok" | "warn" | "danger">(() => {
  if (props.value == null) return "ok";
  const f = props.value / props.max;
  return f >= props.danger ? "danger" : f >= props.warn ? "warn" : "ok";
});

const needleAngle = computed(() => frac.value * SWEEP);

// 11 根主刻度（0/10/.../100%）
const ticks = computed(() => {
  const out: { x1: number; y1: number; x2: number; y2: number }[] = [];
  for (let i = 0; i <= 10; i++) {
    const a = START + (i / 10) * SWEEP;
    const [x1, y1] = pt(a, R + 4);
    const [x2, y2] = pt(a, R + 9);
    out.push({ x1, y1, x2, y2 });
  }
  return out;
});

const display = computed(() =>
  props.value == null ? "--" : props.value.toFixed(props.digits)
);
</script>

<template>
  <div class="gauge" :class="zone">
    <svg :width="SIZE" :height="SIZE" viewBox="0 0 112 112" aria-hidden="true">
      <!-- 底轨 + 三区着色 -->
      <path :d="arcPath(0, 1, R)" class="track" />
      <path :d="arcPath(0, warn, R)" class="zone-ok" />
      <path :d="arcPath(warn, danger, R)" class="zone-warn" />
      <path :d="arcPath(danger, 1, R)" class="zone-danger" />
      <!-- 刻度 -->
      <g v-for="(t, i) in ticks" :key="i">
        <line :x1="t.x1" :y1="t.y1" :x2="t.x2" :y2="t.y2" class="tick" />
      </g>
      <!-- 指针（朝上绘制，整组旋转） -->
      <g class="needle-g" :style="{ transform: `rotate(${needleAngle}deg)` }">
        <line :x1="CX" :y1="CY" :x2="CX" :y2="CY - R + 9" class="needle" />
      </g>
      <circle :cx="CX" :cy="CY" r="3.5" class="cap" />
      <!-- 数值 -->
      <text :x="CX" :y="CY + 25" class="val">{{ display }}</text>
      <text v-if="unit" :x="CX" :y="CY + 37" class="unit">{{ unit }}</text>
    </svg>
    <div class="label">{{ label }}</div>
  </div>
</template>

<style scoped>
.gauge {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0;
}
.track {
  fill: none;
  stroke: rgba(128, 128, 128, 0.18);
  stroke-width: 7;
}
.zone-ok {
  fill: none;
  stroke: rgba(34, 197, 94, 0.22);
  stroke-width: 7;
}
.zone-warn {
  fill: none;
  stroke: rgba(245, 158, 11, 0.3);
  stroke-width: 7;
}
.zone-danger {
  fill: none;
  stroke: rgba(239, 68, 68, 0.38);
  stroke-width: 7;
}
.tick {
  stroke: rgba(128, 128, 128, 0.55);
  stroke-width: 1;
}
.needle-g {
  transform-box: view-box;
  transform-origin: 56px 64px;
  transition: transform 0.6s cubic-bezier(0.4, 0, 0.2, 1);
}
.needle {
  stroke-width: 2.5;
  stroke-linecap: round;
}
.cap {
  fill: currentColor;
  opacity: 0.9;
}
.val {
  font-size: 17px;
  font-weight: 700;
  text-anchor: middle;
  fill: currentColor;
}
.unit {
  font-size: 9.5px;
  text-anchor: middle;
  opacity: 0.6;
  fill: currentColor;
}
.label {
  font-size: 11px;
  opacity: 0.7;
  line-height: 1.2;
}
/* 区间配色（指针 + 数值） */
.gauge.ok .needle,
.gauge.ok .cap {
  stroke: #22c55e;
  fill: #22c55e;
}
.gauge.ok .val {
  fill: #22c55e;
}
.gauge.warn .needle,
.gauge.warn .cap {
  stroke: #f59e0b;
  fill: #f59e0b;
}
.gauge.warn .val {
  fill: #f59e0b;
}
.gauge.danger .needle,
.gauge.danger .cap {
  stroke: #ef4444;
  fill: #ef4444;
}
.gauge.danger .val {
  fill: #ef4444;
}
</style>
