import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import ConfirmModal from "./ConfirmModal";
import { api, errorMessage } from "../lib/ipc";
import type { Settings, TtsDownloadEvent, TtsModelView } from "../types";

/**
 * 设置页「朗读语音」区块（ADR-21）：朗读引擎二选一 + 语音包按需下载。
 *
 *  - 引擎与所选模型点了立即保存（与「自动朗读」开关同一交互习惯，不走上方「保存设置」）
 *  - 语音包不随安装包：从这里下载到数据目录 tts-model/，一次下载永久离线
 *  - 本地引擎缺模型 / 合成失败时后端自动兜底回系统朗读，这里只做提示
 *  - onSaved：把立即保存的结果回传父页面——设置页顶部的「保存设置」与各开关
 *    以挂载时的设置快照为基线全量保存，不同步会把引擎选择静默覆盖回旧值（审查 P1-1）
 */
export default function TtsSettings({
  notify,
  onSaved,
}: {
  notify: (kind: "ok" | "error", text: string) => void;
  onSaved: (s: Settings) => void;
}) {
  // 已保存的引擎设置（与设置页其它「立即保存」开关同源：都以后端为准）
  const [engine, setEngine] = useState<"system" | "local">("system");
  const [modelId, setModelId] = useState<string | null>(null);
  const [models, setModels] = useState<TtsModelView[]>([]);
  const [loadFailed, setLoadFailed] = useState(false);
  const [deleting, setDeleting] = useState<TtsModelView | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [cfg, list] = await Promise.all([api.getSettings(), api.listTtsModels()]);
      setEngine(cfg.ttsEngine === "local" ? "local" : "system");
      setModelId(cfg.ttsModelId ?? null);
      setModels(list);
      setLoadFailed(false);
    } catch (e) {
      setLoadFailed(true);
      notify("error", "语音包列表读取失败：" + errorMessage(e));
    }
  }, [notify]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // 下载进度：事件驱动 + 列表自带快照（页面刷新后恢复进度条）
  const applyProgress = useCallback((e: TtsDownloadEvent) => {
    setModels((prev) =>
      prev.map((m) =>
        m.id === e.modelId
          ? {
              ...m,
              downloading:
                e.phase === "downloading" || e.phase === "extracting"
                  ? { modelId: e.modelId, phase: e.phase, downloaded: e.downloaded, total: e.total }
                  : null,
            }
          : m,
      ),
    );
  }, []);

  useEffect(() => {
    // 组件挂载期间持续监听；done/error/canceled 后刷新列表（安装状态 / 可用性变化）
    const un = listen<TtsDownloadEvent>("tts-download-progress", (event) => {
      const e = event.payload;
      applyProgress(e);
      if (e.phase === "done") {
        notify("ok", "语音包下载完成，可以点「试听」听听效果");
        void refresh();
      } else if (e.phase === "error") {
        notify("error", "语音包下载失败：" + (e.message ?? "未知错误") + "（已保留断点，重试会继续）");
        void refresh();
      } else if (e.phase === "canceled") {
        void refresh();
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, [applyProgress, notify, refresh]);

  const saveEngineChoice = async (nextEngine: "system" | "local", nextModel: string | null) => {
    try {
      const cfg: Settings = { ...(await api.getSettings()), ttsEngine: nextEngine, ttsModelId: nextModel };
      const saved = await api.saveSettings(cfg);
      setEngine(saved.ttsEngine === "local" ? "local" : "system");
      setModelId(saved.ttsModelId ?? null);
      // 同步回父页面：顶部「保存设置」与各开关都用旧快照全量保存，漏了这步会覆盖回旧值
      onSaved(saved);
      notify(
        "ok",
        saved.ttsEngine === "local" ? "朗读已切换到本地语音包" : "朗读已切换回系统语音",
      );
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const download = async (id: string) => {
    try {
      await api.downloadTtsModel(id);
      // 立即占住按钮（审查 P2-6）：TCP/TLS 建立的 1~3 秒空窗里再点会弹「已有任务」
      setModels((prev) =>
        prev.map((m) =>
          m.id === id
            ? { ...m, downloading: { modelId: id, phase: "downloading", downloaded: 0, total: 0 } }
            : m,
        ),
      );
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const cancel = async (id: string) => {
    try {
      await api.cancelTtsDownload(id);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const preview = async (id: string) => {
    try {
      // 直接指定模型试听：临时加载引擎可能要 1~2 秒（首次），之后同模型秒出
      await api.speakText("你好，这是这个语音包的声音", id);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const doDelete = async () => {
    if (!deleting) return;
    const m = deleting;
    setDeleting(null);
    try {
      await api.deleteTtsModel(m.id);
      notify(
        "ok",
        "已删除 " + m.name + (m.id === modelId ? "；朗读将自动改用系统语音" : ""),
      );
      await refresh();
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const installed = models.filter((m) => m.installed);
  const activeInstalled = installed.some((m) => m.id === modelId);

  return (
    <section className="mt-5 rounded-xl border border-stone-200 bg-white p-5">
      <h2 className="text-sm font-medium text-stone-700">朗读语音</h2>
      <p className="mt-1 text-xs text-stone-500">
        默认用系统自带的离线语音。想要更自然的声音，可下载一个本地语音包（一次下载，永久离线，不用联网）
      </p>

      {loadFailed && (
        <p className="mt-3 text-xs text-red-500">语音包列表读取失败，可刷新页面重试</p>
      )}

      <div className="mt-4 flex flex-wrap gap-2">
        <button
          onClick={() => void saveEngineChoice("system", modelId)}
          className={
            "rounded-md border px-3 py-1.5 text-sm " +
            (engine === "system"
              ? "border-stone-800 bg-stone-800 text-white"
              : "border-stone-200 text-stone-600 hover:border-stone-300")
          }
        >
          系统语音（默认）
        </button>
        <button
          onClick={() => {
            // 切到本地：没选过模型就用第一个已安装的；一个都没装则先提示去下载
            const first = modelId ?? installed[0]?.id ?? null;
            if (!first) {
              notify("error", "还没有已下载的语音包，先在下面下载一个");
              return;
            }
            void saveEngineChoice("local", first);
          }}
          className={
            "rounded-md border px-3 py-1.5 text-sm " +
            (engine === "local"
              ? "border-stone-800 bg-stone-800 text-white"
              : "border-stone-200 text-stone-600 hover:border-stone-300")
          }
        >
          本地语音包
        </button>
      </div>

      {engine === "local" && !activeInstalled && (
        <p className="mt-2 rounded-md bg-amber-50 px-3 py-2 text-xs text-amber-700">
          {installed.length
            ? "所选语音包不在本机，朗读暂用系统语音兜底；可在下面重新选择或下载"
            : "本地语音包还没下载，朗读暂用系统语音兜底；下载完成并选择后自动切换"}
        </p>
      )}

      <div className="mt-4 space-y-3 border-t border-stone-100 pt-4">
        {models.map((m) => {
          const dl = m.downloading;
          const isActive = engine === "local" && m.id === modelId;
          const pct = dl && dl.total > 0 ? Math.floor((dl.downloaded / dl.total) * 100) : 0;
          return (
            <div
              key={m.id}
              className={
                "rounded-lg border p-3 " + (isActive ? "border-stone-800 bg-stone-50" : "border-stone-200")
              }
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-sm font-medium text-stone-700">
                    {m.name}
                    {isActive && <span className="ml-2 text-[11px] text-stone-500">使用中</span>}
                  </p>
                  <p className="mt-0.5 text-xs text-stone-500">{m.description}</p>
                  <p className="mt-0.5 text-[11px] text-stone-400">
                    约 {m.sizeMb} MB
                    {m.licenseNote ? " · " + m.licenseNote : ""}
                  </p>
                </div>
                <div className="flex shrink-0 flex-wrap justify-end gap-1.5">
                  {m.installed && !isActive && (
                    <button
                      onClick={() => void saveEngineChoice("local", m.id)}
                      className="rounded border border-stone-300 px-2 py-1 text-xs text-stone-700 hover:bg-stone-100"
                    >
                      使用
                    </button>
                  )}
                  {m.installed && (
                    <button
                      onClick={() => void preview(m.id)}
                      className="rounded border border-stone-200 px-2 py-1 text-xs text-stone-600 hover:bg-stone-50"
                    >
                      试听
                    </button>
                  )}
                  {m.installed && (
                    <button
                      onClick={() => setDeleting(m)}
                      className="rounded border border-red-200 px-2 py-1 text-xs text-red-500 hover:bg-red-50"
                    >
                      删除
                    </button>
                  )}
                  {!m.installed && !dl && (
                    <button
                      onClick={() => void download(m.id)}
                      className="rounded bg-amber-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-amber-700"
                    >
                      下载
                    </button>
                  )}
                  {dl?.phase === "downloading" && (
                    <button
                      onClick={() => void cancel(m.id)}
                      className="rounded border border-stone-200 px-2 py-1 text-xs text-stone-600 hover:bg-stone-50"
                    >
                      取消
                    </button>
                  )}
                </div>
              </div>

              {dl && (
                <div className="mt-2">
                  {dl.phase === "extracting" ? (
                    <p className="text-xs text-stone-500">解压安装中…（大概十几秒，别关应用）</p>
                  ) : (
                    <>
                      <div className="h-1.5 overflow-hidden rounded-full bg-stone-100">
                        <div
                          className="h-full rounded-full bg-amber-500 transition-all"
                          style={{ width: pct + "%" }}
                        />
                      </div>
                      <p className="mt-1 text-[11px] text-stone-400">
                        下载中 {pct}%{dl.total > 0 ? " · " + mb(dl.downloaded) + " / " + mb(dl.total) : ""}
                        （失败可重试，会从断点继续）
                      </p>
                    </>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {deleting && (
        <ConfirmModal
          title="删除语音包"
          body={"确定删除「" + deleting.name + "」吗？删除后想用需要重新下载。"}
          confirmText="删除"
          onConfirm={() => void doDelete()}
          onCancel={() => setDeleting(null)}
        />
      )}
    </section>
  );
}

function mb(bytes: number): string {
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}
