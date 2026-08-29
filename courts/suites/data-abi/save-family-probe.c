/*
 * SAVE-001 — differential probe of the xmlSave* / xmlOutputBuffer* serialization
 * family (11.1-I serialization closure).
 *
 * Compiled twice (oracle DSO vs candidate DSO); output must be byte-identical.
 * Exercises: xmlSaveToBuffer/xmlSaveDoc with XML_SAVE_FORMAT /
 * XML_SAVE_NO_DECL, xmlSaveSetIndentString, xmlSaveTree, xmlSaveFlush,
 * xmlSaveClose, xmlSaveFinish, xmlSaveFormatFileTo / xmlSaveFileTo into an
 * xmlBuffer, xmlOutputBufferGetContent/GetSize, xmlOutputBufferWriteEscape
 * with the standard xmlEscapeEntities-ish escaping, xmlAllocOutputBuffer.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/tree.h>
#include <libxml/xmlsave.h>
#include <libxml/xmlIO.h>

static void show(const char *name, const char *s) {
    printf("%s[%zu]=", name, strlen(s));
    for (const unsigned char *p = (const unsigned char *) s; *p; p++) {
        if (*p == '\n') printf("\\n");
        else if (*p == '\t') printf("\\t");
        else if (*p < 32 || *p > 126) printf("\\x%02x", *p);
        else putchar(*p);
    }
    printf("\n");
}

static xmlDocPtr make_doc(void) {
    xmlDocPtr doc = xmlNewDoc((const xmlChar *) "1.0");
    xmlNodePtr root = xmlNewNode(NULL, (const xmlChar *) "root");
    xmlDocSetRootElement(doc, root);
    xmlNodePtr child = xmlNewNode(NULL, (const xmlChar *) "child");
    xmlAddChild(root, child);
    xmlNodePtr text = xmlNewText((const xmlChar *) "a & b < c");
    xmlAddChild(child, text);
    xmlNodePtr empty = xmlNewNode(NULL, (const xmlChar *) "empty");
    xmlAddChild(root, empty);
    return doc;
}

int main(void) {
    xmlDocPtr doc = make_doc();

    /* 1. formatted save */
    xmlBufferPtr buf = xmlBufferCreate();
    xmlSaveCtxtPtr ctxt = xmlSaveToBuffer(buf, NULL, XML_SAVE_FORMAT);
    long n = xmlSaveDoc(ctxt, doc);
    xmlSaveFlush(ctxt);
    xmlSaveClose(ctxt);
    show("format", (const char *) xmlBufferContent(buf));
    xmlBufferFree(buf);

    /* 2. no-decl + custom indent */
    buf = xmlBufferCreate();
    ctxt = xmlSaveToBuffer(buf, NULL, XML_SAVE_FORMAT | XML_SAVE_NO_DECL);
    xmlSaveSetIndentString(ctxt, "  ");
    xmlSaveDoc(ctxt, doc);
    xmlSaveFinish(ctxt);
    show("nodecl-indent", (const char *) xmlBufferContent(buf));
    xmlBufferFree(buf);

    /* 3. xmlSaveTree of a subtree */
    buf = xmlBufferCreate();
    ctxt = xmlSaveToBuffer(buf, NULL, 0);
    xmlNodePtr root = xmlDocGetRootElement(doc);
    xmlSaveTree(ctxt, root->children);
    xmlSaveFinish(ctxt);
    show("tree", (const char *) xmlBufferContent(buf));
    xmlBufferFree(buf);

    /* 4. xmlSaveFormatFileTo into a buffer-IO output buffer */
    {
        xmlBufferPtr out = xmlBufferCreate();
        xmlOutputBufferPtr ob = xmlOutputBufferCreateBuffer(out, NULL);
        int r = xmlSaveFormatFileTo(ob, doc, NULL, 1);
        printf("savefileto ret=%d\n", r);
        show("savefileto", (const char *) xmlBufferContent(out));
        xmlBufferFree(out);
    }

    /* 5. xmlOutputBufferWriteEscape with a simple escaping fn */
    {
        xmlBufferPtr out = xmlBufferCreate();
        xmlOutputBufferPtr ob = xmlOutputBufferCreateBuffer(out, NULL);
        int r = xmlOutputBufferWriteEscape(ob, (const xmlChar *) "<x>&\"'", NULL);
        printf("escape-none ret=%d content=%s\n", r, (const char *) xmlBufferContent(out));
        xmlBufferFree(out);
    }

    /* 6. xmlAllocOutputBuffer + write + content/size */
    {
        xmlOutputBufferPtr ob = xmlAllocOutputBuffer(NULL);
        xmlOutputBufferWrite(ob, 5, "hello");
        int size = xmlOutputBufferGetSize(ob);
        const char *content = (const char *) xmlOutputBufferGetContent(ob);
        printf("alloc size=%d content=%.5s\n", size, content);
        xmlOutputBufferClose(ob);
    }

    xmlFreeDoc(doc);
    return 0;
}
