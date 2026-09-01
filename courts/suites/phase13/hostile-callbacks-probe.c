/*
 * hostile-callbacks-probe.c — Phase 13 HOSTILE-CALLBACKS attack court.
 *
 * Exercises the callback surfaces with adversarial behaviours a real
 * consumer (lxml, PHP, nokogiri) can install:
 *
 *   C1. external entity loader that always fails (returns NULL);
 *   C2. entity loader returning an empty input (no data);
 *   C3. input I/O callback returning 0 bytes immediately (EOF);
 *   C4. input I/O callback returning -1 (I/O error);
 *   C5. input I/O callback feeding the document one byte per call;
 *   C6. output I/O callback returning -1 (write error) — save must fail;
 *   C7. output I/O callback returning 0 (nothing written) — save outcome;
 *   C8. structured error handler counting errors on garbage input;
 *   C9. generic error handler counting errors on garbage input;
 *   C10. SAX error handler counting errors during xmlSAXUserParseMemory.
 *
 * Every callback is deterministic; stdout and stderr are compared
 * byte-for-byte between the oracle and the candidate.
 *
 * Court family: HOSTILE-CALLBACKS (Phase 13 hostile audit, dimension 4:
 * rare/adversarial callbacks)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlIO.h>
#include <libxml/xmlerror.h>
#include <libxml/xmlsave.h>

/* ── entity loader state ──────────────────────────────────────────────── */
static int loader_mode = 0; /* 0 = fail(NULL), 1 = empty input */
static int loader_calls = 0;

static xmlParserInputPtr
hostile_loader(const char *URL, const char *ID, xmlParserCtxtPtr ctxt) {
    (void)URL;
    (void)ID;
    loader_calls++;
    if (loader_mode == 0)
        return NULL;
    /* mode 1: return an empty input (valid parser input, no data) */
    return xmlNewInputFromMemory("mem://hostile", BAD_CAST "", 0,
                                 XML_INPUT_BUF_STATIC);
}

/* ── input I/O callback state ─────────────────────────────────────────── */
static const char *io_data = NULL;
static int io_len = 0;
static int io_pos = 0;
static int io_mode = 0; /* 0 = EOF(0), 1 = error(-1), 2 = one byte/call */

static int hostile_read(void *ctx, char *buf, int len) {
    (void)ctx;
    if (io_mode == 0)
        return 0;
    if (io_mode == 1)
        return -1;
    /* mode 2: hand over a single byte per call */
    if (io_pos >= io_len)
        return 0;
    if (len < 1)
        return 0;
    buf[0] = io_data[io_pos++];
    return 1;
}

static int hostile_close(void *ctx) {
    (void)ctx;
    return 0;
}

/* ── output I/O callback state ────────────────────────────────────────── */
static int out_mode = 0; /* 0 = error(-1), 1 = nothing(0) */

static int hostile_write(void *ctx, const char *buf, int len) {
    (void)ctx;
    (void)buf;
    if (out_mode == 0)
        return -1;
    return 0; /* writes nothing, reports success */
}

static int hostile_close_out(void *ctx) {
    (void)ctx;
    return 0;
}

/* ── error handler counting ───────────────────────────────────────────── */
static int struct_errors = 0;
static int struct_warnings = 0;
static void on_structured(void *ctx, const xmlError *err) {
    (void)ctx;
    if (err == NULL)
        return;
    if (err->level == XML_ERR_WARNING)
        struct_warnings++;
    else
        struct_errors++;
}

static int generic_errors = 0;
static void on_generic(void *ctx, const char *msg, ...) {
    (void)ctx;
    (void)msg;
    generic_errors++;
}

/* ── SAX error counting ───────────────────────────────────────────────── */
static int sax_errors = 0;
static int sax_warnings = 0;
static void sax_error(void *ctx, const char *msg, ...) {
    (void)ctx;
    (void)msg;
    sax_errors++;
}
static void sax_warning(void *ctx, const char *msg, ...) {
    (void)ctx;
    (void)msg;
    sax_warnings++;
}

int main(void) {
    LIBXML_TEST_VERSION

    /* ── C1/C2. hostile external entity loaders ────────────────────────── */
    {
        const char *doc = "<!DOCTYPE r [<!ENTITY e SYSTEM \"miss\">]><r>&e;</r>";
        xmlExternalEntityLoader old = xmlGetExternalEntityLoader();

        loader_mode = 0;
        xmlSetExternalEntityLoader(hostile_loader);
        loader_calls = 0;
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "t", NULL,
                                    XML_PARSE_DTDLOAD | XML_PARSE_NOENT);
        printf("C1 loader-fail: doc=%s loader_calls=%d\n",
               d ? "(ptr)" : "(nil)", loader_calls);
        if (d) xmlFreeDoc(d);

        loader_mode = 1;
        loader_calls = 0;
        d = xmlReadMemory(doc, (int)strlen(doc), "t", NULL,
                          XML_PARSE_DTDLOAD | XML_PARSE_NOENT);
        printf("C2 loader-empty: doc=%s loader_calls=%d\n",
               d ? "(ptr)" : "(nil)", loader_calls);
        if (d) xmlFreeDoc(d);

        xmlSetExternalEntityLoader(old);
    }

    /* ── C3/C4/C5. input I/O callbacks ─────────────────────────────────── */
    {
        io_mode = 0; /* immediate EOF */
        xmlParserCtxtPtr c = xmlNewParserCtxt();
        xmlDocPtr d = c ? xmlCtxtReadIO(c, hostile_read, hostile_close, NULL,
                                        "t", NULL, 0)
                        : NULL;
        printf("C3 io-EOF: doc=%s\n", d ? "(ptr)" : "(nil)");
        if (d) xmlFreeDoc(d);
        if (c) xmlFreeParserCtxt(c);

        io_mode = 1; /* I/O error */
        c = xmlNewParserCtxt();
        d = c ? xmlCtxtReadIO(c, hostile_read, hostile_close, NULL, "t", NULL, 0)
              : NULL;
        printf("C4 io-error: doc=%s\n", d ? "(ptr)" : "(nil)");
        if (d) xmlFreeDoc(d);
        if (c) xmlFreeParserCtxt(c);

        io_mode = 2; /* one byte per call */
        io_data = "<r>abc</r>";
        io_len = (int)strlen(io_data);
        io_pos = 0;
        c = xmlNewParserCtxt();
        d = c ? xmlCtxtReadIO(c, hostile_read, hostile_close, NULL, "t", NULL, 0)
              : NULL;
        printf("C5 io-bytewise: doc=%s\n", d ? "(ptr)" : "(nil)");
        if (d) {
            xmlNodePtr r = xmlDocGetRootElement(d);
            printf("C5 root=%s children=%s\n",
                   r && r->name ? (const char *)r->name : "(null)",
                   r && r->children ? (const char *)r->children->content : "(null)");
            xmlFreeDoc(d);
        }
        if (c) xmlFreeParserCtxt(c);
    }

    /* ── C6/C7. output I/O callbacks ───────────────────────────────────── */
    {
        xmlDocPtr d = xmlReadMemory("<r>t</r>", 8, "t", NULL, 0);
        out_mode = 0; /* write error */
        xmlSaveCtxtPtr s = d ? xmlSaveToIO(hostile_write, hostile_close_out,
                                           NULL, "UTF-8", 0)
                             : NULL;
        printf("C6 save-ctx=%s\n", s ? "(ptr)" : "(nil)");
        if (s) {
            long n = xmlSaveDoc(s, d);
            printf("C6 save-doc=%ld\n", n);
            int rc = xmlSaveClose(s);
            printf("C6 save-close=%d\n", rc);
        }
        out_mode = 1; /* writes nothing, reports success */
        s = d ? xmlSaveToIO(hostile_write, hostile_close_out, NULL, "UTF-8", 0)
              : NULL;
        printf("C7 save-ctx=%s\n", s ? "(ptr)" : "(nil)");
        if (s) {
            long n = xmlSaveDoc(s, d);
            printf("C7 save-doc=%ld\n", n);
            int rc = xmlSaveClose(s);
            printf("C7 save-close=%d\n", rc);
        }
        if (d) xmlFreeDoc(d);
    }

    /* ── C8. structured error handler counting ─────────────────────────── */
    {
        xmlSetStructuredErrorFunc(NULL, on_structured);
        struct_errors = 0;
        struct_warnings = 0;
        xmlDocPtr d = xmlReadMemory("<<<", 3, "t", NULL, XML_PARSE_RECOVER);
        printf("C8 structured: doc=%s errors=%d warnings=%d\n",
               d ? "(ptr)" : "(nil)", struct_errors, struct_warnings);
        if (d) xmlFreeDoc(d);
        xmlSetStructuredErrorFunc(NULL, NULL);
    }

    /* ── C9. generic error handler counting ────────────────────────────── */
    {
        xmlSetGenericErrorFunc(NULL, on_generic);
        generic_errors = 0;
        xmlDocPtr d = xmlReadMemory("<<<", 3, "t", NULL, 0);
        printf("C9 generic: doc=%s errors=%d\n", d ? "(ptr)" : "(nil)",
               generic_errors);
        if (d) xmlFreeDoc(d);
        xmlSetGenericErrorFunc(NULL, NULL);
    }

    /* ── C10. SAX error handler counting ───────────────────────────────── */
    {
        xmlSAXHandler sax;
        memset(&sax, 0, sizeof(sax));
        sax.error = sax_error;
        sax.warning = sax_warning;
        sax.initialized = XML_SAX2_MAGIC;
        sax_errors = 0;
        sax_warnings = 0;
        int rc = xmlSAXUserParseMemory(&sax, NULL, "<<<", 3);
        printf("C10 sax: rc=%d errors=%d warnings=%d\n", rc, sax_errors,
               sax_warnings);
    }

    /* ── final marker ──────────────────────────────────────────────────── */
    printf("HOSTILE-CALLBACKS VERDICT PASS\n");
    return 0;
}
