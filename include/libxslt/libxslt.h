/**
 * @file
 *
 * Umbrella header for libxslt (libxml-rs). Upstream counterpart:
 * libxslt/libxslt.h — downstream code that includes <libxslt/libxslt.h>
 * expects the whole public surface, so this header pulls in every public
 * libxslt/libexslt header in the same order upstream does.
 */

#ifndef __XSLT_LIBXSLT_H__
#define __XSLT_LIBXSLT_H__

#include <libxslt/xsltconfig.h>
#include <libxml/xmlversion.h>

/* Upstream libxslt.h includes the build-generated config.h (a private
 * artifact). The candidate exposes the same configuration surface through
 * xsltconfig.h, so no config.h include is needed for consumers. */

#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/transform.h>
#include <libxslt/xsltutils.h>
#include <libxslt/security.h>
#include <libxslt/namespaces.h>
#include <libxslt/variables.h>
#include <libxslt/keys.h>
#include <libxslt/numbersInternals.h>
#include <libxslt/extensions.h>
#include <libxslt/extra.h>
#include <libxslt/functions.h>
#include <libxslt/attributes.h>
#include <libxslt/imports.h>
#include <libxslt/documents.h>
#include <libxslt/preproc.h>
#include <libxslt/templates.h>
#include <libexslt/exslt.h>

#endif /* __XSLT_LIBXSLT_H__ */
