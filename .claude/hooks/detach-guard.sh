#!/usr/bin/env bash
# PreToolUse hook (Bash): refuse a command that detaches a process from the harness: a
# background `&` (`cmd &`, `( cmd &)`), `nohup`, `disown`, or `setsid`. A detached run sends no
# completion notice and can outlive the session holding a lock (docs/process/agent-brief.md
# item 2). `&&`, `|&`, `&>`, `>&` and `<&` (as in `2>&1`) are not background operators, and
# quoted text, comments, and here-document bodies are not scanned. The words are refused at
# the start of a command (after `;`, `|`, `&&`, `||`, `(`, `{`, a newline, or `env`, `exec`,
# `command`, `sudo`, `time`, and variable assignments).
# Exit 2 blocks the tool call with the stderr message; any other failure lets the call through.
set -uo pipefail

input="$(cat)"
command -v python3 >/dev/null 2>&1 || exit 0

python3 -I -c '
import json, re, sys
try:
    data = json.loads(sys.argv[1])
except ValueError:
    sys.exit(0)
cmd = (data.get("tool_input") or {}).get("command")
if not isinstance(cmd, str):
    sys.exit(0)

WORDS = {"nohup", "disown", "setsid"}
PREFIX = {"env", "exec", "command", "sudo", "time", "builtin"}


def refusal(cmd):
    i, n = 0, len(cmd)
    at_start = True      # next word is in command position
    heredocs = []        # delimiters waiting for the end of the line
    while i < n:
        c = cmd[i]
        if c == "\n":
            i += 1
            for delim in heredocs:
                while i < n:
                    end = cmd.find("\n", i)
                    end = n if end < 0 else end
                    line = cmd[i:end].strip("\t")
                    i = min(end + 1, n)
                    if line == delim:
                        break
            heredocs = []
            at_start = True
        elif c in " \t":
            i += 1
        elif c == "\\":
            i += 2
            at_start = False
        elif c == "\x27":
            end = cmd.find("\x27", i + 1)
            i = n if end < 0 else end + 1
            at_start = False
        elif c == "\"":
            i += 1
            while i < n and cmd[i] != "\"":
                i += 2 if cmd[i] == "\\" else 1
            i += 1
            at_start = False
        elif c == "#":
            end = cmd.find("\n", i)
            i = n if end < 0 else end
        elif c == "&":
            nxt = cmd[i + 1] if i + 1 < n else ""
            prev = cmd[i - 1] if i > 0 else ""
            if nxt == "&":
                i += 2
                at_start = True
            elif nxt == ">" or prev in "<>|":
                i += 1
            else:
                return "a background &"
        elif c in ";|({":
            i += 1
            at_start = True
        elif c == "<" and cmd.startswith("<<", i) and not cmd.startswith("<<<", i):
            m = re.match(r"<<-?\s*([\x27\"]?)([A-Za-z0-9_]+)\1", cmd[i:])
            if m:
                heredocs.append(m.group(2))
                i += m.end()
            else:
                i += 2
        else:
            m = re.match(r"[^\s;|&(){}<>\x27\"\\#]+", cmd[i:])
            if not m:
                i += 1
                at_start = False
                continue
            word = m.group(0)
            i += m.end()
            if at_start and word in WORDS:
                return word
            assignment = re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", word) is not None
            at_start = at_start and (word in PREFIX or assignment)
    return None


found = refusal(cmd)
if found:
    print(
        f"BLOCKED: {found} detaches the command; start it with run_in_background and wait "
        "for its notification (docs/process/agent-brief.md item 2).",
        file=sys.stderr,
    )
    sys.exit(2)
' "$input"
