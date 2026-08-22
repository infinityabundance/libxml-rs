/**
 * @file ABI-SYMBOL-0001-symbolcheck.c
 * @brief ABI probe: verify symbol presence for key libxml-2 APIs.
 *
 * Court Casefile: ABI-SYMBOL-0001
 * Description:   Symbol visibility ABI compliance check.
 *                Declares extern function pointers for core libxml-2
 *                APIs, calls version / init / cleanup functions at
 *                runtime to confirm they are linkable and executable.
 *                Compilation failure indicates a missing or renamed
 *                symbol; runtime failure indicates a broken implementation.
 *
 * Build:
 *   Oracle mode (link system libxml2):
 *     gcc -o symbolcheck-oracle ABI-SYMBOL-0001-symbolcheck.c \
 *         -lxml2 -lxslt
 *
 *   Candidate mode (our headers only, no link):
 *     gcc -fsyntax-only -c ABI-SYMBOL-0001-symbolcheck.c \
 *         -I /path/to/include
 *
 * Usage:
 *   ./symbolcheck-oracle
 *
 * Output: Structured JSON-like lines.  Return code 0 on success.
 */

#include <stddef.h>
#include <stdio.h>
#include <libxml/tree.h>
#include <libxml/dict.h>
#include <libxml/hash.h>
#include <libxml/parser.h>
#include <libxml/xpath.h>
#include <libxml/xmlversion.h>
#include <libxml/xmlerror.h>
#include <libxml/valid.h>
#include <libxml/SAX2.h>
#include <libxml/xmlreader.h>
#include <libxml/xmlwriter.h>
#include <libxml/xinclude.h>
#include <libxml/c14n.h>
#include <libxml/uri.h>
#include <libxml/encoding.h>
#include <libxml/list.h>

/* BAD_CAST is not always provided by xmlstring.h; define if missing */
#ifndef BAD_CAST
#define BAD_CAST (const xmlChar *)
#endif

/* ------------------------------------------------------------------ */
/*  Print version info and exercise a few runtime-resolvable calls     */
/* ------------------------------------------------------------------ */
static int exercise_runtime_symbols(void)
{
    int failures = 0;

    printf("  \"version_check\": {\n");
    printf("    \"LIBXML_DOTTED_VERSION\": \"%s\",\n",   LIBXML_DOTTED_VERSION);
    printf("    \"LIBXML_VERSION\": %d,\n",               LIBXML_VERSION);
    printf("    \"LIBXML_VERSION_STRING\": \"%s\"\n",   LIBXML_VERSION_STRING);
#ifdef LIBXSLT_DOTTED_VERSION
    printf("    \"LIBXSLT_DOTTED_VERSION\": \"%s\"\n",   LIBXSLT_DOTTED_VERSION);
#else
    printf("    \"LIBXSLT_DOTTED_VERSION\": \"(not available)\"\n");
#endif
    printf("  },\n");

    printf("  \"runtime_calls\": {\n");

    /* xmlInitParser / xmlCleanupParser (safe to call multiple times) */
    xmlInitParser();
    printf("    \"xmlInitParser\": \"ok\",\n");
    printf("    \"xmlIsInitialized\": \"(runtime check not available in all libxml2 versions)\",\n");

    /* xmlNewDoc / xmlFreeDoc */
    {
        xmlDocPtr d = xmlNewDoc(BAD_CAST "1.0");
        if (d) {
            printf("    \"xmlNewDoc\": \"ok\",\n");
            xmlFreeDoc(d);
            printf("    \"xmlFreeDoc\": \"ok\",\n");
        } else {
            printf("    \"xmlNewDoc\": \"FAIL (NULL)\",\n");
            failures++;
        }
    }

    /* xmlNewNode / xmlFreeNode */
    {
        xmlNodePtr n = xmlNewNode(NULL, BAD_CAST "test");
        if (n) {
            printf("    \"xmlNewNode\": \"ok\",\n");
            xmlFreeNode(n);
            printf("    \"xmlFreeNode\": \"ok\",\n");
        } else {
            printf("    \"xmlNewNode\": \"FAIL (NULL)\",\n");
            failures++;
        }
    }

    /* xmlNewText / xmlFreeNodeList */
    {
        xmlNodePtr t = xmlNewText(BAD_CAST "hello");
        if (t) {
            printf("    \"xmlNewText\": \"ok\",\n");
            xmlFreeNodeList(t);
            printf("    \"xmlFreeNodeList\": \"ok\",\n");
        } else {
            printf("    \"xmlNewText\": \"FAIL (NULL)\",\n");
            failures++;
        }
    }

    /* xmlBufferCreate / xmlBufferFree */
    {
        xmlBufferPtr b = xmlBufferCreate();
        if (b) {
            printf("    \"xmlBufferCreate\": \"ok\",\n");
            xmlBufferEmpty(b);
            printf("    \"xmlBufferEmpty\": \"ok\",\n");
            xmlBufferFree(b);
            printf("    \"xmlBufferFree\": \"ok\",\n");
        } else {
            printf("    \"xmlBufferCreate\": \"FAIL (NULL)\",\n");
            failures++;
        }
    }

    /* xmlSetGenericErrorFunc / xmlResetLastError */
    xmlSetGenericErrorFunc(NULL, NULL);
    printf("    \"xmlSetGenericErrorFunc\": \"ok\",\n");
    xmlResetLastError();
    printf("    \"xmlResetLastError\": \"ok\",\n");

    xmlCleanupParser();
    printf("    \"xmlCleanupParser\": \"ok\"\n");
    printf("  },\n");

    return failures;
}

/* ------------------------------------------------------------------ */
/*  main                                                               */
/* ------------------------------------------------------------------ */
int main(void)
{
    int failures = 0;

    printf("{\n");
    printf("  \"probe\": \"ABI-SYMBOL-0001\",\n");
    printf("  \"description\": \"Symbol presence and runtime resolution\",\n");
    printf("  \"library_version\": \"" LIBXML_DOTTED_VERSION "\",\n");

    failures += exercise_runtime_symbols();

    printf("  \"result\": \"%s\"\n", failures == 0 ? "PASS" : "FAIL");
    printf("}\n");

    return failures;
}
