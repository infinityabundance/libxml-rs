#!/usr/bin/env python3
"""
Generate compatible C headers for libxml-rs from the Rust ABI exports.

This script reads the Rust source files in src/abi/ and generates matching
C header files in include/libxml/ and include/libxslt/.

It ensures perfect ABI alignment by extracting function signatures directly
from the #[no_mangle] extern "C" function declarations.

Usage:
    python3 tools/gen_headers.py
"""

import re
import os
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC_ABI = REPO_ROOT / "src" / "abi"
INCLUDE_LIBXML = REPO_ROOT / "include" / "libxml"
INCLUDE_LIBXSLT = REPO_ROOT / "include" / "libxslt"

# Ensure output directories exist
INCLUDE_LIBXML.mkdir(parents=True, exist_ok=True)
INCLUDE_LIBXSLT.mkdir(parents=True, exist_ok=True)

# ────────────────────────────────────────────────────────────
# Parse exports_xml2.rs to extract function signatures
# ────────────────────────────────────────────────────────────

def parse_exports(filepath):
    """Parse a Rust exports file and extract #[no_mangle] extern "C" function signatures."""
    with open(filepath) as f:
        content = f.read()
    
    functions = []
    
    # Match #[no_mangle] followed by pub unsafe extern "C" fn name(...) -> ReturnType
    # or pub extern "C" fn name(...) -> ReturnType
    pattern = re.compile(
        r'#\[no_mangle\]\s*\n'
        r'(?:///(?:[^\n]*\n)*?)?'  # optional doc comment
        r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+'
        r'(\w+)\s*'
        r'\(([^)]*)\)\s*'
        r'(?:->\s*([^{]+))?',
        re.MULTILINE
    )
    
    for match in pattern.finditer(content):
        name = match.group(1)
        params_str = match.group(2).strip()
        ret_type = match.group(3)
        if ret_type:
            ret_type = ret_type.strip()
        else:
            ret_type = "void"
        
        # Parse parameters
        params = []
        if params_str:
            for p in params_str.split(','):
                p = p.strip()
                if p and not p.startswith('//'):
                    params.append(p)
        
        functions.append({
            'name': name,
            'params': params,
            'ret_type': ret_type,
        })
    
    return functions


def rust_type_to_c(rust_type):
    """Convert a Rust type reference to a C type."""
    type_map = {
        'c_int': 'int',
        'c_uint': 'unsigned int',
        'c_char': 'char',
        'c_void': 'void',
        'c_double': 'double',
        'c_float': 'float',
        'c_long': 'long',
        'c_ulong': 'unsigned long',
        'usize': 'size_t',
        'f64': 'double',
        'c_ushort': 'unsigned short',
        'c_uchar': 'unsigned char',
    }
    
    # Remove whitespace
    t = rust_type.strip()
    
    # Handle pointer types
    if t.endswith('*mut c_char') or t == '*mut c_char':
        return 'char *'
    if t.endswith('*const c_char') or t == '*const c_char':
        return 'const char *'
    if t.endswith('*mut c_void') or t == '*mut c_void':
        return 'void *'
    if t.endswith('*const c_void') or t == '*const c_void':
        return 'const void *'
    if t.endswith('*mut u8') or t == '*mut u8':
        return 'unsigned char *'
    if t.endswith('*const u8') or t == '*const u8':
        return 'const unsigned char *'
    
    # Handle xmlChar* types
    if 'xmlChar' in t and '*' in t:
        if '*mut' in t:
            return 'xmlChar *'
        return 'const xmlChar *'
    
    # Handle *mut _xml* types
    if '_xml' in t and '*mut' in t:
        # e.g., *mut _xmlDoc -> xmlDocPtr
        match = re.search(r'_xml(\w+)', t)
        if match:
            return f'xml{match.group(1)}Ptr'
    
    if '_xml' in t and '*const' in t:
        match = re.search(r'_xml(\w+)', t)
        if match:
            return f'const xml{match.group(1)}Ptr'
    
    # Handle *mut _xslt* types
    if '_xslt' in t and '*mut' in t:
        match = re.search(r'_xslt(\w+)', t)
        if match:
            return f'xslt{match.group(1)}Ptr'
    
    # Handle pointer to pointer types
    if t.startswith('*mut *mut '):
        inner = t.replace('*mut *mut ', '')
        c_inner = rust_type_to_c(inner)
        return f'{c_inner}*' if not c_inner.endswith('*') else f'{c_inner}*'
    
    # Handle *mut _xmlNode (bare pointer type)
    if t.startswith('*mut _xml'):
        match = re.search(r'_xml(\w+)', t)
        if match:
            return f'xml{match.group(1)}Ptr'
    if t.startswith('*const _xml'):
        match = re.search(r'_xml(\w+)', t)
        if match:
            return f'const xml{match.group(1)}Ptr'
    if t.startswith('*mut _xslt'):
        match = re.search(r'_xslt(\w+)', t)
        if match:
            return f'xslt{match.group(1)}Ptr'
    
    # Handle direct struct types
    if t.startswith('*mut _xml'):
        match = re.search(r'_xml(\w+)', t)
        if match:
            return f'xml{match.group(1)}Ptr'
    
    # Handle Option<...> types for callbacks
    if t.startswith('Option<'):
        inner = t[7:-1].strip()
        return rust_callback_to_c(inner)
    
    # Direct types
    if t in type_map:
        return type_map[t]
    
    # Types that are already xml*Ptr
    if t.endswith('Ptr') or t.startswith('xml'):
        return t
    
    # xmlChar
    if t == 'xmlChar':
        return 'xmlChar'
    
    # Default: pass through
    return t


def rust_callback_to_c(rust_cb):
    """Convert a Rust callback type to a C function pointer type."""
    # xmlGenericErrorFunc
    if 'xmlGenericErrorFunc' in rust_cb:
        return 'xmlGenericErrorFunc'
    if 'xmlStructuredErrorFunc' in rust_cb:
        return 'xmlStructuredErrorFunc'
    if 'xmlInputReadCallback' in rust_cb:
        return 'xmlInputReadCallback'
    if 'xmlInputCloseCallback' in rust_cb:
        return 'xmlInputCloseCallback'
    if 'xmlOutputWriteCallback' in rust_cb:
        return 'xmlOutputWriteCallback'
    if 'xmlOutputCloseCallback' in rust_cb:
        return 'xmlOutputCloseCallback'
    
    # Generic: just return the inner type name
    return rust_cb


def rust_param_to_c(param_str):
    """Convert a Rust parameter declaration to a C parameter declaration."""
    param_str = param_str.strip()
    
    # Handle `self: *mut c_void` -> just `void *self`
    if param_str.startswith('self'):
        param_str = re.sub(r'^self\s*:\s*', '', param_str)
        c_type = rust_type_to_c(param_str)
        return f'{c_type} self'
    
    # Handle patterns like `_name: *const xmlChar`
    # or `handler: Option<...>`
    # Split on ': '
    parts = param_str.split(': ', 1)
    if len(parts) == 1:
        # Just a type, no name
        return rust_type_to_c(param_str)
    
    name = parts[0].strip()
    rtype = parts[1].strip()
    
    # Skip leading underscore in parameter names for C
    c_name = name.lstrip('_')
    if not c_name:
        # It was just underscores
        return ''
    
    # Handle `Option<...>` types
    if rtype.startswith('Option<'):
        inner = rtype[7:-1].strip()
        # Check if it's a function pointer
        if inner.startswith('unsafe extern "C" fn'):
            # Extract function pointer type
            fn_match = re.match(
                r'unsafe\s+extern\s+"C"\s+fn\s*'
                r'\(([^)]*)\)\s*'
                r'(?:->\s*([^{]+))?',
                inner
            )
            if fn_match:
                fn_params_str = fn_match.group(1)
                fn_ret = fn_match.group(2)
                fn_ret = fn_ret.strip() if fn_ret else 'void'
                
                # Parse inner params
                inner_params = []
                if fn_params_str.strip():
                    for ip in fn_params_str.split(','):
                        ip = ip.strip()
                        if ip:
                            inner_params.append(rust_param_to_c(ip))
                
                c_fn_params = ', '.join(ip for ip in inner_params if ip)
                c_ret = rust_type_to_c(fn_ret)
                return f'{c_ret} (*{c_name})({c_fn_params})'
        
        c_type = rust_type_to_c(rtype)
        return f'{c_type} {c_name}'
    
    c_type = rust_type_to_c(rtype)
    return f'{c_type} {c_name}'


def generate_header(filename, guard, includes, typedefs, function_decls, extra_content=""):
    """Generate a C header file."""
    lines = []
    lines.append('/**')
    lines.append(' * @file')
    lines.append(' *')
    lines.append(f' * Compatible C header for libxml-rs — {filename}')
    lines.append(' *')
    lines.append(' * Auto-generated from Rust ABI exports. Do not edit directly.')
    lines.append(' */')
    lines.append('')
    lines.append(f'#ifndef {guard}')
    lines.append(f'#define {guard}')
    lines.append('')
    
    for inc in includes:
        lines.append(f'#include {inc}')
    
    if includes:
        lines.append('')
    
    lines.append('#ifdef __cplusplus')
    lines.append('extern "C" {')
    lines.append('#endif')
    lines.append('')
    
    if typedefs:
        for td in typedefs:
            lines.append(td)
        lines.append('')
    
    if extra_content:
        lines.append(extra_content)
        lines.append('')
    
    for decl in function_decls:
        lines.append(f'{decl};')
    
    lines.append('')
    lines.append('#ifdef __cplusplus')
    lines.append('}')
    lines.append('#endif')
    lines.append('')
    lines.append(f'#endif /* {guard} */')
    lines.append('')
    
    return '\n'.join(lines)


# ────────────────────────────────────────────────────────────
# Generate each header file
# ────────────────────────────────────────────────────────────

def gen_xmlexports():
    """Generate include/libxml/xmlexports.h"""
    content = '''/**
 * @file
 *
 * Symbol export/import macros for libxml-rs
 *
 * Auto-generated from Rust ABI exports.
 */

#ifndef __XML_EXPORTS_H__
#define __XML_EXPORTS_H__

/* Symbol visibility */
#if (defined(_WIN32) || defined(__CYGWIN__)) && !defined(LIBXML_STATIC)
  #if defined(IN_LIBXML)
    #define XMLPUBFUN __declspec(dllexport)
    #define XMLPUBVAR __declspec(dllexport) extern
  #else
    #define XMLPUBFUN __declspec(dllimport)
    #define XMLPUBVAR __declspec(dllimport) extern
  #endif
#else /* not Windows */
  #define XMLPUBFUN
  #define XMLPUBVAR extern
#endif /* platform switch */

/* Compatibility */
#define XMLCALL
#define XMLCDECL
#ifndef LIBXML_DLL_IMPORT
  #define LIBXML_DLL_IMPORT XMLPUBVAR
#endif

/* Attributes */
#if !defined(__clang__) && (__GNUC__ * 100 + __GNUC_MINOR__ >= 403)
  #define LIBXML_ATTR_ALLOC_SIZE(x) __attribute__((alloc_size(x)))
#else
  #define LIBXML_ATTR_ALLOC_SIZE(x)
#endif

#if __GNUC__ * 100 + __GNUC_MINOR__ >= 303
  #define LIBXML_ATTR_FORMAT(fmt,args) \\
    __attribute__((__format__(__printf__,fmt,args)))
#else
  #define LIBXML_ATTR_FORMAT(fmt,args)
#endif

#ifndef XML_DEPRECATED
  #if defined(IN_LIBXML)
    #define XML_DEPRECATED
  #elif __GNUC__ * 100 + __GNUC_MINOR__ >= 405
    #define XML_DEPRECATED __attribute__((deprecated("See https://gnome.pages.gitlab.gnome.org/libxml2/html/deprecated.html")))
  #elif __GNUC__ * 100 + __GNUC_MINOR__ >= 301
    #define XML_DEPRECATED __attribute__((deprecated))
  #elif defined(_MSC_VER) && _MSC_VER >= 1400
    #define XML_DEPRECATED __declspec(deprecated("..."))
  #else
    #define XML_DEPRECATED
  #endif
#endif

#ifndef XML_DEPRECATED_MEMBER
  #if defined(IN_LIBXML)
    #define XML_DEPRECATED_MEMBER
  #elif __GNUC__ * 100 + __GNUC_MINOR__ >= 301
    #define XML_DEPRECATED_MEMBER __attribute__((deprecated))
  #else
    #define XML_DEPRECATED_MEMBER
  #endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xmlCheckVersion(int version);

#ifdef __cplusplus
}
#endif

#endif /* __XML_EXPORTS_H__ */
'''
    (INCLUDE_LIBXML / "xmlexports.h").write_text(content)
    print("  Generated include/libxml/xmlexports.h")


def gen_xmlversion():
    """Generate include/libxml/xmlversion.h"""
    content = '''/**
 * @file
 *
 * Compile-time version information for libxml-rs
 */

#ifndef __XML_VERSION_H__
#define __XML_VERSION_H__

#define LIBXML_DOTTED_VERSION "2.15.3"
#define LIBXML_VERSION 21503
#define LIBXML_VERSION_STRING "21503"
#define LIBXML_VERSION_EXTRA ""

/* Feature macros — all enabled by default in libxml-rs */
#define LIBXML_THREAD_ENABLED
#define LIBXML_TREE_ENABLED
#define LIBXML_OUTPUT_ENABLED
#define LIBXML_PUSH_ENABLED
#define LIBXML_READER_ENABLED
#define LIBXML_PATTERN_ENABLED
#define LIBXML_WRITER_ENABLED
#define LIBXML_SAX1_ENABLED
#define LIBXML_VALID_ENABLED
#define LIBXML_HTML_ENABLED
#define LIBXML_C14N_ENABLED
#define LIBXML_CATALOG_ENABLED
#define LIBXML_SGML_CATALOG_ENABLED
#define LIBXML_XPATH_ENABLED
#define LIBXML_XPTR_ENABLED
#define LIBXML_XINCLUDE_ENABLED
#define LIBXML_ICONV_ENABLED
#define LIBXML_ICU_ENABLED
#define LIBXML_ISO8859X_ENABLED
#define LIBXML_DEBUG_ENABLED
#define LIBXML_REGEXP_ENABLED
#define LIBXML_AUTOMATA_ENABLED
#define LIBXML_RELAXNG_ENABLED
#define LIBXML_SCHEMAS_ENABLED
#define LIBXML_SCHEMATRON_ENABLED
#define LIBXML_MODULES_ENABLED
#define LIBXML_MODULE_EXTENSION ".so"
#define LIBXML_ZLIB_ENABLED
#define LIBXML_HTTP_STUBS_ENABLED

/* libxslt version */
#define LIBXSLT_DOTTED_VERSION "1.1.45"
#define LIBXSLT_VERSION 10145
#define LIBXSLT_VERSION_STRING "10145"
#define LIBXSLT_VERSION_EXTRA ""

#define LIBXML_TEST_VERSION xmlCheckVersion(21503);

#include <libxml/xmlexports.h>

#endif /* __XML_VERSION_H__ */
'''
    (INCLUDE_LIBXML / "xmlversion.h").write_text(content)
    print("  Generated include/libxml/xmlversion.h")


def gen_xmlstring():
    """Generate include/libxml/xmlstring.h"""
    content = '''/**
 * @file
 *
 * String utility functions for libxml-rs
 */

#ifndef __XML_STRING_H__
#define __XML_STRING_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned char xmlChar;

XMLPUBFUN xmlChar *xmlStrdup(const xmlChar *cur);
XMLPUBFUN xmlChar *xmlStrndup(const xmlChar *cur, int len);
XMLPUBFUN int xmlStrlen(const xmlChar *str);
XMLPUBFUN int xmlStrcmp(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrncmp(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN int xmlStrcasecmp(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrncasecmp(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN int xmlStrEqual(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrQEqual(const xmlChar *pref, const xmlChar *name, const xmlChar *str);
XMLPUBFUN xmlChar *xmlStrcat(xmlChar *cur, const xmlChar *add);
XMLPUBFUN xmlChar *xmlStrncat(xmlChar *cur, const xmlChar *add, int len);
XMLPUBFUN xmlChar *xmlStrncatNew(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN xmlChar *xmlStrcpy(xmlChar *dst, const xmlChar *src);
XMLPUBFUN xmlChar *xmlStrncpy(xmlChar *dst, const xmlChar *src, int len);
XMLPUBFUN xmlChar *xmlStrsub(const xmlChar *str, int start, int len);

#ifdef __cplusplus
}
#endif

#endif /* __XML_STRING_H__ */
'''
    (INCLUDE_LIBXML / "xmlstring.h").write_text(content)
    print("  Generated include/libxml/xmlstring.h")


def gen_xmlmemory():
    """Generate include/libxml/xmlmemory.h"""
    content = '''/**
 * @file
 *
 * Memory allocator interface for libxml-rs
 */

#ifndef __DEBUG_MEMORY_ALLOC__
#define __DEBUG_MEMORY_ALLOC__

#include <stdio.h>
#include <stdlib.h>
#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*xmlFreeFunc)(void *mem);
typedef void *(*xmlMallocFunc)(size_t size);
typedef void *(*xmlReallocFunc)(void *mem, size_t size);
typedef char *(*xmlStrdupFunc)(const char *str);

XMLPUBVAR xmlMallocFunc xmlMalloc;
XMLPUBVAR xmlMallocFunc xmlMallocAtomic;
XMLPUBVAR xmlReallocFunc xmlRealloc;
XMLPUBVAR xmlFreeFunc xmlFree;
XMLPUBVAR xmlStrdupFunc xmlMemStrdup;

XMLPUBFUN int xmlMemSetup(xmlFreeFunc freeFunc,
                           xmlMallocFunc mallocFunc,
                           xmlReallocFunc reallocFunc,
                           xmlStrdupFunc strdupFunc);
XMLPUBFUN int xmlMemGet(xmlFreeFunc *freeFunc,
                         xmlMallocFunc *mallocFunc,
                         xmlReallocFunc *reallocFunc,
                         xmlStrdupFunc *strdupFunc);
XMLPUBFUN int xmlGcMemSetup(xmlFreeFunc freeFunc,
                             xmlMallocFunc mallocFunc,
                             xmlMallocFunc mallocAtomicFunc,
                             xmlReallocFunc reallocFunc,
                             xmlStrdupFunc strdupFunc);
XMLPUBFUN int xmlGcMemGet(xmlFreeFunc *freeFunc,
                           xmlMallocFunc *mallocFunc,
                           xmlMallocFunc *mallocAtomicFunc,
                           xmlReallocFunc *reallocFunc,
                           xmlStrdupFunc *strdupFunc);
XMLPUBFUN int xmlInitMemory(void);
XMLPUBFUN void xmlCleanupMemory(void);
XMLPUBFUN size_t xmlMemSize(void *ptr);
XMLPUBFUN int xmlMemUsed(void);
XMLPUBFUN int xmlMemBlocks(void);
XMLPUBFUN void xmlMemDisplay(FILE *fp);
XMLPUBFUN void xmlMemDisplayLast(FILE *fp, long nbBytes);
XMLPUBFUN void xmlMemShow(FILE *fp, int nr);
XMLPUBFUN void xmlMemoryDump(void);
XMLPUBFUN void *xmlMemMalloc(size_t size);
XMLPUBFUN void *xmlMemRealloc(void *ptr, size_t size);
XMLPUBFUN void xmlMemFree(void *ptr);
XMLPUBFUN char *xmlMemoryStrdup(const char *str);
XMLPUBFUN void *xmlMallocLoc(size_t size, const char *file, int line);
XMLPUBFUN void *xmlReallocLoc(void *ptr, size_t size, const char *file, int line);
XMLPUBFUN void *xmlMallocAtomicLoc(size_t size, const char *file, int line);
XMLPUBFUN char *xmlMemStrdupLoc(const char *str, const char *file, int line);

#ifdef __cplusplus
}
#endif

#endif /* __DEBUG_MEMORY_ALLOC__ */
'''
    (INCLUDE_LIBXML / "xmlmemory.h").write_text(content)
    print("  Generated include/libxml/xmlmemory.h")


def gen_xmlerror():
    """Generate include/libxml/xmlerror.h"""
    content = '''/**
 * @file
 *
 * Error handling API for libxml-rs
 */

#ifndef __XML_ERROR_H__
#define __XML_ERROR_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    XML_ERR_NONE = 0,
    XML_ERR_WARNING = 1,
    XML_ERR_ERROR = 2,
    XML_ERR_FATAL = 3
} xmlErrorLevel;

/* Error domains */
#define XML_FROM_NONE 0
#define XML_FROM_PARSER 1
#define XML_FROM_TREE 2
#define XML_FROM_NAMESPACE 3
#define XML_FROM_DTD 4
#define XML_FROM_HTML 5
#define XML_FROM_MEMORY 6
#define XML_FROM_OUTPUT 7
#define XML_FROM_IO 8
#define XML_FROM_FTP 9
#define XML_FROM_HTTP 10
#define XML_FROM_XINCLUDE 11
#define XML_FROM_XPATH 12
#define XML_FROM_XPOINTER 13
#define XML_FROM_REGEXP 14
#define XML_FROM_DATATYPE 15
#define XML_FROM_SCHEMASP 16
#define XML_FROM_SCHEMASV 17
#define XML_FROM_RELAXNGP 18
#define XML_FROM_RELAXNGV 19
#define XML_FROM_CATALOG 20
#define XML_FROM_C14N 21
#define XML_FROM_XSLT 22
#define XML_FROM_VALID 23
#define XML_FROM_CHECK 24
#define XML_FROM_WRITER 25
#define XML_FROM_MODULE 26
#define XML_FROM_I18N 27
#define XML_FROM_SCHEMATRONV 28
#define XML_FROM_BUFFER 29
#define XML_FROM_URI 30

/* Error codes */
#define XML_ERR_OK 0
#define XML_ERR_INTERNAL_ERROR 1
#define XML_ERR_NO_MEMORY 2
#define XML_ERR_DOCUMENT_START 3
#define XML_ERR_DOCUMENT_EMPTY 4
#define XML_ERR_DOCUMENT_END 5
#define XML_ERR_INVALID_HEX_CHARREF 6
#define XML_ERR_INVALID_DEC_CHARREF 7
#define XML_ERR_INVALID_CHARREF 8
#define XML_ERR_INVALID_CHAR 9
#define XML_ERR_CHARREF_AT_EOF 10
#define XML_ERR_CHARREF_IN_PROLOG 11
#define XML_ERR_CHARREF_IN_EPILOG 12
#define XML_ERR_CHARREF_IN_DTD 13
#define XML_ERR_ENTITYREF_AT_EOF 14
#define XML_ERR_ENTITYREF_IN_PROLOG 15
#define XML_ERR_ENTITYREF_IN_EPILOG 16
#define XML_ERR_ENTITYREF_IN_DTD 17
#define XML_ERR_PEREF_AT_EOF 18
#define XML_ERR_PEREF_IN_PROLOG 19
#define XML_ERR_PEREF_IN_EPILOG 20
#define XML_ERR_PEREF_IN_INT_SUBSET 21
#define XML_ERR_ENTITYREF_NO_NAME 22
#define XML_ERR_ENTITYREF_SEMICOL_MISSING 23
#define XML_ERR_PEREF_NO_NAME 24
#define XML_ERR_PEREF_SEMICOL_MISSING 25
#define XML_ERR_UNDECLARED_ENTITY 26
#define XML_WAR_UNDECLARED_ENTITY 27
#define XML_ERR_UNPARSED_ENTITY 28
#define XML_ERR_ENTITY_IS_EXTERNAL 29
#define XML_ERR_ENTITY_IS_PARAMETER 30
#define XML_ERR_UNKNOWN_ENCODING 31
#define XML_ERR_UNSUPPORTED_ENCODING 32
#define XML_ERR_STRING_NOT_STARTED 33
#define XML_ERR_STRING_NOT_CLOSED 34
#define XML_ERR_NS_DECL_ERROR 35
#define XML_ERR_ENTITY_NOT_STARTED 36
#define XML_ERR_ENTITY_NOT_FINISHED 37
#define XML_ERR_LT_IN_ATTRIBUTE 38
#define XML_ERR_ATTRIBUTE_NOT_STARTED 39
#define XML_ERR_ATTRIBUTE_NOT_FINISHED 40
#define XML_ERR_ATTRIBUTE_WITHOUT_VALUE 41
#define XML_ERR_ATTRIBUTE_REDEFINED 42
#define XML_ERR_LITERAL_NOT_STARTED 43
#define XML_ERR_LITERAL_NOT_FINISHED 44
#define XML_ERR_COMMENT_NOT_FINISHED 45
#define XML_ERR_PI_NOT_STARTED 46
#define XML_ERR_PI_NOT_FINISHED 47
#define XML_ERR_NOTATION_NOT_STARTED 48
#define XML_ERR_NOTATION_NOT_FINISHED 49
#define XML_ERR_ATTLIST_NOT_STARTED 50
#define XML_ERR_ATTLIST_NOT_FINISHED 51
#define XML_ERR_MIXED_NOT_STARTED 52
#define XML_ERR_MIXED_NOT_FINISHED 53
#define XML_ERR_ELEMCONTENT_NOT_STARTED 54
#define XML_ERR_ELEMCONTENT_NOT_FINISHED 55
#define XML_ERR_XMLDECL_NOT_STARTED 56
#define XML_ERR_XMLDECL_NOT_FINISHED 57
#define XML_ERR_CONDSEC_NOT_STARTED 58
#define XML_ERR_CONDSEC_NOT_FINISHED 59
#define XML_ERR_EXT_SUBSET_NOT_FINISHED 60
#define XML_ERR_DOCTYPE_NOT_FINISHED 61
#define XML_ERR_MISPLACED_CDATA_END 62
#define XML_ERR_CDATA_NOT_FINISHED 63
#define XML_ERR_MISPLACED_XML_PI 64
#define XML_ERR_XML_DECL_AT_EOF 65
#define XML_ERR_XML_DECL_IN_PROLOG 66
#define XML_ERR_XML_DECL_IN_EPILOG 67
#define XML_ERR_XML_DECL_IN_DTD 68
#define XML_ERR_NOT_WELL_BALANCED 69
#define XML_ERR_EXTRA_CONTENT 70
#define XML_ERR_INVALID_ENCODING 71
#define XML_ERR_ENTITY_CHAR_ERROR 72
#define XML_ERR_ENTITY_PE_INTERNAL 73
#define XML_ERR_ENTITY_LOOP 74
#define XML_ERR_ENTITY_BOUNDARY 75
#define XML_ERR_INVALID_URI 76
#define XML_ERR_URI_FRAGMENT 77
#define XML_WAR_CATALOG_PI 78
#define XML_ERR_NO_DTD 79
#define XML_ERR_CONDSEC_INVALID 80
#define XML_ERR_CONDSEC_INVALID_KEYWORD 81
#define XML_ERR_INVALID_DECIMAL 82
#define XML_ERR_INVALID_HEXIDECIMAL 83
#define XML_ERR_INVALID_UNICODE 84
#define XML_ERR_INVALID_NMTOKEN 85
#define XML_ERR_INVALID_NAME 86
#define XML_ERR_NAME_TOO_LONG 87
#define XML_ERR_INVALID_ENUM 88
#define XML_ERR_SPACE_REQUIRED 89
#define XML_ERR_NAME_REQUIRED 90
#define XML_ERR_NMTOKEN_REQUIRED 91
#define XML_ERR_ATTRIBUTE_NOT_RESOLVED 92
#define XML_ERR_LT_REQUIRED 93
#define XML_ERR_GT_REQUIRED 94
#define XML_ERR_TAG_NAME_MISMATCH 95
#define XML_ERR_TAG_NOT_FINISHED 96
#define XML_ERR_STANDALONE_VALUE 97
#define XML_ERR_VERSION_MISSING 98

/* Error structure */
typedef struct _xmlError xmlError;
typedef xmlError *xmlErrorPtr;
struct _xmlError {
    int domain;
    int code;
    char *message;
    int level;
    char *file;
    int line;
    char *str1;
    char *str2;
    char *str3;
    int int1;
    int int2;
    void *ctxt;
    void *node;
};

/* Callback types */
typedef void (*xmlGenericErrorFunc)(void *ctx, const char *msg, ...);
typedef void (*xmlStructuredErrorFunc)(void *ctx, xmlErrorPtr error);

/* Functions */
XMLPUBFUN void xmlSetGenericErrorFunc(void *ctx, xmlGenericErrorFunc handler);
XMLPUBFUN void xmlSetStructuredErrorFunc(void *ctx, xmlStructuredErrorFunc handler);
XMLPUBFUN xmlErrorPtr xmlGetLastError(void);
XMLPUBFUN int xmlCopyError(const xmlError *from, xmlError *to);
XMLPUBFUN void xmlResetError(xmlError *err);
XMLPUBFUN void xmlResetLastError(void);

#ifdef __cplusplus
}
#endif

#endif /* __XML_ERROR_H__ */
'''
    (INCLUDE_LIBXML / "xmlerror.h").write_text(content)
    print("  Generated include/libxml/xmlerror.h")


def gen_tree():
    """Generate include/libxml/tree.h"""
    content = '''/**
 * @file
 *
 * Document tree API for libxml-rs
 */

#ifndef __XML_TREE_H__
#define __XML_TREE_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/xmlmemory.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Forward declarations */
typedef struct _xmlBuffer xmlBuffer;
typedef xmlBuffer *xmlBufferPtr;

typedef struct _xmlBuf xmlBuf;
typedef xmlBuf *xmlBufPtr;

typedef struct _xmlNode xmlNode;
typedef xmlNode *xmlNodePtr;

typedef struct _xmlDoc xmlDoc;
typedef xmlDoc *xmlDocPtr;

typedef struct _xmlNs xmlNs;
typedef xmlNs *xmlNsPtr;

typedef struct _xmlAttr xmlAttr;
typedef xmlAttr *xmlAttrPtr;

typedef struct _xmlDtd xmlDtd;
typedef xmlDtd *xmlDtdPtr;

typedef struct _xmlEntity xmlEntity;
typedef xmlEntity *xmlEntityPtr;

typedef struct _xmlNotation xmlNotation;
typedef xmlNotation *xmlNotationPtr;

typedef struct _xmlElementContent xmlElementContent;
typedef xmlElementContent *xmlElementContentPtr;

typedef struct _xmlAttribute xmlAttribute;
typedef xmlAttribute *xmlAttributePtr;

typedef struct _xmlEnumeration xmlEnumeration;
typedef xmlEnumeration *xmlEnumerationPtr;

/* Element types */
typedef enum {
    XML_ELEMENT_NODE = 1,
    XML_ATTRIBUTE_NODE = 2,
    XML_TEXT_NODE = 3,
    XML_CDATA_SECTION_NODE = 4,
    XML_ENTITY_REF_NODE = 5,
    XML_ENTITY_NODE = 6,
    XML_PI_NODE = 7,
    XML_COMMENT_NODE = 8,
    XML_DOCUMENT_NODE = 9,
    XML_DOCUMENT_TYPE_NODE = 10,
    XML_DOCUMENT_FRAG_NODE = 11,
    XML_NOTATION_NODE = 12,
    XML_HTML_DOCUMENT_NODE = 13,
    XML_DTD_NODE = 14,
    XML_ELEMENT_DECL = 15,
    XML_ATTRIBUTE_DECL = 16,
    XML_ENTITY_DECL = 17,
    XML_NAMESPACE_DECL = 18,
    XML_XINCLUDE_START = 19,
    XML_XINCLUDE_END = 20
} xmlElementType;

typedef enum {
    XML_ATTRIBUTE_CDATA = 1,
    XML_ATTRIBUTE_ID = 2,
    XML_ATTRIBUTE_IDREF = 3,
    XML_ATTRIBUTE_IDREFS = 4,
    XML_ATTRIBUTE_ENTITY = 5,
    XML_ATTRIBUTE_ENTITIES = 6,
    XML_ATTRIBUTE_NMTOKEN = 7,
    XML_ATTRIBUTE_NMTOKENS = 8,
    XML_ATTRIBUTE_ENUMERATION = 9,
    XML_ATTRIBUTE_NOTATION = 10
} xmlAttributeType;

typedef enum {
    XML_ATTRIBUTE_NONE = 1,
    XML_ATTRIBUTE_REQUIRED = 2,
    XML_ATTRIBUTE_IMPLIED = 3,
    XML_ATTRIBUTE_FIXED = 4
} xmlAttributeDefault;

typedef enum {
    XML_INTERNAL_GENERAL_ENTITY = 1,
    XML_EXTERNAL_GENERAL_PARSED_ENTITY = 2,
    XML_EXTERNAL_GENERAL_UNPARSED_ENTITY = 3,
    XML_INTERNAL_PARAMETER_ENTITY = 4,
    XML_EXTERNAL_PARAMETER_ENTITY = 5,
    XML_INTERNAL_PREDEFINED_ENTITY = 6
} xmlEntityType;

typedef enum {
    XML_BUFFER_ALLOC_DOUBLEIT,
    XML_BUFFER_ALLOC_EXACT,
    XML_BUFFER_ALLOC_IMMUTABLE,
    XML_BUFFER_ALLOC_IO,
    XML_BUFFER_ALLOC_HYBRID,
    XML_BUFFER_ALLOC_BOUNDED
} xmlBufferAllocationScheme;

/* Document properties */
#define XML_DOC_WELLFORMED 1
#define XML_DOC_NSVALID 2
#define XML_DOC_OLD10 4
#define XML_DOC_DTDVALID 8
#define XML_DOC_XINCLUDE 16
#define XML_DOC_USERBUILT 32
#define XML_DOC_INTERNAL 64
#define XML_DOC_HTML 128

/* Well-known namespaces */
#define XML_XML_NAMESPACE ((const xmlChar *) "http://www.w3.org/XML/1998/namespace")
#define XML_XMLNS_NAMESPACE ((const xmlChar *) "http://www.w3.org/2000/xmlns/")
#define XML_XMLNS_PREFIX ((const xmlChar *) "xmlns")

/* Limits */
#define XML_MAX_TEXT_LENGTH 1000000000
#define XML_MAX_NAME_LENGTH 50000
#define XML_MAX_DICTIONARY_LIMIT 10000000
#define XML_MAX_LOOKUP_LIMIT 1000000
#define XML_MAX_HUGE_LENGTH 100000000
#define XML_MAX_NAMELEN 50000
#define XML_MAX_ATTRIBUTE_LENGTH 50000

/* Buffer structure */
struct _xmlBuffer {
    xmlChar *content;
    unsigned int use;
    unsigned int size;
    xmlBufferAllocationScheme alloc;
    xmlChar *contentIO;
};

/* Node structure */
struct _xmlNode {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    xmlNsPtr ns;
    xmlChar *content;
    xmlAttrPtr properties;
    xmlNsPtr nsDef;
    void *psvi;
    unsigned short line;
    unsigned short extra;
};

/* Attribute structure */
struct _xmlAttr {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlAttrPtr next;
    xmlAttrPtr prev;
    xmlDocPtr doc;
    xmlNsPtr ns;
    int atype;
    void *psvi;
    int id;
};

/* Namespace structure */
struct _xmlNs {
    xmlNsPtr next;
    int type;
    xmlChar *href;
    xmlChar *prefix;
    void *_private;
    void *context;
};

/* Document structure */
struct _xmlDoc {
    void *_private;
    int type;
    char *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    int compression;
    int standalone;
    xmlDtdPtr intSubset;
    xmlDtdPtr extSubset;
    xmlNsPtr oldNs;
    const xmlChar *version;
    const xmlChar *encoding;
    void *ids;
    void *refs;
    const xmlChar *URL;
    int charset;
    void *dict;
    void *psvi;
    int parseFlags;
    int properties;
};

/* DTD structure */
struct _xmlDtd {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    void *notations;
    void *elements;
    void *attributes;
    void *entities;
    const xmlChar *ExternalID;
    const xmlChar *SystemID;
    void *pentities;
};

/* Entity structure */
struct _xmlEntity {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    xmlChar *orig;
    xmlChar *content;
    int length;
    int etype;
    const xmlChar *ExternalID;
    const xmlChar *SystemID;
    xmlEntityPtr nexte;
    const xmlChar *URI;
    int owner;
    int flags;
    int expandedSize;
};

/* Notation structure */
struct _xmlNotation {
    const xmlChar *name;
    const xmlChar *PublicID;
    const xmlChar *SystemID;
};

/* Element content structure */
struct _xmlElementContent {
    int type;
    int ocur;
    const xmlChar *name;
    xmlElementContentPtr c1;
    xmlElementContentPtr c2;
    xmlElementContentPtr parent;
    const xmlChar *prefix;
};

/* Enumeration structure */
struct _xmlEnumeration {
    struct _xmlEnumeration *next;
    const xmlChar *name;
};

/* Attribute declaration structure */
struct _xmlAttribute {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    xmlAttributePtr nexth;
    int atype;
    int def;
    const xmlChar *defaultValue;
    xmlEnumerationPtr tree;
    const xmlChar *prefix;
    const xmlChar *elem;
};

/* Element declaration types */
typedef enum {
    XML_ELEMENT_TYPE_UNDEFINED = 0,
    XML_ELEMENT_TYPE_EMPTY = 1,
    XML_ELEMENT_TYPE_ANY = 2,
    XML_ELEMENT_TYPE_MIXED = 3,
    XML_ELEMENT_TYPE_ELEMENT = 4
} xmlElementTypeVal;

/* Namespace type */
#define XML_LOCAL_NAMESPACE 0
typedef int xmlNsType;

/* Tree functions */
XMLPUBFUN xmlDocPtr xmlNewDoc(const xmlChar *version);
XMLPUBFUN void xmlFreeDoc(xmlDocPtr doc);
XMLPUBFUN xmlDocPtr xmlCopyDoc(const xmlDocPtr doc, int recursive);
XMLPUBFUN xmlNodePtr xmlNewNode(xmlNsPtr ns, const xmlChar *name);
XMLPUBFUN void xmlFreeNode(xmlNodePtr node);
XMLPUBFUN void xmlFreeNodeList(xmlNodePtr node);
XMLPUBFUN xmlNodePtr xmlCopyNode(const xmlNodePtr node, int extended);
XMLPUBFUN void xmlUnlinkNode(xmlNodePtr node);
XMLPUBFUN xmlNodePtr xmlAddChild(xmlNodePtr parent, xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlAddSibling(xmlNodePtr cur, xmlNodePtr sibling);
XMLPUBFUN xmlNodePtr xmlNewChild(xmlNodePtr parent, xmlNsPtr ns,
                                  const xmlChar *name, const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewText(const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewComment(const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewPI(const xmlChar *name, const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewCDataBlock(xmlDocPtr doc, const xmlChar *content, int len);
XMLPUBFUN xmlNodePtr xmlDocSetRootElement(xmlDocPtr doc, xmlNodePtr root);
XMLPUBFUN xmlNodePtr xmlDocGetRootElement(const xmlDoc *doc);
XMLPUBFUN long xmlGetLineNo(const xmlNode *node);
XMLPUBFUN xmlNsPtr xmlNewNs(xmlNodePtr node, const xmlChar *href, const xmlChar *prefix);
XMLPUBFUN void xmlSetNs(xmlNodePtr node, xmlNsPtr ns);
XMLPUBFUN xmlNsPtr *xmlGetNsList(xmlDocPtr doc, const xmlNode *node);
XMLPUBFUN xmlNsPtr xmlSearchNs(xmlDocPtr doc, xmlNodePtr node, const xmlChar *nameSpace);
XMLPUBFUN xmlNsPtr xmlSearchNsByHref(xmlDocPtr doc, xmlNodePtr node, const xmlChar *href);
XMLPUBFUN xmlAttrPtr xmlSetProp(xmlNodePtr node, const xmlChar *name, const xmlChar *value);
XMLPUBFUN xmlChar *xmlGetProp(const xmlNode *node, const xmlChar *name);
XMLPUBFUN xmlChar *xmlGetNsProp(const xmlNode *node, const xmlChar *name, const xmlChar *nameSpace);
XMLPUBFUN xmlAttrPtr xmlSetNsProp(xmlNodePtr node, xmlNsPtr ns,
                                   const xmlChar *name, const xmlChar *value);
XMLPUBFUN int xmlRemoveProp(xmlAttrPtr attr);
XMLPUBFUN xmlDtdPtr xmlGetIntSubset(const xmlDoc *doc);
XMLPUBFUN xmlDtdPtr xmlNewDtd(xmlDocPtr doc, const xmlChar *name,
                               const xmlChar *ExternalID, const xmlChar *SystemID);
XMLPUBFUN xmlEntityPtr xmlNewEntity(xmlDocPtr doc, const xmlChar *name, int type,
                                     const xmlChar *ExternalID, const xmlChar *SystemID,
                                     const xmlChar *content);
XMLPUBFUN xmlEntityPtr xmlGetDocEntity(const xmlDoc *doc, const xmlChar *name);
XMLPUBFUN xmlEntityPtr xmlGetParameterEntity(const xmlDoc *doc, const xmlChar *name);
XMLPUBFUN xmlBufferPtr xmlBufferCreate(void);
XMLPUBFUN xmlBufferPtr xmlBufferCreateSize(size_t size);
XMLPUBFUN xmlBufferPtr xmlBufferCreateStatic(void *mem, size_t size);
XMLPUBFUN void xmlBufferFree(xmlBufferPtr buf);
XMLPUBFUN void xmlBufferEmpty(xmlBufferPtr buf);
XMLPUBFUN xmlChar *xmlBufferContent(const xmlBuffer *buf);
XMLPUBFUN int xmlBufferLength(const xmlBuffer *buf);
XMLPUBFUN int xmlBufferAdd(xmlBufferPtr buf, const xmlChar *str, int len);
XMLPUBFUN int xmlBufferAddHead(xmlBufferPtr buf, const xmlChar *str, int len);
XMLPUBFUN void xmlBufferSetAllocationScheme(xmlBufferPtr buf, int scheme);
XMLPUBFUN int xmlBufferShrink(xmlBufferPtr buf, int len);
XMLPUBFUN int xmlBufferGrow(xmlBufferPtr buf, int len);
XMLPUBFUN int xmlBufferReserve(xmlBufferPtr buf, int len);
XMLPUBFUN xmlChar *xmlBufferDetach(xmlBufferPtr buf);

#ifdef __cplusplus
}
#endif

#endif /* __XML_TREE_H__ */
'''
    (INCLUDE_LIBXML / "tree.h").write_text(content)
    print("  Generated include/libxml/tree.h")


def gen_dict():
    """Generate include/libxml/dict.h"""
    content = '''/**
 * @file
 *
 * Dictionary API for libxml-rs
 */

#ifndef __XML_DICT_H__
#define __XML_DICT_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlDict xmlDict;
typedef xmlDict *xmlDictPtr;

XMLPUBFUN xmlDictPtr xmlDictCreate(void);
XMLPUBFUN xmlDictPtr xmlDictCreateSub(xmlDictPtr sub);
XMLPUBFUN const xmlChar *xmlDictLookup(xmlDictPtr dict, const xmlChar *name, int len);
XMLPUBFUN const xmlChar *xmlDictExists(xmlDictPtr dict, const xmlChar *name, int len);
XMLPUBFUN unsigned int xmlDictSize(const xmlDictPtr dict);
XMLPUBFUN void xmlDictFree(xmlDictPtr dict);
XMLPUBFUN unsigned int xmlDictSetLimit(xmlDictPtr dict, unsigned int limit);
XMLPUBFUN unsigned int xmlDictGetUsage(const xmlDictPtr dict);

#ifdef __cplusplus
}
#endif

#endif /* __XML_DICT_H__ */
'''
    (INCLUDE_LIBXML / "dict.h").write_text(content)
    print("  Generated include/libxml/dict.h")


def gen_hash():
    """Generate include/libxml/hash.h"""
    content = '''/**
 * @file
 *
 * Hash table API for libxml-rs
 */

#ifndef __XML_HASH_H__
#define __XML_HASH_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlHashTable xmlHashTable;
typedef xmlHashTable *xmlHashTablePtr;

typedef void (*xmlHashDeallocator)(void *payload, xmlChar *name);
typedef void *(*xmlHashCopier)(void *payload, xmlChar *name);
typedef void (*xmlHashScanner)(void *payload, void *data, xmlChar *name);
typedef void (*xmlHashScannerFull)(void *payload, void *data, xmlChar *name, xmlChar *name2, xmlChar *name3);

XMLPUBFUN xmlHashTablePtr xmlHashCreate(int size);
XMLPUBFUN xmlHashTablePtr xmlHashCreateDict(int size, xmlDictPtr dict);
XMLPUBFUN void xmlHashFree(xmlHashTablePtr table, xmlHashDeallocator f);
XMLPUBFUN int xmlHashAddEntry(xmlHashTablePtr table, const xmlChar *name, void *userdata);
XMLPUBFUN int xmlHashAddEntry2(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2, void *userdata);
XMLPUBFUN int xmlHashAddEntry3(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2, const xmlChar *name3, void *userdata);
XMLPUBFUN int xmlHashUpdateEntry(xmlHashTablePtr table, const xmlChar *name,
                                  void *userdata, xmlHashDeallocator f);
XMLPUBFUN int xmlHashUpdateEntry2(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, void *userdata, xmlHashDeallocator f);
XMLPUBFUN int xmlHashUpdateEntry3(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, const xmlChar *name3,
                                   void *userdata, xmlHashDeallocator f);
XMLPUBFUN void *xmlHashLookup(xmlHashTablePtr table, const xmlChar *name);
XMLPUBFUN void *xmlHashLookup2(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2);
XMLPUBFUN void *xmlHashLookup3(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2, const xmlChar *name3);
XMLPUBFUN int xmlHashSize(xmlHashTablePtr table);
XMLPUBFUN int xmlHashRemoveEntry(xmlHashTablePtr table, const xmlChar *name,
                                  xmlHashDeallocator f);
XMLPUBFUN int xmlHashRemoveEntry2(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, xmlHashDeallocator f);
XMLPUBFUN int xmlHashRemoveEntry3(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, const xmlChar *name3,
                                   xmlHashDeallocator f);
XMLPUBFUN void xmlHashScan(xmlHashTablePtr table, xmlHashScanner f, void *data);
XMLPUBFUN void xmlHashScanFull(xmlHashTablePtr table, xmlHashScannerFull f, void *data);
XMLPUBFUN xmlHashTablePtr xmlHashCopy(xmlHashTablePtr table, xmlHashCopier f);

#ifdef __cplusplus
}
#endif

#endif /* __XML_HASH_H__ */
'''
    (INCLUDE_LIBXML / "hash.h").write_text(content)
    print("  Generated include/libxml/hash.h")


def gen_list():
    """Generate include/libxml/list.h"""
    content = '''/**
 * @file
 *
 * Linked list API for libxml-rs
 */

#ifndef __XML_LIST_H__
#define __XML_LIST_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlList xmlList;
typedef xmlList *xmlListPtr;

typedef void (*xmlListDeallocator)(void *data);
typedef int (*xmlListDataCompare)(const void *data1, const void *data2);
typedef int (*xmlListWalker)(void *data, void *user_data);

XMLPUBFUN xmlListPtr xmlListCreate(xmlListDeallocator deallocator,
                                    xmlListDataCompare compare);
XMLPUBFUN void xmlListDelete(xmlListPtr list);
XMLPUBFUN void *xmlListSearch(xmlListPtr list, void *data);
XMLPUBFUN void xmlListWalk(xmlListPtr list, xmlListWalker walker, void *data);
XMLPUBFUN int xmlListPushBack(xmlListPtr list, void *data);
XMLPUBFUN int xmlListPushFront(xmlListPtr list, void *data);
XMLPUBFUN void xmlListPopBack(xmlListPtr list);
XMLPUBFUN void xmlListPopFront(xmlListPtr list);
XMLPUBFUN int xmlListInsert(xmlListPtr list, void *data);
XMLPUBFUN int xmlListAppend(xmlListPtr list, void *data);
XMLPUBFUN int xmlListRemoveFirst(xmlListPtr list, void *data);
XMLPUBFUN int xmlListRemoveLast(xmlListPtr list, void *data);
XMLPUBFUN int xmlListRemoveAll(xmlListPtr list, void *data);
XMLPUBFUN void xmlListClear(xmlListPtr list);
XMLPUBFUN int xmlListEmpty(xmlListPtr list);
XMLPUBFUN void *xmlListFront(xmlListPtr list);
XMLPUBFUN void *xmlListBack(xmlListPtr list);
XMLPUBFUN int xmlListSize(xmlListPtr list);
XMLPUBFUN void xmlListSort(xmlListPtr list);
XMLPUBFUN void xmlListReverse(xmlListPtr list);
XMLPUBFUN void xmlListReverseSplice(xmlListPtr list, xmlListPtr list2);
XMLPUBFUN void xmlListMerge(xmlListPtr list, xmlListPtr list2);

#ifdef __cplusplus
}
#endif

#endif /* __XML_LIST_H__ */
'''
    (INCLUDE_LIBXML / "list.h").write_text(content)
    print("  Generated include/libxml/list.h")


def gen_parser():
    """Generate include/libxml/parser.h"""
    content = '''/**
 * @file
 *
 * XML parser API for libxml-rs
 */

#ifndef __XML_PARSER_H__
#define __XML_PARSER_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/xmlmemory.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxml/dict.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Parser options */
#define XML_PARSE_RECOVER 1
#define XML_PARSE_NOENT 2
#define XML_PARSE_DTDLOAD 4
#define XML_PARSE_DTDATTR 8
#define XML_PARSE_DTDVALID 16
#define XML_PARSE_NOERROR 32
#define XML_PARSE_NOWARNING 64
#define XML_PARSE_PEDANTIC 128
#define XML_PARSE_NOBLANKS 256
#define XML_PARSE_SAX1 512
#define XML_PARSE_XINCLUDE 1024
#define XML_PARSE_NONET 2048
#define XML_PARSE_NODICT 4096
#define XML_PARSE_NSCLEAN 8192
#define XML_PARSE_NOCDATA 16384
#define XML_PARSE_NOXINCNODE 32768
#define XML_PARSE_COMPACT 65536
#define XML_PARSE_OLD10 131072
#define XML_PARSE_NOBASEFIX 262144
#define XML_PARSE_HUGE 524288
#define XML_PARSE_OLDSAX 1048576
#define XML_PARSE_IGNORE_ENC 2097152
#define XML_PARSE_BIG_LINES 4194304

/* Parser mode */
typedef enum {
    XML_PARSE_UNKNOWN = 0,
    XML_PARSE_DOM = 1,
    XML_PARSE_SAX = 2,
    XML_PARSE_PUSH_DOM = 3,
    XML_PARSE_PUSH_SAX = 4,
    XML_PARSE_READER = 5
} xmlParserMode;

/* Parser input states */
typedef enum {
    XML_PARSER_EOF = -1,
    XML_PARSER_START = 0,
    XML_PARSER_MISC = 1,
    XML_PARSER_DTD = 2,
    XML_PARSER_PROLOG = 3,
    XML_PARSER_CONTENT = 4,
    XML_PARSER_CDATA_SECTION = 5,
    XML_PARSER_ENTITY_REF = 6,
    XML_PARSER_ENTITY_VALUE = 7,
    XML_PARSER_ATTRIBUTE_VALUE = 8,
    XML_PARSER_SYSTEM_LITERAL = 9,
    XML_PARSER_EPILOG = 10,
    XML_PARSER_IGNORE = 11,
    XML_PARSER_PUBLIC_LITERAL = 12
} xmlParserInputState;

/* Parser input structure */
typedef struct _xmlParserInput xmlParserInput;
typedef xmlParserInput *xmlParserInputPtr;
struct _xmlParserInput {
    xmlParserInputBufferPtr buf;
    const char *filename;
    const char *directory;
    const xmlChar *base;
    const xmlChar *cur;
    const xmlChar *end;
    int length;
    int line;
    int col;
    unsigned long consumed;
    xmlFreeFunc free;
    const char *encoding;
    const xmlChar *version;
    int flags;
    int id;
    unsigned long parentConsumed;
    xmlEntityPtr entity;
};

/* Parser input buffer */
typedef struct _xmlParserInputBuffer xmlParserInputBuffer;
typedef xmlParserInputBuffer *xmlParserInputBufferPtr;
struct _xmlParserInputBuffer {
    void *context;
    xmlInputReadCallback readcallback;
    xmlInputCloseCallback closecallback;
    xmlCharEncodingHandlerPtr encoder;
    xmlBufferPtr buffer;
    xmlBufferPtr raw;
    int compressed;
    int error;
    unsigned long rawconsumed;
};

/* Output buffer */
typedef struct _xmlOutputBuffer xmlOutputBuffer;
typedef xmlOutputBuffer *xmlOutputBufferPtr;
struct _xmlOutputBuffer {
    void *context;
    xmlOutputWriteCallback writecallback;
    xmlOutputCloseCallback closecallback;
    xmlCharEncodingHandlerPtr encoder;
    xmlBufferPtr buffer;
    xmlBufferPtr conv;
    int written;
    int error;
};

/* Parser context */
typedef struct _xmlParserCtxt xmlParserCtxt;
typedef xmlParserCtxt *xmlParserCtxtPtr;
struct _xmlParserCtxt {
    xmlSAXHandlerPtr sax;
    void *userData;
    xmlDocPtr myDoc;
    int wellFormed;
    int replaceEntities;
    const xmlChar *version;
    const xmlChar *encoding;
    int standalone;
    int html;
    xmlParserInputPtr input;
    int inputNr;
    int inputMax;
    xmlParserInputPtr *inputTab;
    xmlNodePtr node;
    int nodeNr;
    int nodeMax;
    xmlNodePtr *nodeTab;
    int record_info;
    int node_seq;
    int errNo;
    int hasExternalSubset;
    int hasPErefs;
    int external;
    int valid;
    int validate;
    xmlValidCtxtPtr vctxt;
    int instate;
    int token;
    char *directory;
    xmlChar *name;
    int nameNr;
    int nameMax;
    xmlChar **nameTab;
    long nbChars;
    long checkIndex;
    int keepBlanks;
    int disableSAX;
    int inSubset;
    const xmlChar *intSubName;
    xmlChar *extSubURI;
    xmlChar *extSubSystem;
    int *space;
    int spaceNr;
    int spaceMax;
    int *spaceTab;
    int depth;
    xmlEntityPtr entity;
    int charset;
    int nodelen;
    int nodemem;
    int pedantic;
    void *_private;
    int loadsubset;
    int linenumbers;
    void *catalogs;
    int recovery;
    int progressive;
    xmlDictPtr dict;
    const xmlChar **atts;
    int maxatts;
    int docdict;
    const xmlChar *str_xml;
    const xmlChar *str_xmlns;
    const xmlChar *str_xml_ns;
    int sax2;
    int nsNr;
    int nsMax;
    xmlNsPtr *nsTab;
    int attallocs;
    xmlNodePtr *pushTab;
    xmlHashTablePtr attsDefault;
    xmlHashTablePtr attsSpecial;
    int nsWellFormed;
    int options;
    int dictNames;
    int freeElemsNr;
    xmlNodePtr *freeElems;
    int freeAttrsNr;
    xmlAttrPtr *freeAttrs;
    xmlError lastError;
    int parseMode;
    int nbentities;
    int sizeentities;
    xmlParserNodeInfoPtr nodeInfo;
    int nodeInfoNr;
    int nodeInfoMax;
    xmlParserNodeInfo *nodeInfoTab;
    int input_id;
    int sizeentcopy;
    int endCheckState;
    int nbErrors;
    int nbWarnings;
    int maxAmpl;
    int nsdb;
    int attrHashMax;
    xmlHashTablePtr attrHash;
    xmlGenericErrorFunc errorHandler;
    void *errorCtxt;
    xmlResourceLoader resourceLoader;
    void *resourceCtxt;
    xmlCharEncodingInputFunc convImpl;
    void *convCtxt;
};

/* Validation context */
typedef struct _xmlValidCtxt xmlValidCtxt;
typedef xmlValidCtxt *xmlValidCtxtPtr;
struct _xmlValidCtxt {
    void *userData;
    xmlValidityErrorFunc error;
    xmlValidityWarningFunc warning;
    xmlNodePtr node;
    int nodeNr;
    int nodeMax;
    xmlNodePtr *nodeTab;
    unsigned int flags;
    xmlDocPtr doc;
    int valid;
    xmlValidState *vstate;
    int vstateNr;
    int vstateMax;
    xmlValidState *vstateTab;
    xmlAutomataPtr am;
    int state;
};

/* SAX handler */
typedef struct _xmlSAXHandler xmlSAXHandler;
typedef xmlSAXHandler *xmlSAXHandlerPtr;
struct _xmlSAXHandler {
    internalSubsetSAXFunc internalSubset;
    isStandaloneSAXFunc isStandalone;
    hasInternalSubsetSAXFunc hasInternalSubset;
    hasExternalSubsetSAXFunc hasExternalSubset;
    resolveEntitySAXFunc resolveEntity;
    getEntitySAXFunc getEntity;
    entityDeclSAXFunc entityDecl;
    notationDeclSAXFunc notationDecl;
    attributeDeclSAXFunc attributeDecl;
    elementDeclSAXFunc elementDecl;
    unparsedEntityDeclSAXFunc unparsedEntityDecl;
    setDocumentLocatorSAXFunc setDocumentLocator;
    startDocumentSAXFunc startDocument;
    endDocumentSAXFunc endDocument;
    startElementSAXFunc startElement;
    endElementSAXFunc endElement;
    referenceSAXFunc reference;
    charactersSAXFunc characters;
    ignorableWhitespaceSAXFunc ignorableWhitespace;
    processingInstructionSAXFunc processingInstruction;
    commentSAXFunc comment;
    warningSAXFunc warning;
    errorSAXFunc error;
    fatalErrorSAXFunc fatalError;
    getParameterEntitySAXFunc getParameterEntity;
    cdataBlockSAXFunc cdataBlock;
    externalSubsetSAXFunc externalSubset;
    unsigned int initialized;
    void *_private;
    startElementNsSAX2Func startElementNs;
    endElementNsSAX2Func endElementNs;
    xmlStructuredErrorFunc serror;
};

/* Parser node info */
typedef struct _xmlParserNodeInfo xmlParserNodeInfo;
typedef xmlParserNodeInfo *xmlParserNodeInfoPtr;
struct _xmlParserNodeInfo {
    xmlNodePtr node;
    unsigned long begin_pos;
    unsigned long begin_line;
    unsigned long end_pos;
    unsigned long end_line;
};

/* Init and cleanup */
XMLPUBFUN void xmlInitParser(void);
XMLPUBFUN void xmlCleanupParser(void);
XMLPUBFUN int xmlIsInitialized(void);

/* Reading APIs */
XMLPUBFUN xmlDocPtr xmlReadDoc(const xmlChar *cur, const char *URL,
                                const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadFile(const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadMemory(const char *buffer, int size,
                                   const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadFd(int fd, const char *URL,
                               const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadIO(xmlInputReadCallback ioread,
                               xmlInputCloseCallback ioclose,
                               void *ioctx, const char *URL,
                               const char *encoding, int options);

/* Parse APIs */
XMLPUBFUN xmlDocPtr xmlParseDoc(const xmlChar *cur);
XMLPUBFUN xmlDocPtr xmlParseFile(const char *filename);
XMLPUBFUN xmlDocPtr xmlParseMemory(const char *buffer, int size);

/* SAX APIs */
XMLPUBFUN xmlDocPtr xmlSAXParseDoc(xmlSAXHandlerPtr sax, const xmlChar *cur, int recovery);
XMLPUBFUN xmlDocPtr xmlSAXParseFile(xmlSAXHandlerPtr sax, const char *filename, int recovery);
XMLPUBFUN xmlDocPtr xmlSAXParseMemory(xmlSAXHandlerPtr sax,
                                       const char *buffer, int size, int recovery);
XMLPUBFUN int xmlSAXUserParseFile(xmlSAXHandlerPtr sax, void *user_data,
                                   const char *filename);
XMLPUBFUN int xmlSAXUserParseMemory(xmlSAXHandlerPtr sax, void *user_data,
                                     const char *buffer, int size);

/* Context APIs */
XMLPUBFUN xmlParserCtxtPtr xmlCreateFileParserCtxt(const char *filename);
XMLPUBFUN xmlParserCtxtPtr xmlCreateDocParserCtxt(const xmlChar *cur);
XMLPUBFUN int xmlParseDocument(xmlParserCtxtPtr ctxt);
XMLPUBFUN void xmlFreeParserCtxt(xmlParserCtxtPtr ctxt);
XMLPUBFUN int xmlCtxtUseOptions(xmlParserCtxtPtr ctxt, int options);
XMLPUBFUN int xmlParseChunk(xmlParserCtxtPtr ctxt, const char *chunk,
                             int size, int terminate);

/* Input buffer APIs */
XMLPUBFUN xmlParserInputBufferPtr xmlParserInputBufferCreateMem(
    const char *buffer, int size, int enc);
XMLPUBFUN xmlParserInputBufferPtr xmlParserInputBufferCreateFilename(
    const char *URI, int enc);
XMLPUBFUN xmlParserInputBufferPtr xmlParserInputBufferCreateIO(
    xmlInputReadCallback ioread, xmlInputCloseCallback ioclose,
    void *ioctx, int enc);
XMLPUBFUN void xmlFreeParserInputBuffer(xmlParserInputBufferPtr buf);
XMLPUBFUN xmlParserInputPtr xmlNewInputFromFile(xmlParserCtxtPtr ctxt,
                                                 const char *filename);
XMLPUBFUN void xmlFreeInputStream(xmlParserInputPtr input);

/* Output buffer APIs */
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateFilename(
    const char *URI, xmlCharEncodingHandlerPtr encoder, int compression);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateFd(
    int fd, xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateIO(
    xmlOutputWriteCallback iowrite, xmlOutputCloseCallback ioclose,
    void *ioctx, xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN int xmlOutputBufferClose(xmlOutputBufferPtr out);
XMLPUBFUN int xmlOutputBufferFlush(xmlOutputBufferPtr out);
XMLPUBFUN int xmlOutputBufferWrite(xmlOutputBufferPtr out, int len, const char *data);
XMLPUBFUN int xmlOutputBufferWriteString(xmlOutputBufferPtr out, const char *str);

#ifdef __cplusplus
}
#endif

#endif /* __XML_PARSER_H__ */
'''
    (INCLUDE_LIBXML / "parser.h").write_text(content)
    print("  Generated include/libxml/parser.h")


def gen_SAX2():
    """Generate include/libxml/SAX2.h"""
    content = '''/**
 * @file
 *
 * SAX2 API for libxml-rs
 */

#ifndef __XML_SAX2_H__
#define __XML_SAX2_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/parser.h>

#ifdef __cplusplus
extern "C" {
#endif

#define XML_SAX2_MAGIC 0xDEEDBEAF

/* SAX2 callback types */
typedef void (*startDocumentSAXFunc)(void *ctx);
typedef void (*endDocumentSAXFunc)(void *ctx);
typedef void (*startElementSAXFunc)(void *ctx, const xmlChar *name,
                                     const xmlChar **atts);
typedef void (*endElementSAXFunc)(void *ctx, const xmlChar *name);
typedef void (*charactersSAXFunc)(void *ctx, const xmlChar *ch, int len);
typedef void (*processingInstructionSAXFunc)(void *ctx,
                                              const xmlChar *target,
                                              const xmlChar *data);
typedef void (*commentSAXFunc)(void *ctx, const xmlChar *value);
typedef void (*warningSAXFunc)(void *ctx, const char *msg, ...);
typedef void (*errorSAXFunc)(void *ctx, const char *msg, ...);
typedef void (*fatalErrorSAXFunc)(void *ctx, const char *msg, ...);
typedef void (*cdataBlockSAXFunc)(void *ctx, const xmlChar *value, int len);
typedef void (*referenceSAXFunc)(void *ctx, const xmlChar *name);
typedef void (*ignorableWhitespaceSAXFunc)(void *ctx, const xmlChar *ch, int len);
typedef void (*setDocumentLocatorSAXFunc)(void *ctx, xmlSAXLocatorPtr loc);
typedef xmlParserInputPtr (*resolveEntitySAXFunc)(void *ctx,
                                                    const xmlChar *publicId,
                                                    const xmlChar *systemId);
typedef xmlEntityPtr (*getEntitySAXFunc)(void *ctx, const xmlChar *name);
typedef xmlEntityPtr (*getParameterEntitySAXFunc)(void *ctx, const xmlChar *name);
typedef void (*entityDeclSAXFunc)(void *ctx, const xmlChar *name, int type,
                                   const xmlChar *publicId, const xmlChar *systemId,
                                   xmlChar *content);
typedef void (*notationDeclSAXFunc)(void *ctx, const xmlChar *name,
                                     const xmlChar *publicId,
                                     const xmlChar *systemId);
typedef void (*attributeDeclSAXFunc)(void *ctx, const xmlChar *elem,
                                      const xmlChar *fullname, int type, int def,
                                      const xmlChar *defaultValue,
                                      xmlEnumerationPtr tree);
typedef void (*elementDeclSAXFunc)(void *ctx, const xmlChar *name, int type,
                                    xmlElementContentPtr content);
typedef void (*unparsedEntityDeclSAXFunc)(void *ctx, const xmlChar *name,
                                           const xmlChar *publicId,
                                           const xmlChar *systemId,
                                           const xmlChar *notationName);
typedef void (*internalSubsetSAXFunc)(void *ctx, const xmlChar *name,
                                       const xmlChar *ExternalID,
                                       const xmlChar *SystemID);
typedef int (*isStandaloneSAXFunc)(void *ctx);
typedef int (*hasInternalSubsetSAXFunc)(void *ctx);
typedef int (*hasExternalSubsetSAXFunc)(void *ctx);
typedef void (*externalSubsetSAXFunc)(void *ctx, const xmlChar *name,
                                       const xmlChar *ExternalID,
                                       const xmlChar *SystemID);

/* SAX2 element handlers */
typedef void (*startElementNsSAX2Func)(void *ctx,
                                        const xmlChar *localname,
                                        const xmlChar *prefix,
                                        const xmlChar *URI,
                                        int nb_namespaces,
                                        const xmlChar **namespaces,
                                        int nb_attributes,
                                        int nb_defaulted,
                                        const xmlChar **attributes);
typedef void (*endElementNsSAX2Func)(void *ctx,
                                      const xmlChar *localname,
                                      const xmlChar *prefix,
                                      const xmlChar *URI);

/* SAX locator */
typedef struct _xmlSAXLocator xmlSAXLocator;
typedef xmlSAXLocator *xmlSAXLocatorPtr;
struct _xmlSAXLocator {
    xmlChar *(*getPublicId)(void *ctx);
    xmlChar *(*getSystemId)(void *ctx);
    int (*getLineNumber)(void *ctx);
    int (*getColumnNumber)(void *ctx);
};

XMLPUBFUN int xmlSAX2IsInitialized(void *ctx);
XMLPUBFUN void xmlSAX2InitDefaultSAXHandler(xmlSAXHandlerPtr handler, int warning);
XMLPUBFUN void xmlSAX2InitHtmlDefaultSAXHandler(xmlSAXHandlerPtr handler);

#ifdef __cplusplus
}
#endif

#endif /* __XML_SAX2_H__ */
'''
    (INCLUDE_LIBXML / "SAX2.h").write_text(content)
    print("  Generated include/libxml/SAX2.h")


def gen_SAX():
    """Generate include/libxml/SAX.h"""
    content = '''/**
 * @file
 *
 * SAX1 API for libxml-rs
 */

#ifndef __XML_SAX_H__
#define __XML_SAX_H__

#include <libxml/xmlversion.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>

#ifdef __cplusplus
extern "C" {
#endif

/* SAX1 is now a subset of SAX2. All SAX1 callbacks are defined in SAX2.h */

#ifdef __cplusplus
}
#endif

#endif /* __XML_SAX_H__ */
'''
    (INCLUDE_LIBXML / "SAX.h").write_text(content)
    print("  Generated include/libxml/SAX.h")


def gen_entities():
    """Generate include/libxml/entities.h"""
    content = '''/**
 * @file
 *
 * Entity handling API for libxml-rs
 */

#ifndef __XML_ENTITIES_H__
#define __XML_ENTITIES_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Entity functions are declared in tree.h */

#ifdef __cplusplus
}
#endif

#endif /* __XML_ENTITIES_H__ */
'''
    (INCLUDE_LIBXML / "entities.h").write_text(content)
    print("  Generated include/libxml/entities.h")


def gen_encoding():
    """Generate include/libxml/encoding.h"""
    content = '''/**
 * @file
 *
 * Character encoding API for libxml-rs
 */

#ifndef __XML_ENCODING_H__
#define __XML_ENCODING_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Encoding identifiers */
typedef enum {
    XML_CHAR_ENCODING_ERROR = -1,
    XML_CHAR_ENCODING_NONE = 0,
    XML_CHAR_ENCODING_UTF8 = 1,
    XML_CHAR_ENCODING_UTF16LE = 2,
    XML_CHAR_ENCODING_UTF16BE = 3,
    XML_CHAR_ENCODING_UCS4LE = 4,
    XML_CHAR_ENCODING_UCS4BE = 5,
    XML_CHAR_ENCODING_EBCDIC = 6,
    XML_CHAR_ENCODING_UCS4_2143 = 7,
    XML_CHAR_ENCODING_UCS4_3412 = 8,
    XML_CHAR_ENCODING_UCS2 = 9,
    XML_CHAR_ENCODING_8859_1 = 10,
    XML_CHAR_ENCODING_8859_2 = 11,
    XML_CHAR_ENCODING_8859_3 = 12,
    XML_CHAR_ENCODING_8859_4 = 13,
    XML_CHAR_ENCODING_8859_5 = 14,
    XML_CHAR_ENCODING_8859_6 = 15,
    XML_CHAR_ENCODING_8859_7 = 16,
    XML_CHAR_ENCODING_8859_8 = 17,
    XML_CHAR_ENCODING_8859_9 = 18,
    XML_CHAR_ENCODING_2022_JP = 19,
    XML_CHAR_ENCODING_SHIFT_JIS = 20,
    XML_CHAR_ENCODING_EUC_JP = 21,
    XML_CHAR_ENCODING_ASCII = 22
} xmlCharEncoding;

typedef struct _xmlCharEncodingHandler xmlCharEncodingHandler;
typedef xmlCharEncodingHandler *xmlCharEncodingHandlerPtr;

typedef int (*xmlCharEncodingInputFunc)(unsigned char *out, int *outlen,
                                         const unsigned char *in, int *inlen);
typedef int (*xmlCharEncodingOutputFunc)(unsigned char *out, int *outlen,
                                          const unsigned char *in, int *inlen);

XMLPUBFUN int xmlGetCharEncoding(const char *name);
XMLPUBFUN xmlCharEncodingHandlerPtr xmlFindCharEncodingHandler(const char *name);
XMLPUBFUN int xmlCharEncCloseFunc(xmlCharEncodingHandlerPtr handler);
XMLPUBFUN int xmlCharEncInput(xmlParserInputBufferPtr input, int to);
XMLPUBFUN int xmlCharEncOutput(xmlOutputBufferPtr output, int to);

#ifdef __cplusplus
}
#endif

#endif /* __XML_ENCODING_H__ */
'''
    (INCLUDE_LIBXML / "encoding.h").write_text(content)
    print("  Generated include/libxml/encoding.h")


def gen_xpath():
    """Generate include/libxml/xpath.h"""
    content = '''/**
 * @file
 *
 * XPath API for libxml-rs
 */

#ifndef __XML_XPATH_H__
#define __XML_XPATH_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

#ifdef __cplusplus
extern "C" {
#endif

/* XPath object types */
typedef enum {
    XPATH_UNDEFINED = 0,
    XPATH_NODESET = 1,
    XPATH_BOOLEAN = 2,
    XPATH_NUMBER = 3,
    XPATH_STRING = 4,
    XPATH_POINT = 5,
    XPATH_RANGE = 6,
    XPATH_LOCATIONSET = 7,
    XPATH_USERS = 8,
    XPATH_XSLT_TREE = 9
} xmlXPathObjectType;

/* Node set */
typedef struct _xmlNodeSet xmlNodeSet;
typedef xmlNodeSet *xmlNodeSetPtr;
struct _xmlNodeSet {
    int nodeNr;
    int nodeMax;
    xmlNodePtr *nodeTab;
};

/* XPath object */
typedef struct _xmlXPathObject xmlXPathObject;
typedef xmlXPathObject *xmlXPathObjectPtr;
struct _xmlXPathObject {
    int type;
    xmlNodeSetPtr nodesetval;
    int boolval;
    double floatval;
    xmlChar *stringval;
    void *user;
    int index;
    void *user2;
    int index2;
};

/* XPath context */
typedef struct _xmlXPathContext xmlXPathContext;
typedef xmlXPathContext *xmlXPathContextPtr;
struct _xmlXPathContext {
    xmlDocPtr doc;
    xmlNodePtr node;
    int nb_variables_unused;
    int max_variables_unused;
    void *varHash;
    int nb_types;
    int max_types;
    void *types;
    int nb_funcs_unused;
    int max_funcs_unused;
    void *funcHash;
    int nb_axis;
    int max_axis;
    void **axis;
    xmlNsPtr *namespaces;
    int nsNr;
    void *user;
    int contextSize;
    int proximityPosition;
    int xptr;
    xmlNodePtr here;
    xmlNodePtr origin;
    void *nsHash;
    xmlXPathVariableLookupFunc varLookupFunc;
    void *varLookupData;
    void *extra;
    xmlXPathFunction function;
    const xmlChar *functionURI;
    xmlXPathFuncLookupFunc funcLookupFunc;
    void *funcLookupData;
    xmlNsPtr *tmpNsList;
    int tmpNsNr;
    void *userData;
    xmlXPathErrorFunc error;
    xmlError lastError;
    xmlNodePtr debugNode;
    xmlDictPtr dict;
    int flags;
    void *cache;
    int opLimit;
    int opCount;
    int depth;
};

/* XPath function type */
typedef void (*xmlXPathFunction)(xmlXPathParserContextPtr ctxt, int nargs);
typedef xmlXPathObjectPtr (*xmlXPathVariableLookupFunc)(void *ctxt,
                                                         const xmlChar *name);
typedef xmlXPathFunction (*xmlXPathFuncLookupFunc)(void *ctxt,
                                                    const xmlChar *name,
                                                    const xmlChar *ns_uri);

/* XPath API */
XMLPUBFUN xmlXPathContextPtr xmlXPathNewContext(xmlDocPtr doc);
XMLPUBFUN void xmlXPathFreeContext(xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr xmlXPathEvalExpression(const xmlChar *str,
                                                    xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr xmlXPathEval(const xmlChar *str,
                                          xmlXPathContextPtr ctxt);
XMLPUBFUN void xmlXPathFreeObject(xmlXPathObjectPtr obj);
XMLPUBFUN void *xmlXPathCompile(const xmlChar *str);
XMLPUBFUN void xmlXPathFreeCompExpr(void *comp);
XMLPUBFUN int xmlXPathRegisterNs(xmlXPathContextPtr ctxt,
                                  const xmlChar *prefix, const xmlChar *ns_uri);
XMLPUBFUN int xmlXPathRegisterFunc(xmlXPathContextPtr ctxt,
                                    const xmlChar *name, xmlXPathFunction f);
XMLPUBFUN int xmlXPathRegisterFuncNS(xmlXPathContextPtr ctxt,
                                      const xmlChar *name, const xmlChar *ns_uri,
                                      xmlXPathFunction f);
XMLPUBFUN int xmlXPathRegisterVariable(xmlXPathContextPtr ctxt,
                                        const xmlChar *name,
                                        xmlXPathObjectPtr value);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewNodeSet(xmlNodePtr val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewCString(const xmlChar *val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewFloat(double val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewBoolean(int val);

#ifdef __cplusplus
}
#endif

#endif /* __XML_XPATH_H__ */
'''
    (INCLUDE_LIBXML / "xpath.h").write_text(content)
    print("  Generated include/libxml/xpath.h")


def gen_xinclude():
    """Generate include/libxml/xinclude.h"""
    content = '''/**
 * @file
 *
 * XInclude API for libxml-rs
 */

#ifndef __XML_XINCLUDE_H__
#define __XML_XINCLUDE_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN int xmlXIncludeProcess(xmlDocPtr doc);
XMLPUBFUN int xmlXIncludeProcessFlags(xmlDocPtr doc, int flags);

#ifdef __cplusplus
}
#endif

#endif /* __XML_XINCLUDE_H__ */
'''
    (INCLUDE_LIBXML / "xinclude.h").write_text(content)
    print("  Generated include/libxml/xinclude.h")


def gen_catalog():
    """Generate include/libxml/catalog.h"""
    content = '''/**
 * @file
 *
 * Catalog API for libxml-rs
 */

#ifndef __XML_CATALOG_H__
#define __XML_CATALOG_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Catalog allow values */
typedef enum {
    XML_CATA_ALLOW_NONE = 0,
    XML_CATA_ALLOW_GLOBAL = 1,
    XML_CATA_ALLOW_DOCUMENT = 2,
    XML_CATA_ALLOW_ALL = 3
} xmlCatalogAllow;

XMLPUBFUN void *xmlCatalogLoad(const char *catalogs);
XMLPUBFUN xmlChar *xmlCatalogResolvePublic(const xmlChar *pubID);
XMLPUBFUN xmlChar *xmlCatalogResolveSystem(const xmlChar *sysID);
XMLPUBFUN xmlChar *xmlCatalogResolveURI(const xmlChar *URI);
XMLPUBFUN void xmlCatalogSetDefaults(xmlCatalogAllowValue allow);
XMLPUBFUN xmlCatalogAllowValue xmlCatalogGetDefaults(void);
XMLPUBFUN int xmlCatalogAdd(const xmlChar *type, const xmlChar *orig,
                             const xmlChar *replace);
XMLPUBFUN int xmlCatalogRemove(const xmlChar *value);
XMLPUBFUN void xmlCatalogCleanup(void);
XMLPUBFUN xmlDocPtr xmlCatalogConvert(void);

#ifdef __cplusplus
}
#endif

#endif /* __XML_CATALOG_H__ */
'''
    (INCLUDE_LIBXML / "catalog.h").write_text(content)
    print("  Generated include/libxml/catalog.h")


def gen_xmlIO():
    """Generate include/libxml/xmlIO.h"""
    content = '''/**
 * @file
 *
 * I/O API for libxml-rs
 */

#ifndef __XML_IO_H__
#define __XML_IO_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* I/O callback types */
typedef int (*xmlInputReadCallback)(void *context, char *buffer, int len);
typedef int (*xmlInputCloseCallback)(void *context);
typedef int (*xmlOutputWriteCallback)(void *context, const char *buffer, int len);
typedef int (*xmlOutputCloseCallback)(void *context);
typedef void *(*xmlResourceLoader)(const char *URL, const char *encoding,
                                    int options, void *ctxt);

#ifdef __cplusplus
}
#endif

#endif /* __XML_IO_H__ */
'''
    (INCLUDE_LIBXML / "xmlIO.h").write_text(content)
    print("  Generated include/libxml/xmlIO.h")


def gen_html():
    """Generate include/libxml/HTMLparser.h and HTMLtree.h"""
    # HTMLparser.h
    content = '''/**
 * @file
 *
 * HTML parser API for libxml-rs
 */

#ifndef __HTML_PARSER_H__
#define __HTML_PARSER_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/parser.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef xmlDocPtr htmlDocPtr;

XMLPUBFUN htmlDocPtr htmlParseFile(const char *filename, const char *encoding);
XMLPUBFUN htmlDocPtr htmlParseMemory(const char *buffer, int size);
XMLPUBFUN htmlDocPtr htmlParseDoc(const xmlChar *cur, const char *encoding);
XMLPUBFUN void *htmlCreateFileParserCtxt(const char *filename, const char *encoding);
XMLPUBFUN void htmlFreeParserCtxt(void *ctxt);
XMLPUBFUN void htmlInitParser(void);
XMLPUBFUN void htmlCleanupParser(void);

#ifdef __cplusplus
}
#endif

#endif /* __HTML_PARSER_H__ */
'''
    (INCLUDE_LIBXML / "HTMLparser.h").write_text(content)
    print("  Generated include/libxml/HTMLparser.h")

    # HTMLtree.h
    content = '''/**
 * @file
 *
 * HTML serializer API for libxml-rs
 */

#ifndef __HTML_TREE_H__
#define __HTML_TREE_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* HTML serializer functions - to be implemented */

#ifdef __cplusplus
}
#endif

#endif /* __HTML_TREE_H__ */
'''
    (INCLUDE_LIBXML / "HTMLtree.h").write_text(content)
    print("  Generated include/libxml/HTMLtree.h")


def gen_debug():
    """Generate include/libxml/debugXML.h"""
    content = '''/**
 * @file
 *
 * Debug/dump API for libxml-rs
 */

#ifndef __DEBUG_XML_H__
#define __DEBUG_XML_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xmlDebugDumpDocument(FILE *output, xmlDocPtr doc);
XMLPUBFUN void xmlDebugDumpNode(FILE *output, xmlNodePtr node);
XMLPUBFUN void xmlDebugDumpNodeList(FILE *output, xmlNodePtr node);

#ifdef __cplusplus
}
#endif

#endif /* __DEBUG_XML_H__ */
'''
    (INCLUDE_LIBXML / "debugXML.h").write_text(content)
    print("  Generated include/libxml/debugXML.h")


def gen_threads():
    """Generate include/libxml/threads.h"""
    content = '''/**
 * @file
 *
 * Thread support API for libxml-rs
 */

#ifndef __XML_THREADS_H__
#define __XML_THREADS_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN int xmlInitThreads(void);
XMLPUBFUN void xmlCleanupThreads(void);
XMLPUBFUN void xmlLockLibrary(void);
XMLPUBFUN void xmlUnlockLibrary(void);

#ifdef __cplusplus
}
#endif

#endif /* __XML_THREADS_H__ */
'''
    (INCLUDE_LIBXML / "threads.h").write_text(content)
    print("  Generated include/libxml/threads.h")


def gen_globals():
    """Generate include/libxml/globals.h"""
    content = '''/**
 * @file
 *
 * Global state API for libxml-rs
 */

#ifndef __XML_GLOBALS_H__
#define __XML_GLOBALS_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlerror.h>
#include <libxml/parser.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xmlInitGlobals(void);
XMLPUBFUN void xmlCleanupGlobals(void);

#ifdef __cplusplus
}
#endif

#endif /* __XML_GLOBALS_H__ */
'''
    (INCLUDE_LIBXML / "globals.h").write_text(content)
    print("  Generated include/libxml/globals.h")


def gen_stubs():
    """Generate stub headers for remaining libxml headers."""
    stubs = [
        ("valid.h", "VALID", "DTD validation API"),
        ("uri.h", "URI", "URI handling API"),
        ("xpointer.h", "XPTR", "XPointer API"),
        ("xmlautomata.h", "XMLAUTOMATA", "Automata API"),
        ("xmlregexp.h", "XMLREGEXP", "Regular expression API"),
        ("xmlschemas.h", "SCHEMAS", "XML Schema API"),
        ("xmlschemastypes.h", "SCHEMASTYPES", "XML Schema types API"),
        ("relaxng.h", "RELAXNG", "RELAX NG API"),
        ("schematron.h", "SCHEMATRON", "Schematron API"),
        ("pattern.h", "PATTERN", "Pattern API"),
        ("c14n.h", "C14N", "Canonicalization API"),
        ("chvalid.h", "CHVALID", "Character validation API"),
        ("nanohttp.h", "NANOHTTP", "HTTP stubs API"),
        ("nanoftp.h", "NANO_FTP", "FTP stubs API"),
        ("xmlunicode.h", "XMLUNICODE", "Unicode API"),
        ("xmlreader.h", "XMLREADER", "Reader API"),
        ("xmlwriter.h", "XMLWRITER", "Writer API"),
        ("xmlsave.h", "XMLSAVE", "Save/serialization API"),
        ("xmlmodule.h", "XMLMODULE", "Module API"),
        ("schemasInternals.h", "SCHEMAS_INTERNALS", "Schema internals"),
        ("parserInternals.h", "PARSER_INTERNALS", "Parser internals"),
        ("xlink.h", "XLINK", "XLink API"),
    ]
    
    for filename, guard_base, description in stubs:
        guard = f"__XML_{guard_base}_H__"
        content = f'''/**
 * @file
 *
 * {description} for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef {guard}
#define {guard}

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {{
#endif

/* Functions will be declared here as they are implemented. */

#ifdef __cplusplus
}}
#endif

#endif /* {guard} */
'''
        (INCLUDE_LIBXML / filename).write_text(content)
        print(f"  Generated include/libxml/{filename}")


def gen_libxslt_headers():
    """Generate include/libxslt/ headers."""
    xslt_headers = [
        ("xslt.h", "XSLT", "XSLT main header"),
        ("xsltconfig.h", "XSLTCONFIG", "XSLT configuration"),
        ("xsltutils.h", "XSLTUTILS", "XSLT utilities"),
        ("transform.h", "TRANSFORM", "XSLT transform API"),
        ("security.h", "SECURITY", "XSLT security API"),
        ("extensions.h", "EXTENSIONS", "XSLT extensions API"),
        ("extra.h", "EXTRA", "XSLT extra API"),
        ("functions.h", "FUNCTIONS", "XSLT functions API"),
        ("imports.h", "IMPORTS", "XSLT imports API"),
        ("keys.h", "KEYS", "XSLT keys API"),
        ("namespaces.h", "NAMESPACES", "XSLT namespace alias API"),
        ("templates.h", "TEMPLATES", "XSLT templates API"),
        ("variables.h", "VARIABLES", "XSLT variables API"),
        ("attributes.h", "ATTRIBUTES", "XSLT attribute sets API"),
        ("documents.h", "DOCUMENTS", "XSLT document API"),
        ("preproc.h", "PREPROC", "XSLT preprocessor API"),
        ("numbersInternals.h", "NUMBERS_INTERNALS", "XSLT numbering internals"),
        ("pattern.h", "PATTERN", "XSLT pattern API"),
        ("xsltlocale.h", "XSLTLOCALE", "XSLT locale API"),
    ]
    
    for filename, guard_base, description in xslt_headers:
        guard = f"__{guard_base}_H__"
        
        if filename == "xslt.h":
            content = f'''/**
 * @file
 *
 * {description} for libxml-rs
 */

#ifndef {guard}
#define {guard}

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {{
#endif

/* XSLT version */
#define LIBXSLT_DOTTED_VERSION "1.1.45"
#define LIBXSLT_VERSION 10145
#define LIBXSLT_VERSION_STRING "10145"
#define LIBXSLT_VERSION_EXTRA ""

/* Stylesheet type */
typedef struct _xsltStylesheet xsltStylesheet;
typedef xsltStylesheet *xsltStylesheetPtr;

/* Transform context */
typedef struct _xsltTransformContext xsltTransformContext;
typedef xsltTransformContext *xsltTransformContextPtr;

/* XSLT functions */
XMLPUBFUN int xsltLibxsltVersion(void);
XMLPUBFUN const char *xsltLibxsltVersionString(void);
XMLPUBFUN int xsltCheckVersion(int version);
XMLPUBFUN void xsltInit(void);
XMLPUBFUN void xsltCleanupGlobals(void);
XMLPUBFUN xsltStylesheetPtr xsltParseStylesheetFile(const xmlChar *filename);
XMLPUBFUN xsltStylesheetPtr xsltParseStylesheetDoc(xmlDocPtr doc);
XMLPUBFUN xsltStylesheetPtr xsltParseStylesheetMemory(const char *buf, int len,
                                                       const char *URL);
XMLPUBFUN void xsltFreeStylesheet(xsltStylesheetPtr style);
XMLPUBFUN xmlDocPtr xsltApplyStylesheet(xsltStylesheetPtr style, xmlDocPtr doc,
                                         const char **params);
XMLPUBFUN xmlDocPtr xsltApplyStylesheetUser(xsltStylesheetPtr style, xmlDocPtr doc,
                                             const char **params, const char *output,
                                             FILE *profile,
                                             xsltTransformContextPtr userCtxt);
XMLPUBFUN void xsltFreeTransformResult(xmlDocPtr result);
XMLPUBFUN xsltTransformContextPtr xsltNewTransformContext(xsltStylesheetPtr style,
                                                           xmlDocPtr doc);
XMLPUBFUN void xsltFreeTransformContext(xsltTransformContextPtr ctxt);
XMLPUBFUN int xsltSaveResultToFile(FILE *output, xmlDocPtr result,
                                    xsltStylesheetPtr style);
XMLPUBFUN int xsltSaveResultToFd(int fd, xmlDocPtr result,
                                  xsltStylesheetPtr style);
XMLPUBFUN int xsltSaveResultToString(xmlChar **doc_txt_ptr, int *doc_txt_len,
                                      xmlDocPtr result, xsltStylesheetPtr style);
XMLPUBFUN xmlDocPtr xsltGetStylesheetDoc(xsltStylesheetPtr style);
XMLPUBFUN void xsltSetStylesheetDoc(xsltStylesheetPtr style, xmlDocPtr doc);
XMLPUBFUN const char *xsltEngineVersion(void);

#ifdef __cplusplus
}}
#endif

#endif /* {guard} */
'''
        elif filename == "security.h":
            content = f'''/**
 * @file
 *
 * {description} for libxml-rs
 */

#ifndef {guard}
#define {guard}

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {{
#endif

typedef void *xsltSecurityPrefsPtr;

XMLPUBFUN xsltSecurityPrefsPtr xsltNewSecurityPrefs(void);
XMLPUBFUN void xsltFreeSecurityPrefs(xsltSecurityPrefsPtr sec);
XMLPUBFUN int xsltSetSecurityPrefs(xsltSecurityPrefsPtr sec, int option, int value);
XMLPUBFUN int xsltGetSecurityPrefs(xsltSecurityPrefsPtr sec, int option);
XMLPUBFUN void xsltSetDefaultSecurityPrefs(xsltSecurityPrefsPtr sec);
XMLPUBFUN xsltSecurityPrefsPtr xsltGetDefaultSecurityPrefs(void);

#ifdef __cplusplus
}}
#endif

#endif /* {guard} */
'''
        elif filename == "extensions.h":
            content = f'''/**
 * @file
 *
 * {description} for libxml-rs
 */

#ifndef {guard}
#define {guard}

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {{
#endif

XMLPUBFUN int xsltRegisterExtFunction(xsltTransformContextPtr ctxt,
                                       const xmlChar *name, const xmlChar *NS_uri,
                                       xmlXPathFunction f);
XMLPUBFUN int xsltRegisterExtElement(xsltTransformContextPtr ctxt,
                                      const xmlChar *name, const xmlChar *NS_uri,
                                      void *f);
XMLPUBFUN void exsltRegisterAll(void);
XMLPUBFUN void xsltSetLoaderFunc(void *loader);

#ifdef __cplusplus
}}
#endif

#endif /* {guard} */
'''
        elif filename == "xsltutils.h":
            content = f'''/**
 * @file
 *
 * {description} for libxml-rs
 */

#ifndef {guard}
#define {guard}

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {{
#endif

XMLPUBFUN void xsltSetTransformErrorFunc(xsltTransformContextPtr ctxt,
                                          void *ctx,
                                          xmlGenericErrorFunc handler);
XMLPUBFUN int xsltCheckFeature(int feature);

#ifdef __cplusplus
}}
#endif

#endif /* {guard} */
'''
        elif filename == "xsltconfig.h":
            content = f'''/**
 * @file
 *
 * {description} for libxml-rs
 */

#ifndef {guard}
#define {guard}

#include <libxml/xmlversion.h>

#define LIBXSLT_DOTTED_VERSION "1.1.45"
#define LIBXSLT_VERSION 10145
#define LIBXSLT_VERSION_STRING "10145"

/* libxslt features */
#define LIBXSLT_HAVE_STRUCT_TIMESPEC 1

#ifdef __cplusplus
extern "C" {{
#endif

#ifdef __cplusplus
}}
#endif

#endif /* {guard} */
'''
        else:
            content = f'''/**
 * @file
 *
 * {description} for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef {guard}
#define {guard}

#include <libxml/xmlversion.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {{
#endif

/* Functions will be declared here as they are implemented. */

#ifdef __cplusplus
}}
#endif

#endif /* {guard} */
'''
        
        (INCLUDE_LIBXSLT / filename).write_text(content)
        print(f"  Generated include/libxslt/{filename}")


def gen_exslt_headers():
    """Generate include/libexslt/ headers.

    Upstream ships three headers (oracle/historical/prefix/libxslt-1.1.42/
    include/libexslt/): exslt.h, exsltconfig.h and exsltexports.h. The content
    mirrors upstream 1.1.45 (verified byte-equivalent with the archaeology
    tree). EXSLT carries its own version line: 0.8.25 for the libxslt 1.1.45
    target (configure.ac: LIBEXSLT_MAJOR=0, MINOR=8, MICRO=25).

    NOTE (R-000165): exslt.h declares the full upstream API, but the candidate
    DSO currently exports only `exsltRegisterAll`. The header-compile court
    tracks the remaining declarations as an explicit open residual (see
    courts/suites/header-compile/court-runner.sh, EXSLT_RESIDUAL list); the
    exports land with the R-000165 closure in 11.1-X.
    """
    include_exslt = REPO_ROOT / "include" / "libexslt"
    include_exslt.mkdir(parents=True, exist_ok=True)

    # exsltconfig.h — upstream exsltconfig.h (0.8.25)
    (include_exslt / "exsltconfig.h").write_text('''/*
 * exsltconfig.h: compile-time version information for the EXSLT library
 *
 * UPSTREAM-PARITY: libexslt/exsltconfig.h (libxslt 1.1.45)
 */

#ifndef __XML_EXSLTCONFIG_H__
#define __XML_EXSLTCONFIG_H__

#ifdef __cplusplus
extern "C" {
#endif

/**
 * LIBEXSLT_DOTTED_VERSION:
 *
 * the version string like "1.2.3"
 */
#define LIBEXSLT_DOTTED_VERSION "0.8.25"

/**
 * LIBEXSLT_VERSION:
 *
 * the version number: 1.2.3 value is 10203
 */
#define LIBEXSLT_VERSION 825

/**
 * LIBEXSLT_VERSION_STRING:
 *
 * the version number string, 1.2.3 value is "10203"
 */
#define LIBEXSLT_VERSION_STRING "825"

/**
 * LIBEXSLT_VERSION_EXTRA:
 *
 * extra version information, used to show a Git commit description
 */
#define	LIBEXSLT_VERSION_EXTRA ""

/**
 * WITH_CRYPTO:
 *
 * Whether crypto support is configured into exslt
 */
#if 0
#define EXSLT_CRYPTO_ENABLED
#endif

/**
 * ATTRIBUTE_UNUSED:
 *
 * This macro is used to flag unused function parameters to GCC
 */
#ifdef __GNUC__
#ifndef ATTRIBUTE_UNUSED
#define ATTRIBUTE_UNUSED __attribute__((unused))
#endif
#else
#define ATTRIBUTE_UNUSED
#endif

#ifdef __cplusplus
}
#endif

#endif /* __XML_EXSLTCONFIG_H__ */
''')
    print("  Generated include/libexslt/exsltconfig.h")

    # exsltexports.h — upstream exsltexports.h
    (include_exslt / "exsltexports.h").write_text('''/*
 * Summary: macros for marking symbols as exportable/importable.
 *
 * UPSTREAM-PARITY: libexslt/exsltexports.h (libxslt 1.1.45)
 */

#ifndef __EXSLT_EXPORTS_H__
#define __EXSLT_EXPORTS_H__

#if defined(_WIN32) || defined(__CYGWIN__)
/** DOC_DISABLE */

#ifdef LIBEXSLT_STATIC
  #define EXSLTPUBLIC
#elif defined(IN_LIBEXSLT)
  #define EXSLTPUBLIC __declspec(dllexport)
#else
  #define EXSLTPUBLIC __declspec(dllimport)
#endif

#define EXSLTCALL __cdecl

/** DOC_ENABLE */
#else /* not Windows */

/**
 * EXSLTPUBLIC:
 *
 * Macro which declares a public symbol
 */
#define EXSLTPUBLIC

/**
 * EXSLTCALL:
 *
 * Macro which declares the calling convention for exported functions
 */
#define EXSLTCALL

#endif /* platform switch */

/*
 * EXSLTPUBFUN:
 *
 * Macro which declares an exportable function
 */
#define EXSLTPUBFUN EXSLTPUBLIC

/**
 * EXSLTPUBVAR:
 *
 * Macro which declares an exportable variable
 */
#define EXSLTPUBVAR EXSLTPUBLIC extern

/* Compatibility */
#if !defined(LIBEXSLT_PUBLIC)
#define LIBEXSLT_PUBLIC EXSLTPUBVAR
#endif

#endif /* __EXSLT_EXPORTS_H__ */
''')
    print("  Generated include/libexslt/exsltexports.h")

    # exslt.h — upstream exslt.h (1.1.45) verbatim declarations
    (include_exslt / "exslt.h").write_text('''/*
 * Summary: main header file
 *
 * UPSTREAM-PARITY: libexslt/exslt.h (libxslt 1.1.45)
 *
 * R-000165: the candidate DSO currently exports only `exsltRegisterAll`;
 * the remaining declarations below are the upstream drop-in contract and
 * are tracked by the header-compile court as an explicit open residual.
 */

#ifndef __EXSLT_H__
#define __EXSLT_H__

#include <libxml/tree.h>
#include <libxml/xpath.h>
#include "exsltexports.h"
#include <libexslt/exsltconfig.h>

#ifdef __cplusplus
extern "C" {
#endif

EXSLTPUBVAR const char *exsltLibraryVersion;
EXSLTPUBVAR const int exsltLibexsltVersion;
EXSLTPUBVAR const int exsltLibxsltVersion;
EXSLTPUBVAR const int exsltLibxmlVersion;

/**
 * EXSLT_COMMON_NAMESPACE:
 *
 * Namespace for EXSLT common functions
 */
#define EXSLT_COMMON_NAMESPACE ((const xmlChar *) "http://exslt.org/common")
/**
 * EXSLT_CRYPTO_NAMESPACE:
 *
 * Namespace for EXSLT crypto functions
 */
#define EXSLT_CRYPTO_NAMESPACE ((const xmlChar *) "http://exslt.org/crypto")
/**
 * EXSLT_MATH_NAMESPACE:
 *
 * Namespace for EXSLT math functions
 */
#define EXSLT_MATH_NAMESPACE ((const xmlChar *) "http://exslt.org/math")
/**
 * EXSLT_SETS_NAMESPACE:
 *
 * Namespace for EXSLT set functions
 */
#define EXSLT_SETS_NAMESPACE ((const xmlChar *) "http://exslt.org/sets")
/**
 * EXSLT_FUNCTIONS_NAMESPACE:
 *
 * Namespace for EXSLT functions extension functions
 */
#define EXSLT_FUNCTIONS_NAMESPACE ((const xmlChar *) "http://exslt.org/functions")
/**
 * EXSLT_STRINGS_NAMESPACE:
 *
 * Namespace for EXSLT strings functions
 */
#define EXSLT_STRINGS_NAMESPACE ((const xmlChar *) "http://exslt.org/strings")
/**
 * EXSLT_DATE_NAMESPACE:
 *
 * Namespace for EXSLT date functions
 */
#define EXSLT_DATE_NAMESPACE ((const xmlChar *) "http://exslt.org/dates-and-times")
/**
 * EXSLT_DYNAMIC_NAMESPACE:
 *
 * Namespace for EXSLT dynamic functions
 */
#define EXSLT_DYNAMIC_NAMESPACE ((const xmlChar *) "http://exslt.org/dynamic")

/**
 * SAXON_NAMESPACE:
 *
 * Namespace for SAXON extensions functions
 */
#define SAXON_NAMESPACE ((const xmlChar *) "http://icl.com/saxon")

EXSLTPUBFUN void EXSLTCALL exsltCommonRegister (void);
#ifdef EXSLT_CRYPTO_ENABLED
EXSLTPUBFUN void EXSLTCALL exsltCryptoRegister (void);
#endif
EXSLTPUBFUN void EXSLTCALL exsltMathRegister (void);
EXSLTPUBFUN void EXSLTCALL exsltSetsRegister (void);
EXSLTPUBFUN void EXSLTCALL exsltFuncRegister (void);
EXSLTPUBFUN void EXSLTCALL exsltStrRegister (void);
EXSLTPUBFUN void EXSLTCALL exsltDateRegister (void);
EXSLTPUBFUN void EXSLTCALL exsltSaxonRegister (void);
EXSLTPUBFUN void EXSLTCALL exsltDynRegister(void);

EXSLTPUBFUN void EXSLTCALL exsltRegisterAll (void);

EXSLTPUBFUN int EXSLTCALL exsltDateXpathCtxtRegister (xmlXPathContextPtr ctxt,
                                                      const xmlChar *prefix);
EXSLTPUBFUN int EXSLTCALL exsltMathXpathCtxtRegister (xmlXPathContextPtr ctxt,
                                                      const xmlChar *prefix);
EXSLTPUBFUN int EXSLTCALL exsltSetsXpathCtxtRegister (xmlXPathContextPtr ctxt,
                                                      const xmlChar *prefix);
EXSLTPUBFUN int EXSLTCALL exsltStrXpathCtxtRegister (xmlXPathContextPtr ctxt,
                                                     const xmlChar *prefix);

#ifdef __cplusplus
}
#endif
#endif /* __EXSLT_H__ */
''')
    print("  Generated include/libexslt/exslt.h")


# ────────────────────────────────────────────────────────────
# Main
# ────────────────────────────────────────────────────────────

def main():
    print("Generating C headers for libxml-rs...")
    print()
    
    print("libxml headers:")
    gen_xmlexports()
    gen_xmlversion()
    gen_xmlstring()
    gen_xmlmemory()
    gen_xmlerror()
    gen_tree()
    gen_dict()
    gen_hash()
    gen_list()
    gen_parser()
    gen_SAX2()
    gen_SAX()
    gen_entities()
    gen_encoding()
    gen_xpath()
    gen_xinclude()
    gen_catalog()
    gen_xmlIO()
    gen_html()
    gen_debug()
    gen_threads()
    gen_globals()
    gen_stubs()
    
    print()
    print("libxslt headers:")
    gen_libxslt_headers()

    print()
    print("libexslt headers:")
    gen_exslt_headers()

    print()
    print("Done! Generated headers in include/libxml/, include/libxslt/ and include/libexslt/")
    print()
    print(f"  libxml headers: {len(list(INCLUDE_LIBXML.iterdir()))}")
    print(f"  libxslt headers: {len(list(INCLUDE_LIBXSLT.iterdir()))}")
    print(f"  libexslt headers: {len(list((REPO_ROOT / 'include' / 'libexslt').iterdir()))}")


if __name__ == "__main__":
    main()
