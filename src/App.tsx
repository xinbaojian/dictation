import { useState } from "react";
import appIconUrl from "../app-icon.svg?url";
import { useAutoUpdate } from "./lib/updater";
import History from "./pages/History";
import Mistakes from "./pages/Mistakes";
import Pick from "./pages/Pick";
import PrintSheet from "./pages/PrintSheet";
import Quiz from "./pages/Quiz";
import Review from "./pages/Review";
import SettingsPage from "./pages/Settings";
import Wordbook from "./pages/Wordbook";

type Route =
  | { name: "pick" }
  | { name: "wordbook" }
  | { name: "mistakes" }
  | { name: "history" }
  | { name: "settings" }
  | { name: "quiz"; sessionId: number }
  | { name: "review"; sessionId: number }
  | { name: "print"; mode: "mistakes" | "group"; groupId: number | null; title: string };

const NAV = [
  { name: "pick", label: "默写" },
  { name: "wordbook", label: "词表" },
  { name: "mistakes", label: "错题" },
  { name: "history", label: "记录" },
] as const;

export default function App() {
  const [route, setRoute] = useState<Route>({ name: "pick" });
  const updater = useAutoUpdate();

  const activeNav = route.name === "quiz" || route.name === "review" ? "pick" : route.name;
  const startSession = (sessionId: number) => setRoute({ name: "quiz", sessionId });
  const openReview = (sessionId: number) => setRoute({ name: "review", sessionId });

  const updateAvailable = updater.state.phase === "available" && !updater.state.dismissed;
  // 默写、批改、打印、设置页不弹横幅：前者不能打断孩子，设置页自己有完整卡片
  const showUpdateBanner =
    updateAvailable && !["quiz", "review", "print", "settings"].includes(route.name);

  return (
    <div className="flex h-full">
      <nav className="no-print flex w-16 shrink-0 flex-col items-center gap-1 border-r border-stone-200 bg-white py-3">
        <img
          src={appIconUrl}
          alt=""
          draggable={false}
          className="mb-3 h-9 w-9 select-none"
        />
        {NAV.map((item) => (
          <button
            key={item.name}
            onClick={() => setRoute({ name: item.name } as Route)}
            className={
              "w-11 rounded-md py-2 text-xs transition " +
              (activeNav === item.name
                ? "bg-stone-900 font-medium text-white"
                : "text-stone-500 hover:bg-stone-100")
            }
          >
            {item.label}
          </button>
        ))}
        <button
          onClick={() => setRoute({ name: "settings" })}
          title="设置"
          className={
            "relative mt-auto w-11 rounded-md py-2 text-xs transition " +
            (route.name === "settings"
              ? "bg-stone-900 font-medium text-white"
              : "text-stone-400 hover:bg-stone-100")
          }
        >
          设置
          {updateAvailable && (
            <span className="absolute right-1 top-1 h-2 w-2 rounded-full bg-amber-500" />
          )}
        </button>
      </nav>

      <main className="flex min-w-0 flex-1 flex-col">
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
              className="shrink-0 rounded px-2 py-1 text-amber-700 hover:bg-amber-100"
            >
              稍后
            </button>
          </div>
        )}

        {route.name === "wordbook" && (
          <Wordbook
            onPrint={(groupId, title) =>
              setRoute({ name: "print", mode: "group", groupId, title })
            }
          />
        )}
        {route.name === "mistakes" && (
          <Mistakes
            onStart={startSession}
            onGoWordbook={() => setRoute({ name: "wordbook" })}
            onPrint={(groupId, title) => setRoute({ name: "print", mode: "mistakes", groupId, title })}
          />
        )}
        {route.name === "print" && (
          <PrintSheet
            mode={route.mode}
            groupId={route.groupId}
            title={route.title}
            onBack={() => setRoute({ name: route.mode === "mistakes" ? "mistakes" : "wordbook" } as Route)}
          />
        )}
        {route.name === "history" && <History onStart={startSession} onReview={openReview} />}
        {route.name === "settings" && <SettingsPage updater={updater} />}
        {route.name === "quiz" && (
          <Quiz
            key={route.sessionId}
            sessionId={route.sessionId}
            onFinish={openReview}
            onExit={() => setRoute({ name: "pick" })}
          />
        )}
        {route.name === "review" && (
          <Review
            key={route.sessionId}
            sessionId={route.sessionId}
            onDone={() => setRoute({ name: "pick" })}
            onBack={() => setRoute({ name: "pick" })}
          />
        )}
        {route.name === "pick" && (
          <Pick
            onStart={startSession}
            onReview={openReview}
            onGoWordbook={() => setRoute({ name: "wordbook" })}
          />
        )}
      </main>
    </div>
  );
}
