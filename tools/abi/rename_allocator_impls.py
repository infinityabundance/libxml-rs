#!/usr/bin/env python3
"""Mechanical rename for 11.1-L: the 5 allocator entry points became DATA
globals (xmlMalloc etc. are now function-pointer variables, upstream
xmlmemory.h ABI). Internal Rust code calls the backing implementations
xmlMallocImpl/xmlMallocAtomicImpl/xmlReallocImpl/xmlFreeImpl/xmlMemStrdupImpl.

Rewrites calls, imports and qualified paths crate-wide (allocator.rs itself
was hand-edited). Word-boundary rules keep xmlMallocZero/xmlMallocLoc/
xmlFreeDoc/xmlReallocZero/xmlMemStrdupLoc untouched."""
import re
import sys

NAMES = ["xmlMallocAtomic", "xmlMalloc", "xmlRealloc", "xmlMemStrdup", "xmlFree"]


def rewrite(text):
    for old in NAMES:
        new = old + "Impl"
        # Qualified paths: allocator::xmlMalloc -> allocator::xmlMallocImpl
        text = re.sub(rf"\ballocator::{old}\b", f"allocator::{new}", text)
        # Import names: use ...::xmlFree; and {xmlFree, ...} lists
        text = re.sub(rf"\b{old}(?=[,}};])", new, text)
        # Calls: xmlMalloc( -> xmlMallocImpl(
        text = re.sub(rf"\b{old}\(", f"{new}(", text)
    return text


def main():
    changed = 0
    for path in sys.argv[1:]:
        t = open(path).read()
        nt = rewrite(t)
        if nt != t:
            open(path, "w").write(nt)
            changed += 1
            print(f"rewrote {path}")
    print(f"{changed} files rewritten")


if __name__ == "__main__":
    main()
