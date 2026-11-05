// 生成 GitHub Release 说明：把「上一个 tag .. 当前 tag」之间的 commit message 整理成分组列表。
// 用法：node scripts/gen-release-notes.mjs v0.2.1 release-notes.md
// CI 里由 .github/workflows/release.yml 调用；本地也可直接跑来预览或补写历史 Release。
//
// 为什么不用 GitHub 自带的 generate_release_notes：它只汇总合并过的 Pull Request，
// 本仓库是直接 push commit，所以生成结果只有一行 Full Changelog。
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";

const tag = process.argv[2];
const outFile = process.argv[3] ?? "release-notes.md";
if (!/^v?\d+\.\d+\.\d+[\w.-]*$/.test(tag ?? "")) {
  console.error("usage: node scripts/gen-release-notes.mjs v0.2.1 [output.md]");
  process.exit(1);
}

// execFileSync 直接传参数数组，不经过 shell：tag / 提交信息里的特殊字符不会被解释。
// 忽略 stderr：上面的「找不到上一个 tag / tag 不存在」是正常分支，别让 git 的 fatal 污染 CI 日志。
const git = (...args) =>
  execFileSync("git", args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  }).trim();

try {
  git("rev-parse", "--verify", `refs/tags/${tag}^{commit}`);
} catch {
  console.error(`tag not found: ${tag}`);
  process.exit(1);
}

// 从当前 tag 的父提交往前找最近的 tag；找不到就是首次发版
let prev = "";
try {
  prev = git("describe", "--tags", "--abbrev=0", `${tag}^`);
} catch {
  // 首个 tag / 父提交不存在：退化为列出该 tag 之前的全部提交
}
const range = prev ? `${prev}..${tag}` : tag;

let log = "";
try {
  log = git("log", "--no-merges", "--pretty=format:%H%x09%h%x09%s", range);
} catch {
  console.error(`git log failed for ${range}`);
  process.exit(1);
}
const commits = log
  .split("\n")
  .filter(Boolean)
  .map((line) => {
    const [full, short, subject] = line.split("\t");
    return { full, short, subject };
  })
  // make release 生成的版本号提交对用户没有信息量
  .filter((c) => !/^chore:\s*release\s+v?\d+\.\d+\.\d+\s*$/i.test(c.subject));

const stripPrefix = (s) => s.replace(/^[a-z]+(\([^)]*\))?:\s*/i, "");
const GROUPS = [
  { title: "新功能", re: /^feat(\([^)]*\))?:\s*/i },
  { title: "问题修复", re: /^fix(\([^)]*\))?:\s*/i },
  { title: "构建 / 持续集成", re: /^(ci|build)(\([^)]*\))?:\s*/i },
  { title: "文档", re: /^docs(\([^)]*\))?:\s*/i },
  { title: "维护 / 其他", re: /.*/ },
];

// Actions 内置 GITHUB_REPOSITORY=owner/repo；本地则从 origin 推导，用于拼 commit 链接
const repoUrl = () => {
  if (process.env.GITHUB_REPOSITORY) {
    return `https://github.com/${process.env.GITHUB_REPOSITORY}`;
  }
  let remote = "";
  try {
    remote = git("remote", "get-url", "origin");
  } catch {
    return "";
  }
  const m = remote.match(/^(?:git@github\.com:|https?:\/\/github\.com\/)(.+?)(?:\.git)?$/);
  return m ? `https://github.com/${m[1]}` : "";
};
const base = repoUrl();
const ref = (c) => (base ? `[\`${c.short}\`](${base}/commit/${c.full})` : `\`${c.short}\``);

const buckets = new Map(GROUPS.map((g) => [g.title, []]));
for (const c of commits) {
  buckets.get(GROUPS.find((g) => g.re.test(c.subject)).title).push(c);
}

const out = [prev ? `## 更新内容（${prev} → ${tag}）` : `## 更新内容（${tag}）`, ""];
if (commits.length === 0) {
  out.push("_本次仅版本号更新，无功能改动。_", "");
} else {
  for (const g of GROUPS) {
    const list = buckets.get(g.title);
    if (!list.length) continue;
    out.push(`### ${g.title}`);
    for (const c of list) out.push(`- ${stripPrefix(c.subject)} (${ref(c)})`);
    out.push("");
  }
}
// 与 GitHub 自动生成 notes 的写法保持一致，只是由脚本生成，避免依赖 PR
if (base) {
  out.push(
    prev
      ? `**Full Changelog**: ${base}/compare/${prev}...${tag}`
      : `**Full Changelog**: ${base}/commits/${tag}`
  );
}

writeFileSync(outFile, out.join("\n").replace(/\n+$/, "") + "\n");
console.log(`==> ${outFile}: ${commits.length} commits (${prev || "first release"}..${tag})`);
