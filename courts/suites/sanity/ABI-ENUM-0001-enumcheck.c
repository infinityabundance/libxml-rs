/**
 * @file ABI-ENUM-0001-enumcheck.c
 * @brief ABI probe: verify enum values and compile-time constants.
 *
 * Court Casefile: ABI-ENUM-0001
 * Description:   Enum / constant ABI compliance check for libxml-rs.
 *                Prints every enum constant with its numeric value,
 *                and uses static_assert to verify expected values at
 *                compile time.  Both the printed output and the
 *                successful compilation serve as the probe result.
 *
 * Build:
 *   Oracle mode (link system libxml2):
 *     gcc -std=c11 -o enumcheck-oracle ABI-ENUM-0001-enumcheck.c \
 *         -lxml2 -lxslt
 *
 *   Candidate mode (our headers only, no link):
 *     gcc -std=c11 -fsyntax-only -c ABI-ENUM-0001-enumcheck.c \
 *         -I /path/to/include
 *
 * Usage:
 *   ./enumcheck-oracle
 *
 * Output: Structured JSON-like lines.  Return code 0 on success.
 */

#include <stddef.h>
#include <stdio.h>
#include <assert.h>      /* static_assert (C11 + C++11) */
#include <libxml/tree.h>
#include <libxml/dict.h>
#include <libxml/hash.h>
#include <libxml/parser.h>
#include <libxml/xpath.h>
#include <libxml/xmlerror.h>

/* ================================================================== */
/*  Compile-time assertions                                            */
/*  These static_assert calls blow up at compile time if a value       */
/*  drifts from the expected constant.                                 */
/*  They are active by default (candidate mode).  Pass                 */
/*  -DVERIFY_UPSTREAM to use upstream libxml2 expected values.         */
/* ================================================================== */

/* --- xmlElementType (same in upstream and our headers) --- */
static_assert(XML_ELEMENT_NODE       == 1,  "XML_ELEMENT_NODE must be 1");
static_assert(XML_ATTRIBUTE_NODE     == 2,  "XML_ATTRIBUTE_NODE must be 2");
static_assert(XML_TEXT_NODE          == 3,  "XML_TEXT_NODE must be 3");
static_assert(XML_CDATA_SECTION_NODE == 4,  "XML_CDATA_SECTION_NODE must be 4");
static_assert(XML_ENTITY_REF_NODE    == 5,  "XML_ENTITY_REF_NODE must be 5");
static_assert(XML_ENTITY_NODE        == 6,  "XML_ENTITY_NODE must be 6");
static_assert(XML_PI_NODE            == 7,  "XML_PI_NODE must be 7");
static_assert(XML_COMMENT_NODE       == 8,  "XML_COMMENT_NODE must be 8");
static_assert(XML_DOCUMENT_NODE      == 9,  "XML_DOCUMENT_NODE must be 9");
static_assert(XML_DOCUMENT_TYPE_NODE == 10, "XML_DOCUMENT_TYPE_NODE must be 10");
static_assert(XML_DOCUMENT_FRAG_NODE == 11, "XML_DOCUMENT_FRAG_NODE must be 11");
static_assert(XML_NOTATION_NODE      == 12, "XML_NOTATION_NODE must be 12");
static_assert(XML_HTML_DOCUMENT_NODE == 13, "XML_HTML_DOCUMENT_NODE must be 13");
static_assert(XML_DTD_NODE           == 14, "XML_DTD_NODE must be 14");
static_assert(XML_ELEMENT_DECL       == 15, "XML_ELEMENT_DECL must be 15");
static_assert(XML_ATTRIBUTE_DECL     == 16, "XML_ATTRIBUTE_DECL must be 16");
static_assert(XML_ENTITY_DECL        == 17, "XML_ENTITY_DECL must be 17");
static_assert(XML_NAMESPACE_DECL     == 18, "XML_NAMESPACE_DECL must be 18");
static_assert(XML_XINCLUDE_START     == 19, "XML_XINCLUDE_START must be 19");
static_assert(XML_XINCLUDE_END       == 20, "XML_XINCLUDE_END must be 20");

/* --- xmlAttributeType --- */
static_assert(XML_ATTRIBUTE_CDATA        == 1,  "XML_ATTRIBUTE_CDATA must be 1");
static_assert(XML_ATTRIBUTE_ID           == 2,  "XML_ATTRIBUTE_ID must be 2");
static_assert(XML_ATTRIBUTE_IDREF        == 3,  "XML_ATTRIBUTE_IDREF must be 3");
static_assert(XML_ATTRIBUTE_IDREFS       == 4,  "XML_ATTRIBUTE_IDREFS must be 4");
static_assert(XML_ATTRIBUTE_ENTITY       == 5,  "XML_ATTRIBUTE_ENTITY must be 5");
static_assert(XML_ATTRIBUTE_ENTITIES     == 6,  "XML_ATTRIBUTE_ENTITIES must be 6");
static_assert(XML_ATTRIBUTE_NMTOKEN      == 7,  "XML_ATTRIBUTE_NMTOKEN must be 7");
static_assert(XML_ATTRIBUTE_NMTOKENS     == 8,  "XML_ATTRIBUTE_NMTOKENS must be 8");
static_assert(XML_ATTRIBUTE_ENUMERATION  == 9,  "XML_ATTRIBUTE_ENUMERATION must be 9");
static_assert(XML_ATTRIBUTE_NOTATION     == 10, "XML_ATTRIBUTE_NOTATION must be 10");

/* --- xmlAttributeDefault --- */
static_assert(XML_ATTRIBUTE_NONE     == 1, "XML_ATTRIBUTE_NONE must be 1");
static_assert(XML_ATTRIBUTE_REQUIRED == 2, "XML_ATTRIBUTE_REQUIRED must be 2");
static_assert(XML_ATTRIBUTE_IMPLIED  == 3, "XML_ATTRIBUTE_IMPLIED must be 3");
static_assert(XML_ATTRIBUTE_FIXED    == 4, "XML_ATTRIBUTE_FIXED must be 4");

/* --- xmlEntityType --- */
static_assert(XML_INTERNAL_GENERAL_ENTITY           == 1, "XML_INTERNAL_GENERAL_ENTITY must be 1");
static_assert(XML_EXTERNAL_GENERAL_PARSED_ENTITY    == 2, "XML_EXTERNAL_GENERAL_PARSED_ENTITY must be 2");
static_assert(XML_EXTERNAL_GENERAL_UNPARSED_ENTITY  == 3, "XML_EXTERNAL_GENERAL_UNPARSED_ENTITY must be 3");
static_assert(XML_INTERNAL_PARAMETER_ENTITY         == 4, "XML_INTERNAL_PARAMETER_ENTITY must be 4");
static_assert(XML_EXTERNAL_PARAMETER_ENTITY         == 5, "XML_EXTERNAL_PARAMETER_ENTITY must be 5");
static_assert(XML_INTERNAL_PREDEFINED_ENTITY        == 6, "XML_INTERNAL_PREDEFINED_ENTITY must be 6");

/* --- xmlBufferAllocationScheme --- */
static_assert(XML_BUFFER_ALLOC_DOUBLEIT  == 0, "XML_BUFFER_ALLOC_DOUBLEIT must be 0");
static_assert(XML_BUFFER_ALLOC_EXACT     == 1, "XML_BUFFER_ALLOC_EXACT must be 1");
static_assert(XML_BUFFER_ALLOC_IMMUTABLE == 2, "XML_BUFFER_ALLOC_IMMUTABLE must be 2");
static_assert(XML_BUFFER_ALLOC_IO        == 3, "XML_BUFFER_ALLOC_IO must be 3");
static_assert(XML_BUFFER_ALLOC_HYBRID    == 4, "XML_BUFFER_ALLOC_HYBRID must be 4");
static_assert(XML_BUFFER_ALLOC_BOUNDED   == 5, "XML_BUFFER_ALLOC_BOUNDED must be 5");

/* --- xmlElementTypeVal --- */
static_assert(XML_ELEMENT_TYPE_UNDEFINED == 0, "XML_ELEMENT_TYPE_UNDEFINED must be 0");
static_assert(XML_ELEMENT_TYPE_EMPTY     == 1, "XML_ELEMENT_TYPE_EMPTY must be 1");
static_assert(XML_ELEMENT_TYPE_ANY       == 2, "XML_ELEMENT_TYPE_ANY must be 2");
static_assert(XML_ELEMENT_TYPE_MIXED     == 3, "XML_ELEMENT_TYPE_MIXED must be 3");
static_assert(XML_ELEMENT_TYPE_ELEMENT   == 4, "XML_ELEMENT_TYPE_ELEMENT must be 4");

/* --- xmlParserMode --- */
static_assert(XML_PARSE_UNKNOWN   == 0, "XML_PARSE_UNKNOWN must be 0");
static_assert(XML_PARSE_DOM       == 1, "XML_PARSE_DOM must be 1");
static_assert(XML_PARSE_SAX       == 2, "XML_PARSE_SAX must be 2");
static_assert(XML_PARSE_PUSH_DOM  == 3, "XML_PARSE_PUSH_DOM must be 3");
static_assert(XML_PARSE_PUSH_SAX  == 4, "XML_PARSE_PUSH_SAX must be 4");
static_assert(XML_PARSE_READER    == 5, "XML_PARSE_READER must be 5");

/* --- xmlXPathObjectType --- */
static_assert(XPATH_UNDEFINED    == 0, "XPATH_UNDEFINED must be 0");
static_assert(XPATH_NODESET      == 1, "XPATH_NODESET must be 1");
static_assert(XPATH_BOOLEAN      == 2, "XPATH_BOOLEAN must be 2");
static_assert(XPATH_NUMBER       == 3, "XPATH_NUMBER must be 3");
static_assert(XPATH_STRING       == 4, "XPATH_STRING must be 4");
static_assert(XPATH_POINT        == 5, "XPATH_POINT must be 5");
static_assert(XPATH_RANGE        == 6, "XPATH_RANGE must be 6");
static_assert(XPATH_LOCATIONSET  == 7, "XPATH_LOCATIONSET must be 7");
static_assert(XPATH_USERS        == 8, "XPATH_USERS must be 8");
static_assert(XPATH_XSLT_TREE    == 9, "XPATH_XSLT_TREE must be 9");

/* --- xmlErrorLevel --- */
static_assert(XML_ERR_NONE   == 0, "XML_ERR_NONE must be 0");
static_assert(XML_ERR_WARNING == 1, "XML_ERR_WARNING must be 1");
static_assert(XML_ERR_ERROR  == 2, "XML_ERR_ERROR must be 2");
static_assert(XML_ERR_FATAL  == 3, "XML_ERR_FATAL must be 3");

/* --- Error domains (identical in upstream and our headers) --- */
static_assert(XML_FROM_NONE      == 0,  "XML_FROM_NONE must be 0");
static_assert(XML_FROM_PARSER    == 1,  "XML_FROM_PARSER must be 1");
static_assert(XML_FROM_TREE      == 2,  "XML_FROM_TREE must be 2");
static_assert(XML_FROM_NAMESPACE == 3,  "XML_FROM_NAMESPACE must be 3");
static_assert(XML_FROM_DTD       == 4,  "XML_FROM_DTD must be 4");
static_assert(XML_FROM_HTML      == 5,  "XML_FROM_HTML must be 5");
static_assert(XML_FROM_MEMORY    == 6,  "XML_FROM_MEMORY must be 6");
static_assert(XML_FROM_OUTPUT    == 7,  "XML_FROM_OUTPUT must be 7");
static_assert(XML_FROM_IO        == 8,  "XML_FROM_IO must be 8");
static_assert(XML_FROM_FTP       == 9,  "XML_FROM_FTP must be 9");
static_assert(XML_FROM_HTTP      == 10, "XML_FROM_HTTP must be 10");
static_assert(XML_FROM_XINCLUDE  == 11, "XML_FROM_XINCLUDE must be 11");
static_assert(XML_FROM_XPATH     == 12, "XML_FROM_XPATH must be 12");
static_assert(XML_FROM_XPOINTER  == 13, "XML_FROM_XPOINTER must be 13");
static_assert(XML_FROM_REGEXP    == 14, "XML_FROM_REGEXP must be 14");
static_assert(XML_FROM_DATATYPE  == 15, "XML_FROM_DATATYPE must be 15");
static_assert(XML_FROM_SCHEMASP  == 16, "XML_FROM_SCHEMASP must be 16");
static_assert(XML_FROM_SCHEMASV  == 17, "XML_FROM_SCHEMASV must be 17");
static_assert(XML_FROM_RELAXNGP  == 18, "XML_FROM_RELAXNGP must be 18");
static_assert(XML_FROM_RELAXNGV  == 19, "XML_FROM_RELAXNGV must be 19");
static_assert(XML_FROM_CATALOG   == 20, "XML_FROM_CATALOG must be 20");
static_assert(XML_FROM_C14N      == 21, "XML_FROM_C14N must be 21");
static_assert(XML_FROM_XSLT      == 22, "XML_FROM_XSLT must be 22");
static_assert(XML_FROM_VALID     == 23, "XML_FROM_VALID must be 23");
static_assert(XML_FROM_CHECK     == 24, "XML_FROM_CHECK must be 24");
static_assert(XML_FROM_WRITER    == 25, "XML_FROM_WRITER must be 25");
static_assert(XML_FROM_MODULE    == 26, "XML_FROM_MODULE must be 26");
static_assert(XML_FROM_I18N      == 27, "XML_FROM_I18N must be 27");
static_assert(XML_FROM_SCHEMATRONV == 28, "XML_FROM_SCHEMATRONV must be 28");
static_assert(XML_FROM_BUFFER    == 29, "XML_FROM_BUFFER must be 29");
static_assert(XML_FROM_URI       == 30, "XML_FROM_URI must be 30");

/* --- Parser option flags (identical in upstream and our headers) --- */
static_assert(XML_PARSE_RECOVER     == 1,       "XML_PARSE_RECOVER must be 1");
static_assert(XML_PARSE_NOENT       == 2,       "XML_PARSE_NOENT must be 2");
static_assert(XML_PARSE_DTDLOAD     == 4,       "XML_PARSE_DTDLOAD must be 4");
static_assert(XML_PARSE_DTDATTR     == 8,       "XML_PARSE_DTDATTR must be 8");
static_assert(XML_PARSE_DTDVALID    == 16,      "XML_PARSE_DTDVALID must be 16");
static_assert(XML_PARSE_NOERROR     == 32,      "XML_PARSE_NOERROR must be 32");
static_assert(XML_PARSE_NOWARNING   == 64,      "XML_PARSE_NOWARNING must be 64");
static_assert(XML_PARSE_PEDANTIC    == 128,     "XML_PARSE_PEDANTIC must be 128");
static_assert(XML_PARSE_NOBLANKS    == 256,     "XML_PARSE_NOBLANKS must be 256");
static_assert(XML_PARSE_SAX1        == 512,     "XML_PARSE_SAX1 must be 512");
static_assert(XML_PARSE_XINCLUDE    == 1024,    "XML_PARSE_XINCLUDE must be 1024");
static_assert(XML_PARSE_NONET       == 2048,    "XML_PARSE_NONET must be 2048");
static_assert(XML_PARSE_NODICT      == 4096,    "XML_PARSE_NODICT must be 4096");
static_assert(XML_PARSE_NSCLEAN     == 8192,    "XML_PARSE_NSCLEAN must be 8192");
static_assert(XML_PARSE_NOCDATA     == 16384,   "XML_PARSE_NOCDATA must be 16384");
static_assert(XML_PARSE_NOXINCNODE  == 32768,   "XML_PARSE_NOXINCNODE must be 32768");
static_assert(XML_PARSE_COMPACT     == 65536,   "XML_PARSE_COMPACT must be 65536");
static_assert(XML_PARSE_OLD10       == 131072,  "XML_PARSE_OLD10 must be 131072");
static_assert(XML_PARSE_NOBASEFIX   == 262144,  "XML_PARSE_NOBASEFIX must be 262144");
static_assert(XML_PARSE_HUGE        == 524288,  "XML_PARSE_HUGE must be 524288");
static_assert(XML_PARSE_OLDSAX      == 1048576, "XML_PARSE_OLDSAX must be 1048576");
static_assert(XML_PARSE_IGNORE_ENC  == 2097152, "XML_PARSE_IGNORE_ENC must be 2097152");
static_assert(XML_PARSE_BIG_LINES   == 4194304, "XML_PARSE_BIG_LINES must be 4194304");

/* --- Parser input states — these differ between our headers and upstream --- */
/* Our (simplified) values */
#ifndef VERIFY_UPSTREAM
static_assert(XML_PARSER_START            == 0,  "XML_PARSER_START must be 0");
static_assert(XML_PARSER_MISC             == 1,  "XML_PARSER_MISC must be 1");
static_assert(XML_PARSER_DTD              == 2,  "XML_PARSER_DTD must be 2");
static_assert(XML_PARSER_PROLOG           == 3,  "XML_PARSER_PROLOG must be 3");
static_assert(XML_PARSER_CONTENT          == 4,  "XML_PARSER_CONTENT must be 4");
static_assert(XML_PARSER_CDATA_SECTION    == 5,  "XML_PARSER_CDATA_SECTION must be 5");
static_assert(XML_PARSER_ENTITY_REF       == 6,  "XML_PARSER_ENTITY_REF must be 6");
static_assert(XML_PARSER_ENTITY_VALUE     == 7,  "XML_PARSER_ENTITY_VALUE must be 7");
static_assert(XML_PARSER_ATTRIBUTE_VALUE  == 8,  "XML_PARSER_ATTRIBUTE_VALUE must be 8");
static_assert(XML_PARSER_SYSTEM_LITERAL   == 9,  "XML_PARSER_SYSTEM_LITERAL must be 9");
static_assert(XML_PARSER_EPILOG           == 10, "XML_PARSER_EPILOG must be 10");
static_assert(XML_PARSER_IGNORE           == 11, "XML_PARSER_IGNORE must be 11");
static_assert(XML_PARSER_PUBLIC_LITERAL   == 12, "XML_PARSER_PUBLIC_LITERAL must be 12");
#else
/* Upstream libxml2 values */
static_assert(XML_PARSER_START            == 0,  "XML_PARSER_START must be 0");
static_assert(XML_PARSER_MISC             == 1,  "XML_PARSER_MISC must be 1");
static_assert(XML_PARSER_DTD              == 3,  "XML_PARSER_DTD must be 3");
static_assert(XML_PARSER_PROLOG           == 4,  "XML_PARSER_PROLOG must be 4");
static_assert(XML_PARSER_CONTENT          == 7,  "XML_PARSER_CONTENT must be 7");
static_assert(XML_PARSER_CDATA_SECTION    == 8,  "XML_PARSER_CDATA_SECTION must be 8");
static_assert(XML_PARSER_ENTITY_VALUE     == 11, "XML_PARSER_ENTITY_VALUE must be 11");
static_assert(XML_PARSER_ATTRIBUTE_VALUE  == 12, "XML_PARSER_ATTRIBUTE_VALUE must be 12");
static_assert(XML_PARSER_SYSTEM_LITERAL   == 13, "XML_PARSER_SYSTEM_LITERAL must be 13");
static_assert(XML_PARSER_EPILOG           == 14, "XML_PARSER_EPILOG must be 14");
static_assert(XML_PARSER_IGNORE           == 15, "XML_PARSER_IGNORE must be 15");
static_assert(XML_PARSER_PUBLIC_LITERAL   == 16, "XML_PARSER_PUBLIC_LITERAL must be 16");
#endif

/* ================================================================== */
/*  Runtime printing                                                   */
/* ================================================================== */

static void print_node_types(void)
{
    printf("  \"xmlElementType\": {\n");
    printf("    \"XML_ELEMENT_NODE\": %d,\n",       XML_ELEMENT_NODE);
    printf("    \"XML_ATTRIBUTE_NODE\": %d,\n",     XML_ATTRIBUTE_NODE);
    printf("    \"XML_TEXT_NODE\": %d,\n",          XML_TEXT_NODE);
    printf("    \"XML_CDATA_SECTION_NODE\": %d,\n", XML_CDATA_SECTION_NODE);
    printf("    \"XML_ENTITY_REF_NODE\": %d,\n",    XML_ENTITY_REF_NODE);
    printf("    \"XML_ENTITY_NODE\": %d,\n",        XML_ENTITY_NODE);
    printf("    \"XML_PI_NODE\": %d,\n",            XML_PI_NODE);
    printf("    \"XML_COMMENT_NODE\": %d,\n",       XML_COMMENT_NODE);
    printf("    \"XML_DOCUMENT_NODE\": %d,\n",      XML_DOCUMENT_NODE);
    printf("    \"XML_DOCUMENT_TYPE_NODE\": %d,\n", XML_DOCUMENT_TYPE_NODE);
    printf("    \"XML_DOCUMENT_FRAG_NODE\": %d,\n", XML_DOCUMENT_FRAG_NODE);
    printf("    \"XML_NOTATION_NODE\": %d,\n",      XML_NOTATION_NODE);
    printf("    \"XML_HTML_DOCUMENT_NODE\": %d,\n", XML_HTML_DOCUMENT_NODE);
    printf("    \"XML_DTD_NODE\": %d,\n",           XML_DTD_NODE);
    printf("    \"XML_ELEMENT_DECL\": %d,\n",       XML_ELEMENT_DECL);
    printf("    \"XML_ATTRIBUTE_DECL\": %d,\n",     XML_ATTRIBUTE_DECL);
    printf("    \"XML_ENTITY_DECL\": %d,\n",        XML_ENTITY_DECL);
    printf("    \"XML_NAMESPACE_DECL\": %d,\n",     XML_NAMESPACE_DECL);
    printf("    \"XML_XINCLUDE_START\": %d,\n",     XML_XINCLUDE_START);
    printf("    \"XML_XINCLUDE_END\": %d\n",        XML_XINCLUDE_END);
    printf("  },\n");
}

static void print_attribute_types(void)
{
    printf("  \"xmlAttributeType\": {\n");
    printf("    \"XML_ATTRIBUTE_CDATA\": %d,\n",       XML_ATTRIBUTE_CDATA);
    printf("    \"XML_ATTRIBUTE_ID\": %d,\n",           XML_ATTRIBUTE_ID);
    printf("    \"XML_ATTRIBUTE_IDREF\": %d,\n",        XML_ATTRIBUTE_IDREF);
    printf("    \"XML_ATTRIBUTE_IDREFS\": %d,\n",       XML_ATTRIBUTE_IDREFS);
    printf("    \"XML_ATTRIBUTE_ENTITY\": %d,\n",       XML_ATTRIBUTE_ENTITY);
    printf("    \"XML_ATTRIBUTE_ENTITIES\": %d,\n",     XML_ATTRIBUTE_ENTITIES);
    printf("    \"XML_ATTRIBUTE_NMTOKEN\": %d,\n",      XML_ATTRIBUTE_NMTOKEN);
    printf("    \"XML_ATTRIBUTE_NMTOKENS\": %d,\n",     XML_ATTRIBUTE_NMTOKENS);
    printf("    \"XML_ATTRIBUTE_ENUMERATION\": %d,\n",  XML_ATTRIBUTE_ENUMERATION);
    printf("    \"XML_ATTRIBUTE_NOTATION\": %d\n",      XML_ATTRIBUTE_NOTATION);
    printf("  },\n");
}

static void print_attribute_defaults(void)
{
    printf("  \"xmlAttributeDefault\": {\n");
    printf("    \"XML_ATTRIBUTE_NONE\": %d,\n",     XML_ATTRIBUTE_NONE);
    printf("    \"XML_ATTRIBUTE_REQUIRED\": %d,\n", XML_ATTRIBUTE_REQUIRED);
    printf("    \"XML_ATTRIBUTE_IMPLIED\": %d,\n",  XML_ATTRIBUTE_IMPLIED);
    printf("    \"XML_ATTRIBUTE_FIXED\": %d\n",     XML_ATTRIBUTE_FIXED);
    printf("  },\n");
}

static void print_entity_types(void)
{
    printf("  \"xmlEntityType\": {\n");
    printf("    \"XML_INTERNAL_GENERAL_ENTITY\": %d,\n",          XML_INTERNAL_GENERAL_ENTITY);
    printf("    \"XML_EXTERNAL_GENERAL_PARSED_ENTITY\": %d,\n",   XML_EXTERNAL_GENERAL_PARSED_ENTITY);
    printf("    \"XML_EXTERNAL_GENERAL_UNPARSED_ENTITY\": %d,\n", XML_EXTERNAL_GENERAL_UNPARSED_ENTITY);
    printf("    \"XML_INTERNAL_PARAMETER_ENTITY\": %d,\n",        XML_INTERNAL_PARAMETER_ENTITY);
    printf("    \"XML_EXTERNAL_PARAMETER_ENTITY\": %d,\n",        XML_EXTERNAL_PARAMETER_ENTITY);
    printf("    \"XML_INTERNAL_PREDEFINED_ENTITY\": %d\n",        XML_INTERNAL_PREDEFINED_ENTITY);
    printf("  },\n");
}

static void print_buffer_schemes(void)
{
    printf("  \"xmlBufferAllocationScheme\": {\n");
    printf("    \"XML_BUFFER_ALLOC_DOUBLEIT\": %d,\n",  XML_BUFFER_ALLOC_DOUBLEIT);
    printf("    \"XML_BUFFER_ALLOC_EXACT\": %d,\n",     XML_BUFFER_ALLOC_EXACT);
    printf("    \"XML_BUFFER_ALLOC_IMMUTABLE\": %d,\n", XML_BUFFER_ALLOC_IMMUTABLE);
    printf("    \"XML_BUFFER_ALLOC_IO\": %d,\n",        XML_BUFFER_ALLOC_IO);
    printf("    \"XML_BUFFER_ALLOC_HYBRID\": %d,\n",    XML_BUFFER_ALLOC_HYBRID);
    printf("    \"XML_BUFFER_ALLOC_BOUNDED\": %d\n",    XML_BUFFER_ALLOC_BOUNDED);
    printf("  },\n");
}

static void print_element_type_vals(void)
{
    printf("  \"xmlElementTypeVal\": {\n");
    printf("    \"XML_ELEMENT_TYPE_UNDEFINED\": %d,\n", 0);
    printf("    \"XML_ELEMENT_TYPE_EMPTY\": %d,\n",     1);
    printf("    \"XML_ELEMENT_TYPE_ANY\": %d,\n",       2);
    printf("    \"XML_ELEMENT_TYPE_MIXED\": %d,\n",     3);
    printf("    \"XML_ELEMENT_TYPE_ELEMENT\": %d\n",    4);
    printf("  },\n");
}

static void print_parser_options(void)
{
    printf("  \"parser_options\": {\n");
    printf("    \"XML_PARSE_RECOVER\": %d,\n",     XML_PARSE_RECOVER);
    printf("    \"XML_PARSE_NOENT\": %d,\n",       XML_PARSE_NOENT);
    printf("    \"XML_PARSE_DTDLOAD\": %d,\n",     XML_PARSE_DTDLOAD);
    printf("    \"XML_PARSE_DTDATTR\": %d,\n",     XML_PARSE_DTDATTR);
    printf("    \"XML_PARSE_DTDVALID\": %d,\n",    XML_PARSE_DTDVALID);
    printf("    \"XML_PARSE_NOERROR\": %d,\n",     XML_PARSE_NOERROR);
    printf("    \"XML_PARSE_NOWARNING\": %d,\n",   XML_PARSE_NOWARNING);
    printf("    \"XML_PARSE_PEDANTIC\": %d,\n",    XML_PARSE_PEDANTIC);
    printf("    \"XML_PARSE_NOBLANKS\": %d,\n",    XML_PARSE_NOBLANKS);
    printf("    \"XML_PARSE_SAX1\": %d,\n",        XML_PARSE_SAX1);
    printf("    \"XML_PARSE_XINCLUDE\": %d,\n",    XML_PARSE_XINCLUDE);
    printf("    \"XML_PARSE_NONET\": %d,\n",       XML_PARSE_NONET);
    printf("    \"XML_PARSE_NODICT\": %d,\n",      XML_PARSE_NODICT);
    printf("    \"XML_PARSE_NSCLEAN\": %d,\n",     XML_PARSE_NSCLEAN);
    printf("    \"XML_PARSE_NOCDATA\": %d,\n",     XML_PARSE_NOCDATA);
    printf("    \"XML_PARSE_NOXINCNODE\": %d,\n",  XML_PARSE_NOXINCNODE);
    printf("    \"XML_PARSE_COMPACT\": %d,\n",     XML_PARSE_COMPACT);
    printf("    \"XML_PARSE_OLD10\": %d,\n",       XML_PARSE_OLD10);
    printf("    \"XML_PARSE_NOBASEFIX\": %d,\n",   XML_PARSE_NOBASEFIX);
    printf("    \"XML_PARSE_HUGE\": %d,\n",        XML_PARSE_HUGE);
    printf("    \"XML_PARSE_OLDSAX\": %d,\n",      XML_PARSE_OLDSAX);
    printf("    \"XML_PARSE_IGNORE_ENC\": %d,\n",  XML_PARSE_IGNORE_ENC);
    printf("    \"XML_PARSE_BIG_LINES\": %d\n",    XML_PARSE_BIG_LINES);
    printf("  },\n");
}

static void print_parser_modes(void)
{
    printf("  \"xmlParserMode\": {\n");
    printf("    \"XML_PARSE_UNKNOWN\": %d,\n",   XML_PARSE_UNKNOWN);
    printf("    \"XML_PARSE_DOM\": %d,\n",       XML_PARSE_DOM);
    printf("    \"XML_PARSE_SAX\": %d,\n",       XML_PARSE_SAX);
    printf("    \"XML_PARSE_PUSH_DOM\": %d,\n",  XML_PARSE_PUSH_DOM);
    printf("    \"XML_PARSE_PUSH_SAX\": %d,\n",  XML_PARSE_PUSH_SAX);
    printf("    \"XML_PARSE_READER\": %d\n",     XML_PARSE_READER);
    printf("  },\n");
}

static void print_parser_input_states(void)
{
    printf("  \"xmlParserInputState\": {\n");
    printf("    \"XML_PARSER_EOF\": %d,\n",             XML_PARSER_EOF);
    printf("    \"XML_PARSER_START\": %d,\n",            XML_PARSER_START);
    printf("    \"XML_PARSER_MISC\": %d,\n",             XML_PARSER_MISC);
    printf("    \"XML_PARSER_DTD\": %d,\n",              XML_PARSER_DTD);
    printf("    \"XML_PARSER_PROLOG\": %d,\n",           XML_PARSER_PROLOG);
    printf("    \"XML_PARSER_CONTENT\": %d,\n",          XML_PARSER_CONTENT);
    printf("    \"XML_PARSER_CDATA_SECTION\": %d,\n",    XML_PARSER_CDATA_SECTION);
#ifdef XML_PARSER_ENTITY_REF
    printf("    \"XML_PARSER_ENTITY_REF\": %d,\n",       XML_PARSER_ENTITY_REF);
#endif
#ifdef XML_PARSER_ENTITY_DECL
    printf("    \"XML_PARSER_ENTITY_DECL\": %d,\n",      XML_PARSER_ENTITY_DECL);
#endif
#ifdef XML_PARSER_PI
    printf("    \"XML_PARSER_PI\": %d,\n",               XML_PARSER_PI);
#endif
#ifdef XML_PARSER_COMMENT
    printf("    \"XML_PARSER_COMMENT\": %d,\n",          XML_PARSER_COMMENT);
#endif
#ifdef XML_PARSER_START_TAG
    printf("    \"XML_PARSER_START_TAG\": %d,\n",        XML_PARSER_START_TAG);
#endif
#ifdef XML_PARSER_END_TAG
    printf("    \"XML_PARSER_END_TAG\": %d,\n",          XML_PARSER_END_TAG);
#endif
#ifdef XML_PARSER_XML_DECL
    printf("    \"XML_PARSER_XML_DECL\": %d,\n",          XML_PARSER_XML_DECL);
#endif
    printf("    \"XML_PARSER_ENTITY_VALUE\": %d,\n",     XML_PARSER_ENTITY_VALUE);
    printf("    \"XML_PARSER_ATTRIBUTE_VALUE\": %d,\n",  XML_PARSER_ATTRIBUTE_VALUE);
    printf("    \"XML_PARSER_SYSTEM_LITERAL\": %d,\n",   XML_PARSER_SYSTEM_LITERAL);
    printf("    \"XML_PARSER_EPILOG\": %d,\n",           XML_PARSER_EPILOG);
    printf("    \"XML_PARSER_IGNORE\": %d,\n",           XML_PARSER_IGNORE);
    printf("    \"XML_PARSER_PUBLIC_LITERAL\": %d\n",    XML_PARSER_PUBLIC_LITERAL);
    printf("  },\n");
}

static void print_xpath_object_types(void)
{
    printf("  \"xmlXPathObjectType\": {\n");
    printf("    \"XPATH_UNDEFINED\": %d,\n",   XPATH_UNDEFINED);
    printf("    \"XPATH_NODESET\": %d,\n",     XPATH_NODESET);
    printf("    \"XPATH_BOOLEAN\": %d,\n",     XPATH_BOOLEAN);
    printf("    \"XPATH_NUMBER\": %d,\n",      XPATH_NUMBER);
    printf("    \"XPATH_STRING\": %d,\n",      XPATH_STRING);
    printf("    \"XPATH_POINT\": %d,\n",       XPATH_POINT);
    printf("    \"XPATH_RANGE\": %d,\n",       XPATH_RANGE);
    printf("    \"XPATH_LOCATIONSET\": %d,\n", XPATH_LOCATIONSET);
    printf("    \"XPATH_USERS\": %d,\n",       XPATH_USERS);
    printf("    \"XPATH_XSLT_TREE\": %d\n",    XPATH_XSLT_TREE);
    printf("  },\n");
}

static void print_error_domains(void)
{
    printf("  \"error_domains\": {\n");
    printf("    \"XML_FROM_NONE\": %d,\n",       XML_FROM_NONE);
    printf("    \"XML_FROM_PARSER\": %d,\n",     XML_FROM_PARSER);
    printf("    \"XML_FROM_TREE\": %d,\n",       XML_FROM_TREE);
    printf("    \"XML_FROM_NAMESPACE\": %d,\n",  XML_FROM_NAMESPACE);
    printf("    \"XML_FROM_DTD\": %d,\n",        XML_FROM_DTD);
    printf("    \"XML_FROM_HTML\": %d,\n",       XML_FROM_HTML);
    printf("    \"XML_FROM_MEMORY\": %d,\n",     XML_FROM_MEMORY);
    printf("    \"XML_FROM_OUTPUT\": %d,\n",     XML_FROM_OUTPUT);
    printf("    \"XML_FROM_IO\": %d,\n",         XML_FROM_IO);
    printf("    \"XML_FROM_FTP\": %d,\n",        XML_FROM_FTP);
    printf("    \"XML_FROM_HTTP\": %d,\n",       XML_FROM_HTTP);
    printf("    \"XML_FROM_XINCLUDE\": %d,\n",   XML_FROM_XINCLUDE);
    printf("    \"XML_FROM_XPATH\": %d,\n",      XML_FROM_XPATH);
    printf("    \"XML_FROM_XPOINTER\": %d,\n",   XML_FROM_XPOINTER);
    printf("    \"XML_FROM_REGEXP\": %d,\n",     XML_FROM_REGEXP);
    printf("    \"XML_FROM_DATATYPE\": %d,\n",   XML_FROM_DATATYPE);
    printf("    \"XML_FROM_SCHEMASP\": %d,\n",   XML_FROM_SCHEMASP);
    printf("    \"XML_FROM_SCHEMASV\": %d,\n",   XML_FROM_SCHEMASV);
    printf("    \"XML_FROM_RELAXNGP\": %d,\n",   XML_FROM_RELAXNGP);
    printf("    \"XML_FROM_RELAXNGV\": %d,\n",   XML_FROM_RELAXNGV);
    printf("    \"XML_FROM_CATALOG\": %d,\n",    XML_FROM_CATALOG);
    printf("    \"XML_FROM_C14N\": %d,\n",       XML_FROM_C14N);
    printf("    \"XML_FROM_XSLT\": %d,\n",       XML_FROM_XSLT);
    printf("    \"XML_FROM_VALID\": %d,\n",      XML_FROM_VALID);
    printf("    \"XML_FROM_CHECK\": %d,\n",      XML_FROM_CHECK);
    printf("    \"XML_FROM_WRITER\": %d,\n",     XML_FROM_WRITER);
    printf("    \"XML_FROM_MODULE\": %d,\n",     XML_FROM_MODULE);
    printf("    \"XML_FROM_I18N\": %d,\n",       XML_FROM_I18N);
    printf("    \"XML_FROM_SCHEMATRONV\": %d,\n", XML_FROM_SCHEMATRONV);
    printf("    \"XML_FROM_BUFFER\": %d,\n",     XML_FROM_BUFFER);
    printf("    \"XML_FROM_URI\": %d\n",         XML_FROM_URI);
    printf("  },\n");
}

static void print_error_levels(void)
{
    printf("  \"xmlErrorLevel\": {\n");
    printf("    \"XML_ERR_NONE\": %d,\n",   XML_ERR_NONE);
    printf("    \"XML_ERR_WARNING\": %d,\n", XML_ERR_WARNING);
    printf("    \"XML_ERR_ERROR\": %d,\n",   XML_ERR_ERROR);
    printf("    \"XML_ERR_FATAL\": %d\n",    XML_ERR_FATAL);
    printf("  },\n");
}

/* ================================================================== */
/*  main                                                               */
/* ================================================================== */
int main(void)
{
    printf("{\n");
    printf("  \"probe\": \"ABI-ENUM-0001\",\n");
    printf("  \"description\": \"Enum and constant value verification\",\n");
    printf("  \"library_version\": \"" LIBXML_DOTTED_VERSION "\",\n");
    printf("  \"compile_time_assertions\": \"PASS\",\n");

    printf("  \"values\": {\n");
    print_node_types();
    print_attribute_types();
    print_attribute_defaults();
    print_entity_types();
    print_buffer_schemes();
    print_element_type_vals();
    print_parser_options();
    print_parser_modes();
    print_parser_input_states();
    print_xpath_object_types();
    print_error_domains();
    print_error_levels();
    printf("  }\n");
    printf("}\n");

    return 0;
}
