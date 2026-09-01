import sys
import lxml.html as LH
print("import ok", flush=True)
t = LH.fromstring("<html><head><title>T</title></head><body><p>P</p></body></html>")
print("parse ok", flush=True)
print(LH.tostring(t, encoding="unicode"), flush=True)
