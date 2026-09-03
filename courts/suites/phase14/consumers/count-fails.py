import re
log = open('/out/xpe-six13.log', errors='replace').read()
m = re.search(r'FAILED TEST SUMMARY(.*)', log, re.S)
for line in m.group(1).splitlines():
    if '[ext/xsl/tests/' in line:
        print(line.strip())
