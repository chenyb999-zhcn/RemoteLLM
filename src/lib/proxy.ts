import type { AppSettings, ServerProfile } from "./types";

/** 档案级生效代理：profile.proxy 覆盖 > 全局设置；无代理时返回空串 */
export function effectiveProxy(
  profile: ServerProfile | null | undefined,
  settings: AppSettings,
): string {
  if (profile && profile.proxy !== undefined && profile.proxy !== null) {
    return profile.proxy.trim();
  }
  return settings.proxyEnabled ? settings.proxyUrl.trim() : "";
}
