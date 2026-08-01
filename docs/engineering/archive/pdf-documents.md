# Archived PDF Documents Index

Last updated: 2026-08-02

This index routes readers to current PDF facts without rewriting historical
evidence. Historical documents keep the terminology and conclusions that were
valid when written; `current`, `production`, and `authority` inside them do not
override current code or the sources below.

## Current Sources

- [`../README.md`](../README.md): engineering document governance and the
  single active plan.
- [`../pdf-pipeline.md`](../pdf-pipeline.md): current production PDF execution,
  persistence, preview, recovery, and export behavior.
- [`../conventions/data-models.md`](../conventions/data-models.md): current
  persistent models; its trailing PDF v3 section is explicitly archived and
  non-production.
- [`../plans/2026-07-27-pdf-stabilization-governance.md`](../plans/2026-07-27-pdf-stabilization-governance.md):
  the only active PDF handoff and checkpoint ledger.
- [ADR 0077](../decisions/0077-pdf-production-execution-rollback.md): accepted
  production-routing decision restoring `pdf2zh`.

## Historical Groups

- Native v3 rewrite plan:
  [`../plans/2026-07-16-pdf-v3-native-rewrite.md`](../plans/2026-07-16-pdf-v3-native-rewrite.md).
- Native v3 failure handoff and rollback closeout:
  [`../plans/2026-07-20-pdf-v3-ten-page-benchmark-regression-handoff.md`](../plans/2026-07-20-pdf-v3-ten-page-benchmark-regression-handoff.md)
  and
  [`../plans/2026-07-21-pdf-production-refactor-closeout.md`](../plans/2026-07-21-pdf-production-refactor-closeout.md).
- Original v1 plan and superseded v3 addendum:
  [`../plans/2026-05-12-pdf-v1-support.md`](../plans/2026-05-12-pdf-v1-support.md).
- ADRs 0015 through 0076 record native v3 design contracts. Their
  production-routing portions are superseded by ADR 0077; feature-gated code
  may still preserve selected contracts for tests and source history.
- PDF v3 change-logs dated 2026-07-16 through 2026-07-21 are implementation
  snapshots, not current handoffs.
- PDF v3 benchmarks remain measurement evidence for their named commit,
  fixture, platform, and build only.
- Earlier PDF v1/v2, pack, performance, and release plans are historical unless
  the active governance plan explicitly cites one as a current checkpoint input.

Do not bulk-delete historical documents. If a future implementation revives a
native v3 contract, create a new active plan or ADR and define migration/reset
and three-platform acceptance before changing this index.
