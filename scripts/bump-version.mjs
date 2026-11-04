// 发版时统一改版本号：package.json / package-lock.json / tauri.conf.json / Cargo.toml / Cargo.lock
// 用法：node scripts/bump-version.mjs 0.2.0
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("usage: node scripts/bump-version.mjs 0.2.0");
  process.exit(1);
}

const readJson = (file) => JSON.parse(readFileSync(file, "utf8"));

// 保留原文件的行尾：Windows 工作区是 CRLF，统一写成 LF 会多出整篇 diff
const writeJson = (file, value) => {
  const text = JSON.stringify(value, null, 2) + "\n";
  const crlf = readFileSync(file, "utf8").includes("\r\n");
  writeFileSync(file, crlf ? text.replace(/\n/g, "\r\n") : text);
};

// package.json + package-lock 的根条目（只改这两个，别碰依赖的版本号）
const pkg = readJson("package.json");
if (pkg.version !== version) {
  pkg.version = version;
  writeJson("package.json", pkg);
}

const lock = readJson("package-lock.json");
if (lock.version !== version) {
  lock.version = version;
  if (lock.packages?.[""]) lock.packages[""].version = version;
  writeJson("package-lock.json", lock);
}

// 文本文件只替换第一处 version；已经是目标值就不写
const bumpText = (file, pattern) => {
  const text = readFileSync(file, "utf8");
  const match = text.match(pattern);
  if (!match) {
    console.error(`warn: no version pattern matched in ${file}`);
    return;
  }
  const [, prefix, current, suffix] = match;
  if (current === version) return;
  writeFileSync(file, text.replace(pattern, `${prefix}${version}${suffix}`));
};

bumpText("src-tauri/tauri.conf.json", /("version"\s*:\s*")([\d.]+)(")/);
bumpText("src-tauri/Cargo.toml", /(^version\s*=\s*")([\d.]+)(")/m);
bumpText("src-tauri/Cargo.lock", /(name = "dictation"\r?\nversion = ")([\d.]+)(")/);

console.log(`==> version -> ${version}`);
