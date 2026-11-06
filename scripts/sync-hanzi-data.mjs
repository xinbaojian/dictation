// 把 hanzi-writer-data 的笔画 JSON 打包成 256 个分片，供前端按字懒加载。
//
// 为什么不是从 CDN 取：本应用的默认行为是纯离线（唯一联网是自动升级），
// 笔画数据必须随包发出去，不能依赖 jsdelivr。
//
// 为什么分片而不是一字一文件：一字一文件是 9575 个文件，`vite build` 要逐个
// 复制、安装目录里 9575 个 inode（簇对齐后体积从 32MB 涨到 47MB），光构建就
// 多几分钟。分片规则是「码点 % 256」，前后端各算一次、不需要清单文件；
// 每片约 37 个字 / 130KB，翻一个词最多读两三片，本地协议下无感。
//
// 为什么片内以码点为 key（"6625" 而不是 "春"）：纯 ASCII，省掉 Windows 文件
// 系统 / WebView percent-encoding 链路上的一类隐蔽问题。
//
// public/hanzi/ 是生成物，不进版本库；npm run dev / build 之前由 package.json
// 的 predev / prebuild 自动跑本脚本（缺依赖会直接失败，免得打出一个没有笔画
// 的包还以为是代码问题）。手动重跑：node scripts/sync-hanzi-data.mjs

import { existsSync, mkdirSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

// 不用 import.meta.dirname：它要 Node 20.11+，而 CI 只钉了大版本 20
const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const SRC = join(root, "node_modules", "hanzi-writer-data");
const DEST = join(root, "public", "hanzi");
const STAMP = join(DEST, ".stamp");
const SHARDS = 256;
// 分片口径变了就把这个后缀改掉，脚本会自动重刷（见下面 STAMP 比较）
const FORMAT = "-shard256b";

if (!existsSync(SRC)) {
  console.error("[hanzi] 缺少依赖 hanzi-writer-data，请先执行 npm install");
  process.exit(1);
}

const version = JSON.parse(readFileSync(join(SRC, "package.json"), "utf8")).version;
const stamp = version + FORMAT;

// 跳过条件必须同时看「版本」和「产物还在不在」：上一次跑挂了（扫不到字 / 写盘失败）
// 会留下旧 stamp 却没有分片，只看 stamp 会当成功放行，打出一个没有笔顺动画的包。
const existingShards = existsSync(DEST)
  ? readdirSync(DEST).filter((name) => /^s[0-9a-f]{2}\.json$/.test(name)).length
  : 0;
if (existsSync(STAMP) && readFileSync(STAMP, "utf8").trim() === stamp && existingShards === SHARDS) {
  console.log(`[hanzi] 笔画数据已同步（v${version}），跳过`);
  process.exit(0);
}

// 先扫源、校验通过再动 public/hanzi：反过来的话，中途失败会把上一版可用产物删掉
const shards = Array.from({ length: SHARDS }, () => ({}));
let total = 0;
for (const name of readdirSync(SRC)) {
  if (!name.endsWith(".json")) continue;
  const char = name.slice(0, -".json".length);
  // 目录里还有 package.json 之类的元数据，只有单字符文件才是字
  if ([...char].length !== 1) continue;
  const codePoint = char.codePointAt(0);
  if (codePoint === undefined) continue;
  const json = JSON.parse(readFileSync(join(SRC, name), "utf8"));
  shards[codePoint % SHARDS][codePoint.toString(16)] = json;
  total += 1;
}

// hanzi-writer-data 的目录布局由它自己的 prepublishOnly 决定（历史上就把 data/ 挪过位置）。
// 扫不到字就静默产出 256 个空分片，会打出一个「安装包正常、进去全是空格子」的版本，
// 比构建失败难查得多——所以宁可在这里中断。
if (total < 8000) {
  console.error(
    `[hanzi] 只扫到 ${total} 个字（预期 9000+），检查 node_modules/hanzi-writer-data 的目录结构；已中止`,
  );
  process.exit(1);
}

mkdirSync(DEST, { recursive: true });

// 清掉上一版产物（只动本脚本会生成的那两种命名，别的文件一概不碰）
for (const name of readdirSync(DEST)) {
  if (/^u[0-9a-f]+\.json$/.test(name) || /^s[0-9a-f]{2}\.json$/.test(name)) {
    unlinkSync(join(DEST, name));
  }
}

let bytes = 0;
shards.forEach((bucket, i) => {
  const body = JSON.stringify(bucket);
  bytes += body.length;
  writeFileSync(join(DEST, "s" + i.toString(16).padStart(2, "0") + ".json"), body);
});

// 笔顺数据来自 Make Me a Hanzi，字库本身是 Arphic Public License：
// 随包分发时必须带上许可证原文。
const LICENSE = join(SRC, "ARPHICPL.TXT");
if (existsSync(LICENSE)) writeFileSync(join(DEST, "ARPHICPL.TXT"), readFileSync(LICENSE));

writeFileSync(STAMP, stamp + "\n");
const mb = (bytes / 1024 / 1024).toFixed(1);
console.log(`[hanzi] 已生成 ${total} 个字 / ${SHARDS} 个分片（${mb}MB）-> public/hanzi/（v${version}）`);
