/*
 * READER-001 — differential probe of the xmlTextReader* family additions
 * (11.1-I reader closure).
 *
 * Compiled twice (oracle DSO vs candidate DSO); output must be byte-identical.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/xmlreader.h>
#include <libxml/tree.h>

static void p(const char *name, const xmlChar *v) {
    printf("%s=", name);
    if (v == NULL) { printf("(null)\n"); return; }
    printf("%s\n", (const char *) v);
}

int main(void) {
    static const char doc[] =
        "<?xml version=\"1.0\"?>\n"
        "<root xmlns:x=\"urn:x\">\n"
        "  <x:child id=\"1\" attr=\"v\" x:a=\"nsval\">text</x:child>\n"
        "  <empty/>\n"
        "</root>\n";

    /* 1. xmlReaderForDoc + navigation + const accessors */
    xmlTextReaderPtr r = xmlReaderForDoc((const xmlChar *) doc, "http://base/root.xml",
                                         NULL, XML_PARSE_NOENT);
    printf("reader=%s\n", r ? "ok" : "null");
    int t;
    while ((t = xmlTextReaderRead(r)) == 1) {
        int nt = xmlTextReaderNodeType(r);
        if (nt == 1 || nt == 15) {
            printf("node=%s local=%s prefix=%s depth=%d\n",
                   (const char *) xmlTextReaderConstName(r),
                   (const char *) xmlTextReaderConstLocalName(r),
                   (const char *) xmlTextReaderConstPrefix(r),
                   xmlTextReaderDepth(r));
            printf("  uri=%s lang=%s ver=%s base=%s enc=%s\n",
                   (const char *) xmlTextReaderConstNamespaceUri(r),
                   (const char *) xmlTextReaderConstXmlLang(r),
                   (const char *) xmlTextReaderConstXmlVersion(r),
                   (const char *) xmlTextReaderConstBaseUri(r),
                   (const char *) xmlTextReaderConstEncoding(r));
            if (nt == 1 && xmlTextReaderHasAttributes(r) > 0) {
                int q = xmlTextReaderMoveToFirstAttribute(r);
                printf("  first-attr=%d\n", q);
                while (q == 1) {
                    printf("    attr name=%s value=%s quote=%d uri=%s local=%s prefix=%s\n",
                           (const char *) xmlTextReaderConstName(r),
                           (const char *) xmlTextReaderValue(r),
                           xmlTextReaderQuoteChar(r),
                           (const char *) xmlTextReaderConstNamespaceUri(r),
                           (const char *) xmlTextReaderConstLocalName(r),
                           (const char *) xmlTextReaderConstPrefix(r));
                    q = xmlTextReaderMoveToNextAttribute(r);
                }
            }
            if (nt == 1 && strcmp((const char *) xmlTextReaderConstName(r), "empty") == 0) {
                printf("  is-empty=%d\n", xmlTextReaderIsEmptyElement(r));
            }
        } else if (nt == 3 || nt == 8 || nt == 14) {
            printf("node=%s type=%d\n",
                   (const char *) xmlTextReaderConstName(r), nt);
        }
    }
    printf("read-end=%d\n", t);

    /* 2. xmlReaderWalker */
    xmlDocPtr d = xmlReadMemory(doc, (int) strlen(doc), "walk.xml", NULL, 0);
    xmlTextReaderPtr w = xmlReaderWalker(d);
    printf("walker=%s\n", w ? "ok" : "null");
    int cnt = 0;
    while (xmlTextReaderRead(w) == 1) {
        cnt++;
    }
    printf("walker-count=%d\n", cnt);
    xmlFreeTextReader(w);
    xmlFreeDoc(d);

    /* 3. New* constructor on a NULL reader */
    xmlTextReaderPtr r2 = NULL;
    int nr = xmlReaderNewMemory(r2, doc, (int) strlen(doc), "mem.xml", NULL, 0);
    printf("newmem-ret=%d\n", nr);
    if (r2 != NULL) {
        xmlTextReaderRead(r2);
        p("newmem-name", xmlTextReaderConstName(r2));
    }
    xmlFreeTextReader(r2);

    /* 4. error handler plumbing */
    xmlTextReaderPtr r3 = xmlReaderForDoc((const xmlChar *) doc, NULL, NULL, 0);
    xmlTextReaderSetMaxAmplification(r3, 100);
    const xmlError *le = xmlTextReaderGetLastError(r3);
    printf("last-error=%s\n", le ? (le->message ? le->message : "(no msg)") : "(null)");
    printf("bconsumed=%ld remainder=%s\n", xmlTextReaderByteConsumed(r3),
           xmlTextReaderGetRemainder(r3) ? "nonnull" : "null");
    xmlFreeTextReader(r3);

    /* 5. ns-decl vs property attribute semantics */
    int rc;
    xmlTextReaderPtr r4 = xmlReaderForDoc((const xmlChar *) doc, NULL, NULL, 0);
    while (xmlTextReaderRead(r4) == 1) {
        if (xmlTextReaderNodeType(r4) == 1 &&
            xmlTextReaderConstName(r4) &&
            strcmp((const char *) xmlTextReaderConstName(r4), "x:child") == 0)
            break;
    }
    printf("nsattr-count=%d\n", xmlTextReaderAttributeCount(r4));
    printf("nsattr-move-first=%d\n", xmlTextReaderMoveToFirstAttribute(r4));
    printf("nsattr-name=%s nsdecl=%d value=%s\n",
           (const char *) xmlTextReaderConstName(r4),
           xmlTextReaderIsNamespaceDecl(r4),
           (const char *) xmlTextReaderValue(r4));
    printf("nsattr-move-next=%d nsdecl=%d\n",
           xmlTextReaderMoveToNextAttribute(r4),
           xmlTextReaderIsNamespaceDecl(r4));
    printf("nsattr-move-element=%d\n", xmlTextReaderMoveToElement(r4));
    /* MoveToAttributeNs: xmlns namespace searches ns-decls */
    rc = xmlTextReaderMoveToAttributeNs(r4, (const xmlChar *) "x",
                                        (const xmlChar *) "http://www.w3.org/2000/xmlns/");
    printf("nsattr-movens-xmlns=%d name=%s\n", rc,
           (const char *) xmlTextReaderConstName(r4));
    rc = xmlTextReaderMoveToAttributeNs(r4, (const xmlChar *) "xmlns",
                                        (const xmlChar *) "http://www.w3.org/2000/xmlns/");
    printf("nsattr-movens-xmlns-default=%d name=%s\n", rc,
           (const char *) xmlTextReaderConstName(r4));
    /* MoveToAttributeNs: real namespace searches properties */
    rc = xmlTextReaderMoveToAttributeNs(r4, (const xmlChar *) "a",
                                        (const xmlChar *) "urn:x");
    printf("nsattr-movens-prop=%d name=%s value=%s\n", rc,
           (const char *) xmlTextReaderConstName(r4),
           (const char *) xmlTextReaderValue(r4));
    rc = xmlTextReaderMoveToAttributeNs(r4, (const xmlChar *) "id", NULL);
    printf("nsattr-movens-null-uri=%d\n", rc);
    rc = xmlTextReaderMoveToAttributeNs(r4, (const xmlChar *) "id",
                                        (const xmlChar *) "urn:x");
    printf("nsattr-movens-miss=%d\n", rc);
    /* MoveToAttribute by qualified name */
    rc = xmlTextReaderMoveToAttribute(r4, (const xmlChar *) "id");
    printf("nsattr-moveattr=%d isdefault=%d\n", rc,
           xmlTextReaderIsDefault(r4));
    xmlFreeTextReader(r4);

    return 0;
}
