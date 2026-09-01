#!/bin/bash
# nokogiri-xpath-pywalk.sh — at the SEGV, use gdb Python to walk the context
# element's properties chain and classify each attribute (live/freed/reused).
set -uo pipefail
MODE="${1:?usage: nokogiri-xpath-pywalk.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/walk.py <<'PYEOF'
import gdb
class WalkCmd(gdb.Command):
    def __init__(self):
        super().__init__("walkprops", gdb.COMMAND_USER)
    def invoke(self, arg, from_tty):
        frame = gdb.selected_frame()
        node_val = frame.read_var("node")
        node = int(node_val)
        elem = node
        m = gdb.selected_inferior()
        # field offsets for _xmlNode (x86_64):
        off_name = 16
        off_type = 8
        off_next = 48
        off_parent = 40
        off_props = 88  # properties is a field after content? compute below
        # properties offset: _xmlNode fields... use 88 from earlier ABI check
        # Read element header
        try:
            p = m.read_memory(elem, 16).tobytes()
        except Exception as e:
            print("elem read fail:", e); return
        print("ELEM %#x" % elem)
        # properties pointer is at offset 88 per _xmlNode layout
        elem_type = int.from_bytes(m.read_memory(elem + 8, 8).tobytes(), "little") & 0xffffffff
        elem_name = int.from_bytes(m.read_memory(elem + 16, 8).tobytes(), "little")
        elem_child = int.from_bytes(m.read_memory(elem + 24, 8).tobytes(), "little")
        elem_doc = int.from_bytes(m.read_memory(elem + 64, 8).tobytes(), "little")
        print("ELEM %#x type=%d name=%#x children=%#x doc=%#x" % (elem, elem_type, elem_name, elem_child, elem_doc))
        try:
            print("  elem namebytes=%s" % (m.read_memory(elem_name, 24).tobytes().split(b"\0")[0]))
        except Exception:
            print("  elem name unreadable")
        props = int.from_bytes(m.read_memory(elem + 88, 8).tobytes(), "little")
        print("properties head -> %#x" % props)
        p = props
        idx = 0
        while p and p % 8 == 0 and p < (1<<48):
            try:
                name = int.from_bytes(m.read_memory(p + off_name, 8).tobytes(), "little")
                typ = int.from_bytes(m.read_memory(p + off_type, 8).tobytes(), "little") & 0xffffffff
                nxt = int.from_bytes(m.read_memory(p + off_next, 8).tobytes(), "little")
                par = int.from_bytes(m.read_memory(p + off_parent, 8).tobytes(), "little")
                try:
                    s = m.read_memory(name, 16).tobytes()
                    strval = s.split(b"\0")[0][:32]
                except Exception as e:
                    strval = b"<unreadable: %s>" % str(e).encode()
                print("  #[%d] attr=%#x type=%d name=%#x parent=%#x next=%#x namebytes=%s"
                      % (idx, p, typ, name, par, nxt, strval))
            except Exception as e:
                print("  #[%d] attr=%#x read FAIL: %s" % (idx, p, e)); break
            idx += 1
            p = nxt
            if idx > 40: break
WalkCmd()
PYEOF

timeout 300 gdb -batch \
  -ex 'source /out/walk.py' \
  -ex 'run' \
  -ex 'frame 5' \
  -ex 'walkprops' \
  --args ruby3.1 -rset -Ilib:test:.:test -e 'require "minitest/autorun"; require "test/xml/test_dtd.rb"; require "test/xml/test_document.rb"; require "test/test_nokogiri.rb"' -- --seed 14472 \
  > "/out/${MODE}-pywalk.log" 2>&1
echo "pywalk ${MODE} done"
