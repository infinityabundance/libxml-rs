# lxml court 7ad — copy-name dictionary interning

`candidate_sha=2eece63c` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
baseline (a578906a / lxml-7ac)   101 unique failing ids
now (2eece63c / lxml-7ad)         93 unique failing ids
fixed                              8
new                                0
PHP six-gate                      1250 passed / 0 failed
cargo test --lib                  1325 passed / 0 failed / 3 ignored
```

## The failure

Eight ids, in two clusters:

```
test_elementpath.test_find                              (4 test classes)
test_objectify.test_object_path_addattr_element
test_objectify.test_object_path_addattr_create_element
test_objectify.test_object_path_set_element
test_objectify.test_object_path_set_create_element
```

Both fail only after a node is copied into a document that owns a name
dictionary:

```python
elem[1] = deepcopy(elem[2])          # test_find
path.setattr(root, new_el.sub)       # objectify
```

`test_find` then misses a match for the copied `section`; objectify reports
`no such child: {objectified}a` for a subtree that is visibly present.

## Root cause

lxml matches tags **by dictionary pointer**, not by string content. See
`apihelpers.pxi::_nsTagMatchesExactly`:

```cython
if c_qname.c_name is not NULL and c_qname.c_name is not c_node_name:
    return 0
```

`c_qname.c_name` comes from `xmlDictExists(doc.dict, tag, len)`
(`_mapTagsToQnameMatchArray`). So a node matches only if
`node->name` **is** the dictionary's entry for that name.

Upstream `xmlStaticCopyNode` (tree.c) preserves that:

```c
else if (node->name != NULL) {
    if ((doc != NULL) && (doc->dict != NULL))
        ret->name = xmlDictLookup(doc->dict, node->name, -1);
    else
        ret->name = xmlStrdup(node->name);
}
```

and, for attributes, `xmlNewPropInternal` / `xmlNewDocProp` do the same.

The candidate used `xmlStrdup` unconditionally, so a copied element/attribute
name was a private heap string. Address-based search could not see it. Trees
that were never copied were unaffected because the parser already interns
names.

## Why the naive fix regressed PHP

Interning the copied name *unconditionally* also interned the text/comment
marker (`"text"` / `"comment"`), and then:

1. `xmlStaticCopyNode` interns `"text"` into the source doc's dictionary and
   the copied text node's `name` points there.
2. `xmlSetTreeDoc` → `xmlNodeSetDoc` deliberately does **not** migrate
   text/comment names across documents (upstream keeps them as the shared
   non-dict `xmlStringText` / `xmlStringComment` globals).
3. At teardown the destination document frees the node; with
   `dict = dest->dict`, `xmlDictOwns(dest->dict, name)` is false, so the name
   is freed — but it still belongs to the **source** dictionary.
4. `xmlFreeDoc(source)` frees the dictionary block again.

PHP's modern-DOM `Dom\HTMLDocument::adoptNode($xmlDoc->cloneNode(true))` hits
exactly this path:

```
free(): double free detected in tcache 2
```

Reproducer: `../lxml-7ac/adopt_repro.c`.

## Fix

`src/abi/exports_treedump.rs::copy_name_into_doc` now takes the copied node's
type and mirrors `xmlStaticCopyNode` exactly:

```
static marker            -> keep by pointer
XML_TEXT_NODE/COMMENT    -> heap copy (upstream leaves these non-dict)
otherwise, doc has dict  -> xmlDictLookup(doc->dict, name, -1)
otherwise                -> xmlStrdup(name)
```

`src/abi/exports_tree.rs` adds `intern_name_in_doc` and uses it in
`new_prop_internal` (the `eatname == 0` branch) and `xmlNewDocProp`, matching
`xmlNewPropInternal` / `xmlNewDocProp`. The `eatname == 1` error paths now use
`free_name_unless_dict_owned` so an eaten name that came from the dictionary is
not freed (upstream `xmlNewPropInternal` checks `xmlDictOwns` there).

## Adjacent defect fixed in the same area

`src/abi/data_globals.rs`: `xmlStringTextNoenc` was a 9-byte array
(`*b"textnoenc"`) missing its NUL terminator. Upstream (tree.c 2.15) is
ten bytes:

```c
const xmlChar xmlStringTextNoenc[] =
              { 't', 'e', 'x', 't', 'n', 'o', 'e', 'n', 'c', 0 };
```

It is now the upstream 10-byte NUL-terminated array. `xsltCopyTextString`
still duplicates the marker into a heap string (the node owns and frees its
name; this engine compares the marker by content in `is_noenc_text`), but the
comment no longer claims the static lacks its terminator.

## Court harness fix

`courts/suites/phase14/consumers/lib.sh`: `build.rs` bakes the **host**
artifact path into each generated libtool `.la` `libdir`. The PHP court mounts
`target/debug` at `/candidate`, so that host path does not exist in the
container. While `make` was a no-op the stale `.la` never mattered; any
candidate change forces a relink and libtool hands the linker
`<host>/lib/libexslt.so`, which fails:

```
/usr/bin/ld: cannot find /mnt/1tb_kingston/libxml-rs/target/debug/lib/libexslt.so
```

`lib.sh` now reads each `.la`'s recorded `libdir` and, when it is absent,
bridges it to `/candidate/lib`, making the metadata true inside the container.
This is what makes the PHP gate reproducible after any candidate change, not
just while the build tree happens to be fresh.

## Falsification

The 8 fixed ids pass individually and in the full suite; the set comparison
against the previous receipt shows `new = 0`. The first full run after the
change reported 107 raw ids; a second identical run reported 98, and the nine
extras were the known flaky `test_module_HTML*` / `test_xslt_html_output`
group that only appears when a stale `python3 test.py` process overlaps the
run. The second (isolated) run is the recorded one.
