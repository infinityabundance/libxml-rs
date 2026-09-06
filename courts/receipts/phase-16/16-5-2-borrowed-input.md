# §16.5.2 — Borrowed synchronous memory input

Date: 2026-09-06
Commit: (see git HEAD)
Status: IMPLEMENTED + PROVEN

## Hypothesis

`xmlReadMemory`-family parses copied the caller's buffer before parsing. At
the Phase-16 baseline `InputBuffer::from_memory` performed **two** full
copies per memory parse:

1. `let data = buf.to_vec();`
2. `source: InputSource::Memory(data.clone())`

The second copy was dead weight: the `InputSource::Memory` payload is never
read back for a memory buffer (`read_chunk` returns 0 for memory sources;
`read_all` is only used by the file/callback constructors).

## Design — explicit input ownership

`InputBuffer.data: Vec<u8>` became `InputBytes`:

```rust
enum InputBytes {
    Owned(Vec<u8>),           // push parsing, file/IO, transcoded, reparse
    Borrowed(&'static [u8]),  // caller region, valid for the parse only
}
```

Rules:
- Reads (`len`, indexing, slicing, `populate_parser_input` base/cur/end)
  are transparent (Deref).
- Every MUTATION converts to Owned first (`make_owned()` / `take_owned()`):
  `push_bytes`, the transcoding paths (`convert_declared_native_encoding`,
  `convert_via_registry`, `convert_detected_utf16`), the caller encoding
  overrides, and `duplicate_for_reparse`.
- The `InputSource::Memory` mirror clone and its `push_bytes` maintenance
  were removed (never read).

Borrowable front-ends (create + parse + free their own context inside one
exported call — the boxed buffer and the C-visible `_xmlParserInput`
pointers into it cannot outlive the call):

- `xmlReadDoc`, `xmlReadMemory`, `xmlParseDoc`, `xmlParseMemory`,
  `xmlRecoverDoc`, `xmlRecoverMemory`
- `xmlSAXParseDoc`, `xmlSAXParseDocWithData`, `xmlSAXParseMemory`,
  `xmlSAXParseMemoryWithData`

Multi-phase / long-lived APIs keep the owned (copy) path — exactly like
upstream, which also copies there:

- `xmlCtxtReadMemory`/`xmlCtxtReadDoc` (caller-owned persistent context)
- `xmlCreateDocParserCtxt`, `xmlCreateMemoryParserCtxt` (context returned to
  the caller before parsing)
- `xmlCreatePushParserCtxt` / `xmlCtxtResetPush` (push chunks must outlive
  the call)
- reader / html paths (unchanged this subphase; owned)

Transcoding note: a non-UTF-8 BOM/declaration or a caller `encoding`
argument converts inside the constructor/override (Borrowed → Owned), so
converted inputs copy exactly once — never twice.

## Evidence — allocation probe

`examples/alloc_probe.rs` installs a counting `#[global_allocator]` (the
input copy is a Rust Vec copy; tree content flows through the C allocator
hooks and is not counted) and parses N-byte `<r>xxxx…</r>` documents
through both ownership paths in one process:

| input size | `xmlReadMemory` (borrowed) | `xmlCtxtReadMemory` (owned) | extra copy |
|---|---|---|---|
| 1 MiB  | 1,051,095 B | 2,099,668 B | 1,048,573 B |
| 2 MiB  | 2,099,671 B | 4,196,820 B | 2,097,149 B |
| 4 MiB  | 4,196,823 B | 8,391,124 B | 4,194,301 B |

The borrowed path allocates ≈N (tokenizer text-run staging of the single
text node); the owned path allocates ≈2N — exactly one input copy of N
bytes, matching upstream's own copy-there contract.

## Correctness gates run

- `cargo test --lib`: 1259 passed, 0 failed, 1 ignored.
- ASan fuzz (nightly harness): parse 381k execs / html 663k / xpath 617k —
  clean (the parse target drives `xmlReadMemory`, the borrowed path, so any
  invalid borrow would surface as UAF under ASan).
- Behavioral equivalence is structural: bytes and semantics are identical;
  only allocation ownership changed. `xmlCtxtReadMemory`/push/reader/html
  paths are untouched.
