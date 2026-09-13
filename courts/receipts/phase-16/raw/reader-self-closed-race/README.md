# reader: stop stale self-closed markers from suppressing END_ELEMENT events

## Symptom (the intermittently failing CI job)

`.github/workflows/ci.yml` step `regression court` ran `cargo test --lib`
twice in one job (once directly, once via `tools/courts/regression.sh`). The
second run intermittently failed one reader test:

```
test xml::reader::tests::test_read_simple_document ... FAILED
  left: 4
 right: 5
```

`collect_nodes` returned 4 events instead of 5. Instrumentation showed the
parse was complete and well-formed (`result=0 paused=false truncated=false`),
but the event walk dropped the inner `END_ELEMENT child`:

```
DIAG_NODES len=4 [(ELEMENT,"root",0),(ELEMENT,"child",1),(TEXT,"#text",2),(END_ELEMENT,"root",0)]
```

Reader-only runs never reproduced it; it needed the full binary running many
modules in parallel, so it is cross-test state, not a reader-local bug.
Reproduced locally at `--test-threads=3` roughly 1 in 90 runs.

## Root cause

The whole-tree XML reader rebuilds traversal events from the parsed tree and
cannot distinguish `<a/>` from `<a></a>` in the tree alone, so the parser
records self-closed element nodes in a process-global registry keyed by
**raw document and node addresses**
(`helpers::SELF_CLOSED_NODES: HashMap<doc_addr, HashSet<node_addr>>`). The
reader's event walk consumes a marker with `take_self_closed(doc, node)` and
suppresses the `END_ELEMENT` when a marker is present.

Nothing dropped a document's markers when the reader was torn down: `Drop for
XmlTextReader` freed `self.doc` directly, and `xmlTextReaderCurrentDoc`
transfers the document to the caller without cleanup. A reader that was freed
(or whose document was preserved) with markers still recorded left the entry
behind. Once the document was freed its address could be reused by a later
parse, and if a new element node landed on an old marked address the walk
misread `<child>text</child>` as self-closed and suppressed its END event.
Which address lands where depends on the heap state left by the other tests in
the process — hence the intermittent, order-dependent failure.

## Fix

`src/xml/reader/mod.rs`:

* `Drop for XmlTextReader` now calls `helpers::drop_self_closed(self.doc)`
  before releasing the document, so markers never outlive their document.
* The completed-walk paths (`build_events` / `build_events_with`) also drop the
  document's now-empty registry entry so a long-lived process does not
  accumulate one global-map key per reader parse.

This is a lifecycle fix, not a guard: after teardown the registry cannot hold
addresses of freed documents.

## Verification

* `test_(etree|htmlparser)`-style reproduction: `libxml_rs` unit binary run
  **150x each at `--test-threads={2,3,4,8}` (600 runs), 0 failures**; before
  the fix the same binary failed ~1/90 at `--test-threads=3`.
* `sh tools/courts/regression.sh` -> PASSED (`cargo test --lib`
  1333 passed / 0 failed / 3 ignored; release drop-in links).
* `cargo fmt --check`: clean.
* lxml full suite: Ran 2007 tests, OK (reader change does not regress the
  drop-in court).
* PHP six-gate: 1250 passed / 0 failed.
