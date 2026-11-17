#!/usr/bin/env bash
# Fails when a tracing call logs a field that looks like it carries dictated
# text: `text = …`, `payload = %…`, `incoming_text = …` and the like.
#
# VoxCtrl's rule (docs/privacy.md) is that logs say how many words a dictation
# had, never the words. The app's log goes to stderr, which a desktop session
# usually sends to the journal, and its tail goes into bug reports. A field
# such as `payload = %parsed.payload` puts the dictation there. Log a count
# instead: `words = text.split_whitespace().count()`.
#
# The pattern matches a line that is only `<name> = <expr>,` with one of the
# names below: the shape of a tracing field, not of an assignment (which ends
# in `;`). Test files are skipped: they may log what they like.
#
# Usage: ./scripts/check_log_privacy.sh   (CI runs it on every pull request)

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

names='incoming_text|text|text_payload|payload|transcript|raw_text|cleaned|cleaned_text|prompt|input|reply|utterance'
pattern="^\s*(${names})\s*=\s*[%?]?[A-Za-z_][A-Za-z0-9_.()]*\s*,\s*$"

hits=$(grep -rnE "$pattern" crates src-tauri/src --include='*.rs' \
    | grep -vE '/tests\.rs:|_tests\.rs:|/tests/' || true)

if [[ -n "$hits" ]]; then
    echo "These log fields look like they carry dictated text:" >&2
    echo "$hits" >&2
    echo >&2
    echo "Log a count instead (words = text.split_whitespace().count()). See docs/privacy.md." >&2
    exit 1
fi
echo "No log fields carrying dictated text."
