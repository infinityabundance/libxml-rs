/* pushdiff-probe.c — §16.7.8 push-parser differential driver.
 *
 * Feeds a document to xmlCreatePushParserCtxt + xmlParseChunk under a
 * chosen chunking plan and prints ONE deterministic, merged trace:
 * per-call return codes / errNo / wellFormed / instate, interleaved with
 * every SAX event and structured error record in handler-invocation
 * order. Every record is fflush'ed immediately and goes through a single
 * FILE*, so the oracle and the candidate produce byte-comparable streams
 * from the same driver binary.
 *
 * This is the ground-truth gate for the incremental push parser (16.7.8):
 * oracle == candidate, per call, per chunking. It is compiled with
 * -Wall -Wextra -Werror and every callback uses the EXACT prototype from
 * the provider's own headers (no -w, no ABI drift).
 *
 * Handlers are exact recorders, not reformatters:
 *   - SAX2 attributes: all five components (localname, prefix, URI,
 *     value with the (value, end) length convention, end-present flag),
 *     full ordering preserved, nb_ns/nb_att/nb_def in the header;
 *   - DTD declarations: xmlElementContent trees are serialized
 *     canonically (type, ocur, name, prefix, c1, c2 — recursive) and
 *     xmlEnumeration chains are recorded as one|two|three — never treated
 *     as C strings;
 *   - text/cdata/comment/PI bytes escaped (\n \r \t \\ \xNN), so no byte
 *     is ambiguous.
 *
 * Usage: pushdiff-probe [-s sax1|sax2] <chunkmode> <file> [file ...]
 *   chunkmode:
 *     bN      fixed N-byte chunks (last chunk = remainder)
 *     rS      random splits, seed S (split sizes 1..64 bytes)
 *     rS-span random splits, seed S, split sizes 1..span bytes
 *   (default -s sax2)
 *
 * Every plan feeds the document as terminate=0 chunks and then sends ONE
 * terminating call (empty when the last chunk ended exactly at the end of
 * the document), then one REFEED call on the finished context.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

/* ctxt->instate is XML_DEPRECATED_MEMBER in the 2.15 headers; reading it
 * is intentional (it is an observable of the incremental machine). */
#pragma GCC diagnostic ignored "-Wdeprecated-declarations"

static FILE *TR;

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

/* ── SAX2 elements ─────────────────────────────────────────────────── */

static void rec_s2(void *ctx, const xmlChar *local, const xmlChar *pref,
                   const xmlChar *URI, int nb_ns, const xmlChar **ns,
                   int nb_att, int nb_def, const xmlChar **atts) {
    int i;
    (void)ctx;
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
        /* SAX2 attribute values follow the (value, end) convention: the
         * bytes are only valid during the callback and may not be
         * NUL-terminated. Real consumers copy with the end pointer when
         * present — the recorder does the same. atts[i+4] is the value
         * END pointer (NULL means NUL-terminated). */
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
 * as a C string. Every node prints as
 *   {t=<type> o=<ocur> <body>}
 * where body is empty for PCDATA, "name=… prefix=…" for ELEMENT, and
 * "a=<sub> b=<sub>" for SEQ/OR — fully recursive, exact, and readable. */
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

int main(int argc, char **argv) {
    int sax1 = 0;
    int iarg = 1;
    const char *mode;
    unsigned int span = 64;
    unsigned long seed = 0;
    int fixed = 0;
    size_t fixed_n = 1;
    xmlSAXHandler h;
    int fi;

    TR = stdout;
    if (argc < 3) return 1;
    if (strcmp(argv[1], "-s") == 0) {
        if (argc < 5) return 1;
        if (strcmp(argv[2], "sax1") == 0) sax1 = 1;
        iarg = 3;
    }
    mode = argv[iarg];
    iarg++;

    /* Parse the chunking plan once (fixed N, or random seed[-span]). */
    if (mode[0] == 'b') {
        fixed = 1;
        fixed_n = (size_t)strtoul(mode + 1, NULL, 10);
        if (fixed_n == 0) fixed_n = 1;
    } else if (mode[0] == 'r') {
        char *endp;
        seed = strtoul(mode + 1, &endp, 10);
        if (*endp == '-') span = (unsigned int)strtoul(endp + 1, NULL, 10);
        if (seed == 0) seed = 1;
        if (span == 0) span = 1;
    } else {
        fprintf(TR, "bad-mode %s\n", mode);
        return 1;
    }

    for (fi = iarg; fi < argc; fi++) {
        unsigned char *doc;
        size_t dlen;
        const char *path = argv[fi];
        size_t off = 0;
        int call = 0;

        if (read_file(path, &doc, &dlen) != 0) {
            fprintf(TR, "== %s unreadable\n", path);
            fflush(TR);
            continue;
        }
        fprintf(TR, "== %s bytes=%zu mode=%s\n", path, dlen, mode);
        fflush(TR);

        if (sax1) sax1_table(&h); else sax2_table(&h);
        xmlParserCtxtPtr c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
        if (!c) { fprintf(TR, "no-ctxt\n"); fflush(TR); free(doc); continue; }
        xmlCtxtSetErrorHandler(c, rec_err, NULL);

        /* Seed the PRNG ONCE per document+plan: successive split sizes
         * come from the advancing generator, never a reset sequence. */
        rng_state = seed;

        while (off < dlen) {
            size_t this_chunk;
            int rc;
            if (fixed) {
                this_chunk = dlen - off < fixed_n ? dlen - off : fixed_n;
            } else {
                this_chunk = 1 + rnd(span);
                if (this_chunk > dlen - off) this_chunk = dlen - off;
            }
            fprintf(TR, "> CALL %d len=%zu\n", call, this_chunk);
            fflush(TR);
            rc = xmlParseChunk(c, (const char *)doc + off, (int)this_chunk, 0);
            fprintf(TR, "< CALL %d rc=%d err=%d wf=%d in=%d\n",
                    call, rc, c->errNo, c->wellFormed, c->instate);
            fflush(TR);
            off += this_chunk;
            call++;
        }
        /* Terminating call (possibly empty), exactly like real consumers. */
        {
            int rc;
            fprintf(TR, "> FINAL\n");
            fflush(TR);
            rc = xmlParseChunk(c, NULL, 0, 1);
            fprintf(TR, "< FINAL rc=%d err=%d wf=%d in=%d\n",
                    rc, c->errNo, c->wellFormed, c->instate);
            fflush(TR);
        }
        /* Reuse probe: a second identical feed on the finished context —
         * the oracle raises "Extra content" for the pushed bytes (EOF
         * state + terminate); gh12254's parse-twice surface. */
        {
            int rc;
            fprintf(TR, "> REFEED\n");
            fflush(TR);
            rc = xmlParseChunk(c, (const char *)doc, (int)dlen, 1);
            fprintf(TR, "< REFEED rc=%d err=%d wf=%d in=%d\n",
                    rc, c->errNo, c->wellFormed, c->instate);
            fflush(TR);
        }
        xmlFreeParserCtxt(c);
        free(doc);
    }
    return 0;
}
