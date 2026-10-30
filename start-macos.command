#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
if ! command -v node >/dev/null || ! command -v cargo >/dev/null; then
  echo "请先安装 Node.js 22.12+ 和 Rust stable；具体步骤见 README.md。"
  echo "安装后重新打开终端，运行 npm ci && npm run desktop。"
  read -r -p "按回车退出..."
  exit 1
fi
if [ ! -d node_modules ]; then
  npm ci
fi
npm run doctor
npm run desktop

