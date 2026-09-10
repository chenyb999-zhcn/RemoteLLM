import { createI18n } from "vue-i18n";
import zh from "./zh";
import en from "./en";

/** 应用级 i18n 实例：legacy:false（Composition API 模式），默认中文 */
export const i18n = createI18n({
  legacy: false,
  locale: "zh",
  fallbackLocale: "zh",
  messages: { zh, en },
});

export default i18n;
