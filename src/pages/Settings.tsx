import { useEffect, useState } from "react";
import ConfirmModal from "../components/ConfirmModal";
import { Toast, useNotice } from "../components/Toast";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorMessage } from "../lib/ipc";
import type { BackupInfo, Settings as SettingsType } from "../types";

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

export default function Settings() {
  const [value, setValue] = useState<SettingsType | null>(null);
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [dataDir, setDataDir] = useState("");
  const [backupDir, setBackupDir] = useState("");
  const [importing, setImporting] = useState<{ name: string; path: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const { notice, notify } = useNotice();

  useEffect(() => {
    (async () => {
      try {
        const [cfg, list, dir] = await Promise.all([
          api.getSettings(),
          api.listBackups(),
          api.dataDirPath(),
        ]);
        setValue(cfg);
        setBackups(list);
        setDataDir(dir);
      } catch (e) {
        notify("error", errorMessage(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const save = async () => {
    if (!value || busy) return;
    setBusy(true);
    try {
      setValue(await api.saveSettings(value));
      notify("ok", "设置已保存");
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

  if (!value) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
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

            <div>
              <p className="mb-1.5 text-xs text-stone-500">连字抽词：下一个词优先和上一个词有相同的字</p>
              <div className="flex gap-2">
                {[
                  { value: false, label: "关闭" },
                  { value: true, label: "开启" },
                ].map((o) => (
                  <button
                    key={o.label}
                    onClick={() => setValue({ ...value, chainByChar: o.value })}
                    className={
                      "rounded-md border px-3 py-1.5 text-sm " +
                      (value.chainByChar === o.value
                        ? "border-stone-800 bg-stone-800 text-white"
                        : "border-stone-200 text-stone-600 hover:border-stone-300")
                    }
                  >
                    {o.label}
                  </button>
                ))}
              </div>
              <p className="mt-1.5 text-[11px] leading-relaxed text-stone-400">
                「春天」之后优先出「秋天」「夏天」，帮孩子按字形联想；剩下的词里没有同字的就照常随机。只在随机抽词时生效。
              </p>
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
              <h2 className="text-sm font-medium text-stone-700">数据与备份</h2>
              <p className="mt-1 text-xs text-stone-500">
                数据只存在本机；换电脑或重装前先导出备份
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

      <Toast notice={notice} />
    </div>
  );
}
