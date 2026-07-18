# ADR 0057: PDF v3 Local Provider Bridge

Date: 2026-07-18

Status: Accepted

Refines ADR 0055 and ADR 0056.

## Context

PDF v3 could build an identity-bound `TranslationPagePlan`, but it could not
send that plan to any of Rosetta's local translation providers. The existing
PDF batch/chunk/retry implementation was coupled to
`managed_pdf2zh::worker::PdfTranslationUnit`. Making the new planner construct
that legacy type would reverse the intended dependency and leave old worker
ownership embedded in the replacement architecture.

The provider bridge also needs to preserve planner `{vN}` tokens without ever
sending them to the model, support the existing Lightning, mobile batch and
llama.cpp backends, retain cancellation and retry behavior, and return results
by stable PDF v3 unit identity.

## Decision

Introduce a private provider-owned `ProviderTranslationUnit` containing only
the fields required by batch translation: unit ID, source text and translation
eligibility. All existing chunk preparation, placeholder isolation, provider
batching, split retry, progressive completion and result reconstruction now
operate on this generic type.

The old pdf2zh entry point remains as a thin compatibility adapter that maps
its worker units into provider units. PDF v3 does not construct or reference
the legacy worker type. Its new async bridge maps each plan unit's
`providerText` directly into a provider unit and maps completed output into
`TranslationUnitResult { unitId, translatedText }`.

The bridge reuses the existing provider configuration and behavior:

- Lightning contents batching with its larger prompt/batch limits;
- mobile batch chat role setup and negotiated batch size;
- llama.cpp parallel batch size and bounded split retry;
- `{vN}` placeholders removed from model input and restored around translated
  chunks;
- a shared atomic cancellation flag checked before provider I/O and retries;
- provider request and character-count metrics without source or translated
  text fields.

Before provider I/O, the bridge rejects an unsupported plan schema, invalid
page identity, an empty safe-unit set, empty provider text and duplicate unit
IDs. Failures are typed as `no-translatable-units`, `invalid-plan`, `cancelled`
or `provider`, with an explicit retryability decision. The PDF v3 failure
message is stable and text-free; provider response details do not cross into
scheduler or future frontend state.

The bridge does not assign provider/model identity. The future page processor
must obtain that identity from the selected runtime/component manifest and put
it into `TranslationPatchDraftMetadata` before patch construction. It must
still reassemble against the exact current PageGraph and resolve the pending
patch through the renderer before durable commit.

No Tauri command, scheduler transition, persistent schema or UI state is added
in this slice.

## Evidence

Automated tests cover:

- public PDF v3 bridge execution through a scripted batch provider;
- plan `providerText` chunking and protected placeholder reconstruction;
- stable unit IDs and provider metrics in completed results;
- cancellation before provider I/O with the request left unconsumed;
- empty safe plans and duplicate unit identity rejected without provider I/O;
- all pre-existing legacy PDF chunking, retry and event tests after conversion
  to the generic provider-owned unit.

## Consequences

### Positive

- PDF v3 now reaches every existing local provider without depending on the
  old PDF worker contract.
- The provider batching implementation remains single-source rather than being
  copied into the new pipeline.
- Protected tokens remain outside model input and exact unit identity survives
  batching and result reconstruction.
- Cancellation, retry and metrics behavior is available to the new path.

### Costs

- The shared provider implementation still physically resides under the
  current PDF job module and retains legacy metric/type names. It can move to a
  neutral module after the old orchestration is removed without changing the
  PDF v3 plan contract.
- Provider units clone page-local provider text before async execution. Memory
  remains page-bounded but includes another copy of accepted plan text.
- `PdfV3TranslationWorker::run_batch` is synchronous around its page processor;
  an async renderer-owning processor/worker boundary is still required.
- Provider/model runtime identity and renderer resolution remain pending.

## Rejected Alternatives

- Make PDF v3 construct `managed_pdf2zh::worker::PdfTranslationUnit`.
- Duplicate provider batching and retry logic inside `pdf_v3`.
- Send protected tokens to the model and hope they survive generation.
- Return provider raw errors through durable scheduler or frontend state.
- Treat an empty safe plan as a successful provider request.
