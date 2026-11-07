import { useEffect, useState } from "react";
import ConfirmModal from "../components/ConfirmModal";
import Markdown from "../components/Markdown";
import AiSettings from "../components/AiSettings";
import { Toast, useNotice } from "../components/Toast";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorMessage } from "../lib/ipc";
import type { UpdaterController } from "../lib/updater";
import type {
  BackupInfo,
  Settings as SettingsType,
  SyncConfig,
  SyncStatus,
} from "../types";

const INPUT =
  "w-full rounded-md border border-stone-200 px-2.5 py-1.5 text-sm text-stone-700 outline-none focus:border-stone-400";

const EMPTY_SYNC_FORM: SyncConfig = {
  enabled: false,
  host: "",
  port: 3306,
  database: "",
  user: "",
  deviceName: "",
};

const MASK_OPTIONS = [
  { value: 2, label: "2 秒" },
  { value: 3, label: "3 秒" },
  { value: 5, label: "5 秒" },
  { value: 8, label: "8 秒" },
  { value: 0, label: "一直显示" },
];

const STREAK_OPTIONS = [1, 2, 3];
const FONT_OPTIONS = [
  { value: 72, label: "大" },
  { value: 96, label: "更大" },
  { value: 128, label: "超大" },
];

function formatBytes(n: number): string {
  if (n < 1024 * 1024) return Math.max(1, Math.round(n / 1024)) + " KB";
  return (n / 1024 / 1024).toFixed(1) + " MB";
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString("zh-CN");
}

/** 后端写的是 UTC（`...Z`），必须交给 Date 转本地时区，截字符串会差 8 小时 */
function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export default function Settings({ updater }: { updater: UpdaterController }) {
  const [value, setValue] = useState<SettingsType | null>(null);
  // 最近一次「已保存」的设置快照：版本与更新区的开关即时生效，保存时要以它为基线，
  // 不能把「默写」区还没点保存的表单改动顺带写进去
  const [savedValue, setSavedValue] = useState<SettingsType | null>(null);
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [dataDir, setDataDir] = useState("");
  const [backupDir, setBackupDir] = useState("");
  const [importing, setImporting] = useState<{ name: string; path: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [sync, setSync] = useState<SyncStatus | null>(null);
  const [syncForm, setSyncForm] = useState<SyncConfig | null>(null);
  // 端口单独存字符串：直接受控到 number 上的话，用户删空会立刻变回 0，
  // 没法把旧值删掉重打。提交时再解析成数字。
  const [portText, setPortText] = useState("3306");
  const [syncPassword, setSyncPassword] = useState("");
  const [syncBusy, setSyncBusy] = useState(false);
  const [syncConflict, setSyncConflict] = useState<{ action: "push" | "pull"; message: string } | null>(null);
  const [syncError, setSyncError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [syncAction, setSyncAction] = useState<"discard" | "clearError" | "repair" | null>(null);
  const { notice, notify } = useNotice();
  const u = updater.state;
  const updateBusy = u.phase === "checking" || u.phase === "downloading" || u.phase === "installing";

  useEffect(() => {
    (async () => {
      try {
        const [cfg, list, dir] = await Promise.all([
          api.getSettings(),
          api.listBackups(),
          api.dataDirPath(),
        ]);
        setValue(cfg);
        setSavedValue(cfg);
        setBackups(list);
        setDataDir(dir);
      } catch (e) {
        // 别让页面停在「加载中…」：把失败原因显示出来，用户至少知道要重试
        setLoadError(errorMessage(e));
        notify("error", errorMessage(e));
      }
      // 同步状态单独取：坏掉的 .sync-state.json / 凭据文件不能让整个设置页卡在「加载中…」。
      // 即使失败也要给一份空表单，用户仍可重新保存配置来修复凭据。
      try {
        const syncState = await api.syncStatus();
        setSync(syncState);
        setSyncForm(syncState.config);
        setPortText(String(syncState.config.port || 3306));
        setSyncError("");
      } catch (e) {
        setSync(null);
        setSyncForm(EMPTY_SYNC_FORM);
        setPortText(String(EMPTY_SYNC_FORM.port));
        setSyncError(errorMessage(e));
        notify("error", "同步状态读取失败：" + errorMessage(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const save = async () => {
    if (!value || busy) return;
    setBusy(true);
    try {
      const r = await api.saveSettings(value);
      setValue(r);
      setSavedValue(r);
      notify("ok", "设置已保存");
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  /** 「启动时自动检查更新」开关：点了立即保存生效，无需再点上面的「保存设置」 */
  const toggleAutoCheck = async () => {
    if (!savedValue || busy) return;
    setBusy(true);
    try {
      const r = await api.saveSettings({
        ...savedValue,
        autoCheckUpdates: !savedValue.autoCheckUpdates,
      });
      setSavedValue(r);
      // 只同步开关字段回表单，别覆盖「默写」区还没保存的编辑
      setValue((v) => (v ? { ...v, autoCheckUpdates: r.autoCheckUpdates } : r));
      notify(
        "ok",
        r.autoCheckUpdates
          ? "已开启：每次启动自动检查更新"
          : "已关闭：启动不再自动检查，可随时手动检查",
      );
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  /** 「笔画复习自动朗读」开关：点了立即保存生效，无需再点上面的「保存设置」 */
  const toggleStrokeAutoSpeak = async () => {
    if (!savedValue || busy) return;
    setBusy(true);
    try {
      const r = await api.saveSettings({
        ...savedValue,
        strokeAutoSpeak: !savedValue.strokeAutoSpeak,
      });
      setSavedValue(r);
      // 只同步开关字段回表单，别覆盖「默写」区还没保存的编辑
      setValue((v) => (v ? { ...v, strokeAutoSpeak: r.strokeAutoSpeak } : r));
      notify(
        "ok",
        r.strokeAutoSpeak
          ? "已开启：笔画复习翻到每个词会自动读一遍"
          : "已关闭：进入笔画复习不再自动朗读，页内可点「读一遍」手动读",
      );
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const backup = async () => {
    try {
      const path = await api.createBackup(backupDir || null);
      setBackups(await api.listBackups());
      notify("ok", "已导出：" + path);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const pickBackupDir = async () => {
    try {
      const d = await open({ directory: true, title: "选择备份保存目录" });
      if (typeof d === "string") setBackupDir(d);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const pickImportFile = async () => {
    try {
      const p = await open({
        multiple: false,
        filters: [{ name: "备份文件", extensions: ["json"] }],
      });
      if (typeof p === "string") {
        const name = p.split(/[\\/]/).pop() || p;
        setImporting({ name, path: p });
      }
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const doImport = async () => {
    if (!importing) return;
    const path = importing.path;
    setImporting(null);
    try {
      const r = await api.importBackup(path);
      notify(
        "ok",
        "导入完成：新增 " + r.groupsAdded + " 组 / " + r.wordsAdded + " 词，跳过 " + r.wordsSkipped + " 个已存在词",
      );
      setBackups(await api.listBackups());
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  /** 拿到新状态后同步「表单 + 端口文本」；漏掉端口文本会让输入框显示旧值 */
  const applySyncStatus = (s: SyncStatus) => {
    setSync(s);
    setSyncForm(s.config);
    setPortText(String(s.config.port || 3306));
  };

  const saveSync = async () => {
    if (!syncForm || syncBusy) return;
    setSyncBusy(true);
    try {
      applySyncStatus(await api.saveSyncConfig(syncForm, syncPassword || null));
      setSyncPassword("");
      setSyncError("");
      notify("ok", "同步配置已保存");
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setSyncBusy(false);
    }
  };

  const testSync = async () => {
    if (syncBusy || !syncForm) return;
    setSyncBusy(true);
    try {
      // 直接用当前表单测，不落盘：试错端口不会把已经能用的凭据覆盖掉
      const v = await api.testSyncConnection(syncForm, syncPassword || null);
      notify("ok", "连接成功：MySQL " + v);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setSyncBusy(false);
    }
  };

  const runSync = async (action: "push" | "pull", force = false) => {
    if (syncBusy) return;
    setSyncBusy(true);
    try {
      const r = action === "push" ? await api.syncPush(force) : await api.syncPull(force);
      if (r.needsConfirm) {
        setSyncConflict({ action, message: r.message });
      } else {
        notify(r.kind === "empty" ? "error" : "ok", r.message);
        setSync(await api.syncStatus());
        setSyncError("");
      }
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setSyncBusy(false);
    }
  };

  const confirmSync = () => {
    const action = syncConflict?.action;
    setSyncConflict(null);
    if (action) void runSync(action, true);
  };

  const runSyncAction = async (action: "discard" | "clearError" | "repair") => {
    if (syncBusy) return;
    setSyncBusy(true);
    try {
      const s = action === "repair" ? await api.repairSyncState() : await api.discardSyncPending();
      // 只更新状态，别用已保存的配置覆盖用户正在编辑的表单
      setSync(s);
      setSyncError("");
      notify(
        "ok",
        action === "repair"
          ? "同步状态已重建；下一次推送/拉取会先让你确认，不会自动覆盖云端"
          : action === "discard"
            ? "已放弃待应用快照，本机数据没有改动"
            : "已清除错误提示",
      );
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setSyncBusy(false);
    }
  };

  const confirmSyncAction = () => {
    const action = syncAction;
    setSyncAction(null);
    if (action) void runSyncAction(action);
  };

  /** 重启后当前进程立刻退出，invoke 一定会抛错，这里必须忽略 */
  const restartNow = async () => {
    try {
      await api.restartApp();
    } catch {
      // 进程已退出，忽略
    }
  };

  // 表单与已保存配置不一致（含还没保存的新密码）：推送/拉取用的是后端已保存的配置，
  // 脏着点下去只会拿旧密码去连，用户会误判成「新密码不对」，所以必须先保存
  const syncDirty =
    syncPassword !== "" ||
    (!!syncForm && !!sync && JSON.stringify(syncForm) !== JSON.stringify(sync.config));

  if (!value) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-stone-400">
        {loadError ? "设置加载失败：" + loadError : "加载中…"}
      </div>
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto max-w-2xl px-8 py-8">
        <h1 className="text-xl font-semibold text-stone-800">设置</h1>

        <section className="mt-6 rounded-xl border border-stone-200 bg-white p-5">
          <h2 className="text-sm font-medium text-stone-700">默写</h2>

          <div className="mt-4 space-y-4">
            <div>
              <p className="mb-1.5 text-xs text-stone-500">
                每题自动显示多久后遮住（翻到新词先显示，到点自动盖住）
              </p>
              <div className="flex flex-wrap gap-2">
                {MASK_OPTIONS.map((o) => (
                  <button
                    key={o.value}
                    onClick={() => setValue({ ...value, maskSeconds: o.value })}
                    className={
                      "rounded-md border px-3 py-1.5 text-sm " +
                      (value.maskSeconds === o.value
                        ? "border-stone-800 bg-stone-800 text-white"
                        : "border-stone-200 text-stone-600 hover:border-stone-300")
                    }
                  >
                    {o.label}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <p className="mb-1.5 text-xs text-stone-500">错题连续答对几次算学会（移出错题本）</p>
              <div className="flex gap-2">
                {STREAK_OPTIONS.map((n) => (
                  <button
                    key={n}
                    onClick={() => setValue({ ...value, streakToMaster: n })}
                    className={
                      "rounded-md border px-3 py-1.5 text-sm " +
                      (value.streakToMaster === n
                        ? "border-stone-800 bg-stone-800 text-white"
                        : "border-stone-200 text-stone-600 hover:border-stone-300")
                    }
                  >
                    {n} 次
                  </button>
                ))}
              </div>
            </div>

            <div>
              <p className="mb-1.5 text-xs text-stone-500">默写时词语字号</p>
              <div className="flex gap-2">
                {FONT_OPTIONS.map((o) => (
                  <button
                    key={o.value}
                    onClick={() => setValue({ ...value, wordFontSize: o.value })}
                    className={
                      "rounded-md border px-3 py-1.5 text-sm " +
                      (value.wordFontSize === o.value
                        ? "border-stone-800 bg-stone-800 text-white"
                        : "border-stone-200 text-stone-600 hover:border-stone-300")
                    }
                  >
                    {o.label}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="mt-5 flex justify-end">
            <button
              onClick={save}
              disabled={busy}
              className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:opacity-50"
            >
              保存设置
            </button>
          </div>
        </section>

        <section className="mt-5 rounded-xl border border-stone-200 bg-white p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-sm font-medium text-stone-700">笔画复习</h2>
              <p className="mt-1 text-xs text-stone-500">
                错题本的「复习笔画」页：翻到每个词用系统语音读一遍，孩子照读音默写
              </p>
            </div>
          </div>

          <div className="mt-4 flex items-center justify-between gap-4 border-t border-stone-100 pt-4">
            <div className="min-w-0">
              <p className="text-xs font-medium text-stone-600">自动朗读</p>
              <p className="mt-0.5 text-[11px] leading-relaxed text-stone-400">
                默认关闭。开启后进入笔画复习会翻一个词读一个词（需系统装有中文语音）；
                复习页顶部的「自动朗读」按钮也是这同一个开关
              </p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={!!savedValue?.strokeAutoSpeak}
              onClick={() => void toggleStrokeAutoSpeak()}
              disabled={busy || !savedValue}
              className={
                "relative h-5 w-9 shrink-0 rounded-full transition disabled:opacity-50 " +
                (savedValue?.strokeAutoSpeak ? "bg-amber-600" : "bg-stone-300")
              }
            >
              <span
                className={
                  "absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all " +
                  (savedValue?.strokeAutoSpeak ? "left-4" : "left-0.5")
                }
              />
            </button>
          </div>
        </section>

        <AiSettings notify={notify} />

        <section className="mt-5 rounded-xl border border-stone-200 bg-white p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-sm font-medium text-stone-700">数据与备份</h2>
              <p className="mt-1 text-xs text-stone-500">
                数据默认只存本机；启用同步后云端会有一份整库快照。换电脑或重装前先导出备份
              </p>
            </div>
            <div className="flex shrink-0 gap-2">
              <button
                onClick={backup}
                className="rounded-md bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700"
              >
                导出备份
              </button>
              <button
                onClick={() => {
                  api.openDataDir().catch((e) => notify("error", errorMessage(e)));
                }}
                className="rounded-md border border-stone-200 px-3 py-1.5 text-xs text-stone-600 hover:bg-stone-50"
              >
                打开数据目录
              </button>
            </div>
          </div>

          <p className="mt-3 break-all rounded-md bg-stone-50 px-3 py-2 font-mono text-[11px] text-stone-500">
            {dataDir}
          </p>

          <div className="mt-3 flex items-center gap-2">
            <span className="shrink-0 text-xs text-stone-500">备份保存目录</span>
            <code className="min-w-0 flex-1 truncate rounded bg-stone-50 px-2 py-1 font-mono text-[11px] text-stone-500">
              {backupDir || (dataDir ? dataDir + "/backups" : "（默认备份目录）")}
            </code>
            <button
              onClick={pickBackupDir}
              className="shrink-0 rounded border border-stone-200 px-2 py-1 text-xs text-stone-600 hover:bg-stone-50"
            >
              选择…
            </button>
            {backupDir && (
              <button
                onClick={() => setBackupDir("")}
                className="shrink-0 rounded px-2 py-1 text-xs text-stone-400 hover:bg-stone-100"
              >
                默认
              </button>
            )}
          </div>

          <div className="mt-4 flex items-center justify-between">
            <span className="text-xs font-medium text-stone-600">已有备份</span>
            <button
              onClick={pickImportFile}
              className="rounded border border-stone-200 px-2 py-1 text-xs text-stone-600 hover:bg-stone-50"
            >
              从文件导入…
            </button>
          </div>

          <div className="mt-2">
            {backups.length === 0 ? (
              <p className="text-xs text-stone-400">还没有备份文件</p>
            ) : (
              <ul className="divide-y divide-stone-100">
                {backups.map((b) => (
                  <li key={b.path} className="flex items-center justify-between py-2">
                    <div className="min-w-0">
                      <p className="truncate text-sm text-stone-700">{b.name}</p>
                      <p className="text-xs text-stone-400">{Math.max(1, Math.round(b.size / 1024))} KB</p>
                    </div>
                    <button
                      onClick={() => setImporting({ name: b.name, path: b.path })}
                      className="shrink-0 rounded px-2 py-1 text-xs text-stone-500 hover:bg-stone-100"
                    >
                      导入
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <p className="mt-3 text-[11px] leading-relaxed text-stone-400">
            导入是合并而非覆盖：同编号同名的分组视为同一个，已存在的词跳过，不影响现有进度。
          </p>
        </section>

        <section className="mt-5 rounded-xl border border-stone-200 bg-white p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-sm font-medium text-stone-700">跨设备同步（自有数据库）</h2>
              <p className="mt-1 text-xs text-stone-500">
                数据仍以本机为准；配置后把整个库作为快照传到你自己的 MySQL，离线照常使用
              </p>
            </div>
            <label className="flex shrink-0 cursor-pointer items-center gap-2 text-xs text-stone-600">
              <input
                type="checkbox"
                checked={syncForm?.enabled ?? false}
                onChange={(e) =>
                  setSyncForm(syncForm ? { ...syncForm, enabled: e.target.checked } : syncForm)
                }
                className="h-4 w-4"
              />
              启用
            </label>
          </div>

          <div className="mt-4 grid grid-cols-2 gap-3">
            <label className="block">
              <span className="mb-1 block text-xs text-stone-500">主机（Tailscale 地址）</span>
              <input
                className={INPUT}
                value={syncForm?.host ?? ""}
                placeholder="100.91.153.120"
                onChange={(e) =>
                  setSyncForm(syncForm ? { ...syncForm, host: e.target.value } : syncForm)
                }
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-xs text-stone-500">端口</span>
              <input
                className={INPUT}
                value={portText}
                placeholder="4407"
                inputMode="numeric"
                onChange={(e) => {
                  // 只留数字，并且空串要能留住（否则删到一半就跳回 0，改不动旧值）；
                  // 超过 65535 时把显示值也钳住，否则输入框写着 70000、存下去却是 65535
                  let next = e.target.value.replace(/[^\d]/g, "").slice(0, 5);
                  if (next !== "" && Number(next) > 65535) next = "65535";
                  setPortText(next);
                  setSyncForm(
                    syncForm ? { ...syncForm, port: next === "" ? 0 : Number(next) } : syncForm,
                  );
                }}
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-xs text-stone-500">数据库名</span>
              <input
                className={INPUT}
                value={syncForm?.database ?? ""}
                placeholder="dictation_sync"
                onChange={(e) =>
                  setSyncForm(syncForm ? { ...syncForm, database: e.target.value } : syncForm)
                }
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-xs text-stone-500">账号</span>
              <input
                className={INPUT}
                value={syncForm?.user ?? ""}
                placeholder="dictation"
                onChange={(e) =>
                  setSyncForm(syncForm ? { ...syncForm, user: e.target.value } : syncForm)
                }
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-xs text-stone-500">
                密码{sync?.hasPassword ? "（留空表示不改）" : ""}
              </span>
              <input
                className={INPUT}
                type="password"
                value={syncPassword}
                placeholder="••••••••"
                onChange={(e) => setSyncPassword(e.target.value)}
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-xs text-stone-500">本机名称</span>
              <input
                className={INPUT}
                value={syncForm?.deviceName ?? ""}
                placeholder="书房电脑"
                onChange={(e) =>
                  setSyncForm(syncForm ? { ...syncForm, deviceName: e.target.value } : syncForm)
                }
              />
            </label>
          </div>

          <div className="mt-4 flex flex-wrap gap-2">
            <button
              onClick={saveSync}
              disabled={syncBusy || !!syncConflict}
              className="rounded-md bg-stone-800 px-3 py-1.5 text-xs font-medium text-white hover:bg-stone-900 disabled:opacity-50"
            >
              保存配置
            </button>
            <button
              onClick={testSync}
              disabled={syncBusy || !!syncConflict}
              className="rounded-md border border-stone-200 px-3 py-1.5 text-xs text-stone-600 hover:bg-stone-50 disabled:opacity-50"
            >
              测试连接
            </button>
            <button
              onClick={() => void runSync("push")}
              disabled={
                syncBusy ||
                !!syncConflict ||
                !!sync?.pending ||
                !!sync?.warning ||
                !sync?.enabled ||
                syncDirty
              }
              className="rounded-md bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
            >
              推送到云端
            </button>
            <button
              onClick={() => void runSync("pull")}
              disabled={
                syncBusy ||
                !!syncConflict ||
                !!sync?.pending ||
                !!sync?.warning ||
                !sync?.enabled ||
                syncDirty
              }
              className="rounded-md border border-amber-300 px-3 py-1.5 text-xs text-amber-800 hover:bg-amber-50 disabled:opacity-50"
            >
              从云端拉取
            </button>
            {(sync?.pending || sync?.lastError) && (
              <button
                onClick={() => setSyncAction(sync?.pending ? "discard" : "clearError")}
                disabled={syncBusy}
                className="rounded-md border border-stone-200 px-3 py-1.5 text-xs text-stone-500 hover:bg-stone-50 disabled:opacity-50"
              >
                {sync?.pending ? "放弃待应用快照" : "清除错误提示"}
              </button>
            )}
            {sync?.stateCorrupt && (
              <button
                onClick={() => setSyncAction("repair")}
                disabled={syncBusy}
                className="rounded-md border border-red-300 px-3 py-1.5 text-xs text-red-700 hover:bg-red-50 disabled:opacity-50"
              >
                重置同步状态
              </button>
            )}
          </div>

          {syncDirty && (
            <p className="mt-2 text-[11px] leading-relaxed text-amber-700">
              配置有改动还没保存（含密码）。推送/拉取用的是已保存的配置，请先点「保存配置」。
            </p>
          )}

          {(syncError || sync?.warning) && (
            <p className="mt-3 break-all rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[11px] leading-relaxed text-red-700">
              {syncError || sync?.warning}
              <br />
              {sync?.stateCorrupt
                ? "推送与拉取已暂停。可点「重置同步状态」隔离损坏文件并重建（本机词库与进度不变）。"
                : "推送与拉取已暂停。按上面的提示处理后即可恢复；凭据文件损坏时，把主机、账号、密码重新填一遍再保存会覆盖掉坏文件。"}
            </p>
          )}

          {sync?.lastError && sync.lastError.includes("替换成功") && (
            <p className="mt-3 break-all rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-[11px] leading-relaxed text-emerald-800">
              {sync.lastError}
              <br />
              本机数据已经是云端快照的内容，无需任何操作；点「清除错误提示」关闭这条消息。
            </p>
          )}

          {sync?.lastError && !sync.lastError.includes("替换成功") && (
            <p className="mt-3 break-all rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-[11px] leading-relaxed text-amber-800">
              上次同步快照没有顺利完成：{sync.lastError}
              <br />
              本机数据未被改动、云端快照仍在。
              {sync?.pending
                ? "快照已下载但这次没能应用（多因数据库被其他程序短暂占用），重启应用后会自动重试；也可点「放弃待应用快照」。"
                : "可以重新拉取，或点「清除错误提示」忽略。"}
            </p>
          )}

          <div className="mt-3 rounded-md bg-stone-50 px-3 py-2 text-[11px] leading-relaxed text-stone-500">
            {sync?.configured ? "已配置" : "未配置"} · 设备 {sync?.deviceId ? sync.deviceId.slice(0, 8) : "…"} ·{" "}
            {sync?.lastSyncAt
              ? "上次同步 " +
                (formatDateTime(sync.lastSyncAt) || sync.lastSyncAt.slice(0, 16).replace("T", " ")) +
                (sync.lastSyncFrom ? "（来自 " + sync.lastSyncFrom + "）" : "")
              : "还没同步过"}
            {sync?.pending && (
              <p className="mt-1 font-medium text-amber-700">
                有下载好的快照待应用，重启应用后生效；重启前同步按钮已禁用。
                <button
                  onClick={() => void restartNow()}
                  className="ml-2 rounded border border-amber-300 bg-white px-2 py-0.5 text-[11px] font-normal text-amber-800 hover:bg-amber-50"
                >
                  立即重启
                </button>
              </p>
            )}
          </div>

          <p className="mt-3 text-[11px] leading-relaxed text-stone-400">
            密码只写在本机数据目录的私有文件（类 Unix 下权限 600），不回传界面、不进备份文件。
            两台都改过时会先询问；覆盖前自动把当前库完整备份到 backups/sync/。未启用时程序完全离线。
          </p>
        </section>

        <section className="mt-5 rounded-xl border border-stone-200 bg-white p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-sm font-medium text-stone-700">版本与更新</h2>
              <p className="mt-1 text-xs text-stone-500">
                当前版本 v{u.currentVersion || "…"} · 新版本发布在 GitHub Release，安装包会先校验签名再安装
              </p>
            </div>
            <button
              onClick={() => void updater.checkForUpdate()}
              disabled={updateBusy || u.phase === "dev"}
              className="shrink-0 rounded-md border border-stone-200 px-3 py-1.5 text-xs text-stone-600 hover:bg-stone-50 disabled:opacity-50"
            >
              {u.phase === "checking" ? "检查中…" : "检查更新"}
            </button>
          </div>

          <div className="mt-4 flex items-center justify-between gap-4 border-t border-stone-100 pt-4">
            <div className="min-w-0">
              <p className="text-xs font-medium text-stone-600">启动时自动检查更新</p>
              <p className="mt-0.5 text-[11px] leading-relaxed text-stone-400">
                开启：每次启动后台静默查一次 GitHub Release；关闭：启动不联网检查，只能用上面的按钮手动检查
              </p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={!!savedValue?.autoCheckUpdates}
              onClick={() => void toggleAutoCheck()}
              disabled={busy || !savedValue}
              className={
                "relative h-5 w-9 shrink-0 rounded-full transition disabled:opacity-50 " +
                (savedValue?.autoCheckUpdates ? "bg-amber-600" : "bg-stone-300")
              }
            >
              <span
                className={
                  "absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all " +
                  (savedValue?.autoCheckUpdates ? "left-4" : "left-0.5")
                }
              />
            </button>
          </div>

          {u.phase === "dev" && (
            <p className="mt-3 text-xs text-stone-500">
              开发模式不检查更新；
              {savedValue?.autoCheckUpdates
                ? "打包后的程序每次启动会自动检查一次。"
                : "启动自动检查已关闭，打包后也只能手动检查。"}
            </p>
          )}
          {u.phase === "idle" && (
            <p className="mt-3 text-xs text-stone-500">
              {u.message ||
                (savedValue?.autoCheckUpdates
                  ? "暂无可用更新；程序每次启动会自动检查一次。"
                  : "暂无可用更新；启动自动检查已关闭，可手动检查。")}
            </p>
          )}
          {u.phase === "checking" && <p className="mt-3 text-xs text-stone-500">正在检查 GitHub Release…</p>}
          {u.phase === "error" && <p className="mt-3 break-all text-xs text-red-600">{u.message}</p>}

          {u.phase === "available" && (
            <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 p-3">
              <p className="text-sm font-medium text-amber-900">
                新版本 v{u.version}
                {u.date && formatDate(u.date) ? " · " + formatDate(u.date) : ""}
              </p>
              {u.notes && (
                // notes 是 release-notes.md 原文（Markdown）：渲染成排版而不是把 ### / [](...) 按文字吐出来
                <Markdown
                  source={u.notes}
                  className="mt-2 max-h-40 overflow-y-auto break-words text-xs leading-relaxed text-amber-800"
                  onOpenError={() => notify("error", "打不开链接，可稍后再试或到 GitHub Release 页面查看")}
                />
              )}
              <div className="mt-3 flex gap-2">
                <button
                  onClick={() => void updater.installUpdate()}
                  className="rounded-md bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700"
                >
                  下载并安装
                </button>
                <button
                  onClick={updater.skipVersion}
                  className="rounded-md border border-amber-200 px-3 py-1.5 text-xs text-amber-800 hover:bg-amber-100"
                >
                  忽略此版本
                </button>
              </div>
            </div>
          )}

          {(u.phase === "downloading" || u.phase === "installing") && (
            <div className="mt-3">
              <p className="text-xs text-stone-600">
                {u.phase === "downloading" ? "正在下载更新包…" : "下载完成，正在校验签名并安装…"}
              </p>
              <div className="mt-2 h-2 w-full overflow-hidden rounded-full bg-stone-100">
                <div
                  className="h-full rounded-full bg-stone-700 transition-all"
                  style={{ width: (u.progress ?? (u.phase === "installing" ? 100 : 0)) + "%" }}
                />
              </div>
              <p className="mt-1 text-[11px] text-stone-400">
                {u.progress !== null ? u.progress + "% · " : ""}
                {formatBytes(u.downloaded)}
                {u.total > 0 ? " / " + formatBytes(u.total) : ""}
              </p>
            </div>
          )}

          {u.phase === "ready" && (
            <div className="mt-3 flex items-center justify-between gap-3 rounded-md border border-green-200 bg-green-50 px-3 py-2">
              <span className="text-xs text-green-800">{u.message}</span>
              <button
                onClick={() => void updater.restart()}
                className="shrink-0 rounded-md bg-green-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-green-800"
              >
                立即重启
              </button>
            </div>
          )}

          <p className="mt-3 text-[11px] leading-relaxed text-stone-400">
            未配置自有同步服务时，唯一的联网行为是检查更新；配置后学习数据只以整库快照形式传到你自己的数据库。
            离线时程序照常使用；安装前会校验发布包签名。
          </p>
        </section>
      </div>

      {importing && (
        <ConfirmModal
          title="导入备份"
          body={
            "将合并 " + importing.name + " 中的分组和词条。已存在的词条不会被覆盖，现有练习进度保持不变。"
          }
          confirmText="导入"
          onCancel={() => setImporting(null)}
          onConfirm={doImport}
        />
      )}

      {syncConflict && (
        <ConfirmModal
          title="同步存在冲突"
          body={syncConflict.message}
          confirmText="继续覆盖"
          cancelText="取消"
          onCancel={() => setSyncConflict(null)}
          onConfirm={confirmSync}
        />
      )}

      {syncAction && (
        <ConfirmModal
          title={
            syncAction === "repair"
              ? "重置同步状态"
              : syncAction === "discard"
                ? "放弃待应用快照"
                : "清除错误提示"
          }
          body={
            syncAction === "repair"
              ? "损坏的 .sync-state.json 会被移到 backups/sync/ 留档，然后重建一份新状态；如果还有没应用的待同步快照，也会一起作废（云端快照仍在，可以重新拉取）。本机词库和进度不会变，但本机记录的版本号会归零，下一次推送或拉取会先让你确认是否覆盖云端。"
              : syncAction === "discard"
                ? "已下载但还没应用的快照会被丢弃，本机词库和进度不会变。云端快照仍在，之后可以重新拉取。"
                : "只清掉「上次同步没顺利完成」的提示文字，不动本机数据、不动云端快照，也不改动同步状态。"
          }
          confirmText={
            syncAction === "repair" ? "确认重置" : syncAction === "discard" ? "确认放弃" : "确认清除"
          }
          cancelText="取消"
          onCancel={() => setSyncAction(null)}
          onConfirm={confirmSyncAction}
        />
      )}

      <Toast notice={notice} />
    </div>
  );
}
