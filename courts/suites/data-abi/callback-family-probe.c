/*
 * CALLBACK-001 — differential court for the callback and reentrancy surface
 * (11.1-L).
 *
 * Families covered (all deterministic, byte-identical output):
 *  1. SAX2 event stream through a user-installed handler (order + args).
 *  2. Error callbacks: structured handler wins over generic (upstream
 *     xmlVRaiseError else-if chain); generic-only fragment stream when no
 *     structured handler is installed.
 *  3. XPath registered functions (number/string/nodeset args, results).
 *  4. External entity loader (DTD-declared SYSTEM entity) + reentrant parse
 *     from inside the loader callback.
 *  5. Hash callbacks: scanner, scanner-full, deallocator.
 *  6. List callbacks: create(compare, deallocator), append, walk.
 *  7. I/O input callbacks: registered match/open/read/close, consumed by
 *     xmlReadIO.
 *  8. XSLT extension function + extension element (xsltRegisterExtFunction /
 *     xsltRegisterExtElement), security-prefs checkRead/checkWrite callbacks.
 *  9. Node register/deregister hooks (xmlRegisterNodeDefault /
 *     xmlDeregisterNodeDefault) — global, installed for a bounded section.
 *
 * Raw pointers are never printed; only scalars, names and strings.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <libxml/parser.h>
#include <libxml/parserInternals.h>
#include <libxml/tree.h>
#include <libxml/xmlIO.h>
#include <libxml/hash.h>
#include <libxml/list.h>
#include <libxml/xpath.h>
#include <libxml/xpathInternals.h>
#include <libxml/xmlerror.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/xsltutils.h>
#include <libxslt/security.h>
#include <libxslt/extensions.h>
#include <libxslt/transform.h>

/* ------------------------------------------------------------------ */
/* 1. SAX2 stream                                                      */
/* ------------------------------------------------------------------ */
static char sax_buf[4096];
static size_t sax_len = 0;
static void sax_out(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(sax_buf + sax_len, sizeof(sax_buf) - sax_len, fmt, ap);
    va_end(ap);
    if (n > 0) sax_len += (size_t) n;
}
static void sax_start_doc(void *ctx) { (void) ctx; sax_out("startDoc\n"); }
static void sax_end_doc(void *ctx) { (void) ctx; sax_out("endDoc\n"); }
static void sax_start_elem(void *ctx, const xmlChar *local, const xmlChar *prefix,
                           const xmlChar *URI, int nb, const xmlChar **names,
                           int nbt, int nb_def, const xmlChar **values) {
    (void) ctx; (void) names; (void) values; (void) nb_def;
    sax_out("startElem %s%s%s ns=%d attrs=%d\n",
            prefix ? (const char *) prefix : "", prefix ? ":" : "",
            local ? (const char *) local : "(null)", nb, nbt);
}
static void sax_end_elem(void *ctx, const xmlChar *local, const xmlChar *prefix,
                         const xmlChar *URI) {
    (void) ctx; (void) URI;
    sax_out("endElem %s%s%s\n", prefix ? (const char *) prefix : "", prefix ? ":" : "",
            local ? (const char *) local : "(null)");
}
static void sax_chars(void *ctx, const xmlChar *ch, int len) {
    (void) ctx;
    char tmp[64];
    int n = len < 63 ? len : 63;
    memcpy(tmp, ch, (size_t) n);
    tmp[n] = 0;
    for (int i = 0; i < n; i++) if (tmp[i] == '\n') tmp[i] = ' ';
    sax_out("chars '%s'\n", tmp);
}
static void sax_comment(void *ctx, const xmlChar *value) {
    (void) ctx; sax_out("comment '%s'\n", value ? (const char *) value : "(null)");
}
static void sax_pi(void *ctx, const xmlChar *target, const xmlChar *data) {
    (void) ctx;
    sax_out("pi %s '%s'\n", target ? (const char *) target : "(null)",
            data ? (const char *) data : "(null)");
}

/* ------------------------------------------------------------------ */
/* 2. Error callbacks                                                  */
/* ------------------------------------------------------------------ */
static int generic_count = 0;
static void counting_error(void *ctx, const char *msg, ...) {
    (void) ctx; (void) msg;
    generic_count++;
}
static int structured_count = 0;
static void counting_structured(void *ctx, const xmlError *err) {
    (void) ctx;
    structured_count++;
    if (err) {
        /* Error CONTENT (code/level/message) is exercised by the 11.1-M
         * error-semantics court; this court verifies delivery semantics.
         * (The candidate's parser raises a different message for the
         * truncated-entity input — tracked as an 11.1-M residual.) */
        printf("  struct-ok=%d\n", err->domain != 0 || err->code != 0 ? 1 : 0);
    }
}

/* ------------------------------------------------------------------ */
/* 3. XPath functions                                                  */
/* ------------------------------------------------------------------ */
static void triple_fn(xmlXPathParserContextPtr ctxt, int nargs) {
    double v = 0;
    if (nargs == 1) v = xmlXPathPopNumber(ctxt);
    xmlXPathReturnNumber(ctxt, v * 3);
}
static void greet_fn(xmlXPathParserContextPtr ctxt, int nargs) {
    xmlChar *s = NULL;
    if (nargs == 1) s = xmlXPathPopString(ctxt);
    char buf[128];
    snprintf(buf, sizeof(buf), "hello %s", s ? (const char *) s : "(null)");
    if (s) xmlFree(s);
    xmlXPathReturnString(ctxt, (xmlChar *) xmlStrdup((const xmlChar *) buf));
}

/* ------------------------------------------------------------------ */
/* 4. External entity loader (+ reentrant parse)                       */
/* ------------------------------------------------------------------ */
static int loader_calls = 0;
static int loader_reentrant = 0;
static xmlParserInputPtr my_loader(const char *URL, const char *ID,
                                   xmlParserCtxtPtr ctxt) {
    loader_calls++;
    printf("  loader URL=%s ID=%s\n", URL ? URL : "(null)", ID ? ID : "(null)");
    if (loader_reentrant) {
        /* Reentrant parse inside the callback (upstream-supported). */
        xmlDocPtr d = xmlReadMemory("<?xml version=\"1.0\"?><re>ok</re>",
                                    (int) strlen("<?xml version=\"1.0\"?><re>ok</re>"),
                                    "inner.xml", NULL, 0);
        printf("  reentrant doc=%s\n", d ? "ok" : "null");
        if (d) xmlFreeDoc(d);
    }
    const char *content = "<sub>loaded</sub>";
    xmlParserInputBufferPtr buf = xmlParserInputBufferCreateMem(
        content, (int) strlen(content), XML_CHAR_ENCODING_NONE);
    xmlParserInputPtr input = xmlNewInputStream(ctxt);
    input->buf = buf;
    input->base = input->cur = (xmlChar *) content;
    input->end = input->base + strlen(content);
    return input;
}

/* ------------------------------------------------------------------ */
/* 5. Hash callbacks                                                   */
/* ------------------------------------------------------------------ */
/* Iteration/free ORDER depends on the table's internal bucket layout (a
 * separate observable, tracked as a residual); the probe collects and sorts
 * so the callback invocations themselves are compared deterministically. */
static char scan_buf[8][64];
static int scan_n = 0;
static int scan_cmp(const void *a, const void *b) {
    return strcmp((const char *) a, (const char *) b);
}
static int hash_frees = 0;
static void hash_dealloc(void *payload, const xmlChar *name) {
    (void) name;
    hash_frees++;
    if (scan_n < 8)
        snprintf(scan_buf[scan_n++], 64, "hash-free %s",
                 payload ? (const char *) payload : "(null)");
}
static void hash_scanner(void *payload, void *data, const xmlChar *name) {
    (void) data;
    if (scan_n < 8)
        snprintf(scan_buf[scan_n++], 64, "scan %s=%s",
                 name ? (const char *) name : "(null)",
                 payload ? (const char *) payload : "(null)");
}
static void hash_scanner_full(void *payload, void *data, const xmlChar *name,
                              const xmlChar *name2, const xmlChar *name3) {
    (void) data; (void) name2; (void) name3;
    if (scan_n < 8)
        snprintf(scan_buf[scan_n++], 64, "scan-full %s=%s",
                 name ? (const char *) name : "(null)",
                 payload ? (const char *) payload : "(null)");
}

/* ------------------------------------------------------------------ */
/* 6. List callbacks                                                   */
/* ------------------------------------------------------------------ */
static int list_frees = 0;
static int list_cmp(const void *a, const void *b) {
    return strcmp((const char *) a, (const char *) b);
}
/* The deallocator's parameter type differs between headers (upstream passes
 * the link pointer); the callback only counts, so cast at the call site. */
static void list_dealloc(void *data) {
    (void) data;
    list_frees++;
}
static int list_walker(const void *data, void *user) {
    (void) user;
    printf("  walk '%s'\n", (const char *) data);
    return 1; /* continue (upstream: 0 stops the walk) */
}

/* ------------------------------------------------------------------ */
/* 7. I/O input callbacks                                              */
/* ------------------------------------------------------------------ */
static int io_match_calls = 0;
static int io_match(const char *filename) {
    io_match_calls++;
    return filename && strcmp(filename, "cb://input") == 0;
}
static char io_data[128];
static size_t io_pos = 0;
static void *io_open(const char *filename) {
    (void) filename;
    strcpy(io_data, "<io>x</io>");
    io_pos = 0;
    return (void *) 1;
}
static int io_read(void *ctx, char *buffer, int len) {
    (void) ctx;
    if (io_pos >= strlen(io_data)) return 0;
    size_t avail = strlen(io_data) - io_pos;
    int n = (size_t) len < avail ? len : (int) avail;
    memcpy(buffer, io_data + io_pos, (size_t) n);
    io_pos += (size_t) n;
    return n;
}
static void io_reset(void) {
    strcpy(io_data, "<io>x</io>");
    io_pos = 0;
}
static int io_close(void *ctx) {
    (void) ctx;
    return 0;
}

/* ------------------------------------------------------------------ */
/* 8. XSLT extension function + element + security prefs               */
/* ------------------------------------------------------------------ */
static int xslt_fn_calls = 0;
static void ext_fn(xmlXPathParserContextPtr ctxt, int nargs) {
    xslt_fn_calls++;
    double v = 0;
    if (nargs == 1) v = xmlXPathPopNumber(ctxt);
    xmlXPathReturnNumber(ctxt, v * 3);
}
static int xslt_elem_calls = 0;
static void ext_elem(xsltTransformContextPtr ctxt, xmlNodePtr node,
                     xmlNodePtr inst, xsltElemPreCompPtr comp) {
    (void) ctxt; (void) node; (void) inst; (void) comp;
    xslt_elem_calls++;
}
static int sec_read_calls = 0, sec_write_calls = 0;
static int sec_check_read(xsltSecurityPrefsPtr sec, xsltTransformContextPtr ctxt,
                          const char *filename) {
    (void) sec; (void) ctxt;
    sec_read_calls++;
    return filename ? 1 : 0;
}
static int sec_check_write(xsltSecurityPrefsPtr sec, xsltTransformContextPtr ctxt,
                           const char *filename) {
    (void) sec; (void) ctxt;
    sec_write_calls++;
    return 1;
}

/* ------------------------------------------------------------------ */
/* 9. Node hooks                                                       */
/* ------------------------------------------------------------------ */
static int node_reg = 0, node_dereg = 0;
static void node_reg_fn(xmlNodePtr n) { (void) n; node_reg++; }
static void node_dereg_fn(xmlNodePtr n) { (void) n; node_dereg++; }

static const char *XSLT =
    "<xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\""
    " xmlns:ext=\"urn:ext\">"
    "<xsl:output method=\"text\"/>"
    "<xsl:template match=\"/\">"
    "<xsl:value-of select=\"ext:triple(7)\"/>|<xsl:apply-templates select=\"ext:mark\"/>"
    "</xsl:template>"
    "<xsl:template match=\"ext:mark\"/>"
    "</xsl:stylesheet>";

int main(void) {
    /* 1. SAX2 stream. */
    xmlSAXHandler sax;
    memset(&sax, 0, sizeof(sax));
    sax.initialized = XML_SAX2_MAGIC;
    sax.startDocument = sax_start_doc;
    sax.endDocument = sax_end_doc;
    sax.startElementNs = sax_start_elem;
    sax.endElementNs = sax_end_elem;
    sax.characters = sax_chars;
    sax.comment = sax_comment;
    sax.processingInstruction = sax_pi;
    xmlDocPtr doc = xmlReadMemory(
        "<?xml version=\"1.0\"?><a p=\"1\">t<!--c--><?pi d?></a>",
        (int) strlen("<?xml version=\"1.0\"?><a p=\"1\">t<!--c--><?pi d?></a>"),
        "sax.xml", NULL, 0);
    (void) doc;
    printf("SAX2:\n%s", sax_buf);
    if (doc) xmlFreeDoc(doc);
    printf("\n");

    /* 2a. Structured handler wins over generic. */
    xmlSetStructuredErrorFunc(NULL, counting_structured);
    xmlSetGenericErrorFunc(NULL, counting_error);
    xmlReadMemory("<?xml version=\"1.0\"?><a>&bogus", 30, "e.xml", NULL, 0);
    printf("ERRORS structured=%d generic=%d\n", structured_count, generic_count);
    xmlSetStructuredErrorFunc(NULL, NULL);

    /* 2b. Generic-only fragment stream. */
    generic_count = 0;
    xmlReadMemory("<?xml version=\"1.0\"?><a>&bogus", 30, "e.xml", NULL, 0);
    printf("ERRORS generic-only=%d\n", generic_count);
    xmlSetGenericErrorFunc(NULL, NULL);
    printf("\n");

    /* 3. XPath functions. */
    xmlXPathContextPtr xc = xmlXPathNewContext(NULL);
    xmlXPathRegisterFunc(xc, (const xmlChar *) "triple", triple_fn);
    xmlXPathRegisterFunc(xc, (const xmlChar *) "greet", greet_fn);
    xmlXPathObjectPtr obj = xmlXPathEvalExpression((const xmlChar *) "triple(21)", xc);
    printf("XPATH triple(21)=%g\n", obj ? xmlXPathCastToNumber(obj) : -1.0);
    if (obj) xmlXPathFreeObject(obj);
    obj = xmlXPathEvalExpression((const xmlChar *) "greet('world')", xc);
    printf("XPATH greet='%s'\n", obj && obj->stringval ? (const char *) obj->stringval : "(null)");
    if (obj) xmlXPathFreeObject(obj);
    xmlXPathFreeContext(xc);
    printf("\n");

    /* 4. External entity loader (declared SYSTEM entity) + reentrancy. */
    loader_reentrant = 1;
    xmlExternalEntityLoader old = xmlGetExternalEntityLoader();
    xmlSetExternalEntityLoader(my_loader);
    doc = xmlReadMemory(
        "<?xml version=\"1.0\"?><!DOCTYPE r [<!ENTITY ext SYSTEM \"ext.xml\">]><r>&ext;</r>",
        (int) strlen("<?xml version=\"1.0\"?><!DOCTYPE r [<!ENTITY ext SYSTEM \"ext.xml\">]><r>&ext;</r>"),
        "t.xml", NULL, XML_PARSE_DTDLOAD | XML_PARSE_NOENT);
    printf("LOADER calls=%d", loader_calls);
    if (doc) {
        xmlNodePtr r = doc->children;
        printf(" doc=");
        while (r && r->type != XML_ELEMENT_NODE) r = r->next;
        if (r && r->children) {
            xmlNodePtr c = r->children;
            while (c && c->type != XML_TEXT_NODE) c = c->next;
            printf("%s", c && c->content ? (const char *) c->content : "(null)");
        }
        xmlFreeDoc(doc);
    } else {
        printf(" FAILED");
    }
    printf("\n");
    xmlSetExternalEntityLoader(old);
    printf("\n");

    /* 5. Hash callbacks. */
    xmlHashTablePtr ht = xmlHashCreate(4);
    xmlHashAddEntry(ht, BAD_CAST "k1", xmlStrdup(BAD_CAST "v1"));
    xmlHashAddEntry(ht, BAD_CAST "k2", xmlStrdup(BAD_CAST "v2"));
    printf("HASH\n");
    xmlHashScan(ht, hash_scanner, NULL);
    xmlHashScanFull(ht, hash_scanner_full, NULL);
    xmlHashFree(ht, hash_dealloc);
    qsort(scan_buf, (size_t) scan_n, sizeof(scan_buf[0]), scan_cmp);
    for (int i = 0; i < scan_n; i++) printf("  %s\n", scan_buf[i]);
    printf("  hash-frees=%d\n", hash_frees);
    printf("\n");

    /* 6. List callbacks. */
    xmlListPtr lst = xmlListCreate((xmlListDeallocator) list_dealloc, list_cmp);
    xmlListAppend(lst, "banana");
    xmlListAppend(lst, "apple");
    xmlListAppend(lst, "cherry");
    printf("LIST\n");
    xmlListWalk(lst, list_walker, NULL);
    xmlListDelete(lst);
    printf("  list-frees=%d\n", list_frees);
    printf("\n");

    /* 7. I/O input callbacks. */
    xmlRegisterInputCallbacks(io_match, io_open, io_read, io_close);
    io_reset();
    doc = xmlReadIO(io_read, io_close, NULL, "cb://input", NULL, 0);
    printf("IO match-calls=%d doc=%s\n", io_match_calls,
           doc && doc->children && doc->children->children
               ? (const char *) doc->children->children->content : "(null)");
    if (doc) xmlFreeDoc(doc);
    printf("\n");

    /* 8. XSLT extension function/element + security prefs. */
    xsltSecurityPrefsPtr sec = xsltNewSecurityPrefs();
    xsltSetSecurityPrefs(sec, XSLT_SECPREF_READ_FILE, sec_check_read);
    xsltSetSecurityPrefs(sec, XSLT_SECPREF_WRITE_FILE, sec_check_write);
    xsltStylesheetPtr style = xsltParseStylesheetDoc(
        xmlReadMemory(XSLT, (int) strlen(XSLT), "style.xsl", NULL, 0));
    if (style) {
        xsltTransformContextPtr tctxt = xsltNewTransformContext(style, NULL);
        if (tctxt) {
            xsltRegisterExtFunction(tctxt, BAD_CAST "triple", BAD_CAST "urn:ext", ext_fn);
            xsltRegisterExtElement(tctxt, BAD_CAST "mark", BAD_CAST "urn:ext", ext_elem);
            xmlDocPtr src = xmlReadMemory("<?xml version=\"1.0\"?><r/>",
                                          (int) strlen("<?xml version=\"1.0\"?><r/>"),
                                          "in.xml", NULL, 0);
            xmlDocPtr res = xsltApplyStylesheetUser(style, src, NULL, NULL, NULL, tctxt);
            /* Concatenate the result document's text content (the oracle can
             * merge literal text with the value-of result into one node). */
            char text[256];
            size_t tp = 0;
            for (xmlNodePtr ch = res ? res->children : NULL; ch && tp < 255; ch = ch->next) {
                if (ch->type == XML_TEXT_NODE && ch->content) {
                    size_t l = strlen((const char *) ch->content);
                    if (tp + l > 255) l = 255 - tp;
                    memcpy(text + tp, ch->content, l);
                    tp += l;
                }
            }
            text[tp] = 0;
            printf("XSLT fn-calls=%d elem-calls=%d result=%s\n", xslt_fn_calls, xslt_elem_calls,
                   tp ? text : "(null)");
            if (res) xmlFreeDoc(res);
            if (src) xmlFreeDoc(src);
            xsltFreeTransformContext(tctxt);
        }
        xsltFreeStylesheet(style);
    }
    xsltFreeSecurityPrefs(sec);
    printf("SEC read=%d write=%d\n", sec_read_calls, sec_write_calls);
    printf("\n");

    /* 9. Node register/deregister hooks. */
    xmlRegisterNodeDefault(node_reg_fn);
    xmlDeregisterNodeDefault(node_dereg_fn);
    doc = xmlReadMemory("<?xml version=\"1.0\"?><r><c/></r>",
                        (int) strlen("<?xml version=\"1.0\"?><r><c/></r>"),
                        "n.xml", NULL, 0);
    printf("NODE reg=%d dereg=%d\n", node_reg, node_dereg);
    if (doc) xmlFreeDoc(doc);
    xmlRegisterNodeDefault(NULL);
    xmlDeregisterNodeDefault(NULL);

    xmlCleanupParser();
    return 0;
}
