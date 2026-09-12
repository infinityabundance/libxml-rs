# lxml court 7ar — output IO, empty comments, XML_SKIP_IDS

`candidate_sha=24ead03b` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
previous (bdb8ed79 / lxml-7ap)   58 raw ids, 55 excluding the flaky group
now (24ead03b / lxml-7ar)        52 raw ids, 49 excluding the flaky group
fixed                             6
new                               0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

## 1 — a zero-length file write was an error

`file_write_callback` returned -1 for `len <= 0`, but `xmlOutputBufferFlush`
invokes the write callback unconditionally, including with an empty buffer at
close time (`xmlIO.c`). Any document large enough to have been flushed during
writing therefore made `xmlOutputBufferClose` return -1 and lxml raised
`SerialisationError: unknown error -1`. The callback now returns 0 for a
zero-length write (`xmlFileWrite` semantics).

Fixed `test_write_file_url` (already partly fixed by 2) and both
`test_c14n_file` ids, which write C14N output to a file.

## 2 — xmlOutputBufferCreateFilename did not decode the URL

The name was opened literally. libxml2 treats it as a URL: a `file://` scheme
and authority are stripped and the remaining path is percent-decoded once.
lxml depends on both halves — it escapes `%` to `%25` in a plain filename
before calling and passes `file://` URLs unescaped.

C probe (oracle vs candidate), before the fix:

```
input                                  oracle file              candidate
/tmp/tst_p%2520plain.xml               /tmp/tst_p%20plain.xml   /tmp/tst_p%2520plain.xml
file:///tmp/tst_p%2520url.xml          /tmp/tst_p%20url.xml     (create failed)
file:///tmp/tst_p%20url2.xml           /tmp/tst_p url2.xml      (create failed)
file://localhost/tmp/tst_p%20url3.xml  /tmp/tst_p url3.xml      (create failed)
```

After the fix the candidate matches the oracle for all four.

## 3 — an empty comment vanished

`<!---->` was delivered to the SAX comment handler as NULL, so
`xmlSAX2Comment` created a comment with NULL content. The serializer skips only
a NULL content (`xmlsave.c`), so the comment disappeared. Upstream delivers
`BAD_CAST ""` (`parser.c xmlParseComment`), producing content `""`:

```
oracle    child content=''      xmlNodeDump -> '<!---->'
candidate child content=(nil)   xmlNodeDump -> ''
```

`sax_comment` now always passes a non-NULL (possibly empty) string via
`vec_to_cstr_keep_empty`.

## 4 — XML_SKIP_IDS was ignored

lxml's `_initSaxDocument` sets `ctxt->loadsubset |= XML_SKIP_IDS` when the
parser was created with `collect_ids=False`. The default SAX2 attribute handler
now skips ID/IDREF registration when that flag is set, so
`getelementids()` / `XMLDTDID` return an empty dict.
