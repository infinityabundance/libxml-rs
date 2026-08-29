/**
 * @file
 *
 * libxslt export macros for libxml-rs (11.1-H header-surface closure).
 *
 * Upstream counterpart: libxslt/xsltexports.h
 * The export-macro family must exist with upstream spellings so downstream
 * code including libxslt headers compiles unchanged.
 */

#ifndef __XSLT_EXPORTS_H__
#define __XSLT_EXPORTS_H__

#ifdef _WIN32
  #define XSLTCALL __cdecl
  #ifdef IN_LIBXSLT
    #define XSLTPUBFUN __declspec(dllexport)
    #define XSLTPUBVAR __declspec(dllexport) extern
  #else
    #define XSLTPUBFUN __declspec(dllimport)
    #define XSLTPUBVAR __declspec(dllimport) extern
  #endif
#else
  #define XSLTCALL
  #define XSLTPUBFUN
  #define XSLTPUBVAR extern
#endif

#define LIBXSLT_PUBLIC XSLTPUBVAR

#endif /* __XSLT_EXPORTS_H__ */
