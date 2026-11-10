#!/usr/bin/env bash
# PLAN 0.5 + 0.6 spike: real PHP-FPM behind real Caddy, end to end.
#
# Runs entirely inside a throwaway CORRAL_HOME in high-port mode. It does not
# touch /etc/resolver, the keychain, or your PATH. It downloads:
#   - Caddy from GitHub releases (verified against the release SHA-512 sums)
#   - PHP from static-php.dev (UNVERIFIED: upstream publishes no checksums)
#
# Usage: scripts/spike-e2e.sh [php-version]   (default 8.4)
set -euo pipefail

PHP_MINOR="${1:-8.4}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d -t cs)"
export CORRAL_HOME="$WORK/home"
export CORRAL_LOG="${CORRAL_LOG:-warn}"
CORRAL="$ROOT/target/debug/corral"

cleanup() {
  [[ -n "${ENGINE_PID:-}" ]] && kill "$ENGINE_PID" 2>/dev/null && wait "$ENGINE_PID" 2>/dev/null || true
  echo "Work dir kept for inspection: $WORK"
}
trap cleanup EXIT

echo "==> Building corral"
cargo build -q -p corral-cli --manifest-path "$ROOT/Cargo.toml"

echo "==> Creating sample sites in $WORK/Sites"
mkdir -p "$WORK/Sites/laravelish/public" "$WORK/Sites/wp" "$WORK/Sites/docs"
touch "$WORK/Sites/laravelish/artisan"
cat > "$WORK/Sites/laravelish/public/index.php" <<'PHP'
<?php
header('Content-Type: text/plain');
echo "front-controller php=", PHP_VERSION, " uri=", $_SERVER['REQUEST_URI'], " host=", $_SERVER['HTTP_HOST'], "\n";
PHP
echo "static-file" > "$WORK/Sites/laravelish/public/robots.txt"
echo "<?php echo 'wp-root ', PHP_VERSION;" > "$WORK/Sites/wp/index.php"
touch "$WORK/Sites/wp/wp-config.php"
echo "<h1>docs</h1>" > "$WORK/Sites/docs/index.html"

echo "==> Setup (high ports) + PHP $PHP_MINOR"
"$CORRAL" setup --high-ports
"$CORRAL" php install "$PHP_MINOR" --allow-unverified
"$CORRAL" park "$WORK/Sites"
"$CORRAL" unsecure --site docs
"$CORRAL" sites

echo "==> Starting engine"
"$CORRAL" start > "$WORK/engine.log" 2>&1 &
ENGINE_PID=$!
for _ in $(seq 1 50); do
  curl -s -o /dev/null "http://127.0.0.1:18080" && break
  sleep 0.2
done

CA="$CORRAL_HOME/certs/ca.crt"
pass=0; fail=0
check() { # name, expected substring, curl args...
  local name="$1" want="$2"; shift 2
  local got
  got="$(curl -s --max-time 5 "$@" || true)"
  if [[ "$got" == *"$want"* ]]; then echo "PASS $name"; pass=$((pass+1));
  else echo "FAIL $name: wanted '$want', got '${got:0:200}'"; fail=$((fail+1)); fi
}
R="--resolve laravelish.test:18443:127.0.0.1 --resolve wp.test:18443:127.0.0.1 --resolve docs.test:18080:127.0.0.1 --resolve laravelish.test:18080:127.0.0.1"

# shellcheck disable=SC2086
{
check "front controller over HTTPS (trusted via our CA)" "front-controller php=$PHP_MINOR" --cacert "$CA" $R https://laravelish.test:18443/some/route?x=1
check "pretty URL keeps original URI" "uri=/some/route?x=1" --cacert "$CA" $R https://laravelish.test:18443/some/route?x=1
check "static file bypasses PHP" "static-file" --cacert "$CA" $R https://laravelish.test:18443/robots.txt
check "WordPress-style root index.php" "wp-root $PHP_MINOR" --cacert "$CA" $R https://wp.test:18443/
check "HTTP redirects to HTTPS" "308" -o /dev/null -w "%{http_code}" $R http://laravelish.test:18080/
check "static site over HTTP" "<h1>docs</h1>" $R http://docs.test:18080/
check "unknown host gets 404 text" "no site is configured" -H "Host: nope.test" http://127.0.0.1:18080/
}
got_dns="$(dig +short @127.0.0.1 -p 15353 anything.docs.test A || true)"
if [[ "$got_dns" == "127.0.0.1" ]]; then echo "PASS DNS answers *.test"; pass=$((pass+1));
else echo "FAIL DNS answers *.test: got '$got_dns'"; fail=$((fail+1)); fi
got_dns="$(dig +short @127.0.0.1 -p 15353 example.com A || true)"
if [[ -z "$got_dns" ]]; then echo "PASS DNS refuses other zones"; pass=$((pass+1));
else echo "FAIL DNS refuses other zones: got '$got_dns'"; fail=$((fail+1)); fi

echo "==> Reload: unsecure laravelish, expect plain HTTP within ~2s"
"$CORRAL" unsecure --site laravelish
sleep 2
# shellcheck disable=SC2086
check "live reload after config change" "front-controller" $R http://laravelish.test:18080/

echo "==> Timing (10 warm requests)"
# shellcheck disable=SC2086
for _ in $(seq 1 10); do curl -s -o /dev/null -w "%{time_total}\n" $R http://laravelish.test:18080/; done | sort -n | awk '{a[NR]=$1} END {print "median s:", a[int((NR+1)/2)]}'

echo "==> Second engine must refuse to start while the first runs"
if "$CORRAL" start > "$WORK/engine2.log" 2>&1; then
  echo "FAIL second engine started"; fail=$((fail+1))
elif grep -q "already running" "$WORK/engine2.log"; then
  echo "PASS second engine refused (single-instance lock)"; pass=$((pass+1))
else
  echo "FAIL second engine: $(cat "$WORK/engine2.log")"; fail=$((fail+1))
fi

echo "==> SIGTERM must stop every child process"
kill -TERM "$ENGINE_PID"; wait "$ENGINE_PID" 2>/dev/null || true; ENGINE_PID=""
sleep 0.5
if pgrep -f "$CORRAL_HOME" >/dev/null; then
  echo "FAIL leftover processes after SIGTERM:"; pgrep -fl "$CORRAL_HOME"; fail=$((fail+1))
else
  echo "PASS no leftover processes after SIGTERM"; pass=$((pass+1))
fi

echo "==> SIGKILL leaves orphans; the next start must clean them up"
"$CORRAL" start > "$WORK/engine3.log" 2>&1 &
ENGINE_PID=$!
for _ in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:18080" && break; sleep 0.2; done
sleep 1.5   # let the engine record its child PIDs
kill -KILL "$ENGINE_PID"; wait "$ENGINE_PID" 2>/dev/null || true; ENGINE_PID=""
sleep 0.3
orphans_before=$(pgrep -f "$CORRAL_HOME" | wc -l | tr -d ' ')
"$CORRAL" start > "$WORK/engine4.log" 2>&1 &
ENGINE_PID=$!
ok4=""
for _ in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:18080" && ok4=1 && break; sleep 0.2; done
kill -TERM "$ENGINE_PID"; wait "$ENGINE_PID" 2>/dev/null || true; ENGINE_PID=""
sleep 0.5
if [[ "$orphans_before" -gt 0 && -n "$ok4" ]] && ! pgrep -f "$CORRAL_HOME" >/dev/null; then
  echo "PASS $orphans_before orphan(s) reaped; restart served traffic; clean shutdown"; pass=$((pass+1))
else
  echo "FAIL orphan recovery (orphans_before=$orphans_before served=${ok4:-no})"; pgrep -fl "$CORRAL_HOME" || true; fail=$((fail+1))
fi

echo
echo "Result: $pass passed, $fail failed. Engine log: $WORK/engine.log"
[[ $fail -eq 0 ]]
