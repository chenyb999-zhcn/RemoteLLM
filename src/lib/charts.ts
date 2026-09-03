import {
  CategoryScale,
  Chart as ChartJS,
  Filler,
  Legend,
  LineElement,
  LinearScale,
  PointElement,
  TimeScale,
  Tooltip,
} from "chart.js";
import "chartjs-adapter-date-fns";

ChartJS.register(
  CategoryScale,
  LinearScale,
  TimeScale,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
  Legend,
);

ChartJS.defaults.color = "#999";
ChartJS.defaults.borderColor = "rgba(128,128,128,0.18)";
ChartJS.defaults.font.family =
  "'Segoe UI', 'Microsoft YaHei', Inter, sans-serif";

export const PALETTE = [
  "#63e2b7",
  "#89d4f8",
  "#ffb975",
  "#b7a7ff",
  "#f783ac",
  "#c2e58d",
  "#ffd43b",
  "#74c0fc",
];
