"""Scan a raw /api/chat/stream SSE capture for content that should never reach the user.

Signatures are deliberately conservative: each one is something the app injects or
consumes internally, so its presence in the assistant's visible text is a defect.
"""
import json, re, sys, unicodedata

def load(path):
    raw = open(path, 'rb').read()
    txt = raw.decode('utf-8', errors='replace')
    toks, done = [], None
    for line in txt.split('\n'):
        if not line.startswith('data: '):
            continue
        body = line[6:].strip()
        if body == '[DONE]':
            continue
        try:
            j = json.loads(body)
        except Exception:
            continue
        if 'token' in j:
            toks.append(j['token'])
        elif j.get('type') == 'done':
            done = j
    return raw, ''.join(toks), done

# (label, regex, note)
SIGS = [
    ("probe-prompt-leak",   r"\[SELF-CHECK\]",              "EOS probe question text (task 012)"),
    ("probe-reply-leak",    r"(?m)^\s*DONE\s*$",            "bare DONE on its own line (task 002)"),
    ("utf8-corruption",     "�",                        "U+FFFD replacement char (task 013)"),
    ("special-token",       r"<\|im_(start|end)\|>|<\|endoftext\|>",  "raw special token rendered as text"),
    ("tool-result-marker",  r"\[TOOL_RESULT:",              "internal tool-result marker"),
    ("stall-notice",        r"\[Generation stalled",        "watchdog restart notice"),
    ("template-artifact",   r"\[INST\]|<\|system\|>|<\|user\|>|<\|assistant\|>", "chat-template fragment"),
    ("selfcheck-fragment",  r"Type DONE if yes",            "probe text without the bracket prefix"),
]

def scan(name, path):
    raw, full, done = load(path)
    issues = []
    for label, pat, note in SIGS:
        hits = list(re.finditer(pat, full))
        if hits:
            ctx = full[max(0, hits[0].start()-45):hits[0].end()+45].replace('\n', '\\n')
            issues.append((label, len(hits), note, ctx))
    # tag balance
    t_open, t_close = full.count('<think>'), full.count('</think>')
    tc_open, tc_close = full.count('<tool_call>'), full.count('</tool_call>')
    tr_open, tr_close = full.count('<tool_response>'), full.count('</tool_response>')
    # first think block is prefilled -> closers may legitimately exceed openers by exactly 1
    if t_close - t_open not in (0, 1):
        issues.append(("think-imbalance", abs(t_close-t_open),
                       f"<think>={t_open} </think>={t_close} (expected delta 0 or 1)", ""))
    if tc_open != tc_close:
        issues.append(("toolcall-imbalance", abs(tc_open-tc_close),
                       f"<tool_call>={tc_open} </tool_call>={tc_close}", ""))
    if tr_open != tr_close:
        issues.append(("toolresp-imbalance", abs(tr_open-tr_close),
                       f"<tool_response>={tr_open} </tool_response>={tr_close}", ""))
    fr = (done or {}).get('finish_reason')
    if fr not in (None, 'stop'):
        issues.append(("finish-reason", 1, f"finish_reason={fr!r}", ""))

    status = "CLEAN" if not issues else "LEAKS"
    print(f"\n=== [{status}] {name}  ({len(full)} chars, finish={fr}, think {t_open}/{t_close}, calls {tc_open}/{tc_close}) ===")
    for label, n, note, ctx in issues:
        print(f"  * {label} x{n} — {note}")
        if ctx:
            print(f"      ...{ctx}...")
    return len(issues)

if __name__ == '__main__':
    total = 0
    for arg in sys.argv[1:]:
        name, path = arg.split('=', 1)
        total += scan(name, path)
    print(f"\nTOTAL signature hits across runs: {total}")
