---
name: Idea
about: Something Flint should do better
labels: idea
---

**Which of the four actions does this make better?** Write it down · carry it across
agents · enforce at the moment of action · revise only when the owner says so.

Flint has four actions and deliberately no fifth — that is a design invariant, not a
roadmap gap (see [`ARCHITECTURE.md`](https://github.com/PunkGo/flint/blob/main/ARCHITECTURE.md)). Ideas that make one of the
four sharper are the ones most likely to land. If yours does not fit any of them, say so
plainly and make the case anyway; a good argument for a boundary being wrong is welcome,
a feature that quietly assumes a fifth action is not.

**What are you doing today instead, and where does it break?**

**Does it need to live in `flint-core`?** Core is frozen small on purpose (PIP-0001); the
command surface in `flint-cli` is where most things belong.
