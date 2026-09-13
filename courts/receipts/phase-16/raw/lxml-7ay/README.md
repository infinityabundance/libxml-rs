# lxml court 7ay — NOBLANKS `areBlanks` heuristic and NSCLEAN redundant-namespace pruning

## Root causes fixed

### 1. `remove_blank_text` dropped leaf-element whitespace

The NOBLANKS gate dropped every all-whitespace character run when
`keepBlanks == 0`, so `<b>  </b>` lost its content and round-tripped as
`<b/>`. Upstream does not use a blanket rule: `xmlCharacters` consults
`areBlanks(ctxt, buf, size, isBlank)` (parser.c), which keeps a whitespace run
when it is the character content of a leaf element.

`XmlParser::blanks_ignorable` now ports that heuristic. It is called from both
NOBLANKS gates (`sax_characters_text` for the pull parser and
`sax_characters_run` for the persistent push driver) with the two raw bytes at
and after the input cursor:

```
xml:space is preserve or already consumed content -> keep
cursor byte is neither '<' nor CR                 -> keep
no children and the run ends with "</"            -> keep   (<b>  </b>)
last child is a text node                         -> keep
first child is a text node                        -> keep
otherwise                                         -> drop
```

The pull path reads the cursor from the base input's committed position; the
push driver commits the position before dispatching, exactly as upstream
advances `input->cur` before calling the handler.

Fixes `doc/tutorial.txt` and `test_iterparse_strip`.

### 2. `ns_clean` did not prune redundant namespace declarations

`XML_PARSE_NSCLEAN` was accepted as an option but never implemented, so the
redundant `xmlns="test"` on `<b>` in `<a xmlns="test"><b xmlns="test"/></a>`
survived into the tree and was serialized.

Upstream drops such a declaration in `xmlParserNsPush` (parser.c): when the
prefix is already in scope with the SAME URI it returns without adding an
`xmlNs`. `XmlParser::ns_decl_is_redundant` ports that rule, consulting the
parser-scoped `ns_scope` stack (pure-SAX parses have no tree) and the `nsDef`
lists of the open tree ancestors. A redundant declaration is skipped before it
enters the tag's `ns_decls`, so no `xmlNs` node is created and the SAX
namespace array omits it.

Declarations on the *current* element are deliberately not consulted here;
upstream treats those as duplicate-attribute errors, a separate path.

Fixes `doc/parsing.txt`.

## Evidence

* Full suite: **22 raw unique failing ids**, down from 25 — **3 fixed,
  0 regressions**:
  * `doc/tutorial.txt` (265/265 doctest examples)
  * `doc/parsing.txt` (167/167 doctest examples)
  * `test_iterparse_strip`
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline is exactly the three ids above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
