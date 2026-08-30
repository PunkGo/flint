//! Flint's self-shipped capture default (L1 auto-mine, knowledge-layer spec skeleton §3).
//!
//! A GOVERNANCE default flint ships ON, distinct from the owner's signed Canon: it shapes
//! flint's OWN workflow (how the agent feeds the capture loop), it never JUDGES the owner's
//! actions (allow/warn/block over their code — that stays opt-in signed Canon, the sovereignty
//! line). Because it is advisory it cannot block anything anyway.
//!
//! It is opt-OUT: `[capture] auto_mine = false` in flint.toml turns it off, and then NOT ONE
//! WORD of it reaches the agent (the guarantee that keeps opt-out sovereign — a default you can
//! always see and always turn off). `flint init` writes the toggle explicitly so it is visible.

/// The workflow-only capture nudge appended to the compiled agent advisory. It instructs USE of
/// flint's capture loop — it carries NO adjudication of the owner's actions. Bare markdown, so it
/// reads the same in Claude's `.claude/rules` and Codex's `AGENTS.md`.
const AUTO_MINE_ADVISORY: &str = "\
## Flint capture — feed the loop (flint default, `[capture] auto_mine`)

The moment you hit a wall, learn something worth keeping, or notice a recurring gotcha, capture \
it with `flint pit mark --config <your flint.toml> \"<one-line gist>\"` — right then, from what \
is in front of you (no transcript scrape). A wall you don't mark is a lesson lost; this is how \
flint's knowledge loop is fed.

Capture is not a rule and enforces nothing: a gist bears no weight until YOU review it (`flint \
knowledge review`) and choose to promote it (`flint knowledge promote`). Nothing is ever \
auto-promoted. Turn this nudge off with `[capture] auto_mine = false`.";

/// Append flint's capture default to an advisory `base` when `auto_mine` is on. When off, `base`
/// is returned untouched — not one word of the default reaches the agent (opt-out sovereignty).
/// An empty `base` (no signed advisory rules) still yields the standalone nudge when on, because
/// the nudge carries its own `##` header.
pub fn append_advisory(base: &str, auto_mine: bool) -> String {
    if !auto_mine {
        return base.to_string();
    }
    if base.trim().is_empty() {
        return format!("{AUTO_MINE_ADVISORY}\n");
    }
    format!("{}\n\n{AUTO_MINE_ADVISORY}\n", base.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_returns_base_untouched() {
        assert_eq!(append_advisory("# rules\n\n- a rule\n", false), "# rules\n\n- a rule\n");
        // The sovereignty guarantee: OFF means not one word of the nudge leaks.
        assert!(!append_advisory("# rules\n", false).contains("feed the loop"));
    }

    #[test]
    fn on_appends_the_nudge_after_base() {
        let out = append_advisory("# Flint advisory rules\n\n- verify-before-claiming\n", true);
        assert!(out.starts_with("# Flint advisory rules"));
        assert!(out.contains("- verify-before-claiming"));
        assert!(out.contains("## Flint capture — feed the loop"));
        assert!(out.contains("flint pit mark"));
    }

    #[test]
    fn on_with_empty_base_is_standalone_nudge() {
        // No signed advisory rules → the nudge stands alone with its own header (a valid file).
        let out = append_advisory("", true);
        assert!(out.starts_with("## Flint capture"));
        assert!(out.contains("flint knowledge review"));
    }

    #[test]
    fn nudge_is_workflow_only_no_judgment_verbs() {
        // The sovereignty line: flint's self-governance instructs USE, it does not adjudicate.
        // It must not read as an allow/warn/block gate over the owner's actions.
        let out = append_advisory("", true);
        for verdict in ["allow", "warn", "block", "deny", "critique"] {
            assert!(!out.to_lowercase().contains(verdict), "capture nudge must not carry the verdict word `{verdict}`");
        }
    }
}
