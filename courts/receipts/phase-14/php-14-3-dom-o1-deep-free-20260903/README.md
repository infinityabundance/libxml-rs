# dom O1 — deep-tree free recursion (GH-22570)

Closed 2026-09-03. Full suite **251 → 250**, zero regressions. dom 160 → 159:
`modern/xml/gh22570.phpt` PASS (100k-deep `Dom\XMLDocument`: saveXml +
innerHTML must raise the php stack-limit Error, and teardown must not segv).
Log: phpbuild-c:/out/o1b-six.log (1291-run, 250 failed / 40 skipped).

## Root cause (mirrors tree.c 2.15 xmlFreeNodeList)

The candidate's `free_node_list` (src/xml/tree/mod.rs) freed each node's
children by RECURSION. GH-22570 builds a 100k-deep element chain; the php
side caught its own "Maximum call stack size reached" Error during the
php-serializer recursion, but at shutdown the engine freed the document with
one C frame per level — the Rust frames (~100+ bytes each) overflowed the
thread stack before reaching the leaves (segv in `free_node_list` after
`done` printed). Upstream `xmlFreeNodeList` walks the tree with an explicit
`depth` counter and a `while(1)` descend/resume loop — no recursion at all.

## Fix

`free_node_list` rewritten as an iterative post-order walk:

- an explicit resume stack of `(node, next_sibling)` frames — `next` is read
  BEFORE the descent because the node struct is freed on resume;
- per-node behavior unchanged: DTD members are unlinked-but-not-freed,
  `XML_ENTITY_REF_NODE` children are never descended into, `free_node` runs
  children-subtree-first exactly as before;
- free order (children before parents) unchanged.

## Guard

tree/mod.rs `test_free_deeply_nested_chain_is_iterative`: build a 500k-deep
`a`-chain through the tree API and free it via `free_doc`. A recursion
regression dies in the test thread (whose stack is far below 500k frames);
the iterative walk completes in milliseconds.

Probe: consumers/gh22570-repro.php (php pin; green now).
