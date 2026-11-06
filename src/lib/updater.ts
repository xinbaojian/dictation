// 自动升级：检查/下载/安装走 @tauri-apps/plugin-updater 的插件 API，
// 自定义后端命令统一在 lib/ipc.ts。端点和签名公钥配置在 src-tauri/tauri.conf.json。
//
// 设计前提：这是单机离线应用，联网只用于「查更新 + 下载安装包」，数据仍全在本机 SQLite。
// 因此启动自动检查必须在无网络 / 端点不存在时静默失败，绝不弹错打断默写。
import { useCallback, useEffect, useRef, useState } from "react";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { api, errorMessage } from "./ipc";

const SKIP_KEY = "dictation.skipUpdateVersion";
const CHECK_DELAY_MS = 3000;
const CHECK_TIMEOUT_MS = 8000;

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installing"
  | "ready"
  | "error"
  | "dev";

export interface UpdaterState {
  phase: UpdatePhase;
  currentVersion: string;
  /** 服务端公布的新版本，null 表示没有可用更新 */
  version: string | null;
  notes: string | null;
  date: string | null;
  /** 0–100；服务端没给 contentLength 时为 null */
  progress: number | null;
  downloaded: number;
  total: number;
  message: string | null;
  /** 本次会话不再显示顶部横幅（设置页仍可更新） */
  dismissed: boolean;
}

export interface UpdaterController {
  state: UpdaterState;
  /** silent=true：启动时的自动检查，失败不提示 */
  checkForUpdate: (silent?: boolean) => Promise<void>;
  installUpdate: () => Promise<void>;
  restart: () => Promise<void>;
  skipVersion: () => void;
  dismiss: () => void;
}

const initialState = (): UpdaterState => ({
  phase: import.meta.env.DEV ? "dev" : "idle",
  currentVersion: "",
  version: null,
  notes: null,
  date: null,
  progress: null,
  downloaded: 0,
  total: 0,
  message: null,
  dismissed: false,
});

const readSkipped = (): string | null => {
  try {
    return localStorage.getItem(SKIP_KEY);
  } catch {
    return null;
  }
};

export function useAutoUpdate(): UpdaterController {
  const [state, setState] = useState<UpdaterState>(initialState);
  // Update 持有 Rust 侧资源，必须用 ref 保活；检查出新版本时先 close 旧的
  const updateRef = useRef<Update | null>(null);
  // 同步锁：连点检查 / 连点安装都要挡住（与会话推进的 advancing ref 同理）
  const busyRef = useRef(false);

  useEffect(() => {
    api
      .appVersion()
      .then((v) => setState((s) => ({ ...s, currentVersion: v })))
      .catch(() => {
        // 版本号显示不出来不影响功能
      });
  }, []);

  const checkForUpdate = useCallback(async (silent = false) => {
    if (import.meta.env.DEV) {
      setState((s) => ({ ...s, phase: "dev", message: "开发模式不检查更新" }));
      return;
    }
    if (busyRef.current) return;
    busyRef.current = true;
    setState((s) => ({
      ...s,
      phase: "checking",
      version: null,
      notes: null,
      date: null,
      progress: null,
      message: null,
      dismissed: false,
    }));
    try {
      const previous = updateRef.current;
      updateRef.current = null;
      if (previous) {
        try {
          await previous.close();
        } catch {
          // 资源已失效，忽略
        }
      }

      const update = await check({ timeout: CHECK_TIMEOUT_MS });
      updateRef.current = update;
      if (update) {
        setState((s) => ({
          ...s,
          phase: "available",
          version: update.version,
          notes: update.body ?? null,
          date: update.date ?? null,
          // 手动点「检查更新」时即便之前跳过过也要把结果摆出来
          dismissed: silent && readSkipped() === update.version,
        }));
      } else {
        setState((s) => ({ ...s, phase: "idle", message: silent ? null : "当前已是最新版本" }));
      }
    } catch (e) {
      // 离线、GitHub 不可达、旧 Release 还没有 latest.json 都会走到这里
      setState((s) => ({
        ...s,
        phase: silent ? "idle" : "error",
        message: silent ? null : "检查更新失败：" + errorMessage(e),
      }));
    } finally {
      busyRef.current = false;
    }
  }, []);

  // 启动 3 秒后再查，别拖慢首屏；StrictMode 的双次 effect 由 timer 清理 + busyRef 兜住。
  // 受设置「启动时自动检查更新」控制（默认开启）：关掉后启动不联网，只能手动点「检查更新」。
  // 读不到设置时按默认「开」处理，与后端 serde(default) 的口径一致。
  useEffect(() => {
    if (import.meta.env.DEV) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api
        .getSettings()
        .catch(() => null)
        .then((s) => {
          if (cancelled || s?.autoCheckUpdates === false) return;
          return checkForUpdate(true);
        });
    }, CHECK_DELAY_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [checkForUpdate]);

  const installUpdate = useCallback(async () => {
    const update = updateRef.current;
    if (!update || busyRef.current) return;
    busyRef.current = true;
    let downloaded = 0;
    let total = 0;
    setState((s) => ({
      ...s,
      phase: "downloading",
      progress: null,
      downloaded: 0,
      total: 0,
      message: null,
    }));

    const onEvent = (event: DownloadEvent) => {
      if (event.event === "Started") {
        total = event.data.contentLength ?? 0;
        setState((s) => ({ ...s, total }));
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
        setState((s) => ({
          ...s,
          downloaded,
          progress: total > 0 ? Math.min(99, Math.round((downloaded / total) * 100)) : null,
        }));
      } else {
        // Finished：下载完毕，插件接着校验签名并安装
        setState((s) => ({ ...s, phase: "installing", progress: null }));
      }
    };

    try {
      await update.downloadAndInstall(onEvent);
      updateRef.current = null;
      setState((s) => ({
        ...s,
        phase: "ready",
        progress: 100,
        message: "更新已安装，重启应用后生效",
      }));
    } catch (e) {
      setState((s) => ({ ...s, phase: "error", message: "更新失败：" + errorMessage(e) }));
    } finally {
      busyRef.current = false;
    }
  }, []);

  const restart = useCallback(async () => {
    try {
      await api.restartApp();
    } catch {
      // restart() 会立刻结束进程，前端收不到返回值，报错属正常
    }
  }, []);

  const skipVersion = useCallback(() => {
    if (state.version) {
      try {
        localStorage.setItem(SKIP_KEY, state.version);
      } catch {
        // localStorage 不可用时退化为「本次会话不再提示」
      }
    }
    setState((s) => ({ ...s, dismissed: true }));
  }, [state.version]);

  const dismiss = useCallback(() => setState((s) => ({ ...s, dismissed: true })), []);

  return { state, checkForUpdate, installUpdate, restart, skipVersion, dismiss };
}
