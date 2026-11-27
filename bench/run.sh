#!/usr/bin/env bash
# Runs every benchmark program and prints a table: wall time, SAT calls, circuit and CNF size.
#
#   bench/run.sh [redsat binary]      (default: target/release/redsat, built if missing)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
REDSAT="${1:-target/release/redsat}"
if [ ! -x "$REDSAT" ]; then
    cargo build --quiet --release -p redsat-cli
fi
printf "%-24s %9s %8s %10s %10s %10s\n" program seconds calls "and-gates" variables clauses
for program in bench/*.rsat; do
    start=$(date +%s.%N)
    stats=$("$REDSAT" --stats "$program" 2>&1 >/dev/null || true)
    end=$(date +%s.%N)
    calls=$(grep -o '[0-9]* SAT call' <<<"$stats" | awk '{s += $1} END {print s + 0}')
    gates=$(grep -o '[0-9]* AND gates' <<<"$stats" | awk '{s += $1} END {print s + 0}')
    vars=$(grep -o '[0-9]* variables' <<<"$stats" | awk '{s += $1} END {print s + 0}')
    clauses=$(grep -o '[0-9]* clauses' <<<"$stats" | awk '{s += $1} END {print s + 0}')
    printf "%-24s %9.3f %8s %10s %10s %10s\n" "$(basename "$program" .rsat)" "$(echo "$end - $start" | bc)" "$calls" "$gates" "$vars" "$clauses"
done
