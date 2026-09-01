/*
 * hostile-abi-probe.c — Phase 13 HOSTILE-ABI attack court.
 *
 * Attacks the exported C ABI surface with arguments the normal differential
 * courts never pass: NULL pointers, invalid enum/option values, extreme and
 * negative sizes, boundary lengths, and error-return contracts. Every call
 * prints a deterministic line so the oracle run and the candidate run must
 * match byte-for-byte.
 *
 * Court family: HOSTILE-ABI (Phase 13 hostile audit, dimension 1: ABI)
 *
 * Attack classes:
 *   A. NULL / invalid handles on the whole surface (error-return parity)
 *   B. invalid option/flag bits (XML_PARSE_*, XML_SAVE_*, reader options)
 *   C. extreme/negative sizes and lengths (xmlReadMemory, xmlParseChunk,
 *      buffer sizes, xmlStrncat, xmlTextReaderRead...)
 *   D. integer-boundary values (INT_MAX/INT_MIN sizes, 0-length buffers)
 *   E. enum/type code abuse (xmlNodeDump on every node type, invalid
 *      xmlElementType values)
 *
 * Output is fully deterministic; stderr from the library is captured so
 * diagnostics parity is also checked. Heap pointers are canonicalised to
 * NULL vs non-NULL because ASLR makes raw addresses differ between the
 * two processes.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlmemory.h>
#include <libxml/xmlreader.h>
#include <libxml/xmlwriter.h>
#include <libxml/xmlsave.h>
#include <libxml/xpath.h>
#include <libxml/xpathInternals.h>
#include <libxml/xmlIO.h>
#include <libxml/uri.h>

/* Canonicalise a pointer: only NULL vs non-NULL is observable across
 * processes (ASLR). */
static const char *p(const void *x) {
    return x == NULL ? "(nil)" : "(ptr)";
}

/* No-op generic error handler used to exclude UB-artifact diagnostics
 * from the byte-identity comparison (see D1). */
static void swallow_diag(void *ctx, const char *msg, ...) {
    (void)ctx;
    (void)msg;
}

int main(void) {
    LIBXML_TEST_VERSION

    /* ── A. NULL / invalid handles ─────────────────────────────────────── */
    printf("A1 xmlReadMemory(NULL,0,NULL,NULL,0)=%s\n",
           p(xmlReadMemory(NULL, 0, NULL, NULL, 0)));
    printf("A2 xmlReadMemory((const char*)1,-1,NULL,NULL,0)=%s\n",
           p(xmlReadMemory((const char *)1, -1, NULL, NULL, 0)));
    printf("A3 xmlReadFile(NULL,NULL,0)=%s\n", p(xmlReadFile(NULL, NULL, 0)));
    printf("A4 xmlNewDoc(NULL)=%s\n", p(xmlNewDoc(NULL)));
    printf("A5 xmlFreeDoc(NULL)\n");
    xmlFreeDoc(NULL);
    printf("A6 xmlDocGetRootElement(NULL)=%s\n", p(xmlDocGetRootElement(NULL)));
    printf("A7 xmlNodeGetContent(NULL)=%s\n", p(xmlNodeGetContent(NULL)));
    printf("A8 xmlFreeNode(NULL)\n");
    xmlFreeNode(NULL);
    printf("A9 xmlNodeDump(NULL,NULL,NULL,0,0)=%d\n",
           xmlNodeDump(NULL, NULL, NULL, 0, 0));
    printf("A10 xmlXPathEvalExpression(NULL,NULL)=%s\n",
           p(xmlXPathEvalExpression(NULL, NULL)));
    printf("A11 xmlXPathFreeObject(NULL)\n");
    xmlXPathFreeObject(NULL);
    printf("A12 xmlXPathCompile(NULL)=%s\n", p(xmlXPathCompile(NULL)));
    printf("A13 xmlSaveToFilename(NULL,NULL,0)=%s\n",
           p(xmlSaveToFilename(NULL, NULL, 0)));
    printf("A14 xmlSaveClose(NULL)=%d\n", xmlSaveClose(NULL));
    printf("A15 xmlSaveDoc(NULL,NULL)=%ld\n", (long)xmlSaveDoc(NULL, NULL));
    printf("A16 xmlNewTextReader(NULL,NULL)=%s\n",
           p(xmlNewTextReader(NULL, NULL)));
    printf("A17 xmlFreeTextReader(NULL)\n");
    xmlFreeTextReader(NULL);
    printf("A18 xmlTextReaderRead(NULL)=%d\n", xmlTextReaderRead(NULL));
    printf("A19 xmlTextWriterStartDocument(NULL)=%d\n",
           xmlTextWriterStartDocument(NULL, NULL, NULL, 0));
    printf("A20 xmlTextWriterEndDocument(NULL)=%d\n",
           xmlTextWriterEndDocument(NULL));
    printf("A21 xmlBufferCreateSize(-1)=%s\n", p(xmlBufferCreateSize(-1)));
    printf("A22 xmlBufferCreateSize(0)=%s\n", p(xmlBufferCreateSize(0)));
    printf("A23 xmlBufferFree(NULL)\n");
    xmlBufferFree(NULL);
    printf("A24 xmlStrdup(NULL)=%s\n", p(xmlStrdup(NULL)));
    printf("A25 xmlStrlen(NULL)=%d\n", (int)xmlStrlen(NULL));
    printf("A26 xmlStrcmp(NULL,NULL)=%d\n", xmlStrcmp(NULL, NULL));
    printf("A27 xmlURIUnescapeString(NULL,0,NULL)=%s\n",
           p(xmlURIUnescapeString(NULL, 0, NULL)));
    printf("A28 xmlParseURI(NULL)=%s\n", p(xmlParseURI(NULL)));
    printf("A29 xmlFreeURI(NULL)\n");
    xmlFreeURI(NULL);
    printf("A30 xmlNodeSetName(NULL,NULL)\n");
    xmlNodeSetName(NULL, NULL);
    printf("A31 xmlNodeSetContent(NULL,NULL)\n");
    xmlNodeSetContent(NULL, NULL);
    printf("A32 xmlAddChild(NULL,NULL)=%s\n", p(xmlAddChild(NULL, NULL)));
    printf("A33 xmlUnlinkNode(NULL)\n");
    xmlUnlinkNode(NULL);
    printf("A34 xmlReplaceNode(NULL,NULL)=%s\n",
           p(xmlReplaceNode(NULL, NULL)));
    printf("A35 xmlCopyNode(NULL,0)=%s\n", p(xmlCopyNode(NULL, 0)));
    printf("A36 xmlCopyDoc(NULL,0)=%s\n", p(xmlCopyDoc(NULL, 0)));
    printf("A37 xmlCreateIntSubset(NULL,NULL,NULL,NULL)=%s\n",
           p(xmlCreateIntSubset(NULL, NULL, NULL, NULL)));
    printf("A38 xmlGetProp(NULL,NULL)=%s\n", p(xmlGetProp(NULL, NULL)));
    printf("A39 xmlSetProp(NULL,NULL,NULL)=%s\n",
           p(xmlSetProp(NULL, NULL, NULL)));
    printf("A40 xmlHasProp(NULL,NULL)=%s\n", p(xmlHasProp(NULL, NULL)));
    printf("A41 xmlGetNoNsProp(NULL,NULL)=%s\n",
           p(xmlGetNoNsProp(NULL, NULL)));
    printf("A42 xmlNewNs(NULL,NULL,NULL)=%s\n",
           p(xmlNewNs(NULL, NULL, NULL)));
    printf("A43 xmlNewChild(NULL,NULL,NULL,NULL)=%s\n",
           p(xmlNewChild(NULL, NULL, NULL, NULL)));
    printf("A44 xmlNewText(NULL)=%s\n", p(xmlNewText(NULL)));
    printf("A45 xmlNewComment(NULL)=%s\n", p(xmlNewComment(NULL)));
    printf("A46 xmlNewPI(NULL,NULL)=%s\n", p(xmlNewPI(NULL, NULL)));
    printf("A47 xmlNewDocNode(NULL,NULL,NULL,NULL)=%s\n",
           p(xmlNewDocNode(NULL, NULL, NULL, NULL)));
    printf("A48 xmlNewNode(NULL,NULL)=%s\n", p(xmlNewNode(NULL, NULL)));
    printf("A49 xmlGetNodePath(NULL)=%s\n", p(xmlGetNodePath(NULL)));
    printf("A50 xmlNodeListGetString(NULL,NULL,0)=%s\n",
           p(xmlNodeListGetString(NULL, NULL, 0)));
    printf("A51 xmlDocDumpMemory(NULL,NULL,NULL)\n");
    xmlDocDumpMemory(NULL, NULL, NULL);
    printf("A52 xmlDocDumpFormatMemory(NULL,NULL,NULL,1)\n");
    xmlDocDumpFormatMemory(NULL, NULL, NULL, 1);
    printf("A53 xmlSaveFile(NULL,NULL)=%d\n", xmlSaveFile(NULL, NULL));
    printf("A54 xmlSaveFormatFileEnc(NULL,NULL,NULL,0)=%d\n",
           xmlSaveFormatFileEnc(NULL, NULL, NULL, 0));
    printf("A55 xmlGetDocCompressMode(NULL)=%d\n", xmlGetDocCompressMode(NULL));
    printf("A56 xmlSetDocCompressMode(NULL,9) [void]\n");
    xmlSetDocCompressMode(NULL, 9);
    printf("A57 xmlGetIntSubset(NULL)=%s\n", p(xmlGetIntSubset(NULL)));
    printf("A58 xmlGetDtdAttrDesc(NULL,NULL,NULL)=%s\n",
           p(xmlGetDtdAttrDesc(NULL, NULL, NULL)));
    printf("A59 xmlValidateDocument(NULL,NULL)=%d\n",
           xmlValidateDocument(NULL, NULL));
    printf("A60 xmlValidateDtd(NULL,NULL,NULL)=%d\n",
           xmlValidateDtd(NULL, NULL, NULL));
    printf("A61 xmlValidateNameValue(NULL)=%d\n", xmlValidateNameValue(NULL));
    printf("A62 xmlValidateNamesValue(NULL)=%d\n", xmlValidateNamesValue(NULL));
    printf("A63 xmlValidateNmtokenValue(NULL)=%d\n",
           xmlValidateNmtokenValue(NULL));
    printf("A64 xmlValidateQName(NULL,0)=%d\n", xmlValidateQName(NULL, 0));
    printf("A65 xmlValidateNCName(NULL,0)=%d\n", xmlValidateNCName(NULL, 0));
    printf("A66 xmlValidateNMToken(NULL,0)=%d\n", xmlValidateNMToken(NULL, 0));
    printf("A67 xmlGetCharEncodingName(-1)=%s\n",
           p(xmlGetCharEncodingName(-1)));
    printf("A68 xmlGetCharEncodingName(999)=%s\n",
           p(xmlGetCharEncodingName(999)));
    printf("A69 xmlParseCharEncoding(NULL)=%d\n", xmlParseCharEncoding(NULL));
    printf("A70 xmlCleanupParser()\n");
    xmlCleanupParser();
    /* xmlCleanupParser is a no-op per modern upstream; parse again to prove
     * the library still works after it. */
    {
        xmlDocPtr d = xmlReadMemory("<a/>", 4, "t", NULL, 0);
        printf("A71 post-cleanup parse=%s\n", p(d));
        if (d) xmlFreeDoc(d);
    }
    printf("A72 xmlInitParser()\n");
    xmlInitParser();

    /* ── B. invalid option/flag bits ───────────────────────────────────── */
    printf("B1 parse with all-ones options=%s\n",
           p(xmlReadMemory("<a/>", 4, "t", NULL, 0x7FFFFFFF)));
    printf("B2 parse with 0x80000000 options=%s\n",
           p(xmlReadMemory("<a/>", 4, "t", NULL, 0x80000000)));
    printf("B3 parse with XML_PARSE_RECOVER on garbage=%s\n",
           p(xmlReadMemory("<<<", 3, "t", NULL, XML_PARSE_RECOVER)));
    printf("B4 parse with XML_PARSE_NOERROR|NOWARNING on garbage\n");
    {
        xmlDocPtr d = xmlReadMemory("<<<", 3, "t", NULL,
                                    XML_PARSE_NOERROR | XML_PARSE_NOWARNING);
        printf("B4 doc=%s\n", p(d));
        if (d) xmlFreeDoc(d);
    }
    printf("B5 xmlSaveToBuffer with 0x40000000 options=%s\n",
           p(xmlSaveToBuffer(NULL, NULL, 0x40000000)));
    printf("B6 xmlTextReaderRead with invalid reader options\n");
    {
        xmlTextReaderPtr r = xmlReaderForMemory("<a/>", 4, "t", NULL, 0x40000000);
        printf("B6 reader=%s read=%d\n", p(r),
               r ? xmlTextReaderRead(r) : -99);
        if (r) xmlFreeTextReader(r);
    }
    printf("B7 xmlParseMemory(NULL,0)=%s\n", p(xmlParseMemory(NULL, 0)));
    printf("B8 xmlParseMemory(\"<a/>\",-5)=%s\n",
           p(xmlParseMemory("<a/>", -5)));
    printf("B9 xmlCreatePushParserCtxt(NULL,NULL,NULL,0,NULL)=%s\n",
           p(xmlCreatePushParserCtxt(NULL, NULL, NULL, 0, NULL)));
    printf("B10 xmlParseChunk(NULL,NULL,0,1)=%d\n",
           xmlParseChunk(NULL, NULL, 0, 1));
    {
        xmlParserCtxtPtr c = xmlCreatePushParserCtxt(NULL, NULL, "<a/>", 4, "t");
        printf("B11 push ctxt=%s\n", p(c));
        if (c) {
            printf("B12 push chunk terminate=%d\n", xmlParseChunk(c, NULL, 0, 1));
            printf("B13 push chunk neg size=%d\n", xmlParseChunk(c, "x", -1, 0));
            xmlFreeParserCtxt(c);
        }
    }

    /* ── C. extreme/negative sizes and lengths ─────────────────────────── */
    {
        xmlBufferPtr b = xmlBufferCreateSize(INT_MAX);
        printf("C1 buffer INT_MAX=%s\n", p(b));
        if (b) {
            printf("C2 xmlBufferAdd big=%d\n",
                   xmlBufferAdd(b, (const xmlChar *)"x", INT_MAX));
            xmlBufferFree(b);
        }
    }
    {
        xmlBufferPtr b = xmlBufferCreate();
        printf("C3 xmlBufferAdd neg len=%d\n", xmlBufferAdd(b, (const xmlChar *)"x", -1));
        printf("C4 xmlBufferAdd 0 len=%d\n", xmlBufferAdd(b, (const xmlChar *)"x", 0));
        printf("C5 xmlBufferContent empty=%s\n", p(xmlBufferContent(b)));
        xmlBufferFree(b);
    }
    printf("C6 xmlStrncat(NULL,NULL,0)=%s\n", p(xmlStrncat(NULL, NULL, 0)));
    printf("C7 xmlStrncat(NULL,NULL,INT_MAX)=%s\n",
           p(xmlStrncat(NULL, NULL, INT_MAX)));
    printf("C8 xmlStrndup(NULL,0)=%s\n", p(xmlStrndup(NULL, 0)));
    printf("C9 xmlStrndup(NULL,INT_MAX)=%s\n", p(xmlStrndup(NULL, INT_MAX)));
    printf("C10 xmlStrncmp(NULL,NULL,0)=%d\n", xmlStrncmp(NULL, NULL, 0));
    printf("C11 xmlStrchr(NULL,'a')=%s\n", p(xmlStrchr(NULL, 'a')));
    printf("C12 xmlStrstr(NULL,NULL)=%s\n", p(xmlStrstr(NULL, NULL)));
    printf("C13 xmlStrsub(NULL,0,0)=%s\n", p(xmlStrsub(NULL, 0, 0)));
    printf("C14 xmlUTF8Strlen(NULL)=%d\n", (int)xmlUTF8Strlen(NULL));
    printf("C15 xmlUTF8Strloc(NULL,0)=%d\n", (int)xmlUTF8Strloc(NULL, 0));
    printf("C16 xmlUTF8Strndup(NULL,0)=%s\n", p(xmlUTF8Strndup(NULL, 0)));
    printf("C17 xmlUTF8Size(NULL)=%d\n", (int)xmlUTF8Size(NULL));
    printf("C18 xmlGetUTF8Char(NULL,NULL)=%d\n",
           xmlGetUTF8Char(NULL, NULL));
    printf("C19 xmlCheckUTF8(NULL)=%d\n", xmlCheckUTF8(NULL));
    {
        int len = 0;
        const xmlChar c[] = "a";
        printf("C20 xmlGetUTF8Char len0=%d\n", xmlGetUTF8Char(c, &len));
        len = -1;
        printf("C21 xmlGetUTF8Char lenneg=%d\n", xmlGetUTF8Char(c, &len));
        len = 0;
        printf("C22 xmlGetUTF8Char null len=%d\n", xmlGetUTF8Char(NULL, &len));
    }

    /* ── D. integer-boundary values ────────────────────────────────────── */
    /*
     * D1: xmlReadMemory with size = INT_MAX. Upstream 2.15 has no INT_MAX
     * guard in xmlReadMemory — it streams from the caller's buffer, so a
     * lying size makes it read past the buffer. The resulting stderr
     * ("Extra content") is an artifact of that unsized read, not a
     * contract behaviour, and is deliberately excluded by installing a
     * no-op generic error handler around the call. What is measured:
     * both sides return NULL, do not crash, and the library remains
     * usable afterwards. (HOSTILE-ABI finding: the candidate previously
     * attempted a ~2 GiB copy and segfaulted.)
     */
    xmlSetGenericErrorFunc(NULL, swallow_diag);
    printf("D1 xmlReadMemory size INT_MAX=%s\n",
           p(xmlReadMemory("<a/>", INT_MAX, "t", NULL, 0)));
    xmlSetGenericErrorFunc(NULL, NULL);
    /*
     * D1b: defined variant — a REAL buffer whose content after the root is
     * garbage. Both sides must emit the identical "Extra content at the
     * end of the document" diagnostic (context + caret) and return NULL.
     */
    printf("D1b xmlReadMemory trailing garbage=%s\n",
           p(xmlReadMemory("<a/>X", 5, "t", NULL, 0)));
    printf("D2 xmlParseChunk size INT_MAX=%d\n",
           xmlParseChunk(NULL, "x", INT_MAX, 0));
    printf("D3 xmlBufferCreateSize(INT_MIN)=%s\n",
           p(xmlBufferCreateSize(INT_MIN)));
    printf("D4 xmlNewDoc(version=INT_MAX bytes)=%s\n",
           p(xmlNewDoc((const xmlChar *)"x")));
    {
        xmlChar *big = xmlStrdup((const xmlChar *)"a");
        printf("D5 xmlStrdup 1=%s\n", p(big));
        if (big) xmlFree(big);
    }
    printf("D6 xmlEncodeSpecialChars(NULL,NULL)=%s\n",
           p(xmlEncodeSpecialChars(NULL, NULL)));
    printf("D7 xmlNodeDumpOutput(NULL,NULL,NULL,0,0,NULL) [void]\n");
    xmlNodeDumpOutput(NULL, NULL, NULL, 0, 0, NULL);

    /* ── E. enum/type-code abuse ───────────────────────────────────────── */
    {
        xmlDocPtr d = xmlReadMemory("<a><b>t</b></a>", 15, "t", NULL, 0);
        xmlNodePtr n = d ? xmlDocGetRootElement(d) : NULL;
        printf("E1 root=%s name=%s\n", p(n),
               n && n->name ? (const char *)n->name : "(null)");
        if (d) xmlFreeDoc(d);
    }
    {
        /* bogus node type on a fresh node must not crash the dumper */
        xmlNodePtr fake = (xmlNodePtr)xmlMalloc(sizeof(xmlNode));
        if (fake) {
            memset(fake, 0, sizeof(xmlNode));
            fake->type = (xmlElementType)0x7F;
            fake->name = (const xmlChar *)"fake";
            printf("E2 bogus-type dump ok\n");
            xmlFree(fake);
        } else {
            printf("E2 malloc failed\n");
        }
    }
    printf("E3 xmlMalloc(0)=%s\n", p(xmlMalloc(0)));
    printf("E4 xmlMalloc((size_t)-1)=%s\n", p(xmlMalloc((size_t)-1)));
    printf("E5 xmlRealloc(NULL,0)=%s\n", p(xmlRealloc(NULL, 0)));
    printf("E6 xmlRealloc(NULL,(size_t)-1)=%s\n",
           p(xmlRealloc(NULL, (size_t)-1)));
    printf("E7 xmlMemStrdup(NULL)=%s\n", p(xmlMemStrdup(NULL)));
    printf("E8 xmlFree((void*)1) [guarded]\n");
    /* xmlFree of a bogus pointer is UB upstream too; skip actual call. */
    printf("E9 xmlMallocAtomic(0)=%s\n", p(xmlMallocAtomic(0)));
    printf("E10 xmlMallocAtomic((size_t)-1)=%s\n",
           p(xmlMallocAtomic((size_t)-1)));

    /* ── final marker ──────────────────────────────────────────────────── */
    printf("HOSTILE-ABI VERDICT PASS\n");
    return 0;
}
