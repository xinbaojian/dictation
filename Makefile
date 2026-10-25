# 生词默写 · 构建、打包与部署
#
#   make                      打包 + 部署到 INSTALL_DIR
#   make install              同上
#   make build                只打包（前端构建 + Rust release + NSIS 安装包）
#   make test                 后端单测 + 前端类型检查
#   make dev                  开发模式（Vite + Tauri）
#   make clean                清理构建产物
#   make help                 显示本说明
#
# 覆盖部署目录：make install INSTALL_DIR=E:/Apps/dictation
#
# 注 1：recipe 里的提示文字一律用 ASCII —— Windows 版 make 会按控制台代码页
# 解码 Makefile，中文 echo 在 GBK 控制台下会变成乱码；注释不受影响。
# 注 2：Windows 版 make 只认 Windows 路径，SHELL 写成 /bin/bash 会被静默忽略并
# 退回 cmd.exe —— 那时 `mkdir -p` 会建出一个叫「-p」的目录，cp/rm 只是碰巧
# 命中 PATH 里的 Git coreutils 才不报错。所以下面显式把 Git 自带的 bash 指给
# make：在 Git Bash、PowerShell、cmd 里跑结果都一样。

SHELL := /bin/bash
.DEFAULT_GOAL := install

# 注意：system32 和 WindowsApps 下的 bash.exe 是 WSL 的，路径语义完全不同，不能用
ifeq ($(OS),Windows_NT)
  # 8.3 短路径没有空格，$(wildcard) 才认得（它会按空格切分模式）
  GIT_BASH := $(firstword \
    $(wildcard C:/PROGRA~1/Git/bin/bash.exe) \
    $(wildcard C:/PROGRA~1/Git/usr/bin/bash.exe) \
    $(wildcard $(LOCALAPPDATA)/Programs/Git/bin/bash.exe))
  ifeq ($(strip $(GIT_BASH)),)
    # 8.3 被关掉时的兜底：make 4.4 能正确处理带空格的 SHELL
    GIT_BASH := C:/Program Files/Git/bin/bash.exe
  endif
  SHELL := $(GIT_BASH)
endif

INSTALL_DIR ?= D:/StudyApp/dictation
APP_EXE     := dictation.exe
RELEASE_EXE := src-tauri/target/release/dictation.exe
NSIS_DIR    := src-tauri/target/release/bundle/nsis

.PHONY: install deploy build test typecheck dev clean help

install deploy: build
	@mkdir -p "$(INSTALL_DIR)"
	@if tasklist //FI "IMAGENAME eq $(APP_EXE)" 2>/dev/null | grep -qi "$(APP_EXE)"; then echo "!! $(APP_EXE) is running - close the app window first"; exit 1; fi
	@cp -f "$(RELEASE_EXE)" "$(INSTALL_DIR)/$(APP_EXE)"
	@echo "==> deployed: $(INSTALL_DIR)/$(APP_EXE)"
	@ls -lh "$(INSTALL_DIR)/$(APP_EXE)" | awk '{print "    " $$5 "  " $$6 " " $$7 " " $$8}'
	@echo "    installer: $$(ls $(NSIS_DIR)/*.exe 2>/dev/null | head -1)"

build:
	@echo "==> build: frontend + rust release + nsis"
	@npm run tauri build

test:
	@echo "==> cargo test"
	@cd src-tauri && cargo test
	@echo "==> tsc --noEmit"
	@npm run typecheck

typecheck:
	@npm run typecheck

dev:
	@npm run tauri dev

clean:
	@echo "==> clean: src-tauri/target + dist"
	@cd src-tauri && cargo clean
	@rm -rf dist

help:
	@head -10 Makefile | sed 's/^# \{0,1\}//'
