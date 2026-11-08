import { useEffect, useState } from "react";
import ConfirmModal from "./ConfirmModal";
import { api, errorMessage } from "../lib/ipc";
import type { AiConfigView } from "../types";

/** 设置页「AI 讲解」区块：OpenAI 兼容配置（默认关闭，配置后复习页才有「生成讲解」按钮）。
 *  API Key 只存本机数据目录（.ai-config.json），界面永远只显示掩码；留空 = 不修改。 */
export default function AiSettings({ notify }: { notify: (kind: "ok" | "error", text: string) => void }) {
  const [view, setView] = useState<AiConfigView | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [maxTokens, setMaxTokens] = useState("");
  const [busy, setBusy] = useState(false);
  const [askClear, setAskClear] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const v = await api.getAiConfig();
        setView(v);
        setBaseUrl(v.baseUrl);
        setModel(v.model);
        setMaxTokens(v.maxOutputTokens != null ? String(v.maxOutputTokens) : "");
      } catch (e) {
        notify("error", "AI 配置读取失败：" + errorMessage(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** 最大输出 tokens：空串 → null（不限制，请求不带 max_tokens）；非法输入返回 "invalid"，由调用方统一报错 */
  const parseMaxTokens = (): number | null | "invalid" => {
    const t = maxTokens.trim();
    if (!t) return null;
    const n = Number(t);
    if (!Number.isInteger(n) || n < 1 || n > 1_000_000) return "invalid";
    return n;
  };

  const save = async () => {
    if (busy) return;
    const mt = parseMaxTokens();
    if (mt === "invalid") {
      notify("error", "最大输出 tokens 要是 1~1000000 的整数（留空 = 不限制）");
      return;
    }
    setBusy(true);
    try {
      const v = await api.saveAiConfig(baseUrl.trim(), model.trim(), apiKey.trim() || null, mt);
      setView(v);
      setApiKey("");
      notify("ok", "AI 配置已保存");
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const test = async () => {
    if (busy) return;
    const mt = parseMaxTokens();
    if (mt === "invalid") {
      notify("error", "最大输出 tokens 要是 1~1000000 的整数（留空 = 不限制）");
      return;
    }
    setBusy(true);
    try {
      const reply = await api.testAiConfig(baseUrl.trim(), model.trim(), apiKey.trim() || null, mt);
      notify("ok", "连接正常，模型回复：" + reply);
    } catch (e) {
      notify("error", "连接失败：" + errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const v = await api.clearAiConfig();
      setView(v);
      setBaseUrl("");
      setModel("");
      setApiKey("");
      setMaxTokens("");
      // 后端 I/O 失败已由 catch 报错；这里按返回值判定，configured 仍为 true 属理论不可达的防御分支
      if (!v.configured) notify("ok", "AI 配置已清除，应用回到纯离线状态");
      else notify("error", "清除失败：配置仍生效，请重试");
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  if (!view) return null; // 读取中不占位，避免闪一块空区块

  const inputCls =
    "mt-1 w-full rounded-md border border-stone-200 px-2.5 py-1.5 text-sm text-stone-700 focus:border-amber-400 focus:outline-none";

  return (
    <section className="mt-5 rounded-xl border border-stone-200 bg-white p-5">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="text-sm font-medium text-stone-700">AI 讲解（可选）</h2>
        <span className={view.configured ? "text-xs text-emerald-600" : "text-xs text-stone-400"}>
          {view.configured ? "已启用" : "默认关闭"}
        </span>
      </div>
      <p className="mt-2 text-xs leading-5 text-stone-500">
        配置后可在错题的笔画复习页生成「巧记讲解卡」（部件拆解、记忆口诀、例句、易混字辨析）。
        生成时会把<strong>词语本身及其错字情况、错误次数</strong>发送到你配置的服务，除此之外不发送任何数据；
        内容缓存在本机，生成一次之后离线也能看。不配置则完全离线，与现在一样。
      </p>
      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        <label className="block text-xs font-medium text-stone-600">
          服务地址（Base URL）
          <input
            className={inputCls}
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="https://api.deepseek.com 或 https://open.bigmodel.cn/api/paas/v4"
          />
        </label>
        <label className="block text-xs font-medium text-stone-600">
          模型名
          <input
            className={inputCls}
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="deepseek-chat / glm-4-flash 等"
          />
        </label>
        <label className="block text-xs font-medium text-stone-600">
          API Key
          <input
            type="password"
            className={inputCls}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={
              view.configured
                ? `已保存 ${view.keyMask}（留空不修改）`
                : "sk-…（只存在本机数据目录，不上传）"
            }
          />
        </label>
        <label className="block text-xs font-medium text-stone-600">
          最大输出 tokens（可选）
          <input
            className={inputCls}
            value={maxTokens}
            onChange={(e) => setMaxTokens(e.target.value)}
            inputMode="numeric"
            placeholder="留空 = 不限制；思考型模型建议 100000"
          />
        </label>
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-2">
        <button
          onClick={() => void save()}
          disabled={busy}
          className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-50"
        >
          保存配置
        </button>
        <button
          onClick={() => void test()}
          disabled={busy || (!(baseUrl.trim() && model.trim()) && !view.configured)}
          className="rounded-md border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-40"
        >
          测试连接
        </button>
        {view.configured && (
          <button
            onClick={() => setAskClear(true)}
            className="px-1 text-xs text-stone-400 hover:text-rose-600"
          >
            清除配置
          </button>
        )}
      </div>

      {askClear && (
        <ConfirmModal
          title="清除 AI 配置？"
          body={
            "清除后复习页不再出现「生成讲解」按钮，应用回到纯离线。\nAPI Key 会从数据目录删除；已生成的讲解卡仍保留在本机。"
          }
          confirmText="清除"
          cancelText="再想想"
          onCancel={() => setAskClear(false)}
          onConfirm={() => {
            setAskClear(false);
            void clear();
          }}
        />
      )}
    </section>
  );
}
