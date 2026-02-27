#!/usr/bin/env bash
# test-all.sh — Run cargo test --workspace for every ChainKit module.
#
# Usage:
#   ./test-all.sh              # test all four modules
#   ./test-all.sh chaincodec   # test one module
#   ./test-all.sh --no-fail    # keep going even if a module fails

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PASS=0
FAIL=0
SKIP=0
declare -a FAILED_MODULES=()

# ── ANSI colours ──────────────────────────────────────────────────────────────
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

# ── Option parsing ────────────────────────────────────────────────────────────
NO_FAIL=false
TARGET_MODULE=""

for arg in "$@"; do
  case "$arg" in
    --no-fail) NO_FAIL=true ;;
    --help|-h)
      echo "Usage: $0 [module] [--no-fail]"
      echo "  module   One of: chaincodec chainerrors chainrpc chainindex"
      echo "  --no-fail  Continue even if a module fails (exit 1 at end if any failed)"
      exit 0
      ;;
    *)
      TARGET_MODULE="$arg"
      ;;
  esac
done

# ── Module definitions ────────────────────────────────────────────────────────
# Format: "name:path:extra_cargo_flags"
declare -a MODULES=(
  "chaincodec:chaincodec:"
  "chainerrors:chainerrors:"
  "chainrpc:chainrpc:"
  "chainindex:chainindex:--features sqlite"
)

# ── Helpers ───────────────────────────────────────────────────────────────────
separator() {
  echo -e "${CYAN}────────────────────────────────────────────────────────────${RESET}"
}

run_module() {
  local name="$1"
  local rel_path="$2"
  local extra_flags="$3"
  local module_dir="$REPO_ROOT/$rel_path"

  if [[ ! -d "$module_dir" ]]; then
    echo -e "${YELLOW}  SKIP${RESET} $name — directory not found: $module_dir"
    SKIP=$((SKIP + 1))
    return
  fi

  separator
  echo -e "${BOLD}▶ Testing: $name${RESET}  ($rel_path)"
  echo -e "  ${CYAN}cargo test --workspace $extra_flags${RESET}"
  echo

  local start
  start=$(date +%s)

  # shellcheck disable=SC2086
  if (cd "$module_dir" && cargo test --workspace $extra_flags 2>&1); then
    local end
    end=$(date +%s)
    echo
    echo -e "${GREEN}  ✓ PASS${RESET} $name  ($(( end - start ))s)"
    PASS=$((PASS + 1))
  else
    local end
    end=$(date +%s)
    echo
    echo -e "${RED}  ✗ FAIL${RESET} $name  ($(( end - start ))s)"
    FAIL=$((FAIL + 1))
    FAILED_MODULES+=("$name")

    if [[ "$NO_FAIL" == "false" ]]; then
      exit 1
    fi
  fi
}

# ── Main ──────────────────────────────────────────────────────────────────────
echo
echo -e "${BOLD}ChainKit — Full Test Suite${RESET}"
echo -e "Repo: $REPO_ROOT"
echo -e "Date: $(date)"
echo

TOTAL_START=$(date +%s)

for entry in "${MODULES[@]}"; do
  IFS=':' read -r name rel_path extra_flags <<< "$entry"

  # If a specific module was requested, skip others
  if [[ -n "$TARGET_MODULE" && "$name" != "$TARGET_MODULE" ]]; then
    continue
  fi

  run_module "$name" "$rel_path" "$extra_flags"
done

separator
TOTAL_END=$(date +%s)
ELAPSED=$(( TOTAL_END - TOTAL_START ))

echo
echo -e "${BOLD}Results${RESET}"
echo -e "  ${GREEN}Passed : $PASS${RESET}"
echo -e "  ${RED}Failed : $FAIL${RESET}"
echo -e "  ${YELLOW}Skipped: $SKIP${RESET}"
echo -e "  Total  : ${ELAPSED}s"
echo

if [[ ${#FAILED_MODULES[@]} -gt 0 ]]; then
  echo -e "${RED}Failed modules:${RESET}"
  for m in "${FAILED_MODULES[@]}"; do
    echo -e "  • $m"
  done
  echo
  exit 1
fi

echo -e "${GREEN}All modules passed.${RESET}"
echo
