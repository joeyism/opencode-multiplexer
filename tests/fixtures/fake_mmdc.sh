#!/usr/bin/env bash
set -euo pipefail

# fake_mmdc.sh fixture for ocmux tests
# Mimics mermaid-cli behavior for TDD

INPUT=""
OUTPUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -i) INPUT="$2"; shift 2 ;;
    -o) OUTPUT="$2"; shift 2 ;;
    *) shift ;;
  esac
done

if [[ -n "${OCMUX_MMDC_LOG:-}" ]]; then
  mkdir -p "$(dirname "$OCMUX_MMDC_LOG")"
  echo "$INPUT->$OUTPUT" >> "$OCMUX_MMDC_LOG"
fi

# Edge case: FORCE_FAIL in input
if grep -q "FORCE_FAIL" "$INPUT"; then
  echo "Fake mmdc error: found FORCE_FAIL" >&2
  exit 1
fi

# Edge case: FORCE_SLEEP in input
if grep -q "FORCE_SLEEP" "$INPUT"; then
  sleep 60
fi

# Copy the tiny fixture to the output
cp "$(dirname "$0")/tiny.png" "$OUTPUT"
