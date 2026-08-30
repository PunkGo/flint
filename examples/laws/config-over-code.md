---
schema: flint/v1
id: config-over-code
type: rule
kind: advisory
status: proposed
version: 1
created: 2026-07-06
description: When both accomplish the same, express behavior in configuration, not hardcoded code.
source.kind: human
scope: global
trigger: design-plan, architecture, new-feature, hardcoded-constant, magic-number, environment-specific, retry-timeout-threshold
tags: iron-law, design
---
When a behavior can be expressed as configuration (env vars, JSON/YAML/TOML, frontmatter, declarative DSL, feature flag, a DB row) OR hardcoded in code, and both accomplish the same, choose config: it changes without redeploy, is observable and auditable, varies across environments, and tests in isolation. At design time the first step is to list the variable dimensions — which parameters differ across time, project, or environment — and push those into the config layer, picking the simplest that works (env > static JSON/YAML > DSL); put defaults in the config layer, validate the schema fail-fast at startup. Not when: a true constant (one use, never varies), control flow (if/else and business rules belong in code), or when the config's own complexity exceeds the equivalent code (configuration hell). Boundary: config expresses which parameters vary; code expresses how they are used.
