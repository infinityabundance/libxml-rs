/*
 * DATA-GLOBALS-001 — differential probe of the exported libxml2 data globals
 * and chvalid functions (11.1-G/I data-ABI closure, residual R-000135).
 *
 * Compiled twice: against the oracle DSO (system libxml2) and against the
 * candidate DSO (liblibxml_rs). The output is deterministic (no addresses,
 * no function-pointer VALUES — only NULL/non-NULL slot patterns), so the two
 * runs must be byte-identical.
 *
 * Prints:
 *   - xmlIsPubidChar_tab[256] as hex
 *   - every xmlIs*Group range table (counts + all ranges)
 *   - xmlDefaultSAXHandler / htmlDefaultSAXHandler slot patterns + initialized
 *   - xmlDefaultSAXLocator slot pattern
 *   - xmlLastError initial state (must be all-zero)
 *   - FNV-1a 64 hash of xmlIs{BaseChar,Blank,Char,Combining,Digit,Extender,
 *     Ideographic,PubidChar,Letter} over 0x0..0xFFFF + supplementary samples
 *   - xmlIsBlankNode on a whitespace text node
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxml/xmlstring.h>
#include <libxml/chvalid.h>
#include <libxml/HTMLparser.h>

/* xmlIsLetter lives in parserInternals.h (not installed); it is exported. */
XMLPUBFUN int xmlIsLetter(int c);

static uint64_t fnv(const unsigned char *p, size_t n) {
    uint64_t h = 1469598103934665603ULL;
    for (size_t i = 0; i < n; i++) {
        h ^= p[i];
        h *= 1099511628211ULL;
    }
    return h;
}

static void sweep(const char *name, int (*f)(unsigned int)) {
    /* one result byte per code point: compact + deterministic */
    static unsigned char buf[0x10000 + 17];
    for (unsigned int c = 0; c < 0x10000; c++)
        buf[c] = (unsigned char) f(c);
    /* supplementary-plane samples: every 0x1000th plus edges */
    for (unsigned int c = 0x10000; c <= 0x110000; c += 0x1000)
        buf[0x10000 + (c - 0x10000) / 0x1000] = (unsigned char) f(c);
    printf("%s fnv=%016llx\n", name,
           (unsigned long long) fnv(buf, 0x10000 + 17));
}

static void handler_pattern(const char *name, const xmlSAXHandlerV1 *h) {
    const void **slots = (const void **) h;
    printf("%s initialized=%u pattern=", name, h->initialized);
    /* 27 function-pointer slots precede initialized */
    for (int i = 0; i < 27; i++)
        printf("%d", slots[i] != NULL);
    printf("\n");
}

int main(void) {
    printf("pubid=");
    for (int i = 0; i < 256; i++)
        printf("%02x", xmlIsPubidChar_tab[i]);
    printf("\n");

    const struct { const char *n; const xmlChRangeGroup *g; } groups[] = {
        {"base", &xmlIsBaseCharGroup}, {"char", &xmlIsCharGroup},
        {"combining", &xmlIsCombiningGroup}, {"digit", &xmlIsDigitGroup},
        {"extender", &xmlIsExtenderGroup}, {"ideographic", &xmlIsIdeographicGroup},
    };
    for (size_t gi = 0; gi < sizeof(groups) / sizeof(groups[0]); gi++) {
        const xmlChRangeGroup *g = groups[gi].g;
        printf("group.%s short=%d long=%d", groups[gi].n,
               g->nbShortRange, g->nbLongRange);
        for (int i = 0; i < g->nbShortRange; i++)
            printf(" [%x-%x]", g->shortRange[i].low, g->shortRange[i].high);
        for (int i = 0; i < g->nbLongRange; i++)
            printf(" L[%x-%x]", g->longRange[i].low, g->longRange[i].high);
        printf("\n");
    }

    handler_pattern("xmlDefaultSAXHandler", &xmlDefaultSAXHandler);
    handler_pattern("htmlDefaultSAXHandler", &htmlDefaultSAXHandler);
    printf("xmlDefaultSAXLocator pattern=%d%d%d%d\n",
           xmlDefaultSAXLocator.getPublicId != NULL,
           xmlDefaultSAXLocator.getSystemId != NULL,
           xmlDefaultSAXLocator.getLineNumber != NULL,
           xmlDefaultSAXLocator.getColumnNumber != NULL);

    const unsigned char *le = (const unsigned char *) &xmlLastError;
    int nonzero = 0;
    for (size_t i = 0; i < sizeof(xmlLastError); i++)
        nonzero |= le[i];
    printf("xmlLastError sizeof=%zu allzero=%d\n", sizeof(xmlLastError), !nonzero);

    sweep("xmlIsBaseChar", xmlIsBaseChar);
    sweep("xmlIsBlank", xmlIsBlank);
    sweep("xmlIsChar", xmlIsChar);
    sweep("xmlIsCombining", xmlIsCombining);
    sweep("xmlIsDigit", xmlIsDigit);
    sweep("xmlIsExtender", xmlIsExtender);
    sweep("xmlIsIdeographic", xmlIsIdeographic);
    sweep("xmlIsPubidChar", xmlIsPubidChar);
    sweep("xmlIsLetter", (int (*)(unsigned int)) xmlIsLetter);

    xmlNode node;
    memset(&node, 0, sizeof(node));
    node.type = XML_TEXT_NODE;
    node.content = (xmlChar *) " \t\n\r";
    printf("xmlIsBlankNode ws=%d\n", xmlIsBlankNode(&node));
    node.content = (xmlChar *) " x";
    printf("xmlIsBlankNode nonws=%d\n", xmlIsBlankNode(&node));
    return 0;
}
