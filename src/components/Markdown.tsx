// 更新说明（GitHub Release notes）用的极简 Markdown 渲染。
//
// 为什么不装 markdown 库：notes 由 scripts/gen-release-notes.mjs 生成，语法子集是固定的，
// 引入 remark 那套依赖树只为渲染十几行 changelog 不划算。
// 块级：标题 / 段落 / 有序·无序列表 / 引用 / 分割线 / 围栏代码块（见下方 Block 联合类型）；
// 行内：粗体 / 斜体 / 行内代码 / 链接（见下方 Inline 联合类型）。
// 更要紧的一条：**绝不用 dangerouslySetInnerHTML**。notes 来自网络，
// 一旦当成 HTML 塞进 DOM，等于让远端内容在 WebView 里执行。
//
// 用法：<Markdown source={u.notes} className="text-xs text-amber-800" />
import { Fragment, useMemo, type ReactNode } from "react";
import { openExternalLink } from "../lib/external";

/* ---------------------------------- 行内 ---------------------------------- */

type Inline =
  | { kind: "text"; value: string }
  | { kind: "code"; value: string }
  | { kind: "strong"; children: Inline[] }
  | { kind: "em"; children: Inline[] }
  | { kind: "link"; href: string; children: Inline[] };

// 只按 ASCII 字母数字判断「贴着单词」：file_name / Word_1 里的下划线不该变成斜体，
// 但中文没有词内下划线这回事，_斜体_ 夹在汉字中间时必须照常生效。
const isWordChar = (c: string | undefined): boolean => !!c && /^[A-Za-z0-9]$/.test(c);

/** 只认 http/https：notes 来自网络，`[点我](javascript:...)` / `file:` 一律退化成普通文字 */
const isHttpUrl = (url: string): boolean => /^https?:\/\//i.test(url);

const isHttpAt = (s: string, i: number): boolean => isHttpUrl(s.slice(i, i + 8));

// 链接目标长度上限：既够 release notes 里任何 URL，也挡住畸形 notes 把括号配平扫描拖成平方级
const HREF_MAX = 2048;

// 全角标点：URL 后面接中文时通常没有空格，这些字符一律截断（半角标点走另两条终止条件）。
// 代价：路径里含全角括号 / 引号 / 破折号的 IRI（如 `wiki/《三体》`）会被截断——notes 由本仓库脚本
// 生成、链接只有 GitHub commit / compare，实际打不到，就不为此加复杂度。
const CJK_PUNCT = /[，。；：！？、（）「」『』“”‘’《》〈〉…—～]/;

/** 从 "(" 之后读链接目标：括号按深度配平（`.../Foo_(bar)` 是合法目标，不能停在第一个 ")"），
 *  读不到配平的 ")" 或超长就返回 null，调用方把这一段当普通文字。 */
function readHref(src: string, start: number): { href: string; end: number } | null {
  let depth = 0;
  const limit = Math.min(src.length, start + HREF_MAX);
  for (let j = start; j < limit; j += 1) {
    const c = src[j];
    if (c === "(") {
      depth += 1;
    } else if (c === ")") {
      if (depth === 0) return { href: src.slice(start, j), end: j };
      depth -= 1;
    }
  }
  return null;
}

/** 裸链接（正文里直接出现的 https://…）：自己逐字符扫，既不用切剩余字符串，也能像 readHref 一样
 *  配平括号，免得上例那种 `.../Foo_(bar)` 被截成 `.../Foo_`（等于把用户送去错页面）。 */
function readBareUrl(src: string, start: number): { url: string; end: number } | null {
  const limit = Math.min(src.length, start + HREF_MAX);
  const capped = limit < src.length;
  let j = start;
  let depth = 0;
  while (j < limit) {
    const c = src[j];
    const code = src.charCodeAt(j);
    // 中文正文经常不给 URL 后面留空格（「修了 https://…/12，详见文档」），全角标点必须当终止符，
    // 否则整句会被吞进 href，点出去就是错页面。ASCII 走码值快判，别每个字符都跑正则。
    // 0xa0 是不换行空格、0x3000 是全角空格，也一并当终止符（它们在 CJK_PUNCT 里查不到）。
    if (
      code === 0x20 ||
      code === 0x09 ||
      code === 0x0a ||
      code === 0x0d ||
      code === 0xa0 ||
      code === 0x3000 ||
      code === 0x3c ||
      code === 0x3e
    ) {
      break;
    }
    if (code >= 0x2000 && CJK_PUNCT.test(c)) break;
    if (c === "(") {
      depth += 1;
    } else if (c === ")") {
      if (depth === 0) break;
      depth -= 1;
    }
    j += 1;
  }
  // 括号配不平 / 超过长度上限：跟 readHref 一样整段退回普通文字，不拿半个 URL 当目标
  if (depth !== 0) return null;
  if (j >= limit && capped) return null;
  const url = src.slice(start, j).replace(/[.,;:!?、。，"']+$/, "");
  if (!/^https?:\/\/./i.test(url)) return null;
  return { url, end: start + url.length };
}

/** 行内语法扫描：按出现顺序切分，遇到对称标记就递归解析内部。
 *  allowBareLink=false 用于链接文字内部：否则 `[https://a](https://b)` 会渲染出 <a> 套 <a>。 */
function parseInline(src: string, allowBareLink = true): Inline[] {
  const out: Inline[] = [];
  let buf = "";
  const flush = () => {
    if (buf) {
      out.push({ kind: "text", value: buf });
      buf = "";
    }
  };
  // 一次性记忆：整串里再也找不到 "]" 时，后面的 "[" 就不必再回扫（畸形 notes 的兜底）
  let noBracketAhead = false;

  let i = 0;
  while (i < src.length) {
    const ch = src[i];

    // `code`：不再往下解析，反引号里就是字面量
    if (ch === "`") {
      const end = src.indexOf("`", i + 1);
      if (end > i + 1) {
        flush();
        out.push({ kind: "code", value: src.slice(i + 1, end) });
        i = end + 1;
        continue;
      }
    }

    // **strong** / __strong__
    if ((ch === "*" || ch === "_") && src[i + 1] === ch) {
      const marker = ch + ch;
      const end = src.indexOf(marker, i + 2);
      if (end > i + 2) {
        flush();
        out.push({ kind: "strong", children: parseInline(src.slice(i + 2, end), allowBareLink) });
        i = end + 2;
        continue;
      }
    }

    // *em* / _em_：单字符标记。_ 两侧都贴着字母数字（file_name、Word_1）时按普通下划线处理，
    // 只有一侧贴着的（_italic_、_斜体_）照常当斜体。
    if (ch === "*" || ch === "_") {
      const tight = ch === "_" && isWordChar(src[i - 1]) && isWordChar(src[i + 1]);
      const end = src.indexOf(ch, i + 1);
      const inner = end > i + 1 ? src.slice(i + 1, end) : "";
      const ok =
        !tight &&
        inner.length > 0 &&
        !/^\s/.test(inner) &&
        !/\s$/.test(inner) &&
        !(ch === "_" && isWordChar(src[end + 1]));
      if (ok) {
        flush();
        out.push({ kind: "em", children: parseInline(inner, allowBareLink) });
        i = end + 1;
        continue;
      }
    }

    // [文字](https://…)：先定位 "]" 再确认紧跟 "("。
    // 不用正则整段匹配：畸形 notes（成串不配对的 "["）会让每个 "[" 都回溯到串尾，
    // 实测 6 万个 "[" 要 1.6 秒，而这段解析在设置页 render 里同步跑。
    if (ch === "[" && !noBracketAhead) {
      const close = src.indexOf("]", i + 1);
      if (close < 0) {
        // 之后不会再有 "]" 了，剩下的 "[" 一律当字面量
        noBracketAhead = true;
      } else if (src[close + 1] === "(") {
        // 标签里还夹着一个 "["（前面有没配对的方括号）：当前这个当字面量，
        // 让内层那个去配对，否则会把前缀整段吞成链接文字
        const inner = src.indexOf("[", i + 1);
        if (inner < 0 || inner > close) {
          const found = readHref(src, close + 2);
          if (found && !/\s/.test(found.href) && isHttpUrl(found.href)) {
            flush();
            out.push({
              kind: "link",
              href: found.href,
              children: parseInline(src.slice(i + 1, close), false),
            });
            i = found.end + 1;
            continue;
          }
        }
      }
    }

    // 裸链接（Full Changelog 那行就是 https://… 直接出现）
    if ((ch === "h" || ch === "H") && allowBareLink && isHttpAt(src, i)) {
      const found = readBareUrl(src, i);
      if (found) {
        flush();
        out.push({ kind: "link", href: found.url, children: [{ kind: "text", value: found.url }] });
        i = found.end;
        continue;
      }
    }

    buf += ch;
    i += 1;
  }

  flush();
  return out;
}

function renderInline(nodes: Inline[], onOpenError?: () => void): ReactNode[] {
  return nodes.map((node, idx) => {
    const key = `i${idx}`;
    switch (node.kind) {
      case "text":
        return <Fragment key={key}>{node.value}</Fragment>;
      case "code":
        return (
          <code key={key} className="rounded bg-black/[0.07] px-1 py-px font-mono text-[0.95em]">
            {node.value}
          </code>
        );
      case "strong":
        return (
          <strong key={key} className="font-semibold">
            {renderInline(node.children, onOpenError)}
          </strong>
        );
      case "em":
        return (
          <em key={key} className="italic opacity-90">
            {renderInline(node.children, onOpenError)}
          </em>
        );
      case "link":
        return (
          <a
            key={key}
            href={node.href}
            title={node.href}
            onClick={(e) => {
              // 必须拦掉默认导航：WebView 里跳转会白屏且回不来，交给系统浏览器
              e.preventDefault();
              void openExternalLink(node.href).then((ok) => {
                if (!ok) onOpenError?.();
              });
            }}
            className="cursor-pointer underline decoration-dotted underline-offset-2 hover:opacity-70"
          >
            {renderInline(node.children, onOpenError)}
          </a>
        );
    }
  });
}

/* ---------------------------------- 块级 ---------------------------------- */

type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "ul"; items: string[] }
  | { kind: "ol"; items: string[] }
  | { kind: "quote"; text: string }
  | { kind: "code"; value: string }
  | { kind: "hr" }
  | { kind: "p"; text: string };

const RE_UL = /^\s*[-*+]\s+/;
const RE_OL = /^\s*\d+[.)]\s+/;
const RE_HR = /^\s*([-*_])\s*(\1\s*){2,}$/;
const RE_FENCE = /^\s*```/;

function parseBlocks(src: string): Block[] {
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let para: string[] = [];
  const flushPara = () => {
    if (para.length) {
      blocks.push({ kind: "p", text: para.join(" ") });
      para = [];
    }
  };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];

    if (!line.trim()) {
      flushPara();
      continue;
    }

    // ```代码块```
    if (RE_FENCE.test(line)) {
      flushPara();
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !RE_FENCE.test(lines[i])) {
        body.push(lines[i]);
        i += 1;
      }
      blocks.push({ kind: "code", value: body.join("\n") });
      continue;
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      flushPara();
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2].trim() });
      continue;
    }

    if (RE_HR.test(line)) {
      flushPara();
      blocks.push({ kind: "hr" });
      continue;
    }

    if (RE_UL.test(line)) {
      flushPara();
      const items: string[] = [];
      while (i < lines.length && RE_UL.test(lines[i])) {
        items.push(lines[i].replace(RE_UL, ""));
        i += 1;
      }
      i -= 1; // 上面的 while 多走了一行，还给 for
      blocks.push({ kind: "ul", items });
      continue;
    }

    if (RE_OL.test(line)) {
      flushPara();
      const items: string[] = [];
      while (i < lines.length && RE_OL.test(lines[i])) {
        items.push(lines[i].replace(RE_OL, ""));
        i += 1;
      }
      i -= 1;
      blocks.push({ kind: "ol", items });
      continue;
    }

    if (/^\s*>\s?/.test(line)) {
      flushPara();
      const quoted: string[] = [];
      while (i < lines.length && /^\s*>\s?/.test(lines[i])) {
        quoted.push(lines[i].replace(/^\s*>\s?/, ""));
        i += 1;
      }
      i -= 1;
      blocks.push({ kind: "quote", text: quoted.join(" ") });
      continue;
    }

    para.push(line.trim());
  }

  flushPara();
  return blocks;
}

/* -------------------------------- 组件出口 -------------------------------- */

export default function Markdown({
  source,
  className = "",
  onOpenError,
}: {
  source: string;
  /** 只管排版与颜色的外层类，标题 / 列表的间距由本组件自己控制 */
  className?: string;
  /** 链接唤不起浏览器时回调，用于弹一条提示；不传就静默 */
  onOpenError?: () => void;
}) {
  const blocks = useMemo(() => parseBlocks(source), [source]);
  return (
    <div className={className}>
      {blocks.map((block, idx) => {
        switch (block.kind) {
          case "heading": {
            const Tag = block.level <= 2 ? "h3" : "h4";
            return (
              <Tag
                key={idx}
                className={
                  "mt-3 font-semibold first:mt-0 " +
                  (block.level <= 2 ? "text-[13px]" : "text-xs")
                }
              >
                {renderInline(parseInline(block.text), onOpenError)}
              </Tag>
            );
          }
          case "ul":
            return (
              <ul key={idx} className="mt-1 list-disc space-y-0.5 pl-4">
                {block.items.map((item, j) => (
                  <li key={j}>{renderInline(parseInline(item), onOpenError)}</li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={idx} className="mt-1 list-decimal space-y-0.5 pl-5">
                {block.items.map((item, j) => (
                  <li key={j}>{renderInline(parseInline(item), onOpenError)}</li>
                ))}
              </ol>
            );
          case "quote":
            return (
              <blockquote
                key={idx}
                className="mt-1 border-l-2 border-black/15 pl-2 italic opacity-80"
              >
                {renderInline(parseInline(block.text), onOpenError)}
              </blockquote>
            );
          case "code":
            return (
              <pre
                key={idx}
                className="mt-1 overflow-x-auto rounded bg-black/[0.06] p-2 font-mono text-[11px] leading-relaxed"
              >
                <code>{block.value}</code>
              </pre>
            );
          case "hr":
            return <hr key={idx} className="my-2 border-black/10" />;
          case "p":
            return (
              <p key={idx} className="mt-1 first:mt-0">
                {renderInline(parseInline(block.text), onOpenError)}
              </p>
            );
        }
      })}
    </div>
  );
}
