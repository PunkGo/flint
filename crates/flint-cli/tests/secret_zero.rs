//! Pattern contract for the SHIPPED `secret-zero` sample — the only `response: block`
//! rule in `examples/laws/`, and the one most likely to be copied first. The regex is
//! data (law frontmatter), so its behaviour is pinned against the REAL shipped bytes
//! (`include_str!`), never a re-typed copy.
//!
//! The fixtures encode the contract in both directions:
//!
//!   - a cleartext credential on argv denies — that is the whole point;
//!   - an unexpanded shell variable in the password position does NOT deny. `$DB_PASS`
//!     is a pointer, not a secret, and it is exactly the practice the rule's own message
//!     recommends. A gate that blocks its own recommended fix teaches people to route
//!     around it; the shipped sample carried that false positive until it was measured,
//!     and this test is what stops it coming back.
//!
//! Every credential-shaped fixture is assembled from fragments at runtime. No token-
//! shaped literal belongs in a public repository: it trips the author's own gate while
//! writing the file, and it trips other people's secret scanners after it lands.

use std::collections::BTreeMap;
use std::path::PathBuf;

use flint_core::canon::{parse_rule, reduce, ParsedRule};
use flint_core::touchstone::{judge, Action, Response, TouchstonePolicy};

const SECRET_ZERO: &str = include_str!("../../../examples/laws/secret-zero.md");

fn policy() -> TouchstonePolicy {
    let mut files: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    files.insert(PathBuf::from("rules/secret-zero.md"), SECRET_ZERO.as_bytes().to_vec());
    reduce(&files).expect("the shipped secret-zero sample reduces into a policy")
}

fn bash(command: &str) -> Action {
    Action {
        tool_kind: "exec".into(),
        scopes: vec!["exec:bash".into()],
        command: Some(command.into()),
        ..Default::default()
    }
}

fn verdict_tag(cmd: &str) -> &'static str {
    judge(&policy(), &bash(cmd)).tag()
}

/// Thirty filler characters — long enough to clear every length floor in the pattern.
const FILLER: &str = "abcdefghijklmnopqrstuvwxyz0123";

#[test]
fn the_shipped_sample_is_a_hard_block() {
    let rule = match parse_rule("rules/secret-zero.md", SECRET_ZERO).expect("secret-zero parses") {
        ParsedRule::Gate(cr) => cr,
        ParsedRule::Advisory(_) => panic!("secret-zero is a gate rule, not advisory"),
    };
    assert_eq!(rule.rule.id, "secret-zero");
    assert_eq!(rule.rule.response, Response::Deny, "a leaked credential has no recovery path");
}

#[test]
fn cleartext_credentials_on_argv_deny() {
    let pw = format!("{}{}", "hunt", "er2xyz");
    for cmd in [
        format!("psql postgres://user:{pw}@localhost/db"),
        format!("mysql mysql://root:{pw}@10.0.0.1/app"),
        format!("redis-cli -u redis://default:{pw}@cache:6379"),
        format!("curl -H 'Authorization: {}{FILLER}' https://example.test", "Bearer "),
        format!("git remote add o https://{}{FILLER}@example.test/r.git", "ghp_"),
        format!("export KEY={}{FILLER}", "sk-"),
        format!("aws configure set k {}{}", "AKIA", "0123456789ABCDEF"),
    ] {
        assert_eq!(verdict_tag(&cmd), "deny", "must deny a cleartext credential: {cmd}");
    }
}

#[test]
fn an_unexpanded_variable_in_the_password_position_does_not_deny() {
    // The rule's own message tells you to use an env var. If the gate then blocks the
    // env-var form, the advice is a trap. Both spellings have to pass.
    for cmd in [
        "psql postgres://user:$DB_PASS@localhost/db",
        "psql postgres://user:${DB_PASS}@localhost/db",
        "mysql mysql://root:$MYSQL_PW@10.0.0.1/app",
    ] {
        assert_eq!(verdict_tag(cmd), "affirm", "a pointer is not a secret: {cmd}");
    }
}

#[test]
fn ordinary_commands_are_not_spoken_to() {
    for cmd in [
        "psql postgres://localhost/db",
        "psql postgres://user@localhost/db",
        "psql postgres://u:p@localhost/db", // short placeholder, below the 6-character floor
        "cargo test --workspace",
        "echo 'no secrets here'",
    ] {
        assert_eq!(verdict_tag(cmd), "affirm", "must not fire on: {cmd}");
    }
}
