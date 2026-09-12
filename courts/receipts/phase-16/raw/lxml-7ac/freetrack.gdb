set pagination off
set confirm off
python
import gdb
gdb.live = {}
gdb.armed = False
class MallocBP(gdb.Breakpoint):
    def stop(self):
        try:
            p = int(gdb.parse_and_eval("$rdi")) & 0xffffffffffffffff
        except Exception:
            return False
        if p:
            gdb.live[p] = 1
        return False
class FreeBP(gdb.Breakpoint):
    def stop(self):
        try:
            p = int(gdb.parse_and_eval("$rdi")) & 0xffffffffffffffff
        except Exception:
            return False
        if p:
            if gdb.live.get(p) == "freed":
                print("== DOUBLE FREE %#x ==" % p)
                gdb.execute("bt 25")
                gdb.execute("quit")
            gdb.live[p] = "freed"
        return False
m1 = MallocBP("malloc"); f1 = FreeBP("free")
for b in (m1, f1):
    b.enabled = False
class Arm(gdb.Breakpoint):
    def stop(self):
        for b in (m1, f1):
            b.enabled = True
        return False
a = Arm("php_request_shutdown")
end
run
