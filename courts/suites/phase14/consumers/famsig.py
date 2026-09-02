import glob,os,re,collections,sys
base=sys.argv[1] if len(sys.argv)>1 else "/srcb/php-src/ext/dom/tests"
extn=sys.argv[2] if len(sys.argv)>2 else None
files=[]
for root,_,fs in os.walk(base):
    for f in fs:
        if f.endswith(".diff"): files.append(os.path.join(root,f))
grp=collections.Counter(); bysel={}
def sig(path):
    try: t=open(path,encoding="utf-8",errors="replace").read()
    except Exception: return "?"
    add=[]
    for x in t.splitlines():
        m=re.match(r"^\s*(\d+)\+\s?(.*)$", x)
        if m: add.append(m.group(2).strip())
    for l in add:
        if ("Warning" in l or "Fatal error" in l or "Notice" in l or "Deprecated" in l
            or l.startswith("<") or "Exception" in l or "Segmentation" in l or "recoverable" in l):
            return re.sub(r"0x[0-9a-fA-F]+","A",re.sub(r"[0-9]+","N",l))[:90]
    return ("|".join(x for x in add[:3]))[:90] if add else ""
for f in files:
    s=sig(f); grp[s]+=1; bysel.setdefault(s,[]).append(f)
print("total:",len(files),"groups:",len(grp))
for s,c in grp.most_common(30):
    ex=[os.path.basename(x) for x in bysel[s][:4]]
    print(f"{c:3d}  {s!r}  {ex}")
