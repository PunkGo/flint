//! Verifier: re-run a falsifier method, freeze artifacts into the content store,
//! derive the outcome mechanically (spec §4 / §6.4). std::process + content
//! store only — no kernel, no tokio.
//!
//! Phase-0 contract: the method is a shell command; a *passing* method exits 0
//! (`supports`), a failing one exits non-zero (`refutes`). The richer
//! failure_threshold semantics (e.g. "0 diff lines" parsed from stdout) are a
//! later PIP — the mechanical exit-code rule is enough to drive the first cut
//! and keeps the outcome non-author-fillable.

use anyhow::{Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::content_store::ContentStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Supports,
    Refutes,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Supports => "supports",
            Outcome::Refutes => "refutes",
        }
    }
}

/// Result of running a method once: frozen artifact ids + the mechanically
/// derived outcome + the stdout hash used for P1a replay comparison.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub input_oid: String,
    pub output_oid: String,
    pub artifact_oid: String,
    pub exit_code: i32,
    pub outcome: Outcome,
    pub output_hash: String,
}

/// Run `method` via `sh -c`, freeze command + stdout into the store, derive the
/// outcome from the exit code.
pub fn run_method(method: &str, store: &ContentStore) -> Result<VerifyResult> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(method)
        .output()
        .with_context(|| format!("spawn method: {method}"))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let outcome = if exit_code == 0 {
        Outcome::Supports
    } else {
        Outcome::Refutes
    };
    let output_hash = sha256_hex(&output.stdout);

    let input_oid = store.put(method.as_bytes())?;
    let output_oid = store.put(&output.stdout)?;
    let artifact = json!({
        "method": method,
        "exit_code": exit_code,
        "stdout_sha256": output_hash,
        "stderr_len": output.stderr.len(),
    });
    let artifact_oid = store.put(serde_json::to_vec(&artifact)?.as_slice())?;

    Ok(VerifyResult {
        input_oid,
        output_oid,
        artifact_oid,
        exit_code,
        outcome,
        output_hash,
    })
}

/// P1a replay (`flint check`): re-run the CLAIM's FROZEN method string and
/// confirm the stdout hash + exit code still match what the verify recorded.
///
/// SECURITY (codex P1-R3-1): the method comes from the CLAIM (hash-protected in
/// the falsifier), NOT from a verify-controlled CAS oid. check must never execute
/// attacker-chosen bytes pulled from an arbitrary `input_oid`.
pub fn replay_method(
    method: &str,
    expected_output_hash: &str,
    expected_exit_code: i32,
) -> Result<bool> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(method)
        .output()
        .with_context(|| format!("replay method: {method}"))?;
    // Compare BOTH stdout hash AND exit code: a command can stop supporting
    // (exit code flips) while producing identical stdout (codex P1 #8).
    let exit = output.status.code().unwrap_or(-1);
    Ok(sha256_hex(&output.stdout) == expected_output_hash && exit == expected_exit_code)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

// ─── P1b contract mode (non-deterministic methods) ─────────────────────────
//
// A `forbidden` (P1a) profile compares the full stdout hash + exit code: only
// bit-reproducible methods reach active. A `contract` (P1b) profile instead
// compares an OUTCOME CONTRACT — a `failure_threshold` applied to numeric
// observables the method emits — so a method whose bytes vary run-to-run but
// whose contract holds can still be verified. The outcome stays mechanically
// derived (never author-filled), and the projector re-applies the SAME frozen
// threshold to the verify's recorded observables (a pure, IO-free consistency
// guard mirroring the exit_code check).

/// Extract observables from a method's stdout: the LAST non-empty line parsed as
/// a flat JSON object of `{name: number}`. Returns `None` if absent or not a
/// pure-numeric object (fail-closed: the caller treats `None` as not-evaluable).
pub fn extract_observables(stdout: &[u8]) -> Option<serde_json::Map<String, serde_json::Value>> {
    let text = std::str::from_utf8(stdout).ok()?;
    let last = text.lines().rev().find(|l| !l.trim().is_empty())?;
    let val: serde_json::Value = serde_json::from_str(last.trim()).ok()?;
    let obj = val.as_object()?;
    if obj.is_empty() || !obj.values().all(serde_json::Value::is_number) {
        return None;
    }
    Some(obj.clone())
}

/// Evaluate a pass-condition threshold `<obs> <op> <num>` (op ∈ `< <= > >= == !=`)
/// against extracted observables. `Some(true)` = pass (supports), `Some(false)` =
/// fail (refutes), `None` = grammar/observable unresolvable (fail-closed: the
/// caller must NOT treat `None` as a pass).
pub fn eval_threshold(
    observables: &serde_json::Map<String, serde_json::Value>,
    threshold: &str,
) -> Option<bool> {
    let mut it = threshold.split_whitespace();
    let name = it.next()?;
    let op = it.next()?;
    let rhs: f64 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None; // extra tokens -> reject the grammar (fail-closed)
    }
    let lhs = observables.get(name)?.as_f64()?;
    Some(match op {
        "<" => lhs < rhs,
        "<=" => lhs <= rhs,
        ">" => lhs > rhs,
        ">=" => lhs >= rhs,
        "==" => (lhs - rhs).abs() < f64::EPSILON,
        "!=" => (lhs - rhs).abs() >= f64::EPSILON,
        _ => return None,
    })
}

/// Run `method`, freeze its artifacts (same as `run_method`), and additionally
/// derive the outcome from the CONTRACT (`failure_threshold` over the stdout
/// observables) instead of the exit code. Returns the result plus the extracted
/// observables to record in the verify payload. `None` observables / unresolvable
/// threshold => `Refutes` (fail-closed — an un-evaluable contract does not pass).
pub fn run_method_contract(
    method: &str,
    threshold: &str,
    store: &ContentStore,
) -> Result<(VerifyResult, serde_json::Value)> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(method)
        .output()
        .with_context(|| format!("spawn contract method: {method}"))?;
    let exit_code = output.status.code().unwrap_or(-1);
    let output_hash = sha256_hex(&output.stdout);

    let observables = extract_observables(&output.stdout);
    let pass = observables
        .as_ref()
        .and_then(|o| eval_threshold(o, threshold))
        .unwrap_or(false);
    let outcome = if pass { Outcome::Supports } else { Outcome::Refutes };

    let input_oid = store.put(method.as_bytes())?;
    let output_oid = store.put(&output.stdout)?;
    let observables_json = observables
        .map(serde_json::Value::Object)
        .unwrap_or(serde_json::Value::Null);
    let artifact = json!({
        "method": method,
        "mode": "contract",
        "threshold": threshold,
        "observables": observables_json,
        "pass": pass,
        "exit_code": exit_code,
        "stdout_sha256": output_hash,
    });
    let artifact_oid = store.put(serde_json::to_vec(&artifact)?.as_slice())?;

    Ok((
        VerifyResult {
            input_oid,
            output_oid,
            artifact_oid,
            exit_code,
            outcome,
            output_hash,
        },
        observables_json,
    ))
}

/// Run `method` with `input` fed on stdin, deriving the outcome (exit code in
/// forbidden mode, or the contract over observables). Used by the discrimination
/// check — a method that ignores its stdin yields the same outcome regardless of
/// input, so it cannot discriminate.
fn run_with_input(method: &str, input: &[u8], threshold: &str, contract: bool) -> Result<Outcome> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(method)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn discrimination method: {method}"))?;
    // Write the whole input then drop stdin so the child sees EOF. A method that exits
    // without reading its stdin (e.g. `true`) closes the pipe mid-write — that is not a
    // delivery failure, it is the method ignoring its input, which is exactly what the
    // discrimination check exists to expose (whether the race resolves to EPIPE is
    // platform timing: Linux loses it, macOS often wins it). Any other error is real.
    if let Err(e) = child.stdin.take().context("child stdin missing")?.write_all(input) {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            return Err(e).context("write discrimination input to stdin");
        }
    }
    let output = child.wait_with_output().context("wait discrimination method")?;
    let exit_code = output.status.code().unwrap_or(-1);
    let supports = if contract {
        extract_observables(&output.stdout)
            .and_then(|o| eval_threshold(&o, threshold))
            .unwrap_or(false)
    } else {
        exit_code == 0
    };
    Ok(if supports { Outcome::Supports } else { Outcome::Refutes })
}

/// Discrimination check (spec §3, the mechanical "区分力" half): a falsifier has
/// discriminating power IFF its `method` SUPPORTS the sovereign known-good input
/// and REFUTES the known-bad input (both fed on stdin). A method that ignores its
/// input (e.g. `true`, which always exits 0) yields the same outcome on both and
/// fails — catching trivially-passing falsifiers. The fixtures are sovereign-
/// supplied; an author cannot self-certify discrimination (F10).
pub fn discriminates(
    method: &str,
    good_input: &[u8],
    bad_input: &[u8],
    threshold: &str,
    contract: bool,
) -> Result<bool> {
    // Run good TWICE then bad TWICE: a method with real discriminating power gives
    // the same outcome on repeats, so a stateful "support the Nth call, refute the
    // rest" trick (which ignores its input) gives the wrong outcome on the SECOND
    // good run and fails here.
    //
    // HONEST BOUNDARY (spec §12): this is NOT full isolation. The runs share the
    // filesystem, env, and cwd, so a method that counts total invocations, writes
    // to /tmp, or inspects the environment can still game it. Discrimination raises
    // the cost of self-deception; it does not eliminate it. True per-run isolation
    // (sandbox / container) is a PIP. ALSO (codex P2-R3-2): discrimination feeds
    // the fixtures on STDIN, but a real `verify` runs the method with NO fixture
    // stdin — a method can support known-good stdin, refute known-bad stdin, AND
    // support empty stdin forever, passing discrimination while testing nothing
    // about the real claim. This is an honest smoke test of discriminating POWER,
    // not a proof the falsifier tests the right thing.
    let good1 = run_with_input(method, good_input, threshold, contract)?;
    let good2 = run_with_input(method, good_input, threshold, contract)?;
    let bad1 = run_with_input(method, bad_input, threshold, contract)?;
    let bad2 = run_with_input(method, bad_input, threshold, contract)?;
    Ok(good1 == Outcome::Supports
        && good2 == Outcome::Supports
        && bad1 == Outcome::Refutes
        && bad2 == Outcome::Refutes)
}

/// P1b replay (`flint check`, contract mode): re-run the CLAIM's FROZEN method
/// string (hash-protected, not a verify oid — codex P1-R3-1) and confirm the SAME
/// frozen threshold still yields the same supports/refutes verdict. Unlike P1a
/// this does NOT require a stable stdout hash — only a stable contract verdict.
pub fn replay_method_contract(
    method: &str,
    threshold: &str,
    expected_supports: bool,
) -> Result<bool> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(method)
        .output()
        .with_context(|| format!("replay contract method: {method}"))?;
    let pass = extract_observables(&output.stdout)
        .and_then(|o| eval_threshold(&o, threshold))
        .unwrap_or(false);
    Ok(pass == expected_supports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_method_supports_and_replays() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("obj")).unwrap();
        let r = run_method("printf hello", &store).unwrap();
        assert_eq!(r.outcome, Outcome::Supports);
        assert_eq!(r.exit_code, 0);
        assert!(replay_method("printf hello", &r.output_hash, r.exit_code).unwrap());
    }

    #[test]
    fn failing_method_refutes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("obj")).unwrap();
        let r = run_method("exit 3", &store).unwrap();
        assert_eq!(r.outcome, Outcome::Refutes);
        assert_eq!(r.exit_code, 3);
    }

    #[test]
    fn replay_detects_changed_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("obj")).unwrap();
        let r = run_method("printf hello", &store).unwrap();
        assert!(!replay_method("printf hello", "deadbeef", r.exit_code).unwrap());
    }

    #[test]
    fn extract_observables_takes_last_numeric_json_line() {
        let o = extract_observables(b"noise line\n{\"latency_ms\": 42, \"errors\": 0}\n").unwrap();
        assert_eq!(o.get("latency_ms").unwrap().as_f64(), Some(42.0));
        assert_eq!(o.get("errors").unwrap().as_f64(), Some(0.0));
    }

    #[test]
    fn extract_observables_rejects_non_numeric_or_absent() {
        assert!(extract_observables(b"not json").is_none());
        assert!(extract_observables(b"{\"x\": \"str\"}").is_none());
        assert!(extract_observables(b"{}").is_none());
        assert!(extract_observables(b"").is_none());
    }

    #[test]
    fn eval_threshold_grammar_and_ops() {
        let mut o = serde_json::Map::new();
        o.insert("latency_ms".into(), serde_json::json!(42));
        assert_eq!(eval_threshold(&o, "latency_ms < 100"), Some(true));
        assert_eq!(eval_threshold(&o, "latency_ms < 10"), Some(false));
        assert_eq!(eval_threshold(&o, "latency_ms >= 42"), Some(true));
        assert_eq!(eval_threshold(&o, "latency_ms == 42"), Some(true));
        // unresolvable / bad grammar -> None (fail-closed)
        assert_eq!(eval_threshold(&o, "missing < 1"), None);
        assert_eq!(eval_threshold(&o, "latency_ms ~ 1"), None);
        assert_eq!(eval_threshold(&o, "latency_ms < 1 extra"), None);
    }

    #[test]
    fn contract_method_supports_when_threshold_passes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("obj")).unwrap();
        let (r, obs) = run_method_contract("printf '{\"q\": 3}'", "q < 5", &store).unwrap();
        assert_eq!(r.outcome, Outcome::Supports);
        assert_eq!(obs.get("q").unwrap().as_f64(), Some(3.0));
        assert!(replay_method_contract("printf '{\"q\": 3}'", "q < 5", true).unwrap());
    }

    #[test]
    fn contract_method_refutes_when_threshold_fails_or_unevaluable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("obj")).unwrap();
        let (r, _) = run_method_contract("printf '{\"q\": 9}'", "q < 5", &store).unwrap();
        assert_eq!(r.outcome, Outcome::Refutes);
        // un-evaluable contract (no numeric observable) -> refutes, fail-closed.
        let (r2, _) = run_method_contract("printf 'no json here'", "q < 5", &store).unwrap();
        assert_eq!(r2.outcome, Outcome::Refutes);
    }

    #[test]
    fn discriminates_rejects_input_ignoring_method() {
        // `true` ignores stdin and always exits 0 -> supports on BOTH inputs ->
        // no discriminating power (catches trivially-passing falsifiers, F3).
        assert!(!discriminates("true", b"good", b"bad", "", false).unwrap());
    }

    #[test]
    fn discriminates_accepts_input_sensitive_method() {
        // grep supports (exit 0) on the known-good input, refutes (exit 1) on the
        // known-bad one -> has discriminating power.
        assert!(discriminates("grep -q GOOD", b"has GOOD inside", b"nope", "", false).unwrap());
        // ... and the reverse fixture orientation must NOT pass.
        assert!(!discriminates("grep -q GOOD", b"nope", b"has GOOD inside", "", false).unwrap());
    }

    // codex P1-3: a stateful method that supports its FIRST call and refutes the
    // rest (a /tmp counter), ignoring its input, is caught because good is run
    // TWICE — the second good run returns refute.
    #[test]
    fn discriminates_catches_stateful_first_call_counter() {
        let counter =
            std::env::temp_dir().join(format!("flint-discrim-{}.counter", std::process::id()));
        let _ = std::fs::remove_file(&counter);
        let method = format!(
            "f={}; n=$(cat \"$f\" 2>/dev/null || echo 0); n=$((n+1)); echo $n > \"$f\"; [ $n -eq 1 ]",
            counter.display()
        );
        let r = discriminates(&method, b"good", b"bad", "", false).unwrap();
        let _ = std::fs::remove_file(&counter);
        assert!(!r, "a first-call-only counter is caught by the repeated good run");
    }
}
