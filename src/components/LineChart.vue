<script setup lang="ts">
import { computed } from "vue";
import { Line } from "vue-chartjs";
import { PALETTE } from "../lib/charts";

export interface Series {
  label: string;
  color?: string;
  data: { x: number; y: number | null }[];
}

const props = withDefaults(
  defineProps<{
    series: Series[];
    yLabel?: string;
    yMax?: number | null;
    fill?: boolean;
  }>(),
  { fill: false, yMax: null },
);

const data = computed(() => ({
  datasets: props.series.map((s, i) => {
    const c = s.color ?? PALETTE[i % PALETTE.length];
    return {
      label: s.label,
      data: s.data,
      borderColor: c,
      backgroundColor: c + "33",
      fill: props.fill,
      tension: 0.25,
      pointRadius: 0,
      borderWidth: 1.5,
      spanGaps: true,
    };
  }),
}));

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const options = computed<any>(() => ({
  responsive: true,
  maintainAspectRatio: false,
  animation: false,
  interaction: { mode: "index", intersect: false },
  scales: {
    x: {
      type: "time",
      time: { tooltipFormat: "HH:mm:ss" },
      ticks: { maxTicksLimit: 6, maxRotation: 0 },
    },
    y: {
      beginAtZero: true,
      max: props.yMax ?? undefined,
      title: { display: !!props.yLabel, text: props.yLabel ?? "" },
      ticks: { maxTicksLimit: 5 },
    },
  },
  plugins: {
    legend: { display: props.series.length > 1, labels: { boxWidth: 12 } },
  },
}));
</script>

<template>
  <div class="chart-wrap">
    <Line :data="data" :options="options" />
  </div>
</template>

<style scoped>
.chart-wrap {
  height: 190px;
  width: 100%;
  position: relative;
}
</style>
