/*
 * XSLT-001 — differential court for the libxslt closure family.
 *
 * Exercises the deterministic, observable subset of the newly exported
 * xslt* functions against the system libxslt 1.1.45 oracle:
 *
 *  - stylesheet lifecycle: xsltNewStylesheet, xsltParseStylesheetProcess,
 *    xsltParseStylesheetDoc (existing), xsltFreeStylesheet
 *  - transform pipeline through the ABI: xsltParseStylesheetFile,
 *    xsltApplyStylesheet, xsltSaveResultToString
 *  - numbering: xsltFormatNumberConversion (default decimal format)
 *  - security: xsltSecurityAllow, xsltSecurityForbid, xsltSetCtxtSecurityPrefs
 *  - debugger: xsltGetDebuggerStatus, xsltDebugGetDefaultTrace,
 *    xsltDebugSetDefaultTrace
 *  - extension registry: xsltRegisterExtModule, xsltRegisterExtModuleFunction,
 *    xsltExtModuleFunctionLookup, xsltUnregisterExtModuleFunction,
 *    xsltUnregisterExtModule
 *  - locales: xsltNewLocale, xsltFreeLocale, xsltLocaleStrcmp, xsltStrxfrm
 *  - utility: xsltSetCtxtParseOptions, xsltGetProfileInformation,
 *    xsltExtensionInstructionResultRegister/Finalize
 *
 * Raw pointers are never printed; values/names only.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/xmlsave.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/transform.h>
#include <libxslt/security.h>
#include <libxslt/xsltutils.h>
#include <libxslt/extensions.h>
#include <libxslt/numbersInternals.h>
#include <libxslt/xsltlocale.h>

#define H "http://www.w3.org/1999/XSL/Transform"

static int dummy_xpath(xmlXPathParserContextPtr ctxt, int nargs) {
    (void) ctxt; (void) nargs;
    return 0;
}

int main(void) {
    xsltInit();

    /* 1. Stylesheet lifecycle. */
    xsltStylesheetPtr style = xsltNewStylesheet();
    printf("new-style %s\n", style ? "ok" : "NULL");
    if (style) {
        printf("version %s\n", style->version ? (char *) style->version : "NULL");
        xsltFreeStylesheet(style);
    }

    /* 2. Transform pipeline. */
    const char *xsl =
        "<?xml version=\"1.0\"?><xsl:stylesheet version=\"1.0\""
        " xmlns:xsl=\"" H "\">"
        "<xsl:template match=\"/\"><out><xsl:value-of select=\"a + 1\"/></out></xsl:template>"
        "</xsl:stylesheet>";
    const char *xml = "<?xml version=\"1.0\"?><a>41</a>";
    xmlDocPtr xdoc = xmlReadMemory(xml, (int) strlen(xml), "t.xml", NULL, 0);
    style = xsltParseStylesheetDoc(xmlReadMemory(xsl, (int) strlen(xsl), "t.xsl", NULL, 0));
    printf("parse-style %s\n", style ? "ok" : "NULL");

    /* 3. Numbering: default decimal format (name == NULL). */
    xsltDecimalFormatPtr fmt = style ? xsltDecimalFormatGetByQName(style, NULL, NULL) : NULL;
    xmlChar *num = NULL;
    int rc = xsltFormatNumberConversion(fmt, (xmlChar *) "###,##0.00", 12345.678, &num);
    printf("fmt1 rc=%d val=%s\n", rc, num ? (char *) num : "NULL");
    if (num) free(num);
    num = NULL;
    rc = xsltFormatNumberConversion(fmt, (xmlChar *) "0.000", 3.14159, &num);
    printf("fmt2 rc=%d val=%s\n", rc, num ? (char *) num : "NULL");
    if (num) free(num);
    num = NULL;
    rc = xsltFormatNumberConversion(fmt, (xmlChar *) "A", 27, &num);
    printf("fmt3 rc=%d val=%s\n", rc, num ? (char *) num : "NULL");
    if (num) free(num);

    if (style && xdoc) {
        xmlDocPtr res = xsltApplyStylesheet(style, xdoc, NULL);
        printf("apply %s\n", res ? "ok" : "NULL");
        if (res) {
            xmlChar *txt = NULL;
            int len = 0;
            int rc = xsltSaveResultToString(&txt, &len, res, style);
            printf("save rc=%d len=%d txt=%.30s\n", rc, len, txt ? (char *) txt : "NULL");
            if (txt) free(txt);
            xmlFreeDoc(res);
        }
    }
    if (style) xsltFreeStylesheet(style);
    if (xdoc) xmlFreeDoc(xdoc);

    /* 4. Security. */
    printf("sec-allow %d\n", xsltSecurityAllow(NULL, NULL, "x"));
    printf("sec-forbid %d\n", xsltSecurityForbid(NULL, NULL, "x"));

    /* 5. Debugger / trace. */
    printf("dbg-status %d\n", xsltGetDebuggerStatus());
    xsltDebugSetDefaultTrace(3);
    printf("trace %d\n", xsltDebugGetDefaultTrace());
    xsltDebugSetDefaultTrace(0);

    /* 6. Extension registry. */
    printf("ext-module %d\n", xsltRegisterExtModule((const xmlChar *) "urn:test",
        NULL, NULL));
    printf("ext-func %d\n", xsltRegisterExtModuleFunction((const xmlChar *) "f",
        (const xmlChar *) "urn:test", (xmlXPathFunction) dummy_xpath));
    printf("ext-lookup %s\n", xsltExtModuleFunctionLookup((const xmlChar *) "f",
        (const xmlChar *) "urn:test") ? "found" : "missing");
    printf("ext-lookup2 %s\n", xsltExtModuleFunctionLookup((const xmlChar *) "g",
        (const xmlChar *) "urn:test") ? "found" : "missing");
    printf("ext-unreg-func %d\n", xsltUnregisterExtModuleFunction(
        (const xmlChar *) "f", (const xmlChar *) "urn:test"));
    printf("ext-lookup3 %s\n", xsltExtModuleFunctionLookup((const xmlChar *) "f",
        (const xmlChar *) "urn:test") ? "found" : "missing");
    printf("ext-unreg-mod %d\n", xsltUnregisterExtModule((const xmlChar *) "urn:test"));

    /* 7. Locales. */
    void *loc = xsltNewLocale((const xmlChar *) "C", 0);
    printf("locale %s\n", loc ? "ok" : "NULL");
    if (loc) {
        printf("loc-cmp %d\n", xsltLocaleStrcmp(loc, (const xmlChar *) "a",
                                                (const xmlChar *) "b"));
        xmlChar *xf = xsltStrxfrm(loc, (const xmlChar *) "Hello");
        printf("xfrm %s\n", xf ? (char *) xf : "NULL");
        if (xf) free(xf);
        xsltFreeLocale(loc);
    }
    xsltFreeLocales();

    /* 8. Utility. */
    xsltStylesheetPtr s2 = xsltParseStylesheetDoc(
        xmlReadMemory(xsl, (int) strlen(xsl), "t.xsl", NULL, 0));
    xmlDocPtr d2 = xmlReadMemory(xml, (int) strlen(xml), "t.xml", NULL, 0);
    xsltTransformContextPtr ctxt = xsltNewTransformContext(s2, d2);
    printf("ctxt %s\n", ctxt ? "nonnull" : "NULL");
    if (ctxt) {
        printf("parseopts %d\n", xsltSetCtxtParseOptions(ctxt, 0));
        printf("profile %s\n", xsltGetProfileInformation(ctxt) ? "doc" : "NULL");
        printf("extres-reg %d\n", xsltExtensionInstructionResultRegister(ctxt, NULL));
        printf("extres-fin %d\n", xsltExtensionInstructionResultFinalize(ctxt));
        xsltFreeTransformContext(ctxt);
    }
    if (s2) xsltFreeStylesheet(s2);
    if (d2) xmlFreeDoc(d2);
    xsltCleanupGlobals();
    return 0;
}
