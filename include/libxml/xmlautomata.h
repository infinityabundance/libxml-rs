/**
 * @file
 *
 * Automata API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_XMLAUTOMATA_H__
#define __XML_XMLAUTOMATA_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Automata types (upstream xmlautomata.h) */
typedef struct _xmlAutomata xmlAutomata;
typedef xmlAutomata *xmlAutomataPtr;
typedef struct _xmlAutomataState xmlAutomataState;
typedef xmlAutomataState *xmlAutomataStatePtr;

/* Functions will be declared here as they are implemented. */
































/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN struct _xmlRegexp * xmlAutomataCompile (xmlAutomata *am);
XMLPUBFUN xmlAutomataState * xmlAutomataGetInitState (xmlAutomata *am);
XMLPUBFUN int xmlAutomataIsDeterminist (xmlAutomata *am);
XMLPUBFUN xmlAutomataState * xmlAutomataNewAllTrans (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, int lax);
XMLPUBFUN xmlAutomataState * xmlAutomataNewCountTrans (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, const xmlChar *token, int min, int max, void *data);
XMLPUBFUN xmlAutomataState * xmlAutomataNewCountTrans2 (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, const xmlChar *token, const xmlChar *token2, int min, int max, void *data);
XMLPUBFUN xmlAutomataState * xmlAutomataNewCountedTrans (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, int counter);
XMLPUBFUN int xmlAutomataNewCounter (xmlAutomata *am, int min, int max);
XMLPUBFUN xmlAutomataState * xmlAutomataNewCounterTrans (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, int counter);
XMLPUBFUN xmlAutomataState * xmlAutomataNewEpsilon (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to);
XMLPUBFUN xmlAutomataState * xmlAutomataNewNegTrans (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, const xmlChar *token, const xmlChar *token2, void *data);
XMLPUBFUN xmlAutomataState * xmlAutomataNewOnceTrans (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, const xmlChar *token, int min, int max, void *data);
XMLPUBFUN xmlAutomataState * xmlAutomataNewOnceTrans2 (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, const xmlChar *token, const xmlChar *token2, int min, int max, void *data);
XMLPUBFUN xmlAutomataState * xmlAutomataNewState (xmlAutomata *am);
XMLPUBFUN xmlAutomataState * xmlAutomataNewTransition (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, const xmlChar *token, void *data);
XMLPUBFUN xmlAutomataState * xmlAutomataNewTransition2 (xmlAutomata *am, xmlAutomataState *from, xmlAutomataState *to, const xmlChar *token, const xmlChar *token2, void *data);
XMLPUBFUN int xmlAutomataSetFinalState (xmlAutomata *am, xmlAutomataState *state);
XMLPUBFUN void xmlFreeAutomata (xmlAutomata *am);
XMLPUBFUN xmlAutomata * xmlNewAutomata (void);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLAUTOMATA_H__ */
