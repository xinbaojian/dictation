import { useState } from "react";
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

  const activeNav = route.name === "quiz" || route.name === "review" ? "pick" : route.name;
  const startSession = (sessionId: number) => setRoute({ name: "quiz", sessionId });
  const openReview = (sessionId: number) => setRoute({ name: "review", sessionId });

  return (
    <div className="flex h-full">
      <nav className="no-print flex w-16 shrink-0 flex-col items-center gap-1 border-r border-stone-200 bg-white py-3">
        <div className="mb-3 flex h-9 w-9 items-center justify-center rounded-lg bg-amber-600 text-sm font-bold text-white">
          默
        </div>
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
            "mt-auto w-11 rounded-md py-2 text-xs transition " +
            (route.name === "settings"
              ? "bg-stone-900 font-medium text-white"
              : "text-stone-400 hover:bg-stone-100")
          }
        >
          设置
        </button>
      </nav>

      <main className="flex min-w-0 flex-1 flex-col">
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
        {route.name === "settings" && <SettingsPage />}
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
