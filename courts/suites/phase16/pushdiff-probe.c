/* pushdiff-probe.c — §16.7.8 push-parser differential driver.
 *
 * Feeds documents to xmlCreatePushParserCtxt + xmlParseChunk under a
 * chosen chunking plan and prints ONE deterministic, merged trace:
 * per-call return codes / errNo / wellFormed / instate / logical cursor
 * (input offset, line, col, inputNr, nameNr), interleaved with every SAX
 * event and structured error record in handler-invocation order. Every
 * record is fflush'ed immediately and goes through a single FILE*, so the
 * oracle and the candidate produce byte-comparable streams from the same
 * driver binary.
 *
 * This is the ground-truth gate for the incremental push parser (16.7.8):
 * oracle == candidate, per call, per chunking. Compiled with
 * -Wall -Wextra -Werror against each provider's own headers (exact
 * callback prototypes, no ABI drift).
 *
 * Lifecycle surfaces covered (slice 0.2):
 *   - constructor-initial chunk  (C prefix: first split goes to
 *     xmlCreatePushParserCtxt; parsing continues via xmlParseChunk)
 *   - xmlCtxtResetPush           (R prefix: two-document cells — parse A,
 *     then reset-empty (Rz, feed B under the plan) or reset-with-whole-B
 *     (Ri, B is the reset chunk and only FINAL follows); stale state must
 *     not leak across the reset)
 *   - zero-length non-final calls (zK suffix: K xmlParseChunk(NULL,0,0)
 *     injected after every real chunk — must not finish/emit/reset)
 *   - xmlStopParser              (-S K: stop after the K-th start element;
 *     later chunks must be refused with the recorded error)
 *   - cursor trace on every call (input cur-base / line / col / inputNr /
 *     nameNr) — the parser's logical position must match after EVERY
 *     chunk, not merely the emitted events.
 *
 * Exact recorders: SAX2 attributes print all five components (localname,
 * prefix, URI, value with the (value, end) length convention, end-present
 * flag); DTD xmlElementContent trees serialize canonically (type, ocur,
 * name, prefix, c1, c2 — recursive) and xmlEnumeration chains as
 * one|two|three; text/cdata/comment/PI bytes are escaped (\n \r \t \\ \xNN).
 *
 * Usage:
 *   pushdiff-probe [-s sax1|sax2] [-S stopN] [-W] <chunkmode> <file> [file ...]
 *   chunkmode:  [C][R[z|i]] bN | rSEED[-span] [zK] [i]
 *     bN      fixed N-byte chunks (last chunk = remainder)
 *     rS      random splits, seed S (split sizes 1..64 bytes)
 *     rS-span random splits, seed S, split sizes 1..span bytes
 *     C       pass the first split to xmlCreatePushParserCtxt instead of
 *             the first xmlParseChunk
 *     zK      inject K zero-length non-final calls after each real chunk
 *     i       inline final: the LAST real chunk carries terminate=1, and the
 *             separate empty terminating call is skipped
 *     Rz      reset mode: two files per cell; after A finishes,
 *             xmlCtxtResetPush(NULL, 0) then parse B under the plan
 *     Ri      reset mode: after A finishes, xmlCtxtResetPush(whole B),
 *             then only the terminating call
 *     Rs      reset mode, SUSPENDED A: A is fed under the plan but never
 *             finished (it ends mid-construct — partial lexical token /
 *             pending UTF-8 / pending CR / open element stack / DTD decl),
 *             then xmlCtxtResetPush(NULL, 0) and B is parsed under the
 *             plan: the reset must clear the suspended machinery
 *     Rsi     reset mode, SUSPENDED A, then xmlCtxtResetPush(whole B)
 *
 * Every plan ends with one terminating call (empty when the last chunk
 * ended exactly at the end of the document), then one REFEED call on the
 * finished context.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

/* ctxt->instate/nameNr are XML_DEPRECATED_MEMBER in the 2.15 headers;
 * reading them is intentional (observables of the incremental machine). */
#pragma GCC diagnostic ignored "-Wdeprecated-declarations"

static FILE *TR;
static xmlParserCtxtPtr CUR; /* the live context (xmlStopParser target) */
static int STOP_AFTER = 0;   /* 0 = never stop; else stop at the K-th start */
static int STARTS = 0;       /* start-element counter, reset per document */
/* Shadow-only: additionally emit the PHYSICAL input window (`consumed` and the
 * reconstructed absolute position). Off by default so the frozen 4702-cell
 * trace format is untouched. The oracle-shadow court runs with it on, so the
 * identity `consumed + (cur - base) == absolute` is measured rather than
 * inferred (it is exactly what xmlCtxtGetInputPosition reconstructs). */
static int WINDOW_MODE = 0;

static void esc_bytes(const xmlChar *s, int len) {
    int i;
    for (i = 0; i < len; i++) {
        unsigned char c = s[i];
        if (c == '\\') fprintf(TR, "\\\\");
        else if (c == '\n') fprintf(TR, "\\n");
        else if (c == '\r') fprintf(TR, "\\r");
        else if (c == '\t') fprintf(TR, "\\t");
        else if (c >= 32 && c < 127) fputc(c, TR);
        else fprintf(TR, "\\x%02x", c);
    }
}

static void esc_str(const xmlChar *s) {
    esc_bytes(s, s ? (int)xmlStrlen(s) : 0);
}

/* ── SAX lifecycle ─────────────────────────────────────────────────── */

static void rec_sd(void *ctx) { (void)ctx; fprintf(TR, "startDocument\n"); fflush(TR); }
static void rec_ed(void *ctx) { (void)ctx; fprintf(TR, "endDocument\n"); fflush(TR); }

static void maybe_stop(void) {
    if (STOP_AFTER > 0 && ++STARTS == STOP_AFTER) {
        fprintf(TR, "! xmlStopParser\n");
        fflush(TR);
        xmlStopParser(CUR);
    }
}

/* ── SAX2 elements ─────────────────────────────────────────────────── */

static void rec_s2(void *ctx, const xmlChar *local, const xmlChar *pref,
                   const xmlChar *URI, int nb_ns, const xmlChar **ns,
                   int nb_att, int nb_def, const xmlChar **atts) {
    int i;
    (void)ctx;
    maybe_stop();
    fprintf(TR, "startElementNs local=[");
    esc_bytes(local, local ? (int)xmlStrlen(local) : 0);
    fprintf(TR, "] prefix=");
    esc_str(pref);
    fprintf(TR, " uri=");
    esc_str(URI);
    fprintf(TR, " ns=%d att=%d def=%d", nb_ns, nb_att, nb_def);
    for (i = 0; i < nb_ns * 2; i += 2) {
        fprintf(TR, " {");
        esc_str(ns[i]);
        fprintf(TR, "=");
        esc_str(ns[i + 1]);
        fprintf(TR, "}");
    }
    for (i = 0; i < nb_att * 5; i += 5) {
        const xmlChar *v = atts[i + 3];
        size_t vlen = v ? (atts[i + 4] ? (size_t)(atts[i + 4] - v)
                                       : (size_t)xmlStrlen(v))
                        : 0;
        fprintf(TR, " [a local=");
        esc_str(atts[i]);
        fprintf(TR, " prefix=");
        esc_str(atts[i + 1]);
        fprintf(TR, " uri=");
        esc_str(atts[i + 2]);
        fprintf(TR, " value=");
        esc_bytes(v, (int)vlen);
        fprintf(TR, " end=%d]", atts[i + 4] ? 1 : 0);
    }
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_e2(void *ctx, const xmlChar *local, const xmlChar *pref,
                   const xmlChar *URI) {
    (void)ctx;
    fprintf(TR, "endElementNs local=[");
    esc_bytes(local, local ? (int)xmlStrlen(local) : 0);
    fprintf(TR, "] prefix=");
    esc_str(pref);
    fprintf(TR, " uri=");
    esc_str(URI);
    fprintf(TR, "\n");
    fflush(TR);
}

/* ── SAX1 elements ─────────────────────────────────────────────────── */

static void rec_s1(void *ctx, const xmlChar *name, const xmlChar **atts) {
    int i;
    (void)ctx;
    maybe_stop();
    fprintf(TR, "startElement [");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, "]");
    if (atts) for (i = 0; atts[i] != NULL; i += 2) {
        fprintf(TR, " [");
        esc_bytes(atts[i], (int)xmlStrlen(atts[i]));
        fprintf(TR, "]=");
        esc_str(atts[i + 1]);
    }
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_e1(void *ctx, const xmlChar *name) {
    (void)ctx;
    fprintf(TR, "endElement [");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, "]\n");
    fflush(TR);
}

/* ── content ───────────────────────────────────────────────────────── */

static void rec_ch(void *ctx, const xmlChar *ch, int len) {
    (void)ctx;
    fprintf(TR, "characters len=%d [", len);
    esc_bytes(ch, len);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_iw(void *ctx, const xmlChar *ch, int len) {
    (void)ctx;
    fprintf(TR, "ignorableWhitespace len=%d [", len);
    esc_bytes(ch, len);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_co(void *ctx, const xmlChar *value) {
    (void)ctx;
    fprintf(TR, "comment [");
    esc_str(value);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_cd(void *ctx, const xmlChar *value, int len) {
    (void)ctx;
    fprintf(TR, "cdata len=%d [", len);
    esc_bytes(value, len);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_pi(void *ctx, const xmlChar *target, const xmlChar *data) {
    (void)ctx;
    fprintf(TR, "pi target=");
    esc_str(target);
    fprintf(TR, " data=");
    esc_str(data);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_ref(void *ctx, const xmlChar *name) {
    (void)ctx;
    fprintf(TR, "reference [");
    esc_str(name);
    fprintf(TR, "]\n");
    fflush(TR);
}

/* ── DTD declarations ──────────────────────────────────────────────── */

static void rec_intsubset(void *ctx, const xmlChar *name, const xmlChar *ExternalID,
                          const xmlChar *SystemID) {
    (void)ctx;
    fprintf(TR, "internalSubset name=");
    esc_str(name);
    fprintf(TR, " ext=");
    esc_str(ExternalID);
    fprintf(TR, " sys=");
    esc_str(SystemID);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_extsubset(void *ctx, const xmlChar *name, const xmlChar *ExternalID,
                          const xmlChar *SystemID) {
    (void)ctx;
    fprintf(TR, "externalSubset name=");
    esc_str(name);
    fprintf(TR, " ext=");
    esc_str(ExternalID);
    fprintf(TR, " sys=");
    esc_str(SystemID);
    fprintf(TR, "\n");
    fflush(TR);
}

/* Canonical xmlElementContent serializer: never treats the content tree
 * as a C string. Every node prints as {t=<type> o=<ocur> <body>} where
 * body is empty for PCDATA, "name=… prefix=…" for ELEMENT, and
 * "a=<sub> b=<sub>" for SEQ/OR — fully recursive and exact. */
static void dump_elem_content(xmlElementContentPtr c) {
    if (c == NULL) { fprintf(TR, "nil"); return; }
    fprintf(TR, "{t=%d o=%d ", (int)c->type, (int)c->ocur);
    switch (c->type) {
        case XML_ELEMENT_CONTENT_PCDATA:
            fprintf(TR, "#PCDATA");
            break;
        case XML_ELEMENT_CONTENT_ELEMENT:
            fprintf(TR, "name=");
            esc_str(c->name);
            fprintf(TR, " prefix=");
            esc_str(c->prefix);
            break;
        case XML_ELEMENT_CONTENT_SEQ:
        case XML_ELEMENT_CONTENT_OR:
            fprintf(TR, "a=");
            dump_elem_content(c->c1);
            fprintf(TR, " b=");
            dump_elem_content(c->c2);
            break;
        default:
            fprintf(TR, "type%d", (int)c->type);
            break;
    }
    fprintf(TR, "}");
}

static void rec_entdecl(void *ctx, const xmlChar *name, int type,
                        const xmlChar *publicId, const xmlChar *systemId,
                        xmlChar *content) {
    (void)ctx;
    fprintf(TR, "entityDecl name=");
    esc_str(name);
    fprintf(TR, " type=%d pub=", type);
    esc_str(publicId);
    fprintf(TR, " sys=");
    esc_str(systemId);
    fprintf(TR, " content=");
    esc_str(content);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_attrdecl(void *ctx, const xmlChar *elem, const xmlChar *fullname,
                         int type, int def, const xmlChar *defaultValue,
                         xmlEnumerationPtr tree) {
    xmlEnumerationPtr e;
    (void)ctx;
    fprintf(TR, "attributeDecl elem=");
    esc_str(elem);
    fprintf(TR, " name=");
    esc_str(fullname);
    fprintf(TR, " type=%d def=%d val=", type, def);
    esc_str(defaultValue);
    fprintf(TR, " enum=");
    for (e = tree; e != NULL; e = e->next) {
        if (e != tree) fprintf(TR, "|");
        esc_str(e->name);
    }
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_elemsdecl(void *ctx, const xmlChar *name, int type,
                          xmlElementContentPtr content) {
    (void)ctx;
    fprintf(TR, "elementDecl name=");
    esc_str(name);
    fprintf(TR, " type=%d content=", type);
    dump_elem_content(content);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_notationdecl(void *ctx, const xmlChar *name,
                             const xmlChar *publicId, const xmlChar *systemId) {
    (void)ctx;
    fprintf(TR, "notationDecl name=");
    esc_str(name);
    fprintf(TR, " pub=");
    esc_str(publicId);
    fprintf(TR, " sys=");
    esc_str(systemId);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_unparsed(void *ctx, const xmlChar *name, const xmlChar *publicId,
                         const xmlChar *systemId, const xmlChar *notationName) {
    (void)ctx;
    fprintf(TR, "unparsedEntityDecl name=");
    esc_str(name);
    fprintf(TR, " pub=");
    esc_str(publicId);
    fprintf(TR, " sys=");
    esc_str(systemId);
    fprintf(TR, " nota=");
    esc_str(notationName);
    fprintf(TR, "\n");
    fflush(TR);
}

/* ── structured error record (exact fields, single line) ───────────── */

static void rec_err(void *ctx, const xmlError *err) {
    (void)ctx;
    if (err == NULL) { fprintf(TR, "error(NULL)\n"); fflush(TR); return; }
    fprintf(TR, "error dom=%d code=%d level=%d file=", err->domain, err->code, err->level);
    esc_bytes((const xmlChar *)(err->file ? err->file : ""),
              err->file ? (int)strlen(err->file) : 0);
    fprintf(TR, " line=%d i1=%d i2=%d str1=", err->line, err->int1, err->int2);
    esc_bytes((const xmlChar *)(err->str1 ? err->str1 : ""),
              err->str1 ? (int)strlen((char *)err->str1) : 0);
    fprintf(TR, " str2=");
    esc_bytes((const xmlChar *)(err->str2 ? err->str2 : ""),
              err->str2 ? (int)strlen((char *)err->str2) : 0);
    fprintf(TR, " str3=");
    esc_bytes((const xmlChar *)(err->str3 ? err->str3 : ""),
              err->str3 ? (int)strlen((char *)err->str3) : 0);
    fprintf(TR, " msg=");
    esc_bytes((const xmlChar *)(err->message ? err->message : ""),
              err->message ? (int)strlen(err->message) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

/* ── handler tables (every push-observable slot) ───────────────────── */

static void sax1_table(xmlSAXHandler *h) {
    memset(h, 0, sizeof(*h));
    h->startDocument = rec_sd;
    h->endDocument = rec_ed;
    h->startElement = rec_s1;
    h->endElement = rec_e1;
    h->characters = rec_ch;
    h->ignorableWhitespace = rec_iw;
    h->comment = rec_co;
    h->cdataBlock = rec_cd;
    h->processingInstruction = rec_pi;
    h->reference = rec_ref;
    h->internalSubset = rec_intsubset;
    h->externalSubset = rec_extsubset;
    h->entityDecl = rec_entdecl;
    h->attributeDecl = rec_attrdecl;
    h->elementDecl = rec_elemsdecl;
    h->notationDecl = rec_notationdecl;
    h->unparsedEntityDecl = rec_unparsed;
}

static void sax2_table(xmlSAXHandler *h) {
    memset(h, 0, sizeof(*h));
    h->initialized = XML_SAX2_MAGIC;
    h->startDocument = rec_sd;
    h->endDocument = rec_ed;
    h->startElementNs = rec_s2;
    h->endElementNs = rec_e2;
    h->characters = rec_ch;
    h->ignorableWhitespace = rec_iw;
    h->comment = rec_co;
    h->cdataBlock = rec_cd;
    h->processingInstruction = rec_pi;
    h->reference = rec_ref;
    h->internalSubset = rec_intsubset;
    h->externalSubset = rec_extsubset;
    h->entityDecl = rec_entdecl;
    h->attributeDecl = rec_attrdecl;
    h->elementDecl = rec_elemsdecl;
    h->notationDecl = rec_notationdecl;
    h->unparsedEntityDecl = rec_unparsed;
}

/* ── deterministic PRNG (seeded once per document+plan) ────────────── */

static unsigned long rng_state;
static unsigned int rnd(unsigned int m) {
    rng_state = rng_state * 6364136223846793005UL + 1442695040888963407UL;
    return (unsigned int)((rng_state >> 33) % m);
}

/* ── plan ──────────────────────────────────────────────────────────── */

enum PlanKind { PLAN_FIXED, PLAN_RANDOM };

struct Plan {
    enum PlanKind kind;
    size_t fixed_n;
    unsigned long seed;
    unsigned int span;
    int ctor_init;    /* first split goes to xmlCreatePushParserCtxt */
    int zero_after;   /* K zero-length non-final calls after each chunk */
    int inline_final; /* the LAST real chunk carries terminate=1 */
    int reset_kind;   /* 0 none, 1 Rz, 2 Ri, 3 Rs, 4 Rsi */
};

static int parse_plan(const char *mode, struct Plan *p) {
    const char *m = mode;
    memset(p, 0, sizeof(*p));
    p->kind = PLAN_FIXED;
    p->fixed_n = 1;
    p->seed = 1;
    p->span = 64;
    if (*m == 'C') { p->ctor_init = 1; m++; }
    if (*m == 'R') {
        m++;
        if (*m == 'z') { p->reset_kind = 1; m++; }
        else if (*m == 'i') { p->reset_kind = 2; m++; }
        else if (*m == 's') {
            m++;
            if (*m == 'i') { p->reset_kind = 4; m++; }
            else p->reset_kind = 3;
        }
        else return -1;
    }
    if (*m == 'b') {
        p->kind = PLAN_FIXED;
        p->fixed_n = (size_t)strtoul(m + 1, NULL, 10);
        if (p->fixed_n == 0) p->fixed_n = 1;
        while (*m && *m != 'z' && *m != 'i') m++;
    } else if (*m == 'r') {
        char *endp;
        p->kind = PLAN_RANDOM;
        p->seed = strtoul(m + 1, &endp, 10);
        if (*endp == '-') p->span = (unsigned int)strtoul(endp + 1, NULL, 10);
        if (p->seed == 0) p->seed = 1;
        if (p->span == 0) p->span = 1;
        while (*m && *m != 'z' && *m != 'i') m++;
    } else {
        return -1;
    }
    while (*m == 'z' || *m == 'i') {
        if (*m == 'z') {
            p->zero_after = (int)strtoul(m + 1, NULL, 10);
            if (p->zero_after < 0) p->zero_after = 0;
            while (*m && *m != 'i') m++;
        } else {
            p->inline_final = 1;
            m++;
        }
    }
    return 0;
}

static size_t next_split(struct Plan *p, size_t remaining) {
    size_t n;
    if (p->kind == PLAN_FIXED) {
        n = remaining < p->fixed_n ? remaining : p->fixed_n;
    } else {
        n = 1 + rnd(p->span);
        if (n > remaining) n = remaining;
    }
    return n;
}

/* ── per-call tail: rc + errNo + wellFormed + instate + logical cursor ─ */

static void tail(const char *lbl, int idx, int rc, xmlParserCtxtPtr c) {
    xmlParserInputPtr in = c->input;
    fprintf(TR, "< %s %d rc=%d err=%d wf=%d in=%d",
            lbl, idx, rc, c->errNo, c->wellFormed, c->instate);
    if (in != NULL) {
        fprintf(TR, " p=%ld l=%d col=%d i=%d n=%d",
                (long)(in->cur - in->base), in->line, in->col,
                c->inputNr, c->nameNr);
        if (WINDOW_MODE) {
            unsigned long cons = in->consumed;
            fprintf(TR, " c=%lu abs=%lu", cons,
                    cons + (unsigned long)(in->cur - in->base));
        }
    } else {
        fprintf(TR, " p=-1 l=-1 col=-1 i=-1 n=%d", c->nameNr);
    }
    fprintf(TR, "\n");
    fflush(TR);
}

static int read_file(const char *path, unsigned char **out, size_t *outlen) {
    FILE *f = fopen(path, "rb");
    unsigned char *buf;
    long sz;
    if (!f) return -1;
    fseek(f, 0, SEEK_END);
    sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (sz < 0) { fclose(f); return -1; }
    buf = malloc(sz > 0 ? (size_t)sz : 1);
    if (!buf) { fclose(f); return -1; }
    if (sz > 0 && fread(buf, 1, (size_t)sz, f) != (size_t)sz) {
        free(buf); fclose(f); return -1;
    }
    fclose(f);
    *out = buf;
    *outlen = (size_t)sz;
    return 0;
}

/* Feed one document (or a range of it) under the plan through
 * xmlParseChunk, injecting zero-length calls per the plan. Returns the
 * number of real chunks fed (for the caller's labels). */
static int feed_doc(struct Plan *p, xmlParserCtxtPtr c,
                    const unsigned char *doc, size_t dlen, size_t start,
                    const char *lbl) {
    size_t off = start;
    int call = 0;
    int z;
    while (off < dlen) {
        size_t n = next_split(p, dlen - off);
        int term = (p->inline_final && off + n >= dlen) ? 1 : 0;
        fprintf(TR, "> %s %d len=%zu\n", lbl, call, n);
        fflush(TR);
        {
            int rc = xmlParseChunk(c, (const char *)doc + off, (int)n, term);
            tail(lbl, call, rc, c);
        }
        off += n;
        call++;
        for (z = 0; z < p->zero_after; z++) {
            fprintf(TR, "> %s %d ZERO\n", lbl, call);
            fflush(TR);
            {
                int rc = xmlParseChunk(c, NULL, 0, 0);
                tail(lbl, call, rc, c);
            }
        }
    }
    return call;
}

int main(int argc, char **argv) {
    int sax1 = 0;
    int iarg = 1;
    struct Plan plan;
    xmlSAXHandler h;
    int fi;

    TR = stdout;
    if (argc < 3) return 1;
    if (strcmp(argv[1], "-s") == 0) {
        if (argc < 5) return 1;
        if (strcmp(argv[2], "sax1") == 0) sax1 = 1;
        iarg = 3;
    }
    if (strcmp(argv[iarg], "-S") == 0) {
        if (argc < iarg + 3) return 1;
        STOP_AFTER = atoi(argv[iarg + 1]);
        iarg += 2;
    }
    if (strcmp(argv[iarg], "-W") == 0) {
        WINDOW_MODE = 1;
        iarg += 1;
    }
    if (iarg >= argc) return 1;
    if (parse_plan(argv[iarg], &plan) != 0) {
        fprintf(TR, "bad-mode %s\n", argv[iarg]);
        return 1;
    }
    iarg++;

    if (sax1) sax1_table(&h); else sax2_table(&h);

    if (plan.reset_kind) {
        /* Two files per cell. A is parsed under the plan and finished;
         * then the context is reset (empty, or with whole B) and B is
         * parsed; stale A state must not leak into B. */
        int nfiles = argc - iarg;
        if (nfiles < 2 || (nfiles % 2) != 0) return 1;
        for (fi = iarg; fi + 1 < argc; fi += 2) {
            unsigned char *da, *db;
            size_t la, lb;
            xmlParserCtxtPtr c;
            if (read_file(argv[fi], &da, &la) != 0 ||
                read_file(argv[fi + 1], &db, &lb) != 0) {
                fprintf(TR, "== %s unreadable\n", argv[fi]);
                fflush(TR);
                continue;
            }
            fprintf(TR, "== RESET %s(%zu) -> %s(%zu) mode=%s\n",
                    argv[fi], la, argv[fi + 1], lb, argv[iarg - 1]);
            fflush(TR);
            c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
            if (!c) { fprintf(TR, "no-ctxt\n"); fflush(TR); continue; }
            xmlCtxtSetErrorHandler(c, rec_err, NULL);
            CUR = c;
            STARTS = 0;
            rng_state = plan.seed;
            /* Parse document A under the plan. */
            feed_doc(&plan, c, da, la, 0, "A");
            if (plan.reset_kind == 1 || plan.reset_kind == 2) {
                /* Completed A: finish it before the reset. */
                fprintf(TR, "> A-FINAL\n");
                fflush(TR);
                {
                    int rc = xmlParseChunk(c, NULL, 0, 1);
                    tail("A-FINAL", 0, rc, c);
                }
            } else {
                /* Suspended A: the document is deliberately NOT finished —
                 * it ends mid-construct with partial lexical/UTF-8/CR/
                 * element/DTD state parked. Reset from that state. */
                fprintf(TR, "> A-SUSPENDED\n");
                fflush(TR);
            }
            /* Reset. */
            if (plan.reset_kind == 1 || plan.reset_kind == 3) {
                fprintf(TR, "> RESET(empty)\n");
                fflush(TR);
                {
                    int rc = xmlCtxtResetPush(c, NULL, 0, NULL, NULL);
                    tail("RESET", 0, rc, c);
                }
                STARTS = 0;
                rng_state = plan.seed;
                feed_doc(&plan, c, db, lb, 0, "B");
            } else {
                fprintf(TR, "> RESET(whole-B)\n");
                fflush(TR);
                {
                    int rc = xmlCtxtResetPush(c, (const char *)db, (int)lb,
                                              NULL, NULL);
                    tail("RESET", 0, rc, c);
                }
            }
            fprintf(TR, "> B-FINAL\n");
            fflush(TR);
            {
                int rc = xmlParseChunk(c, NULL, 0, 1);
                tail("B-FINAL", 0, rc, c);
            }
            fprintf(TR, "> REFEED\n");
            fflush(TR);
            {
                int rc = xmlParseChunk(c, (const char *)db, (int)lb, 1);
                tail("REFEED", 0, rc, c);
            }
            xmlFreeParserCtxt(c);
            free(da);
            free(db);
        }
        return 0;
    }

    for (fi = iarg; fi < argc; fi++) {
        unsigned char *doc;
        size_t dlen;
        size_t off = 0;
        xmlParserCtxtPtr c;

        if (read_file(argv[fi], &doc, &dlen) != 0) {
            fprintf(TR, "== %s unreadable\n", argv[fi]);
            fflush(TR);
            continue;
        }
        fprintf(TR, "== %s bytes=%zu mode=%s\n", argv[fi], dlen, argv[iarg - 1]);
        fflush(TR);

        /* Constructor-initial chunk: the first split goes to
         * xmlCreatePushParserCtxt; parsing continues via xmlParseChunk. */
        if (plan.ctor_init && dlen > 0) {
            size_t n;
            rng_state = plan.seed;
            n = next_split(&plan, dlen);
            fprintf(TR, "> CTOR len=%zu\n", n);
            fflush(TR);
            c = xmlCreatePushParserCtxt(&h, NULL, (const char *)doc, (int)n, NULL);
            if (!c) {
                fprintf(TR, "< CTOR ok=0\n");
                fflush(TR);
                free(doc);
                continue;
            }
            xmlCtxtSetErrorHandler(c, rec_err, NULL);
            CUR = c;
            STARTS = 0;
            {
                xmlParserInputPtr in = c->input;
                fprintf(TR, "< CTOR ok=1 err=%d wf=%d in=%d",
                        c->errNo, c->wellFormed, c->instate);
                if (in != NULL) {
                    fprintf(TR, " p=%ld l=%d col=%d i=%d n=%d",
                            (long)(in->cur - in->base), in->line, in->col,
                            c->inputNr, c->nameNr);
                    if (WINDOW_MODE) {
                        unsigned long cons = in->consumed;
                        fprintf(TR, " c=%lu abs=%lu", cons,
                                cons + (unsigned long)(in->cur - in->base));
                    }
                } else {
                    fprintf(TR, " p=-1 l=-1 col=-1 i=-1 n=%d", c->nameNr);
                }
                fprintf(TR, "\n");
                fflush(TR);
            }
            off = n;
            /* The constructor's chunk is the first split; the plan keeps
             * its own rng state so subsequent splits advance normally. */
        } else {
            c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
            if (!c) { fprintf(TR, "no-ctxt\n"); fflush(TR); free(doc); continue; }
            xmlCtxtSetErrorHandler(c, rec_err, NULL);
            CUR = c;
            STARTS = 0;
            rng_state = plan.seed;
        }

        feed_doc(&plan, c, doc, dlen, off, "CALL");

        /* An inline-final plan passes terminate=1 WITH the last real bytes;
         * the separate empty terminating call is then skipped. Feeding bytes
         * and terminating in ONE call is a distinct input shape (the decoder
         * flush sees the bytes and the end-of-stream together). */
        if (!plan.inline_final) {
            fprintf(TR, "> FINAL\n");
            fflush(TR);
            {
                int rc = xmlParseChunk(c, NULL, 0, 1);
                tail("FINAL", 0, rc, c);
            }
        }
        fprintf(TR, "> REFEED\n");
        fflush(TR);
        {
            int rc = xmlParseChunk(c, (const char *)doc, (int)dlen, 1);
            tail("REFEED", 0, rc, c);
        }
        xmlFreeParserCtxt(c);
        free(doc);
    }
    return 0;
}
