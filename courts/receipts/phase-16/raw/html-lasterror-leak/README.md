# html: free ctxt->lastError strings on HTML parser context teardown

## Symptom (the failing CI fuzz job)

`.github/workflows/ci.yml` job `coverage-guided fuzz smoke (nightly)`, step
`fuzz html (30s)`, failed:

```
SUMMARY: AddressSanitizer: 24 byte(s) leaked in 1 allocation(s).
Failing input: [45, 60, 47, 65]  == "-</A"
Reproduce: cargo fuzz run html fuzz/artifacts/html/leak-1e8fe8a4...
```

(The job is `continue-on-error: true`, so the workflow reported success, but
the fuzz step was red and the `fuzz xpath` step never ran.)

## Root cause

`htmlReadMemory` creates an HTML parser context, parses, then calls
`html::free_parser_ctxt`. When the input raises an HTML diagnostic (`-</A`
reaches `handle_end_tag`, which records `HTML parser error : Unexpected end
tag : A`), `raise_error_streamed` stores an `_xmlError` in the per-context
`ctxt->lastError`, whose `message`/`file`/`str1..3` are xmlMalloc'd copies.

`free_parser_ctxt` released the input, filename, encoding and dictionary but
never the `lastError` strings, so the context's final message leaked. Upstream
`xmlFreeParserCtxt` calls `xmlResetError(&ctxt->lastError)` at teardown.

This was latent until the HTML error path started recording that end-tag
diagnostic, at which point the HTML fuzzer found it.

## Fix

`src/xml/html/mod.rs` (`free_parser_ctxt`): call
`globals::free_error_strings(&ctxt->lastError)` and
`errors::reset_error(&mut ctxt->lastError)` before releasing the context
block, mirroring upstream `xmlFreeParserCtxt -> xmlResetError`.
`free_error_strings` is NULL-safe per field, so contexts that never recorded an
error are unaffected.

## Verification

* Exact reproducer: `cargo fuzz run html <(-</A)>` -> `Executed ... in 0 ms`,
  no LeakSanitizer report.
* `cargo +nightly fuzz run html -- -max_total_time=60`: 525401 runs, clean.
* `cargo +nightly fuzz run xpath -- -max_total_time=60`: 825868 runs, clean
  (this step had never executed in CI because the html step aborted).
* `cargo test --lib`: 1333 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* lxml full suite: Ran 2007 tests, OK.
* PHP six-gate: 1250 passed / 0 failed.
