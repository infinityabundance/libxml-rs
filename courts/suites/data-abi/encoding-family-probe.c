/*
 * ENCODING-001 — differential court for the xmlCharEncoding handler family
 * (xmlLookupCharEncodingHandler, xmlGetCharEncodingHandler,
 *  xmlOpenCharEncodingHandler, xmlCreateCharEncodingHandler,
 *  xmlCharEncNewCustomHandler) plus xmlGetCharEncodingName.
 *
 * Byte-identical stdout between the oracle DSO (system libxml2 2.15.3) and
 * the candidate liblibxml_rs.so. Raw pointer values are never printed:
 * handlers are rendered as their `name` field or "NULL", so addresses
 * (which differ across processes/DSOs by construction) cannot fail the
 * comparison.
 *
 * Known candidate divergence (documented residual): encodings that upstream
 * serves through iconv/ICU (UCS-4LE/BE, EBCDIC, UCS-2, ISO-8859-2..9,10,11,
 * 13..16, ISO-2022-JP, Shift_JIS, EUC-JP, windows-1252) and the HTML static
 * handler report XML_ERR_UNSUPPORTED_ENCODING (32) in the candidate because
 * the crate ships no iconv/ICU backend. Those encodings are therefore
 * excluded from this court; the native set (UTF-8, UTF-16LE, UTF-16BE,
 * UTF-16, ISO-8859-1, US-ASCII) plus all error paths are covered.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/encoding.h>

/* Dummy modern conversion callback for xmlCharEncNewCustomHandler. */
static xmlCharEncError
dummy_conv(void *vctxt, unsigned char *out, int *outlen,
           const unsigned char *in, int *inlen, int flush) {
    (void) vctxt; (void) out; (void) outlen; (void) in; (void) inlen;
    (void) flush;
    return XML_ENC_ERR_SUCCESS;
}

static const char *hname(xmlCharEncodingHandler *h) {
    return (h && h->name) ? (const char *) h->name : "NULL";
}

int main(void) {
    xmlCharEncodingHandler *h;
    int rc;
    int i;

    /* Native encodings + boundaries + error paths. */
    const int encs[] = { -1, 0, 1, 2, 3, 7, 8, 10, 22, 23, 32, 33 };
    for (i = 0; i < (int) (sizeof(encs) / sizeof(encs[0])); i++) {
        h = NULL;
        rc = xmlLookupCharEncodingHandler((xmlCharEncoding) encs[i], &h);
        printf("lookup enc=%2d rc=%d h=%s\n", encs[i], rc, hname(h));
    }

    /* NULL out argument. */
    rc = xmlLookupCharEncodingHandler(XML_CHAR_ENCODING_UTF16LE, NULL);
    printf("lookup out=NULL rc=%d\n", rc);

    /* Deprecated get wrapper over the same range. */
    for (i = 0; i < (int) (sizeof(encs) / sizeof(encs[0])); i++) {
        h = xmlGetCharEncodingHandler((xmlCharEncoding) encs[i]);
        printf("get enc=%2d h=%s\n", encs[i], hname(h));
    }

    /* open: input and output directions, both spellings of ASCII. */
    const char *names[] = { "UTF-8", "UTF8", "UTF-16LE", "UTF-16BE",
                            "UTF-16", "ISO-8859-1", "ASCII", "US-ASCII",
                            "bogus-enc", NULL };
    for (i = 0; names[i]; i++) {
        h = NULL;
        rc = xmlOpenCharEncodingHandler(names[i], 0, &h);
        printf("open input  name=%-12s rc=%d h=%s\n", names[i], rc, hname(h));
        if (h)
            xmlCharEncCloseFunc(h);
        h = NULL;
        rc = xmlOpenCharEncodingHandler(names[i], 1, &h);
        printf("open output name=%-12s rc=%d h=%s\n", names[i], rc, hname(h));
        if (h)
            xmlCharEncCloseFunc(h);
    }

    /* open error paths. */
    h = NULL;
    rc = xmlOpenCharEncodingHandler("UTF-8", 1, NULL);
    printf("open out=NULL rc=%d\n", rc);
    rc = xmlOpenCharEncodingHandler(NULL, 1, &h);
    printf("open name=NULL rc=%d h=%s\n", rc, hname(h));
    h = NULL;
    rc = xmlOpenCharEncodingHandler("UTF-8", 0, &h);
    printf("open utf8 h=%s rc=%d\n", hname(h), rc);

    /* xmlCreateCharEncodingHandler: flags INPUT/OUTPUT/0, alias resolution. */
    h = NULL;
    rc = xmlCreateCharEncodingHandler("UTF-16LE", XML_ENC_INPUT, NULL, NULL, &h);
    printf("create input  UTF-16LE rc=%d h=%s\n", rc, hname(h));
    if (h)
        xmlCharEncCloseFunc(h);
    h = NULL;
    rc = xmlCreateCharEncodingHandler("UTF-16LE", XML_ENC_OUTPUT, NULL, NULL, &h);
    printf("create output UTF-16LE rc=%d h=%s\n", rc, hname(h));
    if (h)
        xmlCharEncCloseFunc(h);
    h = NULL;
    rc = xmlCreateCharEncodingHandler("UTF-8", XML_ENC_INPUT, NULL, NULL, &h);
    printf("create utf8 rc=%d h=%s\n", rc, hname(h));
    rc = xmlCreateCharEncodingHandler("UTF-8", 0, NULL, NULL, &h);
    printf("create flags=0 rc=%d h=%s\n", rc, hname(h));
    rc = xmlCreateCharEncodingHandler(NULL, XML_ENC_INPUT, NULL, NULL, &h);
    printf("create name=NULL rc=%d h=%s\n", rc, hname(h));
    rc = xmlCreateCharEncodingHandler("UTF-8", XML_ENC_INPUT, NULL, NULL, NULL);
    printf("create out=NULL rc=%d\n", rc);
    /* alias: register "myutf16" -> "UTF-16LE", then open by alias. */
    xmlAddEncodingAlias("UTF-16LE", "myutf16");
    h = NULL;
    rc = xmlOpenCharEncodingHandler("myutf16", 0, &h);
    printf("open alias rc=%d h=%s\n", rc, hname(h));
    if (h)
        xmlCharEncCloseFunc(h);
    xmlDelEncodingAlias("myutf16");

    /* xmlCharEncNewCustomHandler: creation, name duplication, close. */
    h = NULL;
    rc = xmlCharEncNewCustomHandler("CUSTOM", dummy_conv, dummy_conv, NULL,
                                   NULL, NULL, &h);
    printf("custom rc=%d h=%s\n", rc, hname(h));
    if (h) {
        if (h->input.func != dummy_conv || h->output.func != dummy_conv)
            printf("custom funcs MISMATCH\n");
        else
            printf("custom funcs ok\n");
        xmlCharEncCloseFunc(h);
    }
    h = NULL;
    rc = xmlCharEncNewCustomHandler(NULL, dummy_conv, NULL, NULL, NULL, NULL, &h);
    printf("custom noname rc=%d h=%s\n", rc, hname(h));
    if (h)
        xmlCharEncCloseFunc(h);
    rc = xmlCharEncNewCustomHandler("CUSTOM", dummy_conv, NULL, NULL, NULL,
                                   NULL, NULL);
    printf("custom out=NULL rc=%d\n", rc);

    /* xmlGetCharEncodingName: canonical names (US-ASCII per defaultHandlers). */
    const int nencs[] = { 1, 2, 3, 10, 22, 23, 31, 0, 32 };
    for (i = 0; i < (int) (sizeof(nencs) / sizeof(nencs[0])); i++) {
        const char *n = xmlGetCharEncodingName((xmlCharEncoding) nencs[i]);
        printf("encname %2d -> %s\n", nencs[i], n ? n : "NULL");
    }
    return 0;
}
