#!/usr/bin/env python3
"""Compose flint's .opencodereview/rule.json — run this, never hand-edit the JSON.

    python3 .opencodereview/gen_rule.py

A custom `rules` entry REPLACES the matching built-in rule doc rather than adding
to it (verified experimentally against ocr 1.7.16), so the Rust group forks
upstream's rule doc verbatim from `rust.md.upstream` and appends flint's own
constraints. Refresh the fork base with:

    curl -sL -o .opencodereview/rust.md.upstream \\
      https://raw.githubusercontent.com/alibaba/open-code-review/main/internal/config/rules/rule_docs/rust.md

then diff, re-run this script, and re-check with `ocr rules check <path>`.
"""
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parent

rust_md = (HERE / "rust.md.upstream").read_text(encoding="utf-8").strip()

FLINT_SRC = """
---

#### flint 项目硬约束（违反即 P1,优先于上面任何通用条目）

- **冻结边界** — `flint-core` 是冻结判官。knowledge / 抓矿 / pit triage / capture 的业务逻辑必须留在 `flint-cli`,永不进 `flint-core`。`crates/flint-core/tests/freeze_gate.rs` 是这条的守卫:任何"放宽 freeze_gate 就能过"的改动是设计问题,不是测试问题。
- **agent 永不自动签** — 任何代码路径都不得代人签署 canon。签名那一下必须是人按的键。自动签 = 模型拥有自己的宪法 = 这个项目失去全部意义。看到新增的签名调用,先问"这条路径会不会在无人在场时被触发"。
- **不 auto-promote** — pit → knowledge 的 promote 是人裁,永不自动。任何自动分类、自动晋升、自动入库、把 LLM 判断当准入的代码都是红线。
- **fail-closed** — hook 的失败模式默认 closed。任何把错误路径变成"放行"的改动,必须在 diff 里显式说明为什么安全;没说明就报。
- **secret zero** — 凭据值永不写进文件、日志、argv 或 receipt,只存 Keychain / 环境变量指针。
- **config-over-code** — store 位置、路径、阈值属于配置(flint.toml),永不硬编码默认路径。新出现的字面量路径就是问题。
- **truth = git 里的签名 md** — 数据库 / 索引 / 缓存只能是可重建的 index,永不成为 truth。看到从 DB 读 canon 立即报。
- **两视图不可混用** — working-tree 视图(`law list`)与签名 manifest 视图(`canon list`)不等价,hook 实际执行的只有后者。把两者当同一个数据源用是 bug。
- **每个裁决留 receipt** — Affirm / Critique / Deny 都要写 obs.jsonl。静默裁决是 bug。

#### 静默失败(本项目最高频缺陷族)

这个项目的整个论点是"让判断可见",所以自身的静默比别处严重一个等级。

- 吞掉的 `Err`、`.ok()` 丢弃错误、`unwrap_or_default()` 把失败读成空、`Ok(false)` 被当成功处理 —— 逐个报,并指出运维拿到这个输出时无法区分哪两种情况。
- **silent first-match**:任何 `.find()` / `.position()` / `.next()` 作用在可能有多个匹配的集合上却只取第一个、且没有对"多于一个"报错的,都要报。本仓库出现过两次:store label 的选择、`[[ore_store]]` 的 `active` 标志。
- **互斥选择器被静默吞掉**:两个只应二选一的参数同时给出时,其中一个被 match 臂静默忽略。判例:`--gist` 与 `--index`。
- **provenance 退化**:持久化记录(durable note / receipt / manifest)丢掉了它指向的原始对象的身份,而只把身份 `println!` 到 stdout。终端输出是易失的,持久记录才是账。
- 一个专门为"消灭静默"而做的功能,自己带静默 —— 这是本项目最值得报的形状。

#### 持久化写入(flint 的账)

manifest / sig / receipt / epoch_floor / inbox 都是账。账的写入声称什么属性,就要核代码配不配得上。

- **`fs::write(tmp)` + `rename` 只保证元数据原子**:进程崩溃安全(页缓存还在),掉电 / 内核 panic 不安全(数据可能未落盘,rename 后的目标仍可能是 0 长度)。要真耐崩溃必须 tmp 写完 `sync_all()` 再 rename,必要时 fsync 父目录。
- **声称即债务** — 注释或文档声称了一个持久性属性而代码没实现,按 P1 报。修法可以是降级注释(承认边界),但不能留一个让读者以为有保护的说法。本仓库出现过一次:`bump_epoch_floor` 的注释点名"掉电导致 floor 归零"并声称已消除,而只有 tmp+rename、没有 fsync 的实现并不能消除它。

> 上面这些判例只教**形状**,不声明**现状**。你审的可能是一个历史 commit,也可能是 HEAD —— 某个判例在你眼前这份代码里是否还存在,只能由你读代码判断,不要因为它被列为判例就假设它已修或未修。
- 失败路径要清理临时文件,不能把 `.tmp.*` 留在 `$FLINT_HOME`。
""".strip()

FLINT_TESTS = """
#### 测试质量审查(这一组是测试文件,不要按产品代码的风格条目审)

- **断言必须真的会失败** — 问:把产品代码改坏,这个测试会红吗?永不失败的断言(`assert!(true)`、只断言"没 panic"、断言了一个恒真的形状)等于没有测试,直接报。
- **caller 与 test 走同一个 seam** — 本仓库的集成测试跑真 binary(`crates/*/tests/*.rs` 经 CLI 入口)。新测试若绕过真实入口去直调内部函数,或为了可测性新加一个只有测试用的钩子,这是设计缺陷显形成测试问题,报出来,不要建议把钩子做得更漂亮。
- **loud-failure 要断言消息本身** — 本项目多数缺陷是"静默",所以只断言 `is_err()` 不够:必须断言错误消息 / stderr 里出现了那句人能看懂的话。ambiguous / refused / not-found 这类路径尤其。
- **状态隔离** — 测试不得读写真实的 `~/.flint`(那是 dogfood 实例)。必须 tempdir。看到测试碰真实 HOME、真实 Keychain、真实 git 全局配置,报。
- **不许断言不稳定的身份** — 位置 / 索引 / 时间戳并列 / 随机 UUID 排序都不稳定。判例:`--index` 漂移是一次 codex P1;断言 index 的测试会把 bug 钉成正确。
- **不许为了变绿改测试** — 若 diff 同时改了产品代码和它的断言,检查是不是把契约改成了实现的形状。
- 覆盖缺口只报**这次 diff 新增的行为**里没有测试的那些,不要泛泛要求"提高覆盖率"。
""".strip()

FLINT_SCRIPTS = """
#### bootstrap 脚本审查(install / 引导路径)

- **secret 永不落盘、永不进 argv** — 包括不进 shell history、不进错误回显。
- **幂等** — 重跑不得破坏已签名的 manifest。改写已签名文件的操作必须是 compound(先写 draft 再一次性 pick),把 tamper-brick 窗口压到毫秒。
- `set -euo pipefail`(bash)/ `$ErrorActionPreference = 'Stop'`(PowerShell);缺了就报。
- 静默跳过 = bug:装完但没生效(例如写了 hooks 配置却没提示需要 re-trust)必须显式告知,不能静默成功。
- **Windows 侧已知坑**:bindflt 会咬住 `.cargo\\bin` 下的 shim(元数据报 0 字节);UAC 按文件名把 `install_gate-*.exe` 误判成安装程序(os error 740);正在执行的 exe 不能被覆盖。脚本若触及这些路径,检查有没有绕法。
- 退出码要有意义:harness 可能把非零码统一显示成 1,所以脚本自己的日志必须说清失败原因。
""".strip()

FLINT_ARCH = """
#### ARCHITECTURE.md 漂移检查

这个文件是 agent 面向的 canonical 现状图(模块表 / cli 动词入口 / 生命周期 / 现状表)。本仓库有明确的文档滞后史,所以:

- 只报**能被本次 diff 直接证伪**的不一致:模块表里列的文件被改名/删除、动词入口变了而表没变、现状表里写"已完成"但 diff 显示还在改。
- 指出具体是哪一行对不上哪一个改动。
- **不要**提"文档可以写得更详细 / 可以加图 / 可以补例子"这类没有证伪对象的建议 —— 这个仓库不需要更多文档,需要不说谎的文档。
""".strip()

FLINT_MANIFEST = """
#### install manifest 审查(`scripts/manifest.toml` — 决定 `flint install` 把什么放到哪)

- **target 白名单** — 每个 `[[artifact]]` 的 `target` 只允许落在 `$CLAUDE_HOME` / `$CODEX_HOME` / `$FLINT_HOME` 之下(运行时强制)。出现任何其他前缀、或用 `..` 逃出这三个根,是 P1。
- **`$INSTANCE/` 源必须可缺失** — 那是私有 memory 实例。没有 instance pointer 的机器上这些条目应当 warning 跳过,不是硬失败。新增 `$INSTANCE` 源时确认它走的是跳过路径。
- **`stage = "full"` 的 generator 只从签名 canon 渲染** — 若新增条目暗示从 working tree 读规则,报:那会让 install 产出未签名的策略。
- **source 必须真实存在** — `source` 指向仓库里已删除 / 已改名的文件,install 会在别的机器上炸。已有判例:`session-close.md` 的 step 1 指着已删除的 `pits-save`。
- **开源边界** — `source` 不得把私有文档拉进公开 repo 的构建产物。
- 不要提 TOML 风格 / 排序 / 注释密度这类没有失败模式的建议。
""".strip()

rule = {
    "include": [
        "architecture.md",
    ],
    "exclude": [
        ".opencodereview/**",
        "docs/whitepaper.md",
        "docs/pip-*.md",
        "assets/**",
        "target/**",
    ],
    "rules": [
        {"path": "crates/*/tests/*.rs", "rule": FLINT_TESTS},
        {"path": "crates/*/src/**/*.rs", "rule": rust_md + "\n\n" + FLINT_SRC},
        {"path": "scripts/**/*.{sh,ps1}", "rule": FLINT_SCRIPTS},
        {"path": "scripts/manifest.toml", "rule": FLINT_MANIFEST},
        {"path": "architecture.md", "rule": FLINT_ARCH},
    ],
}

out = REPO / ".opencodereview" / "rule.json"
out.parent.mkdir(exist_ok=True)
out.write_text(json.dumps(rule, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"wrote {out} ({out.stat().st_size} bytes)")
for r in rule["rules"]:
    print(f"  {r['path']:32s} {len(r['rule']):5d} chars")
