#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

/* Mirror php's dom_reconcile_ns + php_libxml_set_old_ns + xmlReconciliateNs
 * exactly, to find where the candidate diverges from the oracle for the
 * gh11500 createElementNS/appendChild sequence. */

static void set_old_ns(xmlDocPtr doc, xmlNsPtr ns) {
    if (doc->oldNs == NULL) {
        doc->oldNs = xmlMalloc(sizeof(xmlNs));
        memset(doc->oldNs, 0, sizeof(xmlNs));
        doc->oldNs->type = XML_LOCAL_NAMESPACE;
        doc->oldNs->href = xmlStrdup(XML_XML_NAMESPACE);
        doc->oldNs->prefix = xmlStrdup((const xmlChar *) "xml");
    } else {
        ns->next = doc->oldNs->next;
    }
    doc->oldNs->next = ns;
}

static void reconcile_internal(xmlDocPtr doc, xmlNodePtr nodep, xmlNodePtr search_parent) {
    if (nodep->nsDef == NULL) return;
    xmlNsPtr nsptr, nsdftptr, curns, prevns = NULL;
    curns = nodep->nsDef;
    while (curns) {
        nsdftptr = curns->next;
        if (curns->href != NULL) {
            if ((nsptr = xmlSearchNsByHref(doc, search_parent, curns->href)) &&
                (curns->prefix == NULL || xmlStrEqual(nsptr->prefix, curns->prefix))) {
                curns->next = NULL;
                if (prevns == NULL) {
                    nodep->nsDef = nsdftptr;
                } else {
                    prevns->next = nsdftptr;
                }
                set_old_ns(doc, curns);
                curns = prevns;
            }
        }
        prevns = curns;
        curns = nsdftptr;
    }
}

static void dom_reconcile_ns(xmlDocPtr doc, xmlNodePtr nodep) {
    if (nodep->type == XML_ELEMENT_NODE) {
        reconcile_internal(doc, nodep, nodep->parent);
        xmlReconciliateNs(doc, nodep);
    }
}

static void dump_el(const char *label, xmlNodePtr n) {
    if (n == NULL) { printf("%s: NULL\n", label); return; }
    printf("%s: name=%s ns=%s(%s) nsDef={", label, (char *) n->name,
           n->ns && n->ns->href ? (char *) n->ns->href : "(none)",
           n->ns && n->ns->prefix ? (char *) n->ns->prefix : "(null)");
    for (xmlNsPtr d = n->nsDef; d != NULL; d = d->next)
        printf("%s%s=%s;", d == n->nsDef ? "" : " ", d->prefix ? (char *) d->prefix : "(null)",
               d->href ? (char *) d->href : "(none)");
    printf("} parent=%s\n", n->parent && n->parent->name ? (char *) n->parent->name : "(doc/none)");
}

static void dump_doc(const char *label, xmlDocPtr d) {
    xmlChar *mem = NULL;
    int size = 0;
    xmlDocDumpMemory(d, &mem, &size);
    printf("%s --- serialized ---\n%.*s\n--- end ---\n", label, size, mem ? (char *) mem : "");
    xmlFree(mem);
}

static void run_one(void) {
    printf("######## MISMATCHED root ns ########\n");
    xmlDocPtr d = xmlNewDoc(BAD_CAST "1.0");
    xmlNodePtr root = xmlNewDocNode(d, NULL, BAD_CAST "root", NULL);
    xmlNsPtr rns = xmlNewNs(root, BAD_CAST "http://example2.com", NULL);
    root->ns = rns;
    xmlDocSetRootElement(d, root);
    dom_reconcile_ns(d, root);
    dump_el("root(after new+setroot+reconcile)", root);

    xmlNodePtr a1 = xmlNewDocNode(d, NULL, BAD_CAST "a1", NULL);
    xmlNsPtr ans = xmlNewNs(a1, BAD_CAST "http://example.com", NULL);
    a1->ns = ans;
    dump_el("a1(after new)", a1);

    xmlNodePtr b1 = xmlNewDocNode(d, NULL, BAD_CAST "b1", NULL);
    xmlNsPtr bns = xmlNewNs(b1, BAD_CAST "http://example.com", NULL);
    b1->ns = bns;
    dump_el("b1(after new)", b1);

    xmlAddChild(a1, b1);
    dom_reconcile_ns(d, b1);
    dump_el("a1(after append b1+reconcile b1)", a1);
    dump_el("b1(after reconcile)", b1);

    {
        xmlNsPtr srch = xmlSearchNsByHref(d, root, BAD_CAST "http://example.com");
        printf("search root-for-example.com => %s\n", srch ? (char *) (srch->href ? (char *) srch->href : "(null-href)") : "NULL");
    }
    xmlAddChild(root, a1);
    reconcile_internal(d, a1, root);
    dump_el("a1(after addChild+reconcile_internal only)", a1);
    xmlReconciliateNs(d, a1);
    dump_el("a1(after xmlReconciliateNs too)", a1);
    dump_doc("mismatch", d);
    xmlFreeDoc(d);
}

static void run_two(void) {
    printf("######## MATCHING root ns ########\n");
    xmlDocPtr d = xmlNewDoc(BAD_CAST "1.0");
    xmlNodePtr root = xmlNewDocNode(d, NULL, BAD_CAST "root", NULL);
    xmlNsPtr rns = xmlNewNs(root, BAD_CAST "http://example.com", NULL);
    root->ns = rns;
    xmlDocSetRootElement(d, root);
    dom_reconcile_ns(d, root);

    xmlNodePtr a1 = xmlNewDocNode(d, NULL, BAD_CAST "a1", NULL);
    xmlNsPtr ans = xmlNewNs(a1, BAD_CAST "http://example.com", NULL);
    a1->ns = ans;

    xmlNodePtr b1 = xmlNewDocNode(d, NULL, BAD_CAST "b1", NULL);
    xmlNsPtr bns = xmlNewNs(b1, BAD_CAST "http://example.com", NULL);
    b1->ns = bns;

    xmlAddChild(a1, b1);
    dom_reconcile_ns(d, b1);
    dump_el("a1(after append b1+reconcile b1)", a1);
    dump_el("b1(after reconcile)", b1);

    {
        xmlNsPtr srch = xmlSearchNsByHref(d, root, BAD_CAST "http://example.com");
        printf("search root-for-example.com => %s\n", srch ? (char *) (srch->href ? (char *) srch->href : "(null-href)") : "NULL");
    }
    xmlAddChild(root, a1);
    reconcile_internal(d, a1, root);
    dump_el("a1(after addChild+reconcile_internal only)", a1);
    xmlReconciliateNs(d, a1);
    dump_el("a1(after xmlReconciliateNs too)", a1);
    dump_doc("match", d);
    xmlFreeDoc(d);
}

int main(void) {
    run_one();
    run_two();
    return 0;
}
