# 2026-07-14 PDF Lightning Unit Segmentation

## Summary

Improved native PDF translation-unit preparation for RWKV Lightning after a
10-page paper reliably produced repeated bibliography output in its final
pages.

The captured 206-item provider request showed both extremes:

- 12 inputs exceeded 800 characters and 8 exceeded 1200 characters;
- the longest input was 2994 characters;
- 19 inputs contained at most 3 characters, including `2.`, `el`, `:`, and
  `.`;
- a 1381-character bibliography fragment ending in a partial `[49]` author
  list caused deterministic repetition in direct API replays.

## Change

The Rust PDF v2 unit translator now:

- splits Lightning text at a `160` estimated-token target and `220` hard cap;
- recognizes consecutive numeric reference markers separated by an entry-sized
  span and splits those bibliography runs at `[n]` boundaries before applying
  the token cap, without splitting nearby inline citations;
- returns pure symbols, pure numbers, and fragments containing at most two
  ASCII letters literally, without sending them to the model;
- reassembles every translated and literal part under the original PDF engine
  `unitId`, preserving render alignment;
- counts `totalInputChars` from provider-bound chunks instead of adding the
  original unit characters and provider characters together.

No PDF persistent-data format or engine contract changed.

## Full I/O Diagnosis

`ROSETTA_RWKV_IO_DEBUG=1` now records the structured Lightning `requestBody`
alongside the response and parsed inputs/outputs. Non-empty body passwords are
replaced with `[redacted]`, and authentication headers are not recorded.

This debug mode is disabled by default and writes document text. It is for a
deliberate local reproduction only; normal job diagnostics remain content-free.

## Validation

Focused Rust tests cover:

- the consecutive-reference badcase splitting into seven provider items;
- long Lightning text respecting the hard prompt budget;
- non-consecutive prose citations remaining intact;
- trivial fragments bypassing the provider and round-tripping literally;
- full request-body debug serialization with password redaction.
