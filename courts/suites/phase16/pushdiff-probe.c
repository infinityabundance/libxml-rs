/* pushdiff-probe.c — §16.7.8 push-parser differential driver.
 *
 * Feeds a document to xmlCreatePushParserCtxt + xmlParseChunk under a
 * chosen chunking plan and prints ONE deterministic, merged trace:
 * per-call return codes / errNo / wellFormed, interleaved with every SAX
 * event and structured error record in handler-invocation order. Every
 * record is fflush'ed immediately and goes through a single FILE*, so the
 * oracle and the candidate produce byte-comparable streams from the same
 * driver binary — this is the ground-truth gate for the incremental push
 * parser (16.7.8): oracle == candidate, per call, per chunking.
 *
 * Handlers are deliberately "dumb recorders": they print exactly what
 * libxml2 handed them (names, prefixes, URIs, attribute triples with
 * defaulted flag, ns decls, text bytes escaped) with no reformatting, so a
 * divergence in event segmentation / arguments / ordering / ownership
 * shows up as a byte diff.
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
 * the document). Chunk boundaries intentionally land at every byte offset
 * across the corpus: fixed sizes exercise construct starts, and the
 * single-byte / random plans exercise split-inside-token,
 * split-inside-UTF-8, CR-at-chunk-end, and multi-call epilog flows.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/xmlerror.h>

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

static void rec_sd(void *u)            { (void)u; fprintf(TR, "startDocument\n"); fflush(TR); }
static void rec_ed(void *u)            { (void)u; fprintf(TR, "endDocument\n"); fflush(TR); }

static void rec_s2(void *u, const xmlChar *local, const xmlChar *pref,
                   const xmlChar *URI, int nb_ns, const xmlChar **ns,
                   int nb_att, int nb_def, const xmlChar **atts) {
    int i;
    (void)u;
    fprintf(TR, "startElementNs [");
    esc_bytes(local, local ? (int)xmlStrlen(local) : 0);
    fprintf(TR, "] pref=");
    esc_bytes(pref, pref ? (int)xmlStrlen(pref) : 0);
    fprintf(TR, " uri=");
    esc_bytes(URI, URI ? (int)xmlStrlen(URI) : 0);
    fprintf(TR, " ns=%d att=%d def=%d", nb_ns, nb_att, nb_def);
    for (i = 0; i < nb_ns * 2; i += 2) {
        fprintf(TR, " {");
        esc_bytes(ns[i], ns[i] ? (int)xmlStrlen(ns[i]) : 0);
        fprintf(TR, "=");
        esc_bytes(ns[i + 1], ns[i + 1] ? (int)xmlStrlen(ns[i + 1]) : 0);
        fprintf(TR, "}");
    }
    for (i = 0; i < nb_att * 5; i += 5) {
        /* SAX2 attribute values follow the (value, end) convention: the
         * bytes are only valid during the callback and may not be
         * NUL-terminated. Real consumers (lxml/PHP) copy with the end
         * pointer when present — the recorder does the same so the trace
         * is exact and cannot overrun. atts[i+4] is the value END
         * pointer (NULL means NUL-terminated), NOT a defaulted flag. */
        const xmlChar *v = atts[i + 3];
        size_t vlen = 0;
        if (v) {
            vlen = atts[i + 4]
                       ? (size_t)(atts[i + 4] - v)
                       : (size_t)xmlStrlen(v);
        }
        fprintf(TR, " [");
        esc_bytes(atts[i], atts[i] ? (int)xmlStrlen(atts[i]) : 0);
        fprintf(TR, "]=" );
        esc_bytes(v, (int)vlen);
        fprintf(TR, "(end=%d)", atts[i + 4] ? 1 : 0);
    }
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_e2(void *u, const xmlChar *local, const xmlChar *pref,
                   const xmlChar *URI) {
    (void)u;
    fprintf(TR, "endElementNs [");
    esc_bytes(local, local ? (int)xmlStrlen(local) : 0);
    fprintf(TR, "] pref=");
    esc_bytes(pref, pref ? (int)xmlStrlen(pref) : 0);
    fprintf(TR, " uri=");
    esc_bytes(URI, URI ? (int)xmlStrlen(URI) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_s1(void *u, const xmlChar *name, const xmlChar **atts) {
    int i;
    (void)u;
    fprintf(TR, "startElement [");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, "]");
    if (atts) for (i = 0; atts[i] != NULL; i += 2) {
        fprintf(TR, " [");
        esc_bytes(atts[i], (int)xmlStrlen(atts[i]));
        fprintf(TR, "]=");
        esc_bytes(atts[i + 1], atts[i + 1] ? (int)xmlStrlen(atts[i + 1]) : 0);
    }
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_e1(void *u, const xmlChar *name) {
    (void)u;
    fprintf(TR, "endElement [");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_ch(void *u, const xmlChar *ch, int len) {
    (void)u;
    fprintf(TR, "characters len=%d [", len);
    esc_bytes(ch, len);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_co(void *u, const xmlChar *data) {
    (void)u;
    fprintf(TR, "comment [");
    esc_bytes(data, data ? (int)xmlStrlen(data) : 0);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_pi(void *u, const xmlChar *target, const xmlChar *data) {
    (void)u;
    fprintf(TR, "pi target=");
    esc_bytes(target, target ? (int)xmlStrlen(target) : 0);
    fprintf(TR, " data=");
    esc_bytes(data, data ? (int)xmlStrlen(data) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_cd(void *u, const xmlChar *data, int len) {
    (void)u;
    fprintf(TR, "cdata len=%d [", len);
    esc_bytes(data, len);
    fprintf(TR, "]\n");
    fflush(TR);
}

static void rec_intsubset(void *u, const xmlChar *name, const xmlChar *ExternalID,
                          const xmlChar *SystemID) {
    (void)u;
    fprintf(TR, "internalSubset name=");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, " ext=");
    esc_bytes(ExternalID, ExternalID ? (int)xmlStrlen(ExternalID) : 0);
    fprintf(TR, " sys=");
    esc_bytes(SystemID, SystemID ? (int)xmlStrlen(SystemID) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_extsubset(void *u, const xmlChar *name, const xmlChar *ExternalID,
                          const xmlChar *SystemID) {
    (void)u;
    fprintf(TR, "externalSubset name=");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, " ext=");
    esc_bytes(ExternalID, ExternalID ? (int)xmlStrlen(ExternalID) : 0);
    fprintf(TR, " sys=");
    esc_bytes(SystemID, SystemID ? (int)xmlStrlen(SystemID) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_entdecl(void *u, const xmlChar *name, int type,
                        const xmlChar *content, const xmlChar *ExternalID,
                        const xmlChar *SystemID, xmlChar *pubid) {
    (void)u;
    fprintf(TR, "entityDecl name=");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, " type=%d content=", type);
    esc_bytes(content, content ? (int)xmlStrlen(content) : 0);
    fprintf(TR, " ext=");
    esc_bytes(ExternalID, ExternalID ? (int)xmlStrlen(ExternalID) : 0);
    fprintf(TR, " sys=");
    esc_bytes(SystemID, SystemID ? (int)xmlStrlen(SystemID) : 0);
    fprintf(TR, " pubid=");
    esc_bytes(pubid, pubid ? (int)xmlStrlen(pubid) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

static void rec_attrdecl(void *u, const xmlChar *elem, const xmlChar *fullname,
                         int type, int def, const xmlChar *defval,
                         const xmlChar *tree) {
    (void)u;
    fprintf(TR, "attributeDecl elem=");
    esc_bytes(elem, elem ? (int)xmlStrlen(elem) : 0);
    fprintf(TR, " name=");
    esc_bytes(fullname, fullname ? (int)xmlStrlen(fullname) : 0);
    fprintf(TR, " type=%d def=%d val=", type, def);
    esc_bytes(defval, defval ? (int)xmlStrlen(defval) : 0);
    fprintf(TR, " tree=%s\n", tree ? "set" : "null");
    fflush(TR);
}

static void rec_elemsdecl(void *u, const xmlChar *name, int type,
                          const xmlChar *content) {
    (void)u;
    fprintf(TR, "elementDecl name=");
    esc_bytes(name, name ? (int)xmlStrlen(name) : 0);
    fprintf(TR, " type=%d content=", type);
    esc_bytes(content, content ? (int)xmlStrlen(content) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

/* Structured error record: fields + message in one line. */
static void rec_err(void *u, xmlErrorPtr err) {
    (void)u;
    if (err == NULL) { fprintf(TR, "error(NULL)\n"); fflush(TR); return; }
    fprintf(TR, "error dom=%d code=%d level=%d file=", err->domain, err->code, err->level);
    esc_bytes((const xmlChar *)(err->file ? err->file : ""), err->file ? (int)strlen(err->file) : 0);
    fprintf(TR, " line=%d i1=%d i2=%d str1=", err->line, err->int1, err->int2);
    esc_bytes((const xmlChar *)(err->str1 ? err->str1 : ""), err->str1 ? (int)strlen((char *)err->str1) : 0);
    fprintf(TR, " str2=");
    esc_bytes((const xmlChar *)(err->str2 ? err->str2 : ""), err->str2 ? (int)strlen((char *)err->str2) : 0);
    fprintf(TR, " str3=");
    esc_bytes((const xmlChar *)(err->str3 ? err->str3 : ""), err->str3 ? (int)strlen((char *)err->str3) : 0);
    fprintf(TR, " msg=");
    esc_bytes((const xmlChar *)(err->message ? err->message : ""), err->message ? (int)strlen(err->message) : 0);
    fprintf(TR, "\n");
    fflush(TR);
}

static void sax1_table(xmlSAXHandler *h) {
    memset(h, 0, sizeof(*h));
    h->startDocument = rec_sd;
    h->endDocument = rec_ed;
    h->startElement = rec_s1;
    h->endElement = rec_e1;
    h->characters = rec_ch;
    h->comment = rec_co;
    h->processingInstruction = rec_pi;
    h->cdataBlock = rec_cd;
    h->internalSubset = rec_intsubset;
    h->externalSubset = rec_extsubset;
    h->entityDecl = rec_entdecl;
    h->attributeDecl = rec_attrdecl;
    h->elementDecl = rec_elemsdecl;
}

static void sax2_table(xmlSAXHandler *h) {
    memset(h, 0, sizeof(*h));
    h->initialized = XML_SAX2_MAGIC;
    h->startDocument = rec_sd;
    h->endDocument = rec_ed;
    h->startElementNs = rec_s2;
    h->endElementNs = rec_e2;
    h->characters = rec_ch;
    h->comment = rec_co;
    h->processingInstruction = rec_pi;
    h->cdataBlock = rec_cd;
    h->internalSubset = rec_intsubset;
    h->externalSubset = rec_extsubset;
    h->entityDecl = rec_entdecl;
    h->attributeDecl = rec_attrdecl;
    h->elementDecl = rec_elemsdecl;
}

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

        /* Determine split points (terminating call is always sent after the
         * document bytes are exhausted). */
        if (sax1) sax1_table(&h); else sax2_table(&h);
        xmlParserCtxtPtr c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
        if (!c) { fprintf(TR, "no-ctxt\n"); fflush(TR); free(doc); continue; }
        xmlCtxtSetErrorHandler(c, rec_err, NULL);

        while (off < dlen) {
            size_t this_chunk;
            int rc;
            if (mode[0] == 'b') {
                size_t n = (size_t)strtoul(mode + 1, NULL, 10);
                if (n == 0) n = 1;
                this_chunk = dlen - off < n ? dlen - off : n;
            } else if (mode[0] == 'r') {
                char *endp;
                unsigned int span = 64;
                unsigned long seed = strtoul(mode + 1, &endp, 10);
                if (*endp == '-') span = (unsigned int)strtoul(endp + 1, NULL, 10);
                if (seed == 0) seed = 1;
                rng_state = seed;
                this_chunk = 1 + rnd(span);
                if (this_chunk > dlen - off) this_chunk = dlen - off;
            } else {
                fprintf(TR, "bad-mode %s\n", mode);
                fflush(TR);
                return 1;
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
        /* Reuse probe: a second identical feed on the finished context — the
         * oracle raises "Extra content" for the pushed bytes (EOF state +
         * terminate); gh12254's xml_parse_into_struct-twice case. */
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
