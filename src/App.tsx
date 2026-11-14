import { useEffect, useState } from "react";
import appIconUrl from "../app-icon.svg?url";
import { api } from "./lib/ipc";
import { useAutoUpdate } from "./lib/updater";
import History from "./pages/History";
import LookupPage from "./pages/Lookup";
import MathSheetPage from "./pages/math/MathSheet";
import Mistakes from "./pages/Mistakes";
import Pick from "./pages/Pick";
import PrintSheet from "./pages/PrintSheet";
import Quiz from "./pages/Quiz";
import Review from "./pages/Review";
import SettingsPage from "./pages/Settings";
import StrokeReview from "./pages/StrokeReview";
import Wordbook from "./pages/Wordbook";
import { NAV_BY_TOOL, TOOLS, type Route, type ToolId } from "./tools/registry";

export default function App() {
  const [route, setRoute] = useState<Route>(TOOLS[0].home);
  // 设置是全局页：进了设置后，顶栏仍保持「从哪个工具进来的」高亮
  const [lastTool, setLastTool] = useState<ToolId>("dictation");
  const updater = useAutoUpdate();
  // 启动时看一眼同步状态：拉取后重启失败、或有快照待生效时，用户不该只能靠「自己想起来去设置页看」
  const [syncNotice, setSyncNotice] = useState<{ title: string; message: string } | null>(null);
  // 「知道了」只消音当前这条内容；内容变了（新事件）仍要重新提示
  const [dismissedNotice, setDismissedNotice] = useState<string | null>(null);

  // 各工具 page 取值全局唯一（pick/wordbook/…/sheet），可当路由 key 用
  const routeKey = route.name === "settings" ? "settings" : route.page;

  useEffect(() => {
    // 每次离开设置页都重取一次：用户可能刚在那里「放弃快照 / 清除错误提示」，
    // 只在挂载时读一次会让横幅与实际状态相反
    if (route.name === "settings") return;
    let alive = true;
    (async () => {
      try {
        const s = await api.syncStatus();
        if (!alive) return;
        let next: { title: string; message: string } | null = null;
        if (s.warning) next = { title: "同步配置需要处理", message: s.warning };
        else if (s.lastError?.includes("替换成功"))
          // 库已经换成本次快照（坏库恢复 / 状态没写成）：结果通报，不是失败告警
          next = { title: "云端快照已生效", message: s.lastError };
        else if (s.lastError)
          next = {
            title: s.pending ? "同步快照待生效" : "上次同步没有完成",
            message: s.lastError,
          };
        else if (s.pending)
          next = { title: "同步快照待生效", message: "已下载云端快照，重启应用后生效" };
        // 用户点过「知道了」的那条内容不再弹；新内容（提示变了）照常弹
        setSyncNotice(next && next.message === dismissedNotice ? null : next);
      } catch {
        // 读不到同步状态不影响使用，设置页里有完整提示
      }
    })();
    return () => {
      alive = false;
    };
  }, [routeKey, dismissedNotice]);

  const navigate = (next: Route) => {
    if (next.name !== "settings") setLastTool(next.name);
    setRoute(next);
  };
  const startSession = (sessionId: number) => navigate({ name: "dictation", page: "quiz", sessionId });
  const openReview = (sessionId: number) => navigate({ name: "dictation", page: "review", sessionId });

  const activeTool: ToolId = route.name === "settings" ? lastTool : route.name;

  // 侧栏高亮：quiz/review/stroke 归「默写 / 错题」，打印页按来源归「词表 / 错题」
  let activeNav: string;
  if (route.name === "settings") activeNav = "";
  else if (route.name === "math") activeNav = "sheet";
  else if (route.name === "lookup") activeNav = "lookup";
  else if (route.page === "quiz" || route.page === "review") activeNav = "pick";
  else if (route.page === "stroke") activeNav = "mistakes";
  else if (route.page === "print") activeNav = route.mode === "mistakes" ? "mistakes" : "wordbook";
  else activeNav = route.page;

  // 生词默写路由收窄（其它工具 / 设置页时为 null），页面挂载判断都从它取
  const dr = route.name === "dictation" ? route : null;

  const updateAvailable = updater.state.phase === "available" && !updater.state.dismissed;
  // 默写、批改、笔画复习、打印、设置页不弹横幅：前几个不能打断孩子，设置页自己有完整卡片
  const quietRoute = ["quiz", "review", "stroke", "print", "settings", "lookup"].includes(routeKey);
  const showUpdateBanner = updateAvailable && !quietRoute;
  const showSyncBanner = !!syncNotice && !quietRoute;
  const settingsDot = updateAvailable || !!syncNotice;

  return (
    <div className="flex h-full flex-col">
      <header className="no-print flex h-11 shrink-0 items-center gap-1 border-b border-stone-200 bg-white px-3">
        <img src={appIconUrl} alt="" draggable={false} className="mr-2 h-6 w-6 select-none" />
        {TOOLS.map((t) => (
          <button
            key={t.id}
            onClick={() => navigate(t.home)}
            className={
              "rounded-md px-3 py-1.5 text-xs transition " +
              (activeTool === t.id
                ? "bg-stone-900 font-medium text-white"
                : "text-stone-500 hover:bg-stone-100")
            }
          >
            {t.label}
          </button>
        ))}
        <button
          onClick={() => setRoute({ name: "settings" })}
          title="设置"
          className={
            "relative ml-auto rounded-md px-3 py-1.5 text-xs transition " +
            (route.name === "settings"
              ? "bg-stone-900 font-medium text-white"
              : "text-stone-400 hover:bg-stone-100")
          }
        >
          设置
          {settingsDot && (
            <span className="absolute right-1 top-1 h-2 w-2 rounded-full bg-amber-500" />
          )}
        </button>
      </header>

      {showUpdateBanner && (
        <div className="no-print flex shrink-0 items-center gap-3 border-b border-amber-200 bg-amber-50 px-6 py-2 text-xs text-amber-900">
          <span className="shrink-0 font-medium">发现新版本 v{updater.state.version}</span>
          <span className="hidden min-w-0 flex-1 truncate text-amber-700 sm:block">
            更新包已发布到 GitHub Release，可在设置中下载安装
          </span>
          <button
            onClick={() => setRoute({ name: "settings" })}
            className="shrink-0 rounded bg-amber-600 px-3 py-1 font-medium text-white hover:bg-amber-700"
          >
            去更新
          </button>
          <button
            onClick={updater.dismiss}
            className="shrink-0 rounded px-3 py-1 text-amber-700 hover:bg-amber-100"
          >
            稍后
          </button>
        </div>
      )}

      {showSyncBanner && (
        <div className="no-print flex shrink-0 items-center gap-3 border-b border-amber-200 bg-amber-50 px-6 py-2 text-xs text-amber-900">
          <span className="shrink-0 font-medium">{syncNotice!.title}</span>
          <span className="hidden min-w-0 flex-1 truncate text-amber-700 sm:block">
            {syncNotice!.message}
          </span>
          <button
            onClick={() => setRoute({ name: "settings" })}
            className="shrink-0 rounded bg-amber-600 px-3 py-1 font-medium text-white hover:bg-amber-700"
          >
            去设置
          </button>
          <button
            onClick={() => {
              setDismissedNotice(syncNotice!.message);
              setSyncNotice(null);
            }}
            className="shrink-0 rounded px-3 py-1 text-amber-700 hover:bg-amber-100"
          >
            知道了
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <nav className="no-print flex w-16 shrink-0 flex-col items-center gap-1 border-r border-stone-200 bg-white py-3">
          {NAV_BY_TOOL[activeTool].map((item) => (
            <button
              key={item.key}
              onClick={() => navigate(item.route)}
              className={
                "w-11 rounded-md py-2 text-xs transition " +
                (activeNav === item.key
                  ? "bg-stone-900 font-medium text-white"
                  : "text-stone-500 hover:bg-stone-100")
              }
            >
              {item.label}
            </button>
          ))}
        </nav>

        <main className="flex min-w-0 flex-1 flex-col">
          {dr?.page === "wordbook" && (
            <Wordbook
              onPrint={(groupId, title) =>
                navigate({ name: "dictation", page: "print", mode: "group", groupId, title })
              }
            />
          )}
          {dr?.page === "mistakes" && (
            <Mistakes
              onStart={startSession}
              onGoWordbook={() => navigate({ name: "dictation", page: "wordbook" })}
              onStroke={(groupId, title, wordId) =>
                navigate({ name: "dictation", page: "stroke", groupId, title, wordId })
              }
              onPrint={(groupId, title) =>
                navigate({ name: "dictation", page: "print", mode: "mistakes", groupId, title })
              }
            />
          )}
          {dr?.page === "stroke" && (
            <StrokeReview
              groupId={dr.groupId}
              title={dr.title}
              wordId={dr.wordId}
              onBack={() => navigate({ name: "dictation", page: "mistakes" })}
            />
          )}
          {dr?.page === "print" && (
            <PrintSheet
              mode={dr.mode}
              groupId={dr.groupId}
              title={dr.title}
              onBack={() =>
                navigate(
                  dr.mode === "mistakes"
                    ? { name: "dictation", page: "mistakes" }
                    : { name: "dictation", page: "wordbook" },
                )
              }
            />
          )}
          {dr?.page === "history" && <History onStart={startSession} onReview={openReview} />}
          {route.name === "settings" && <SettingsPage updater={updater} />}
          {dr?.page === "quiz" && (
            <Quiz
              key={dr.sessionId}
              sessionId={dr.sessionId}
              onFinish={openReview}
              onExit={() => navigate({ name: "dictation", page: "pick" })}
            />
          )}
          {dr?.page === "review" && (
            <Review
              key={dr.sessionId}
              sessionId={dr.sessionId}
              onDone={() => navigate({ name: "dictation", page: "pick" })}
              onBack={() => navigate({ name: "dictation", page: "pick" })}
            />
          )}
          {dr?.page === "pick" && (
            <Pick
              onStart={startSession}
              onReview={openReview}
              onGoWordbook={() => navigate({ name: "dictation", page: "wordbook" })}
            />
          )}
          {route.name === "math" && <MathSheetPage />}
          {route.name === "lookup" && <LookupPage />}
        </main>
      </div>
    </div>
  );
}
