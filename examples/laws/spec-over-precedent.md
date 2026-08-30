---
schema: flint/v1
id: spec-over-precedent
type: rule
kind: advisory
status: proposed
version: 1
created: 2026-07-06
description: Read the vendor's current official spec before adopting a convention — never copy from local precedent or memory.
source.kind: human
scope: global
trigger: vendor-format-file, follow-the-pattern, match-existing-format, copy-convention, third-party-api, sdk-call, new-manifest
tags: iron-law, discipline
---
When adopting a convention in a vendor-defined format (SKILL.md frontmatter, Cargo.toml, package.json, pyproject.toml, MCP schema, Dockerfile, GitHub Actions workflow, .proto, framework config) or calling a third-party API/SDK/CLI, fetch the vendor's current official docs and cite the field table. Never copy from repo precedent, a remembered format, or "how it's usually done" — local precedent may itself have been written without verification, and copying propagates the error silently until someone finally checks. Diff what you plan to write against the spec and list: required fields, optional fields, fields you use that the spec lacks (probable dead fields), fields the spec has that you omit (maybe you should use them). If you genuinely cannot verify, declare "this convention is unverified, may drift" as known debt — never silently. Related to verify-before-claiming: that one guards claiming from memory; this one guards adopting from local precedent.
