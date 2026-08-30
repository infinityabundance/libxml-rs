/**
 * @file
 *
 * Canonicalization API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_C14N_H__
#define __XML_C14N_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Callback type (upstream c14n.h) */
typedef int (*xmlC14NIsVisibleCallback) (void* user_data,
					 xmlNode *node,
					 xmlNode *parent);

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlC14NDocDumpMemory (xmlDoc *doc, xmlNodeSet *nodes, int mode, /* a xmlC14NMode */ xmlChar **inclusive_ns_prefixes, int with_comments, xmlChar **doc_txt_ptr);
XMLPUBFUN int xmlC14NDocSave (xmlDoc *doc, xmlNodeSet *nodes, int mode, /* a xmlC14NMode */ xmlChar **inclusive_ns_prefixes, int with_comments, const char* filename, int compression);
XMLPUBFUN int xmlC14NDocSaveTo (xmlDoc *doc, xmlNodeSet *nodes, int mode, /* a xmlC14NMode */ xmlChar **inclusive_ns_prefixes, int with_comments, xmlOutputBuffer *buf);
XMLPUBFUN int xmlC14NExecute (xmlDoc *doc, xmlC14NIsVisibleCallback is_visible_callback, void* user_data, int mode, /* a xmlC14NMode */ xmlChar **inclusive_ns_prefixes, int with_comments, xmlOutputBuffer *buf);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_C14N_H__ */
