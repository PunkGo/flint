---
schema: flint/v1
id: lean-day-one
type: rule
kind: advisory
status: proposed
version: 1
created: 2026-07-06
description: Price only physical-reality cost — treat build effort, ops-labor, and cross-stack integration as approaching zero.
source.kind: human
scope: global
trigger: architecture-choice, premature-infrastructure, too-low-level, just-ship-mvp, too-much-work, unfamiliar-stack, scaffold-design
tags: iron-law, design
---
Price only physical-reality cost: machine resources, latency, money, failure surface, information/causality limits (unknown future needs, non-existent things you can't pre-claim), the real end-user, the adversary. Treat all non-physical cost — build effort, the human-learns/watches/tunes part of ops, cross-stack and cross-language integration, learning a new system — as approaching zero; those were only ever costs because human time was the bottleneck, and that bottleneck is now AI. Two corollaries: lean in SCOPE (unknown future need is a physical information limit, and every extra part is another physical failure surface — if the need is genuinely uncertain, don't build it), and production-grade (85) in QUALITY (build effort is ~0, so there is no reason to invent a knowingly-broken MVP). Before rejecting an option, label each objection physical or non-physical; cross out the non-physical ones and trade off only on the physical boundary.
