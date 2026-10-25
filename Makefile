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
# 注：recipe 里的提示文字一律用 ASCII —— Windows 版 make 会按控制台代码页
# 解码 Makefile，中文 echo 在 GBK 控制台下会变成乱码；注释不受影响。

SHELL := /bin/bash
.DEFAULT_GOAL := install

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
	@head -10 Makefile | sed 's/^#//'
