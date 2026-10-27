#!/usr/bin/env bash
# Regenerate tests/reference/oracle from the sub2api apicompat package.
#
# Usage:
#   scripts/gen-transcode-oracle.sh [path-to-sub2api]
#
# The command is created temporarily inside sub2api/backend/cmd so Go internal
# package imports stay valid; it is removed on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SUB2API="${1:-$ROOT/../sub2api}"
BACKEND="$SUB2API/backend"
ORACLE_DIR="$BACKEND/cmd/transcode-oracle"
FIXTURES="$ROOT/crates/solidway_transcode/tests/fixtures"
OUT="$ROOT/crates/solidway_transcode/tests/reference/oracle"

if [[ ! -d "$BACKEND" ]]; then
  echo "sub2api backend not found: $BACKEND" >&2
  exit 1
fi

cleanup() { rm -rf "$ORACLE_DIR"; }
trap cleanup EXIT
mkdir -p "$ORACLE_DIR"

cat > "$ORACLE_DIR/main.go" <<'GO'
package main

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/Wei-Shaw/sub2api/internal/pkg/apicompat"
)

func fail(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: transcode-oracle <direction> <fixture>")
		os.Exit(2)
	}
	raw, err := os.ReadFile(os.Args[2])
	fail(err)
	var out any
	switch os.Args[1] {
	case "anthropic_to_chat":
		var req apicompat.AnthropicRequest
		fail(json.Unmarshal(raw, &req))
		out, err = apicompat.AnthropicToChatCompletionsRequest(&req)
	case "anthropic_to_responses":
		var req apicompat.AnthropicRequest
		fail(json.Unmarshal(raw, &req))
		out, err = apicompat.AnthropicToResponses(&req)
	case "chat_to_responses":
		var req apicompat.ChatCompletionsRequest
		fail(json.Unmarshal(raw, &req))
		out, err = apicompat.ChatCompletionsToResponses(&req)
	case "responses_to_chat":
		var req apicompat.ResponsesRequest
		fail(json.Unmarshal(raw, &req))
		out, err = apicompat.ResponsesToChatCompletionsRequest(&req)
	case "responses_to_anthropic":
		var req apicompat.ResponsesRequest
		fail(json.Unmarshal(raw, &req))
		out, err = apicompat.ResponsesToAnthropicRequest(&req)
	default:
		fmt.Fprintln(os.Stderr, "unknown direction")
		os.Exit(2)
	}
	fail(err)
	encoded, err := json.Marshal(out)
	fail(err)
	fmt.Println(string(encoded))
}
GO

run() {
  local direction="$1" fixture="$2"
  mkdir -p "$OUT/$direction"
  (cd "$BACKEND" && go run ./cmd/transcode-oracle "$direction" "$FIXTURES/$fixture.json" > "$OUT/$direction/$fixture.json")
}

for fixture in anthropic_request multimodal_anthropic_request parallel_tools_anthropic_request; do
  run anthropic_to_chat "$fixture"
  run anthropic_to_responses "$fixture"
done
for fixture in chat_request multimodal_chat_request parallel_tools_chat_request; do
  run chat_to_responses "$fixture"
done
for fixture in responses_request multimodal_responses_request parallel_tools_responses_request; do
  run responses_to_chat "$fixture"
  run responses_to_anthropic "$fixture"
done
