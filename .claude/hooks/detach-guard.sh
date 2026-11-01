#!/usr/bin/env bash
# PreToolUse hook (Bash): refuse a command that detaches a process from the harness: a
# background `&` (`cmd &`, `( cmd &)`), `nohup`, `disown`, or `setsid`. A detached run sends no
# completion notice and can outlive the session holding a lock (docs/process/agent-brief.md
# item 2). `&&`, `|&`, `&>`, `>&`, `<&` (as in `2>&1`), the `;&` and `;;&` case terminators,
# and `&` inside `$(( ))` or `(( ))` are not background operators. Quoted text, comments, and
# here-document bodies are not scanned, except `$( )` inside double quotes. The words are
# refused at the start of a command (after `;`, `|`, `&&`, `||`, `(`, `{`, a newline, or the
# prefixes `env`, `exec`, `command`, `sudo`, `time`, `timeout`, `nice`, `ionice`, `stdbuf`, their
# options and numbers, and variable assignments).
# Known limits: this is a guard against accidents, not a shell parser. It does not follow
# backticks, `eval`, `bash -c '...'`, `xargs`, `$[ ]` arithmetic, or a word built by expansion
# (`n=no; ${n}hup x`), and it reads a here-document only when its delimiter follows `<<`
# directly.
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
PREFIX = {"env", "exec", "command", "sudo", "time", "builtin", "timeout", "nice", "ionice",
          "stdbuf"}
OPTION = re.compile(r"-[^\s]*|[0-9][0-9.]*[smhd]?")
ASSIGNMENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*=.*")
WORD = re.compile(r"[^\s;|&(){}<>\x27\"\\#$]+")


def balanced(cmd, i, open_c, close_c):
    """Index just after the group that opens at cmd[i]; the end of cmd when it never closes."""
    depth = 0
    n = len(cmd)
    while i < n:
        if cmd[i] == open_c:
            depth += 1
        elif cmd[i] == close_c:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def refusal(cmd):
    i, n = 0, len(cmd)
    at_start = True      # next word is in command position
    in_prefix = False    # the words before it were prefixes (env, timeout, ...)
    heredocs = []        # delimiters waiting for the end of the line

    def word_done():
        nonlocal at_start, in_prefix
        at_start = False
        in_prefix = False

    while i < n:
        c = cmd[i]
        prev = cmd[i - 1] if i > 0 else ""
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
            at_start, in_prefix = True, False
        elif c in " \t":
            i += 1
        elif c == "\\":
            i += 2
            word_done()
        elif c == "\x27":
            end = cmd.find("\x27", i + 1)
            i = n if end < 0 else end + 1
            word_done()
        elif c == "\"":
            i += 1
            while i < n and cmd[i] != "\"":
                if cmd.startswith("$(", i) and not cmd.startswith("$((", i):
                    end = balanced(cmd, i + 1, "(", ")")
                    found = refusal(cmd[i + 2:end - 1])
                    if found:
                        return found
                    i = end
                else:
                    i += 2 if cmd[i] == "\\" else 1
            i += 1
            word_done()
        elif c == "#" and (i == 0 or prev in " \t\n;|&("):
            end = cmd.find("\n", i)
            i = n if end < 0 else end
        elif c == "$" and cmd.startswith("$((", i):
            i = balanced(cmd, i + 1, "(", ")")
            word_done()
        elif c == "$" and cmd.startswith("${", i):
            i = balanced(cmd, i + 1, "{", "}")
            word_done()
        elif c == "$":
            # `$(`: the command substitution is scanned as part of the line; `$#`, `$$`, `$x`.
            if cmd.startswith("$(", i):
                i += 2
                at_start, in_prefix = True, False
            else:
                i += 2 if i + 1 < n and cmd[i + 1] in "#$?!@*-0123456789" else 1
                word_done()
        elif c == "(" and cmd.startswith("((", i) and at_start:
            i = balanced(cmd, i, "(", ")")
            word_done()
        elif c == "&":
            nxt = cmd[i + 1] if i + 1 < n else ""
            if nxt == "&":
                i += 2
                at_start, in_prefix = True, False
            elif nxt == ">" or prev in "<>|":
                i += 1
            elif prev == ";":
                i += 1   # the `;&` and `;;&` case terminators
            else:
                return "a background &"
        elif c in ";|({":
            i += 1
            at_start, in_prefix = True, False
        elif c == "<" and cmd.startswith("<<", i) and not cmd.startswith("<<<", i):
            m = re.match(r"<<-?\s*([\x27\"]?)([A-Za-z0-9_]+)\1", cmd[i:])
            if m:
                heredocs.append(m.group(2))
                i += m.end()
            else:
                i += 2
        else:
            m = WORD.match(cmd, i)
            if not m:
                i += 1
                word_done()
                continue
            word = m.group(0)
            i = m.end()
            if at_start and word in WORDS:
                return word
            if at_start and word in PREFIX:
                in_prefix = True
            elif at_start and ASSIGNMENT.fullmatch(word):
                pass
            elif in_prefix and OPTION.fullmatch(word):
                pass
            else:
                word_done()
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
