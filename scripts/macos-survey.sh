#!/usr/bin/env bash
# One step of the macOS survey (issue #68): runs a command, keeps its output in survey/<name>.log and appends a row to the
# job summary. It never fails the job: the survey wants every step's outcome, not the first failure.
# Usage: scripts/macos-survey.sh <name> <command> [args...]      (bash 3.2 compatible: macOS ships it)
name="$1"
shift
mkdir -p survey
log="survey/$name.log"
start=$(date +%s)
"$@" > "$log" 2>&1
code=$?
secs=$(( $(date +%s) - start ))
if [ "$code" -eq 0 ]; then
  verdict="ok"
else
  verdict="FAILED ($code)"
fi
# The first line that looks like an error, cut to a table cell.
first=$(grep -m1 -iE 'error|panicked|FAILED|fatal|not found|No such file' "$log" | tr '|' '/' | cut -c1-160)
row="| $name | $verdict | ${secs}s | ${first:-} |"
echo "$row"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  echo "$row" >> "$GITHUB_STEP_SUMMARY"
fi
exit 0
