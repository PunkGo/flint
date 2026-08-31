"""Execute the shell blocks SETUP.md actually ships, against adversarial fixtures.

The install manual is a program the reader runs, so it is checked the way a program is:
by extracting its code blocks and running them, never by reading them. Every case here
is a defect that shipped in a previous version of that document —

  A  a Codex-only operator was wired for Claude and Grok instead
  B  a merge that removed another tool's hook along with flint's stale one
  C  a failed compile that had already truncated the operator's config
  D  a Codex path-rule install whose only governance file was never written

Run from the repo root, with a release binary built:  python3 scripts/check-setup-doc.py
"""
import re, subprocess, tempfile, os, shutil, json, sys, stat

setup = open("SETUP.md").read()
blocks = re.findall(r"```sh\n(.*?)```", setup, re.S)
ctx  = next(b for b in blocks if "--target-dir" in b)
WIRE = os.path.abspath("scripts/wire-harness.sh")
BIN = os.path.abspath("target/release/flint")
fail = 0

def fresh_home():
    tmp = tempfile.mkdtemp(); fake = os.path.join(tmp, "home"); os.makedirs(fake)
    subprocess.run([BIN, "init", "--home", f"{fake}/.flint", "--scope", "t"], capture_output=True)
    shutil.copy("examples/laws/secret-zero.md", f"{fake}/.flint/canon/rules/")
    subprocess.run([BIN, "law", "accept", "--all", "--config", f"{fake}/.flint/flint.toml",
                    "--key", f"{fake}/.flint/keys/sovereign_ed25519"], capture_output=True)
    return tmp, fake

def run(fake, harnesses):
    env = dict(os.environ, HOME=fake, FLINT=BIN)
    return subprocess.run(["bash", WIRE, "--config", f"{fake}/.flint/flint.toml", *harnesses.split()],
                          capture_output=True, text=True, env=env)

def check(label, cond, detail=""):
    global fail
    print(f"    {'OK  ' if cond else 'FAIL'} {label}{'  ' + detail if detail and not cond else ''}")
    if not cond: fail = 1

# A: codex-only operator — the bug codex found: does it wire ONLY codex?
print("A. operator named codex only")
tmp, fake = fresh_home(); r = run(fake, "codex")
check("codex file written", os.path.exists(f"{fake}/.codex/hooks.json"))
check("claude file NOT created", not os.path.exists(f"{fake}/.claude/settings.json"))
check("grok file NOT created", not os.path.exists(f"{fake}/.grok/hooks/flint.json"))
if os.path.exists(f"{fake}/.codex/hooks.json"):
    d = json.load(open(f"{fake}/.codex/hooks.json"))
    named = {re.search(r"--harness (\w+)", h["command"]).group(1)
             for e in d["hooks"]["PreToolUse"] for h in e["hooks"] if "flint hook" in h["command"]}
    check("wiring names codex", named == {"codex"}, str(named))
    check("both matchers present", {e["matcher"] for e in d["hooks"]["PreToolUse"]} == {"Bash", "apply_patch"})
shutil.rmtree(tmp)

# B: a foreign hook SHARING an entry with a stale flint hook — must survive
print("B. another tool's hook shares an entry with a stale flint hook")
tmp, fake = fresh_home(); os.makedirs(f"{fake}/.claude", exist_ok=True)
json.dump({"model": "opus", "hooks": {"PreToolUse": [
    {"matcher": "*", "hooks": [
        {"type": "command", "command": "/usr/local/bin/other-tool --check"},
        {"type": "command", "command": "/old/path/flint hook --harness claude"}]}]}},
    open(f"{fake}/.claude/settings.json", "w"))
os.chmod(f"{fake}/.claude/settings.json", 0o600)
r = run(fake, "claude")
d = json.load(open(f"{fake}/.claude/settings.json"))
cmds = [h["command"] for e in d["hooks"]["PreToolUse"] for h in e["hooks"]]
check("the other tool's hook survived", any("other-tool" in c for c in cmds), str(cmds))
check("the stale flint hook is gone", not any("/old/path/flint" in c for c in cmds))
check("exactly one flint hook", sum("flint hook" in c for c in cmds) == 1)
check("unrelated key kept", d.get("model") == "opus")
mode = stat.S_IMODE(os.stat(f"{fake}/.claude/settings.json").st_mode)
check("file permissions preserved (0600)", mode == 0o600, oct(mode))
check("backup taken", any(f.startswith("settings.json.bak-") for f in os.listdir(f"{fake}/.claude")))
shutil.rmtree(tmp)

# C: compile fails — the config must be left intact, not truncated
print("C. compile fails mid-way (bad --config)")
tmp, fake = fresh_home(); os.makedirs(f"{fake}/.grok/hooks", exist_ok=True)
open(f"{fake}/.grok/hooks/flint.json", "w").write('{"precious":"do not truncate me"}')
env = dict(os.environ, HOME=fake, FLINT=BIN)
subprocess.run(["bash", WIRE, "--config", f"{fake}/.flint/NOSUCH.toml", "grok"],
               capture_output=True, text=True, env=env)
kept = json.load(open(f"{fake}/.grok/hooks/flint.json")).get("precious")
check("existing grok file NOT truncated by a failed compile", kept == "do not truncate me", repr(kept))
shutil.rmtree(tmp)


# D: codex-only operator whose canon holds ONLY a path rule — the AGENTS.md block is
# their only path governance, so the doc's context-file step must still write it.
print("D. codex-only, path-rule-only canon (AGENTS.md is the only governance)")
tmp = tempfile.mkdtemp(); fake = os.path.join(tmp, "home"); os.makedirs(fake)
subprocess.run([BIN, "init", "--home", f"{fake}/.flint", "--scope", "t"], capture_output=True)
open(f"{fake}/.flint/canon/rules/no-secrets.md", "w").write(
    "---\nschema: flint/v1\nid: no-secrets-dir\ntype: rule\nkind: path\nstatus: proposed\n"
    "description: Never write into secrets/.\nglob: secrets/**\nresponse: block\n"
    "reversibility: irreversible\n---\nDo not write into secrets/.\n")
subprocess.run([BIN, "law", "accept", "--all", "--config", f"{fake}/.flint/flint.toml",
                "--key", f"{fake}/.flint/keys/sovereign_ed25519"], capture_output=True)
os.makedirs(f"{fake}/.codex", exist_ok=True)
open(f"{fake}/.codex/AGENTS.md", "w").write("# my own codex instructions\nkeep me\n")
env = dict(os.environ, HOME=fake, PATH=f"{os.path.dirname(BIN)}:{os.environ['PATH']}",
           HARNESSES="codex", CFG=f"{fake}/.flint/flint.toml")
subprocess.run(["bash", "-c", 'HARNESSES="$HARNESSES"\nCFG="$CFG"\n' + ctx], capture_output=True, text=True, env=env)
agents = f"{fake}/.codex/AGENTS.md"
body = open(agents).read() if os.path.exists(agents) else ""
check("AGENTS.md written", bool(body))
check("carries the path-rule governance", "secrets" in body)
check("operator's own instructions preserved", "keep me" in body)
check("no claude dir created for a codex-only operator", not os.path.exists(f"{fake}/.claude"))
shutil.rmtree(tmp)
print("\n" + ("case D passed" if not fail else "case D: FAILURES ABOVE"))
sys.exit(fail)
