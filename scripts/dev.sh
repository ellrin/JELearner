#!/bin/sh
# Development launcher for macOS.
#
# Debug builds change identity on every rebuild, so macOS asks for the login
# password each time they read the keychain. This script lets Apple's own
# `security` tool hold the key instead and passes it to the server through
# ANTHROPIC_API_KEY for this run only. Nothing is written to a file.
#
#   scripts/dev.sh                 # http://127.0.0.1:7979
#   scripts/dev.sh --reset-key     # forget the stored key and ask again
#   scripts/dev.sh --mock          # no key, no network: mock Claude + news

set -e
SVC=JELearner-dev
ACCT=anthropic-api-key
cd "$(dirname "$0")/.."

if [ "$1" = "--mock" ]; then
  cargo build -q -p jl-server --example mock_services
  ./target/debug/examples/mock_services &
  MOCK=$!
  trap 'kill $MOCK' EXIT INT TERM
  sleep 1
  JELEARNER_ENGINE=api JELEARNER_CLAUDE_BASE_URL=http://127.0.0.1:7980 ANTHROPIC_API_KEY=sk-ant-mock JELEARNER_NO_KEYCHAIN=1 \
    cargo run -p jl-server
  exit
fi

if [ "$1" = "--reset-key" ]; then
  security delete-generic-password -s "$SVC" -a "$ACCT" >/dev/null 2>&1 || true
  shift
fi

if ! key=$(security find-generic-password -s "$SVC" -a "$ACCT" -w 2>/dev/null); then
  echo "第一次使用：貼上 Claude API key（輸入時不會顯示，按 Enter 後需再輸入一次確認）"
  security add-generic-password -s "$SVC" -a "$ACCT" -w
  key=$(security find-generic-password -s "$SVC" -a "$ACCT" -w)
fi

ANTHROPIC_API_KEY="$key" JELEARNER_NO_KEYCHAIN=1 exec cargo run -p jl-server
