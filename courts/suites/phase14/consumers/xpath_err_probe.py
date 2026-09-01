import lxml.etree as ET

t = ET.fromstring(b"<r><a i='1'>x</a><b/></r>")

for expr in ["//a[", "unknown-fn()", "//n:c", "//a["]:
    try:
        ev = ET.XPath(expr)
        r = ev(t)
        print(expr, "->", r)
    except Exception as e:
        print(expr, "-> EXC:", type(e).__name__, str(e))
        log = getattr(e, "error_log", None)
        if log is not None:
            for entry in log:
                print("    entry:", entry.type, entry.domain, entry.level_name, repr(entry.message))
