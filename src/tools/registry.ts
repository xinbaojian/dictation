/** 工具注册表：顶栏一级菜单与各工具的二级侧栏菜单都登记在这里。
 *  加第 3 个工具：Route 加变体 + TOOLS / NAV_BY_TOOL 各加一项 + App.tsx 挂页面。 */

export type ToolId = "dictation" | "math";

/** 全局路由：name 是唯一判别字段（工具 id 或 "settings"）；settings 是全局页（顶栏入口） */
export type Route =
  // ── 生词默写 ──
  | { name: "dictation"; page: "pick" }
  | { name: "dictation"; page: "wordbook" }
  | { name: "dictation"; page: "mistakes" }
  | { name: "dictation"; page: "history" }
  | { name: "dictation"; page: "quiz"; sessionId: number }
  | { name: "dictation"; page: "review"; sessionId: number }
  | { name: "dictation"; page: "print"; mode: "mistakes" | "group"; groupId: number | null; title: string }
  // ── 口算练习 ──
  | { name: "math"; page: "sheet" }
  // ── 全局 ──
  | { name: "settings" };

export interface ToolDef {
  id: ToolId;
  /** 顶栏 tab 文案 */
  label: string;
  /** 点顶栏 tab 进入的默认路由 */
  home: Route;
}

export const TOOLS: ToolDef[] = [
  { id: "dictation", label: "生词默写", home: { name: "dictation", page: "pick" } },
  { id: "math", label: "口算练习", home: { name: "math", page: "sheet" } },
];

/** 各工具的二级菜单（侧栏）；「设置」是全局页，不在任何工具菜单里 */
export const NAV_BY_TOOL: Record<ToolId, readonly { page: string; label: string }[]> = {
  dictation: [
    { page: "pick", label: "默写" },
    { page: "wordbook", label: "词表" },
    { page: "mistakes", label: "错题" },
    { page: "history", label: "记录" },
  ],
  math: [{ page: "sheet", label: "出题打印" }],
};
