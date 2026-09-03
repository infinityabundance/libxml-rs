import re

def cases(path):
    log = open(path, errors='replace').read()
    m = re.search(r'FAILED TEST SUMMARY(.*)', log, re.S)
    out = []
    for line in m.group(1).splitlines():
        mm = re.search(r'\[(ext/[a-z]+)/tests/', line)
        if mm:
            out.append(mm.group(1))
    return out

old = cases('/out/xpe-six12.log')
new = cases('/out/xpe-six13.log')
for k in ['dom', 'simplexml', 'xml', 'xmlreader', 'xmlwriter', 'xsl']:
    o = sum(1 for x in old if x == f'ext/{k}')
    n = sum(1 for x in new if x == f'ext/{k}')
    print(k, 'old', o, 'new', n)
print('TOTAL old', len(old), 'new', len(new))
