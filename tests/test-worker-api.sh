#!/usr/bin/env bash
# Worker API integration tests.
# Usage:
#   ./tests/test-worker-api.sh                          # tests against https://zrtc.ini.workers.dev
#   ./tests/test-worker-api.sh http://localhost:8787    # tests against local dev server
set -euo pipefail

BASE_URL="${1:-https://zrtc.ini.workers.dev}"
PASS=0
FAIL=0
TOTAL=0

# ── helpers ──────────────────────────────────────────

green() { printf "\033[32m%s\033[0m\n" "$1"; }
red()   { printf "\033[31m%s\033[0m\n" "$1"; }
bold()  { printf "\033[1m%s\033[0m\n" "$1"; }

assert_status() {
  local label="$1" expected="$2" actual="$3"
  TOTAL=$((TOTAL + 1))
  if [ "$actual" = "$expected" ]; then
    green "  ✓ $label (HTTP $actual)"
    PASS=$((PASS + 1))
  else
    red "  ✗ $label — expected HTTP $expected, got $actual"
    FAIL=$((FAIL + 1))
  fi
}

assert_json() {
  local label="$1" field="$2" expected="$3" body="$4"
  TOTAL=$((TOTAL + 1))
  local actual
  actual=$(echo "$body" | python3 -c "import sys,json; print(json.load(sys.stdin).get('$field',''))" 2>/dev/null || echo "PARSE_ERROR")
  if [ "$actual" = "$expected" ]; then
    green "  ✓ $label ($field=$actual)"
    PASS=$((PASS + 1))
  else
    red "  ✗ $label — expected $field='$expected', got '$actual'"
    FAIL=$((FAIL + 1))
  fi
}

assert_json_exists() {
  local label="$1" field="$2" body="$3"
  TOTAL=$((TOTAL + 1))
  local actual
  actual=$(echo "$body" | python3 -c "import sys,json; d=json.load(sys.stdin); print('yes' if '$field' in d and d['$field'] else 'no')" 2>/dev/null || echo "no")
  if [ "$actual" = "yes" ]; then
    green "  ✓ $label ($field exists)"
    PASS=$((PASS + 1))
  else
    red "  ✗ $label — $field missing or empty"
    FAIL=$((FAIL + 1))
  fi
}

post() {
  curl -s -w "\n%{http_code}" -X POST "$BASE_URL$1" \
    -H "Content-Type: application/json" \
    -d "$2" 2>/dev/null
}

get() {
  curl -s -w "\n%{http_code}" "$BASE_URL$1" 2>/dev/null
}

parse_body() { echo "$1" | sed '$d'; }
parse_code() { echo "$1" | tail -1; }

# ── tests ────────────────────────────────────────────

bold "Testing worker at: $BASE_URL"
echo ""

# ── 1. Health check ──
bold "1. GET / (health check)"
RESP=$(get "/")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "health returns 200" "200" "$CODE"
assert_json "status is ok" "status" "ok" "$BODY"
assert_json "service is zrtc" "service" "zrtc" "$BODY"
echo ""

# ── 2. Method not allowed ──
bold "2. GET /new (wrong method)"
RESP=$(get "/new")
CODE=$(parse_code "$RESP")
assert_status "GET /new returns 405" "405" "$CODE"
echo ""

# ── 3. POST /new (create channel) ──
bold "3. POST /new (create channel)"
RESP=$(post "/new" '{}')
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "create returns 200" "200" "$CODE"
assert_json_exists "channel_id present" "channel_id" "$BODY"
assert_json_exists "passcode present" "passcode" "$BODY"
assert_json_exists "expires_at present" "expires_at" "$BODY"

CHANNEL_ID=$(echo "$BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['channel_id'])")
PASSCODE=$(echo "$BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['passcode'])")
echo "  → channel_id=$CHANNEL_ID passcode=$PASSCODE"
echo ""

# ── 4. POST /new with custom token ──
bold "4. POST /new/:token (custom token)"
RESP=$(post "/new/MyCustomToken123" '{}')
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "custom token returns 200" "200" "$CODE"
assert_json "channel_id matches token" "channel_id" "MyCustomToken123" "$BODY"
CUSTOM_PASS=$(echo "$BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['passcode'])")
echo ""

# ── 5. Duplicate channel ──
bold "5. POST /new/:token (duplicate)"
RESP=$(post "/new/MyCustomToken123" '{}')
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "duplicate returns 409" "409" "$CODE"
assert_json "error code" "error" "channel_id_exists" "$BODY"
echo ""

# ── 6. Invalid passcode ──
bold "6. POST /listen (wrong passcode)"
RESP=$(post "/listen" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"WRONG\",\"role\":\"callee\",\"signal\":{\"test\":true}}")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "wrong passcode returns 401" "401" "$CODE"
assert_json "error code" "error" "invalid_passcode" "$BODY"
echo ""

# ── 7. Listen (callee posts signal) ──
bold "7. POST /listen (callee signal)"
CALLEE_SIGNAL='{"ice_ufrag":"abc123","ice_pwd":"secret","dtls_fingerprint":"AAAA","candidates":[]}'
RESP=$(post "/listen" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"$PASSCODE\",\"role\":\"callee\",\"signal\":$CALLEE_SIGNAL}")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "listen returns 200" "200" "$CODE"
assert_json "status is waiting" "status" "waiting" "$BODY"
echo ""

# ── 8. Listen again (wrong state) ──
bold "8. POST /listen (already waiting)"
RESP=$(post "/listen" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"$PASSCODE\",\"role\":\"callee\",\"signal\":$CALLEE_SIGNAL}")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "second listen returns 409" "409" "$CODE"
assert_json "error code" "error" "invalid_state" "$BODY"
echo ""

# ── 9. Poll (waiting, no caller yet) ──
bold "9. POST /poll (still waiting)"
RESP=$(post "/poll" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"$PASSCODE\",\"role\":\"callee\"}")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "poll returns 200" "200" "$CODE"
assert_json "status is waiting" "status" "waiting" "$BODY"
echo ""

# ── 10. Join (caller posts signal) ──
bold "10. POST /join (caller joins)"
CALLER_SIGNAL='{"ice_ufrag":"xyz789","ice_pwd":"secret2","dtls_fingerprint":"BBBB","candidates":[]}'
RESP=$(post "/join" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"$PASSCODE\",\"role\":\"caller\",\"signal\":$CALLER_SIGNAL}")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "join returns 200" "200" "$CODE"
assert_json "status is locked" "status" "locked" "$BODY"
assert_json_exists "callee_signal present" "callee_signal" "$BODY"
echo ""

# ── 11. Poll (locked, caller signal available) ──
bold "11. POST /poll (locked, caller signal)"
RESP=$(post "/poll" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"$PASSCODE\",\"role\":\"callee\"}")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "poll returns 200" "200" "$CODE"
assert_json "status is locked" "status" "locked" "$BODY"
assert_json_exists "caller_signal present" "caller_signal" "$BODY"
echo ""

# ── 12. Join again (already locked) ──
bold "12. POST /join (already locked)"
RESP=$(post "/join" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"$PASSCODE\",\"role\":\"caller\",\"signal\":$CALLER_SIGNAL}")
CODE=$(parse_code "$RESP")
assert_status "second join returns 403" "403" "$CODE"
echo ""

# ── 13. Hangup ──
bold "13. POST /hangup"
RESP=$(post "/hangup" "{\"channel_id\":\"$CHANNEL_ID\",\"passcode\":\"$PASSCODE\",\"role\":\"callee\"}")
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "hangup returns 200" "200" "$CODE"
assert_json "status is terminated" "status" "terminated" "$BODY"
echo ""

# ── 14. Channel not found ──
bold "14. POST /poll (nonexistent channel)"
RESP=$(post "/poll" '{"channel_id":"DOESNOTEXIST","passcode":"000000","role":"callee"}')
CODE=$(parse_code "$RESP")
BODY=$(parse_body "$RESP")
assert_status "missing channel returns 404" "404" "$CODE"
assert_json "error code" "error" "channel_not_found" "$BODY"
echo ""

# ── 15. Not found route ──
bold "15. POST /unknown (404)"
RESP=$(post "/unknown" '{}')
CODE=$(parse_code "$RESP")
assert_status "unknown route returns 404" "404" "$CODE"
echo ""

# ── Cleanup: hangup custom token channel ──
post "/hangup" "{\"channel_id\":\"MyCustomToken123\",\"passcode\":\"$CUSTOM_PASS\",\"role\":\"callee\"}" > /dev/null 2>&1

# ── Summary ──────────────────────────────────────────
echo ""
bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ "$FAIL" -eq 0 ]; then
  green "All $TOTAL tests passed ✓"
else
  red "$FAIL/$TOTAL tests failed"
  green "$PASS/$TOTAL tests passed"
fi
bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
exit "$FAIL"
