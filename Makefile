# 生词默写 · 构建、打包与部署（Windows / macOS 通用）
#
#   make                      打包 + 部署到 INSTALL_DIR（按平台选默认目录）
#   make install              同上
#   make build                只打包（前端构建 + Rust release + 平台安装包）
#   make dmg                  macOS：只产出 .dmg（等价于 make build）
#   make test                 后端单测 + 前端类型检查
#   make typecheck            仅前端类型检查
#   make dev                  开发模式（Vite + Tauri）
#   make clean                清理构建产物
#   make help                 显示本说明
#
# 覆盖部署目录：make install INSTALL_DIR=/Applications
#   Windows 默认 D:/StudyApp/dictation，macOS 默认 ~/Applications
#
# 注 1：Windows 版 make 会按控制台代码页解码 Makefile，中文 echo 在 GBK 控制台下
#   会变成乱码，所以 recipe 里的提示文字一律用 ASCII（注释不受影响）。
# 注 2：Windows 版 make 只认 Windows 路径，需把 Git 自带的 bash 指给 make；
#   macOS / Linux 用系统默认 shell 即可，无需特殊设置。

# ---- 平台检测 ----
# make 只解析被命中的分支，未命中的分支里的 $(shell …) / $(wildcard …) 不会执行。
ifeq ($(OS),Windows_NT)
  PLATFORM := windows
else
  UNAME_S := $(shell uname -s 2>/dev/null || echo Unknown)
  ifeq ($(UNAME_S),Darwin)
    PLATFORM := macos
  else ifeq ($(UNAME_S),Linux)
    PLATFORM := linux
  else
    PLATFORM := unknown
  endif
endif

# ---- 平台相关变量 ----
ifeq ($(PLATFORM),windows)
  # 8.3 短路径没有空格，$(wildcard) 才认得；system32 下的 bash.exe 是 WSL 的不能用
  GIT_BASH := $(firstword \
    $(wildcard C:/PROGRA~1/Git/bin/bash.exe) \
    $(wildcard C:/PROGRA~1/Git/usr/bin/bash.exe) \
    $(wildcard $(LOCALAPPDATA)/Programs/Git/bin/bash.exe))
  ifeq ($(strip $(GIT_BASH)),)
    GIT_BASH := C:/Program Files/Git/bin/bash.exe
  endif
  SHELL := $(GIT_BASH)

  INSTALL_DIR ?= D:/StudyApp/dictation
  APP_NAME    := dictation
  APP_EXE     := dictation.exe
  RELEASE_BIN := src-tauri/target/release/dictation.exe
  BUNDLE_DIR  := src-tauri/target/release/bundle/nsis
  BUNDLE_EXT  := exe
  BUNDLES_ARG := nsis
  RUN_CHECK   := tasklist //FI "IMAGENAME eq $(APP_EXE)" 2>/dev/null | grep -qi "$(APP_EXE)"
else ifeq ($(PLATFORM),macos)
  INSTALL_DIR ?= $(HOME)/Applications
  APP_NAME    := Dictation
  APP_EXE     := Dictation
  RELEASE_APP := src-tauri/target/release/bundle/macos/Dictation.app
  BUNDLE_DIR  := src-tauri/target/release/bundle/dmg
  BUNDLE_EXT  := dmg
  BUNDLES_ARG := dmg
  RUN_CHECK   := pgrep -x "$(APP_EXE)" >/dev/null 2>&1
else ifeq ($(PLATFORM),linux)
  INSTALL_DIR ?= $(HOME)/.local/share/applications
  APP_NAME    := dictation
  APP_EXE     := dictation
  RELEASE_APP := src-tauri/target/release/bundle/deb/dictation_0.1.0_amd64.deb
  BUNDLE_DIR  := src-tauri/target/release/bundle/deb
  BUNDLE_EXT  := deb
  BUNDLES_ARG := deb
  RUN_CHECK   := pgrep -x "$(APP_EXE)" >/dev/null 2>&1
else
  $(error Unsupported platform: $(PLATFORM))
endif

.DEFAULT_GOAL := install

.PHONY: install deploy build dmg test typecheck dev clean help

install deploy: build
	@mkdir -p "$(INSTALL_DIR)"
ifeq ($(PLATFORM),windows)
	@if $(RUN_CHECK); then echo "!! $(APP_EXE) is running - close the app window first"; exit 1; fi
	@cp -f "$(RELEASE_BIN)" "$(INSTALL_DIR)/$(APP_EXE)"
	@echo "==> deployed: $(INSTALL_DIR)/$(APP_EXE)"
	@ls -lh "$(INSTALL_DIR)/$(APP_EXE)" | awk '{print "    " $$5 "  " $$6 " " $$7 " " $$8}'
else
	@if $(RUN_CHECK); then echo "!! $(APP_EXE) is running - quit the app first"; exit 1; fi
	@rm -rf "$(INSTALL_DIR)/$(APP_NAME).app"
	@cp -R "$(RELEASE_APP)" "$(INSTALL_DIR)/"
	@echo "==> deployed: $(INSTALL_DIR)/$(APP_NAME).app"
	@du -h -d0 "$(INSTALL_DIR)/$(APP_NAME).app" | awk '{print "    " $$1}'
endif
	@echo "    installer: $$(ls $(BUNDLE_DIR)/*.$(BUNDLE_EXT) 2>/dev/null | head -1)"

build:
	@echo "==> build: frontend + rust release + $(BUNDLES_ARG)"
	@npm run tauri -- build --bundles $(BUNDLES_ARG)

dmg:
ifeq ($(PLATFORM),macos)
	@$(MAKE) build
else
	@echo "!! 'make dmg' is macOS-only (use 'make build' on $(PLATFORM))"
endif

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
	@head -14 Makefile | sed 's/^# \{0,1\}//'
