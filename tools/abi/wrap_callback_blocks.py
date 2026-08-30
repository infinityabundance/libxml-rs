#!/usr/bin/env python3
"""Wrap every '[11.1-L] begin ... end' appended block in an extern-C guard so
C++ consumers see consistent linkage (the blocks were appended after the
header's own extern "C" region)."""
import glob

for path in glob.glob("include/libxml/*.h") + glob.glob("include/libxslt/*.h"):
    text = open(path).read()
    if "[11.1-L] begin" not in text:
        continue
    out = []
    i = 0
    while True:
        b = text.find("/* [11.1-L] begin", i)
        if b == -1:
            out.append(text[i:])
            break
        e = text.find("/* [11.1-L] end", b)
        assert e != -1, path
        e = text.find("*/", e) + 2
        out.append(text[i:b])
        out.append("#ifdef __cplusplus\nextern \"C\" {\n#endif\n")
        out.append(text[b:e])
        out.append("\n#ifdef __cplusplus\n}\n#endif\n")
        i = e
    new = "".join(out)
    if new != text:
        open(path, "w").write(new)
        print(f"wrapped {path}")
