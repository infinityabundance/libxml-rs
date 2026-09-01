/*
 * hostile-ownership-probe.c — Phase 13 HOSTILE-OWNERSHIP attack court.
 *
 * Attacks tree/document/buffer ownership semantics with the transfers and
 * lifecycle edge cases the benign courts never exercise: unattached nodes,
 * unlink/re-add cycles, deep copies that must outlive their originals,
 * sibling insertion at boundaries, node-registration callback counts, and
 * reader/buffer lifecycle handoffs.
 *
 * Every operation here is DEFINED on both the oracle and the candidate
 * (real double-free / use-after-free are UB upstream and are deliberately
 * NOT performed — the court instead verifies the defined contracts and
 * that NULL-handles are inert). Output is deterministic; stderr from the
 * library is compared byte-for-byte.
 *
 * Court family: HOSTILE-OWNERSHIP (Phase 13 hostile audit, dimension 2:
 * ownership & lifecycle)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlreader.h>
#include <libxml/xmlIO.h>

static int node_regs = 0;
static int node_deregs = 0;

static void on_reg(xmlNodePtr n) { (void)n; node_regs++; }
static void on_dereg(xmlNodePtr n) { (void)n; node_deregs++; }

/* strlen-based reader: no hardcoded sizes to miscount. */
static xmlDocPtr rd(const char *s) {
    return xmlReadMemory(s, (int)strlen(s), "t", NULL, 0);
}

static void serialize(xmlDocPtr d, const char *label) {
    xmlChar *mem = NULL;
    int size = 0;
    xmlDocDumpFormatMemoryEnc(d, &mem, &size, "UTF-8", 1);
    if (mem == NULL) {
        printf("%s: dump=NULL size=%d\n", label, size);
        return;
    }
    printf("%s: size=%d content=[%s]\n", label, size, (const char *)mem);
    xmlFree(mem);
}

int main(void) {
    LIBXML_TEST_VERSION

    /* ── O1. unattached node from xmlNewChild(NULL,...) ────────────────── */
    {
        xmlNodePtr n = xmlNewChild(NULL, NULL, BAD_CAST "a", BAD_CAST "txt");
        printf("O1 newchild NULL parent=%s name=%s\n", n ? "(ptr)" : "(nil)",
               n && n->name ? (const char *)n->name : "(null)");
        printf("O1 parent link=%s\n", n && n->parent ? "(set)" : "(null)");
        if (n) xmlFreeNode(n);
    }

    /* ── O2. unlink/re-add ownership cycle ─────────────────────────────── */
    {
        xmlDocPtr d = rd("<r><c/></r>");
        xmlNodePtr r = d ? xmlDocGetRootElement(d) : NULL;
        xmlNodePtr c = r ? r->children : NULL;
        printf("O2 root=%s child=%s\n", r ? "(ptr)" : "(nil)",
               c ? (const char *)c->name : "(null)");
        if (c) {
            xmlUnlinkNode(c);
            printf("O2 after unlink: parent=%s children=%s\n",
                   c->parent ? "(set)" : "(null)",
                   r->children ? "(set)" : "(null)");
            /* Re-add via xmlAddChild — ownership returns to doc */
            xmlNodePtr back = xmlAddChild(r, c);
            printf("O2 re-added=%s\n", back == c ? "(same)" : "(diff)");
        }
        serialize(d, "O2 final");
        if (d) xmlFreeDoc(d);
    }

    /* ── O3. deep copy outlives the original ───────────────────────────── */
    {
        xmlDocPtr d = rd("<r><a x=\"1\"><b>t</b></a><c/></r>");
        xmlNodePtr copy = d ? xmlCopyNode(xmlDocGetRootElement(d), 1) : NULL;
        if (d) xmlFreeDoc(d);
        printf("O3 copy after free=%s\n", copy ? "(ptr)" : "(nil)");
        if (copy) {
            printf("O3 copy name=%s children=%s\n",
                   copy->name ? (const char *)copy->name : "(null)",
                   copy->children ? "(set)" : "(null)");
            xmlNodePtr sub = copy->children;
            printf("O3 copy child=%s content=%s\n",
                   sub && sub->name ? (const char *)sub->name : "(null)",
                   sub && sub->children && sub->children->content
                       ? (const char *)sub->children->content
                       : "(null)");
            xmlFreeNode(copy);
        }
    }

    /* ── O4. xmlReplaceNode ────────────────────────────────────────────── */
    {
        xmlDocPtr d = rd("<r><a/><b/></r>");
        xmlNodePtr r = d ? xmlDocGetRootElement(d) : NULL;
        xmlNodePtr a = r ? r->children : NULL;
        xmlNodePtr fresh = xmlNewChild(NULL, NULL, BAD_CAST "x", NULL);
        xmlNodePtr rep = xmlReplaceNode(a, fresh);
        printf("O4 replaced=%s children=%s\n", rep ? "(ptr)" : "(nil)",
               r && r->children ? (const char *)r->children->name : "(null)");
        if (rep) xmlFreeNode(rep);
        serialize(d, "O4 final");
        if (d) xmlFreeDoc(d);
    }

    /* ── O5. reader lifecycle: reader owns its document ────────────────── */
    {
        xmlTextReaderPtr rd2 = xmlReaderForMemory("<r>data</r>", 11, "t", NULL, 0);
        printf("O5 reader=%s\n", rd2 ? "(ptr)" : "(nil)");
        if (rd2) {
            int ret;
            while ((ret = xmlTextReaderRead(rd2)) == 1) {
                const xmlChar *name = xmlTextReaderConstName(rd2);
                printf("O5 node=%s type=%d\n",
                       name ? (const char *)name : "(null)",
                       xmlTextReaderNodeType(rd2));
            }
            printf("O5 read ret=%d\n", ret);
            xmlFreeTextReader(rd2); /* frees the doc it owns */
            printf("O5 reader freed\n");
        }
    }

    /* ── O6. buffer lifecycle ──────────────────────────────────────────── */
    {
        xmlBufferPtr b = xmlBufferCreate();
        printf("O6 buffer=%s\n", b ? "(ptr)" : "(nil)");
        if (b) {
            xmlBufferAdd(b, BAD_CAST "abc", 3);
            printf("O6 use=%u content=%s\n", b->use,
                   b->content ? (const char *)b->content : "(null)");
            xmlBufferEmpty(b);
            printf("O6 after empty use=%u\n", b->use);
            xmlBufferAddHead(b, BAD_CAST "Z", 1);
            printf("O6 after addhead=%s\n",
                   b->content ? (const char *)b->content : "(null)");
            xmlBufferFree(b);
            printf("O6 freed\n");
        }
    }

    /* ── O7. sibling insertion boundaries ──────────────────────────────── */
    {
        xmlDocPtr d = rd("<r><a/><b/></r>");
        xmlNodePtr r = d ? xmlDocGetRootElement(d) : NULL;
        xmlNodePtr a = r ? r->children : NULL;
        xmlNodePtr b = a ? a->next : NULL;
        xmlNodePtr n1 = xmlNewChild(NULL, NULL, BAD_CAST "m", NULL);
        xmlNodePtr n2 = xmlNewChild(NULL, NULL, BAD_CAST "n", NULL);
        printf("O7 a=%s b=%s\n", a ? (const char *)a->name : "(null)",
               b ? (const char *)b->name : "(null)");
        if (b && n1) {
            xmlNodePtr s = xmlAddPrevSibling(b, n1);
            printf("O7 prev-sib=%s\n", s == n1 ? "(same)" : "(diff)");
        }
        if (r && n2) {
            xmlNodePtr s = xmlAddNextSibling(r, n2);
            printf("O7 next-sib-of-root=%s\n", s ? "(ptr)" : "(nil)");
            if (s) xmlUnlinkNode(s);
        }
        serialize(d, "O7 final");
        if (n2) xmlFreeNode(n2);
        if (d) xmlFreeDoc(d);
    }

    /* ── O8. xmlCopyDoc deep ───────────────────────────────────────────── */
    {
        xmlDocPtr d = rd("<r><a>1</a><b>2</b></r>");
        xmlDocPtr d2 = d ? xmlCopyDoc(d, 1) : NULL;
        if (d) xmlFreeDoc(d);
        printf("O8 copydoc after free=%s\n", d2 ? "(ptr)" : "(nil)");
        if (d2) {
            xmlNodePtr r2 = xmlDocGetRootElement(d2);
            printf("O8 root=%s children=%s\n",
                   r2 && r2->name ? (const char *)r2->name : "(null)",
                   r2 && r2->children ? "(set)" : "(null)");
            xmlFreeDoc(d2);
        }
    }

    /* ── O9. xmlDocCopyNode into a different document ──────────────────── */
    {
        xmlDocPtr d1 = rd("<r><c/></r>");
        xmlDocPtr d2 = xmlNewDoc(BAD_CAST "1.0");
        xmlNodePtr c = d1 ? xmlDocGetRootElement(d1)->children : NULL;
        xmlNodePtr moved = d2 && c ? xmlDocCopyNode(c, d2, 1) : NULL;
        printf("O9 moved=%s doc-of-node=%s\n", moved ? "(ptr)" : "(nil)",
               moved && moved->doc ? "(set)" : "(null)");
        if (moved) xmlFreeNode(moved);
        if (d2) xmlFreeDoc(d2);
        if (d1) xmlFreeDoc(d1);
    }

    /* ── O10. node register/deregister hook counts ─────────────────────── */
    {
        xmlRegisterNodeDefault(on_reg);
        xmlDeregisterNodeDefault(on_dereg);
        node_regs = 0;
        node_deregs = 0;
        xmlDocPtr d = rd("<r><a/><b/></r>");
        printf("O10 regs during parse=%d\n", node_regs);
        if (d) xmlFreeDoc(d);
        printf("O10 deregs during free=%d\n", node_deregs);
        /* restore defaults */
        xmlRegisterNodeDefault(NULL);
        xmlDeregisterNodeDefault(NULL);
    }

    /* ── O11. xmlFreeNodeList on a detached sibling chain ─────────────── */
    {
        xmlDocPtr d = rd("<r><a/><b/><c/></r>");
        xmlNodePtr r = d ? xmlDocGetRootElement(d) : NULL;
        if (r && r->children) {
            xmlNodePtr a = r->children;
            xmlNodePtr b = a->next;
            xmlNodePtr c = b ? b->next : NULL;
            /* detach all three; keep c in the document */
            xmlUnlinkNode(a);
            if (b) xmlUnlinkNode(b);
            if (c) {
                xmlUnlinkNode(c);
                xmlAddChild(r, c);
            }
            /* re-link a->b into a detached chain for xmlFreeNodeList */
            a->next = b;
            if (b) b->prev = a;
            xmlFreeNodeList(a);
            printf("O11 freed chain, remaining=%s\n",
                   r->children ? (const char *)r->children->name : "(null)");
        }
        serialize(d, "O11 final");
        if (d) xmlFreeDoc(d);
    }

    /* ── O12. NULL-handle inertness across the lifecycle surface ───────── */
    xmlUnlinkNode(NULL);
    xmlFreeNodeList(NULL);
    xmlReplaceNode(NULL, NULL);
    xmlAddChild(NULL, NULL);
    xmlAddPrevSibling(NULL, NULL);
    xmlAddNextSibling(NULL, NULL);
    xmlFreeNode(NULL);
    xmlFreeDoc(NULL);
    xmlFreeTextReader(NULL);
    xmlBufferFree(NULL);
    printf("O12 all NULL-handle ops inert\n");

    /* ── final marker ──────────────────────────────────────────────────── */
    printf("HOSTILE-OWNERSHIP VERDICT PASS\n");
    return 0;
}
