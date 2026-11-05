// 生成 Tauri updater 的静态清单 latest.json，随 GitHub Release 一起发布。
// 用法：node scripts/gen-updater-manifest.mjs v0.2.2 artifacts latest.json
//
// CI 里 artifacts/ 是 actions/download-artifact 的产物，每个子目录对应一个构建 job；
// 脚本从各目录里找 Tauri 生成的 .sig，拼出该平台安装包的 Release 下载地址。
// 端点是 https://github.com/<owner>/<repo>/releases/latest/download/latest.json
//
// 清单格式见 https://tauri.app/plugin/updater/ 的「Static JSON File」：
//   version / notes / pub_date + platforms["OS-ARCH"] = { signature, url }
// 注意：Tauri 会先校验整份 JSON，任一列出的平台缺少 signature 或 url 都会让所有客户端检查失败。
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";

const tag = process.argv[2];
const artifactsDir = process.argv[3] ?? "artifacts";
const outFile = process.argv[4] ?? "latest.json";
if (!/^v\d+\.\d+\.\d+[\w.-]*$/.test(tag ?? "")) {
  console.error("usage: node scripts/gen-updater-manifest.mjs v0.2.2 [artifacts-dir] [output.json]");
  process.exit(1);
}

// artifact 目录名 -> updater 的 OS-ARCH 键；.app.tar.gz 是 macOS 的更新包（不是 .dmg），
// universal 包同时供 Intel 和 Apple Silicon 使用，所以一个 .sig 写两个键。
const PLATFORMS = [
  { dir: "win-x86_64-pc-windows-msvc", keys: ["windows-x86_64"], sig: /\.exe\.sig$/i },
  { dir: "win-i686-pc-windows-msvc", keys: ["windows-i686"], sig: /\.exe\.sig$/i },
  { dir: "win-aarch64-pc-windows-msvc", keys: ["windows-aarch64"], sig: /\.exe\.sig$/i },
  { dir: "macos", keys: ["darwin-x86_64", "darwin-aarch64"], sig: /\.app\.tar\.gz\.sig$/i },
  { dir: "linux-x86_64-unknown-linux-gnu", keys: ["linux-x86_64"], sig: /\.AppImage\.sig$/i },
  { dir: "linux-aarch64-unknown-linux-gnu", keys: ["linux-aarch64"], sig: /\.AppImage\.sig$/i },
];

const repoUrl = () => {
  if (process.env.GITHUB_REPOSITORY) return process.env.GITHUB_REPOSITORY;
  let remote = "";
  try {
    remote = execFileSync("git", ["remote", "get-url", "origin"], { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
  const m = remote.match(/^(?:git@github\.com:|https?:\/\/github\.com\/)(.+?)(?:\.git)?$/);
  return m ? m[1] : "";
};

const repo = repoUrl();
if (!repo) {
  console.error("cannot determine GitHub repository (set GITHUB_REPOSITORY or git remote origin)");
  process.exit(1);
}

const walk = (dir) => {
  if (!existsSync(dir)) return [];
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(p));
    else if (entry.isFile()) out.push(p);
  }
  return out;
};

const platforms = {};
const missing = [];
for (const p of PLATFORMS) {
  const dir = join(artifactsDir, p.dir);
  const files = walk(dir);
  const sigFile = files.find((f) => p.sig.test(basename(f)));
  if (!sigFile) {
    missing.push(`${p.keys.join("/")}: 没有找到 .sig（${dir}）`);
    continue;
  }
  const asset = basename(sigFile).replace(/\.sig$/i, "");
  const signature = readFileSync(sigFile, "utf8").trim();
  if (!signature) {
    missing.push(`${p.keys.join("/")}: .sig 内容为空`);
    continue;
  }
  // 清单里的 url 指向 GitHub Release 资产，本体必须一起上传，否则客户端 404
  if (!files.some((f) => basename(f) === asset)) {
    missing.push(`${p.keys.join("/")}: 找到签名但缺少更新包 ${asset}`);
    continue;
  }
  const url = `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(asset)}`;
  for (const key of p.keys) platforms[key] = { signature, url };
}

if (Object.keys(platforms).length === 0) {
  console.error("no signed updater artifacts found, refuse to publish an empty manifest");
  for (const m of missing) console.error("  - " + m);
  process.exit(1);
}
for (const m of missing) console.warn("WARN " + m);

const notes = existsSync("release-notes.md") ? readFileSync("release-notes.md", "utf8").trim() : "";
const manifest = {
  version: tag.replace(/^v/, ""),
  notes,
  pub_date: new Date().toISOString(),
  platforms,
};

writeFileSync(outFile, JSON.stringify(manifest, null, 2) + "\n");
console.log(
  `==> ${outFile}: ${manifest.version}, ${Object.keys(platforms).length} platforms` +
    (missing.length ? ` (${missing.length} missing)` : ""),
);
for (const [key, value] of Object.entries(platforms)) {
  console.log(`    ${key.padEnd(18)} ${value.url}`);
}
