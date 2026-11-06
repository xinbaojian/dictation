/** 工具注册表：顶栏一级菜单与各工具的二级侧栏菜单都登记在这里。
 *  加第 3 个工具：Route 加变体 + TOOLS / NAV_BY_TOOL 各加一项 + App.tsx 挂页面。 */

export type ToolId = "dictation" | "math";

/** 全局路由：name 是唯一判别字段（工具 id 或 "settings"）；settings 是全局页（顶栏入口）。
 *  注意：各工具的 page 取值必须全局唯一——App.tsx 的 routeKey 靠它判等（横幅拉取时机 / quietRoute）。 */
export type Route =
  // ── 生词默写 ──
  | { name: "dictation"; page: "pick" }
  | { name: "dictation"; page: "wordbook" }
  | { name: "dictation"; page: "mistakes" }
  | { name: "dictation"; page: "history" }
  | { name: "dictation"; page: "quiz"; sessionId: number }
  | { name: "dictation"; page: "review"; sessionId: number }
  | { name: "dictation"; page: "stroke"; groupId: number | null; title: string }
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

/** 二级菜单项：route 直接给出完整路由（限无参数页面），App.tsx 挂载时无需强转 */
export interface NavItem {
  /** 高亮判定用的 key（与无参数页面的 page 同名） */
  key: string;
  label: string;
  route: Route;
}

/** 各工具的二级菜单（侧栏）；「设置」是全局页，不在任何工具菜单里。
 *  注意：label 保持 ≤2 个汉字——侧栏按钮宽 w-11（44px），更长的文案会换行。 */
export const NAV_BY_TOOL: Record<ToolId, readonly NavItem[]> = {
  dictation: [
    { key: "pick", label: "默写", route: { name: "dictation", page: "pick" } },
    { key: "wordbook", label: "词表", route: { name: "dictation", page: "wordbook" } },
    { key: "mistakes", label: "错题", route: { name: "dictation", page: "mistakes" } },
    { key: "history", label: "记录", route: { name: "dictation", page: "history" } },
  ],
  math: [{ key: "sheet", label: "出题", route: { name: "math", page: "sheet" } }],
};
