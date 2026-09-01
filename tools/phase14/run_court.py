#!/usr/bin/env python3
"""Phase 14 — host-side downstream custodian court orchestration.

Runs the pinned consumers (lxml, nokogiri, php) against the canonical oracle
and against the candidate inside minimal per-distro Docker images, then
byte-compares the observable outputs and classifies every divergence.

Per (distro, consumer) run:
  1. build the distro image once:  libxml-rs/phase14-<distro>:1
  2. docker run <image> /court/consumers/<consumer>-run.sh oracle
  3. docker run <image> -v <repo>/target/debug:/candidate:ro
                           /court/consumers/<consumer>-run.sh candidate
  4. compare outputs, classify divergences, write receipt.

Receipts: courts/receipts/phase-14/<distro>-<consumer>-<ts>.json
"""
import argparse
import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-14")
DOCKER_DIR = os.path.join(ROOT, "courts", "suites", "phase14", "docker")
CONSUMERS = os.path.join(ROOT, "courts", "suites", "phase14", "consumers")
CANDIDATE = os.path.join(ROOT, "target", "debug")

DISTROS = {
    "debian": ("debian:bookworm", "Dockerfile.debian"),
    "ubuntu": ("ubuntu:26.04", "Dockerfile.ubuntu"),
    "arch": ("archlinux:latest", "Dockerfile.arch"),
    "opensuse": ("opensuse/leap:16.0", "Dockerfile.opensuse"),
    "almalinux": ("almalinux:10.2", "Dockerfile.almalinux"),
}
CONSUMERS_LIST = ("lxml", "nokogiri", "php")

# Known residuals whose fingerprints are EXPECTED to differ on the executed
# platform. A differing corpus op whose name starts with one of these is
# classified KNOWN_R000157 (the iconv/ICU-only encoding surface) instead of
# being counted as a new divergence.
KNOWN_RESIDUAL_PREFIXES = ("enc-",)


def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    return r.returncode, r.stdout, r.stderr


def sha(path):
    if not os.path.exists(path):
        return None
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def build_image(distro):
    tag = f"libxml-rs/phase14-{distro}:1"
    rc, _, _ = run(["docker", "image", "inspect", tag])
    if rc == 0:
        return tag, True
    _base, dockerfile = DISTROS[distro]
    print(f"[{distro}] building {tag} ...")
    rc, out, err = run(["docker", "build", "-t", tag,
                        "-f", os.path.join(DOCKER_DIR, dockerfile), ROOT],
                       timeout=5400)
    if rc != 0:
        print(f"[{distro}] IMAGE BUILD FAILED:\n{(out + err)[-3000:]}")
        sys.exit(1)
    return tag, False


def collect_out(out_dir, mode):
    files = {}
    for fn in os.listdir(out_dir):
        if fn.startswith(f"{mode}-"):
            p = os.path.join(out_dir, fn)
            if os.path.isfile(p):
                files[fn] = open(p, "rb").read()
    return files


def classify_lxml_corpus_diff(diff_lines):
    known, new = [], []
    for name in diff_lines:
        if name.startswith(KNOWN_RESIDUAL_PREFIXES):
            known.append(name)
        else:
            new.append(name)
    return known, new


def compare(distro, consumer, out_dir):
    o = collect_out(out_dir, "oracle")
    c = collect_out(out_dir, "candidate")
    keys = sorted(set(o) | set(c))
    diff_files = [k for k in keys if o.get(k) != c.get(k)]
    verdict = "PASS" if not diff_files else "FAIL"
    detail = {}
    if consumer == "lxml":
        detail["corpus_diff_known_r157"] = []
        detail["corpus_diff_new"] = []
        if "oracle-corpus.txt" in o and "candidate-corpus.txt" in c:
            ol = o["oracle-corpus.txt"].decode().splitlines()
            cl = c["candidate-corpus.txt"].decode().splitlines()
            names = []
            for a, b in zip(ol, cl):
                if a != b:
                    names.append(a.split(":", 1)[0] if ":" in a else a)
            detail["corpus_diff_count"] = len(names)
            detail["corpus_diff_known_r157"], detail["corpus_diff_new"] = \
                classify_lxml_corpus_diff(names)
    for k in ("build.log", "libver.txt", "tests.log", "ldd.txt"):
        ok, ck = f"oracle-{k}", f"candidate-{k}"
        if ok in o and ck in c and o[ok] != c[ck]:
            detail[f"{k}_differs"] = True
    return verdict, diff_files, detail


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--distros", default=",".join(DISTROS))
    ap.add_argument("--consumers", default=",".join(CONSUMERS_LIST))
    ap.add_argument("--build-only", action="store_true")
    args = ap.parse_args()

    distros = [d for d in args.distros.split(",") if d]
    consumers = [c for c in args.consumers.split(",") if c]
    os.makedirs(RECEIPTS, exist_ok=True)

    for distro in distros:
        tag, existed = build_image(distro)
        print(f"[{distro}] image {tag} {'(cached)' if existed else '(built)'}")
        if args.build_only:
            continue
        for consumer in consumers:
            script = f"{consumer}-run.sh"
            if not os.path.exists(os.path.join(CONSUMERS, script)):
                print(f"[{distro}/{consumer}] no runner {script}")
                continue
            print(f"[{distro}/{consumer}] oracle run ...")
            out_dir = os.path.join(RECEIPTS, f"{distro}-{consumer}-out")
            os.makedirs(out_dir, exist_ok=True)
            rc, _, _ = run(["docker", "run", "--rm",
                            "-v", f"{out_dir}:/out",
                            tag, "/court/consumers/" + script, "oracle"],
                           timeout=10800)
            print(f"[{distro}/{consumer}] oracle rc={rc}")
            print(f"[{distro}/{consumer}] candidate run ...")
            rc2, _, _ = run(["docker", "run", "--rm",
                             "-v", f"{out_dir}:/out",
                             "-v", f"{CANDIDATE}:/candidate:ro",
                             tag, "/court/consumers/" + script, "candidate"],
                            timeout=10800)
            print(f"[{distro}/{consumer}] candidate rc={rc2}")

            verdict, diff_files, detail = compare(distro, consumer, out_dir)
            ts = datetime.datetime.now(datetime.timezone.utc).strftime(
                "%Y%m%dT%H%M%SZ")
            receipt = os.path.join(RECEIPTS, f"{distro}-{consumer}-{ts}.json")
            with open(receipt, "w") as f:
                json.dump({
                    "court": f"DOWNSTREAM-{consumer.upper()}",
                    "phase": "14",
                    "timestamp": ts,
                    "distro": distro,
                    "distro_base": DISTROS[distro][0],
                    "image": tag,
                    "consumer": consumer,
                    "verdict": verdict,
                    "diff_files": diff_files,
                    "detail": detail,
                    "oracle_rc": rc,
                    "candidate_rc": rc2,
                    "oracle": {k: sha(os.path.join(out_dir, k))
                               for k in sorted(os.listdir(out_dir))
                               if k.startswith("oracle-")},
                    "candidate": {k: sha(os.path.join(out_dir, k))
                                  for k in sorted(os.listdir(out_dir))
                                  if k.startswith("candidate-")},
                }, f, indent=1)
            print(f"[{distro}/{consumer}] verdict={verdict} "
                  f"diff_files={diff_files}")
            for k, v in detail.items():
                print(f"    {k}: {v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
