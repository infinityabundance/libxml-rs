//! Default SAX2 handler — builds a DOM tree from SAX events (§20, §85 Phase 3).
//!
//! These are the default implementations that populate a `_xmlSAXHandler` when
//! `xmlSAX2InitDefaultSAXHandler` is called. They reconstruct the document tree
//! from the sequence of SAX callbacks, mirroring the upstream behavior of
//! `xmlSAX2DefaultSAXHandler` / `xmlSAX2*` functions in libxml2's `SAX2.c`.
//!
//! # Architecture
//!
//! The parser context (`_xmlParserCtxt`) is stored as `userData` (the `ctx`
//! pointer passed to every SAX callback). The default handlers use this context
//! to:
//!
//! - Access the document being built (`myDoc`)
//! - Manage the element stack (`nodeTab`, `nodeNr`, `nodeMax`)
//! - Access the current element (`node`)
//!
//! # UPSTREAM-PARITY
//!
//! These functions correspond to the `xmlSAX2*` function family in upstream
//! libxml2 (`SAX2.c`), which are the default SAX2 handlers installed by
//! `xmlSAX2InitDefaultSAXHandler`.

#![allow(non_snake_case)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::allocator;
use crate::abi::callbacks::*;
use crate::abi::structs::*;
use crate::abi::types::xmlChar;
use crate::abi::types::xmlDocProperties::{
    XML_DOC_DTDVALID, XML_DOC_NSVALID, XML_DOC_USERBUILT, XML_DOC_WELLFORMED,
};
use crate::abi::types::xmlElementType::*;
use crate::abi::types::XML_PARSE_COMPACT;
use crate::xml::tree;

/// Default SAX2 handlers that build a DOM tree from SAX events.
pub(crate) mod default_sax_handler {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════
    // Helper: extract the parser context from the SAX user data pointer
    // ═══════════════════════════════════════════════════════════════════════

    /// Extract the parser context from the SAX callback `ctx` pointer.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt` that was passed
    ///   as `userData` to the SAX handler.
    /// - The caller must ensure the context is still alive and valid.
    unsafe fn ctxt_from_ctx(ctx: *mut c_void) -> *mut _xmlParserCtxt {
        ctx as *mut _xmlParserCtxt
    }

    /// Current source line from the parser input (0 when unavailable).
    unsafe fn current_line(ctxt: *mut _xmlParserCtxt) -> u16 {
        if ctxt.is_null() || (*ctxt).input.is_null() {
            return 0;
        }
        let l = (*(*ctxt).input).line;
        if l < 0 {
            0
        } else {
            l as u16
        }
    }

    /// Merge character data into an existing text node (upstream
    /// `xmlNodeAddContent` semantics): the bytes are appended to the node's
    /// own content buffer, keeping the tree flat (a single text node).
    ///
    /// Compact (inline) content is promoted to a heap allocation on the first
    /// merge — an interrupted character-data stream (e.g. an entity
    /// reference) is never compact, matching the oracle's debug output.
    unsafe fn merge_into_text_node(node: *mut _xmlNode, ch: *const xmlChar, len: c_int) {
        if node.is_null() || ch.is_null() || len <= 0 {
            return;
        }
        unsafe {
            let existing_len = crate::xml::string::xml_strlen((*node).content);
            let total = existing_len + len as usize;
            if content_is_inline(node) {
                // The current content lives inside the node struct; promote
                // it to a heap buffer before appending.
                let merged = allocator::xmlMallocImpl(total + 1) as *mut xmlChar;
                if merged.is_null() {
                    return;
                }
                ptr::copy_nonoverlapping((*node).content, merged, existing_len);
                ptr::copy_nonoverlapping(ch, merged.add(existing_len), len as usize);
                *merged.add(total) = 0;
                (*node).content = merged;
            } else {
                let new = allocator::xmlReallocImpl((*node).content as *mut c_void, total + 1)
                    as *mut xmlChar;
                if new.is_null() {
                    return;
                }
                ptr::copy_nonoverlapping(ch, new.add(existing_len), len as usize);
                *new.add(total) = 0;
                (*node).content = new;
            }
        }
    }

    /// True when the node's `content` points into its own struct (compact
    /// text storage, as produced by the parser under `XML_PARSE_COMPACT`).
    ///
    /// UPSTREAM-PARITY: `debugXML.c` identifies compact text via
    /// `node->content == (xmlChar *) &(node->properties)`; the parser stores
    /// short strings in the memory occupied by the unused `properties` field.
    unsafe fn content_is_inline(node: *mut _xmlNode) -> bool {
        if node.is_null() {
            return false;
        }
        unsafe {
            let inline_addr =
                std::ptr::addr_of_mut!((*node).properties) as *mut xmlChar as *const c_void;
            (*node).content as *const c_void == inline_addr
        }
    }

    /// Create a parser text node holding `len` bytes of character data.
    ///
    /// UPSTREAM-PARITY: `xmlSAX2TextNode` (SAX2.c) stores short strings
    /// (`len < 2 * sizeof(void*)`) inside the node struct, overriding the
    /// unused `properties` and `nsDef` fields, when `XML_PARSE_COMPACT` is
    /// set:
    ///
    /// ```c
    /// if ((len < (int) (2 * sizeof(void *))) &&
    ///     (ctxt->options & XML_PARSE_COMPACT)) {
    ///     xmlChar *tmp = (xmlChar *) &(ret->properties);
    ///     memcpy(tmp, str, len);
    ///     tmp[len] = 0;
    ///     intern = tmp;
    /// }
    /// ```
    ///
    /// Returns the new node, or NULL on allocation failure.
    unsafe fn parser_new_text_node(
        ctxt: *mut _xmlParserCtxt,
        ch: *const xmlChar,
        len: c_int,
    ) -> *mut _xmlNode {
        unsafe {
            let compact = !ctxt.is_null()
                && ((*ctxt).options & XML_PARSE_COMPACT) != 0
                && (len as usize) < 16;
            if compact {
                let node = allocator::xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
                if !node.is_null() {
                    (*node).type_ = XML_TEXT_NODE as c_int;
                    (*node).name = allocator::xmlMemStrdupImpl(b"text\0".as_ptr() as *const c_char)
                        as *mut xmlChar;
                    let inline = std::ptr::addr_of_mut!((*node).properties) as *mut xmlChar;
                    ptr::copy_nonoverlapping(ch, inline, len as usize);
                    *inline.add(len as usize) = 0;
                    (*node).content = inline;
                    // UPSTREAM-PARITY (tree.c): registration hook fires
                    // after the node is fully initialised.
                    crate::abi::data_globals::register_node_hook(node);
                }
                node
            } else {
                // Create a null-terminated copy of the character data.
                let content = allocator::xmlMallocImpl((len + 1) as usize) as *mut xmlChar;
                if content.is_null() {
                    return ptr::null_mut();
                }
                ptr::copy_nonoverlapping(ch, content, len as usize);
                *content.add(len as usize) = 0;

                let text = tree::new_text(content as *const xmlChar);
                allocator::xmlFreeImpl(content as *mut c_void);
                text
            }
        }
    }

    /// Set an attribute on `node` with `len` bytes of value, mirroring
    /// `tree::set_prop` but building the value text node with
    /// `parser_new_text_node` so short attribute values are compact under
    /// `XML_PARSE_COMPACT` (upstream `xmlSAX2AttributeNs` behavior).
    ///
    /// `prefix`/`uri` resolve the attribute's namespace exactly like upstream
    /// `xmlSAX2AttributeNs` (xmlSAX2.c): a non-NULL prefix is looked up in the
    /// namespace scope of the element; a NULL prefix means the attribute is
    /// NOT in the default namespace.
    ///
    /// KNOWN RESIDUAL: upstream attribute values that contain entity or
    /// character references take the `xmlNodeParseAttValue` path and are
    /// never compact; our tokenizer decodes references before the SAX layer,
    /// losing that signal, so such values may be marked compact in `--debug`
    /// dumps where the oracle shows a plain `TEXT`. Content, serialization
    /// and XPath results are identical.
    ///
    /// Returns the attribute, or NULL on failure.
    unsafe fn parser_set_prop(
        ctxt: *mut _xmlParserCtxt,
        node: *mut _xmlNode,
        name: *const xmlChar,
        prefix: *const xmlChar,
        value: *const xmlChar,
        value_len: isize,
    ) -> *mut _xmlAttr {
        if node.is_null() || name.is_null() || value.is_null() || value_len <= 0 {
            return ptr::null_mut();
        }
        unsafe {
            let n = &mut *node;

            // UPSTREAM-PARITY (xmlSAX2AttributeNs): the attribute namespace is
            // resolved by prefix against the parser's namespace scope — the
            // element's own declarations plus its ancestors. The new element is
            // not yet linked to its parent (add_child runs after attributes),
            // so the own-decl chain is scanned directly and the ancestor scope
            // via ctxt->node (the parent before the element push).
            let ns = if prefix.is_null() {
                ptr::null_mut()
            } else {
                let mut found = ptr::null_mut();
                let mut own = (*node).nsDef;
                while !own.is_null() {
                    let d = &*own;
                    if !d.prefix.is_null()
                        && crate::abi::exports_xml2::xmlStrEqual(d.prefix, prefix) != 0
                    {
                        found = own;
                        break;
                    }
                    own = (*own).next;
                }
                if found.is_null() && !(*ctxt).node.is_null() {
                    found = tree::search_ns(n.doc, (*ctxt).node, prefix);
                }
                found
            };

            // Update an existing attribute with the same name and namespace.
            let mut existing = n.properties;
            while !existing.is_null() {
                let attr = &*existing;
                let same_name = !attr.name.is_null()
                    && crate::abi::exports_xml2::xmlStrEqual(attr.name, name) != 0;
                let same_ns = match (attr.ns, ns) {
                    (a, b) if a == b => true,
                    (a, b) if a.is_null() || b.is_null() => false,
                    (a, b) => {
                        let (ah, bh) = ((*a).href, (*b).href);
                        !ah.is_null()
                            && !bh.is_null()
                            && crate::abi::exports_xml2::xmlStrEqual(ah, bh) != 0
                    }
                };
                if same_name && same_ns {
                    if !attr.children.is_null() {
                        tree::free_node_list(attr.children);
                        let attr_mut = existing as *mut _xmlAttr;
                        (*attr_mut).children = ptr::null_mut();
                        (*attr_mut).last = ptr::null_mut();
                    }
                    let text = parser_new_text_node(ctxt, value, value_len as c_int);
                    if !text.is_null() {
                        let attr_mut = existing as *mut _xmlAttr;
                        (*attr_mut).children = text;
                        (*attr_mut).last = text;
                        (*text).parent = existing as *mut _xmlNode;
                        (*text).doc = (*ctxt).myDoc;
                    }
                    return existing;
                }
                existing = (*existing).next;
            }

            // Create a new attribute.
            let attr = allocator::xmlMallocZero(size_of::<_xmlAttr>() as usize) as *mut _xmlAttr;
            if attr.is_null() {
                return ptr::null_mut();
            }
            (*attr).type_ = XML_ATTRIBUTE_NODE as c_int;
            (*attr).name = crate::xml::string::xml_strdup(name);
            (*attr).ns = ns;
            (*attr).parent = node;
            // UPSTREAM-PARITY (SAX2.c xmlSAX2AttributeNs): the attribute
            // belongs to the parser's document (the element's own doc pointer
            // is only propagated when the node is linked into the tree).
            (*attr).doc = (*ctxt).myDoc;
            // UPSTREAM-PARITY (tree.c xmlNewProp): atype is left at the
            // zero-initialised value (0) for instance attributes; the
            // XML_ATTRIBUTE_* values are used by DTD attribute declarations.

            let text = parser_new_text_node(ctxt, value, value_len as c_int);
            if !text.is_null() {
                (*attr).children = text;
                (*attr).last = text;
                (*text).parent = attr as *mut _xmlNode;
                (*text).doc = (*ctxt).myDoc;
            }

            // Add to the node's property list.
            if n.properties.is_null() {
                n.properties = attr;
            } else {
                let mut last = n.properties;
                while !(*last).next.is_null() {
                    last = (*last).next;
                }
                (*last).next = attr;
                (*attr).prev = last;
            }
            attr
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Document lifecycle
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `startDocument` handler.
    ///
    /// Creates a new document and stores it in the parser context.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    pub unsafe extern "C" fn startDocument(ctx: *mut c_void) {
        // SAFETY: Caller guarantees `ctx` is a valid parser context.
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;

            // Create a new document with default version "1.0".
            let doc = tree::new_doc(ptr::null());
            if doc.is_null() {
                return;
            }

            c.myDoc = doc;
            c.wellFormed = 1;

            // UPSTREAM-PARITY (SAX2.c xmlSAX2StartDocument): the parser
            // owns the document properties; the flags are set at
            // endDocument. parseFlags mirrors the parse options, and the
            // document shares the parser's dictionary (refcounted).
            (*doc).properties = 0;
            (*doc).parseFlags = c.options;
            (*doc).standalone = c.standalone;
            if c.dictNames != 0 {
                if c.dict.is_null() {
                    c.dict = crate::abi::exports_xml2::xmlDictCreate();
                }
                if !c.dict.is_null() {
                    (*doc).dict = c.dict;
                    crate::abi::exports_hash::xmlDictReference(c.dict);
                }
            }
        }
    }

    /// Default `endDocument` handler.
    ///
    /// Finalizes the document (marks it well-formed if applicable).
    /// Propagates version/encoding from the parser context to the document.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    pub unsafe extern "C" fn endDocument(ctx: *mut c_void) {
        // SAFETY: Caller guarantees `ctx` is a valid parser context.
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;
            if !c.myDoc.is_null() {
                // Propagate version from context to document.
                if !c.version.is_null() {
                    if (*c.myDoc).version.is_null() {
                        let dup = allocator::xmlMemStrdupImpl(c.version as *const c_char);
                        (*c.myDoc).version = dup as *mut xmlChar;
                    }
                }

                // Propagate encoding from context to document.
                if !c.encoding.is_null() {
                    if (*c.myDoc).encoding.is_null() {
                        let dup = allocator::xmlMemStrdupImpl(c.encoding as *const c_char);
                        (*c.myDoc).encoding = dup as *mut xmlChar;
                    }
                }

                // Mark the document as well-formed if no errors occurred
                // (upstream xmlSAX2EndDocument flag set).
                if c.wellFormed != 0 {
                    (*(c.myDoc)).properties |= XML_DOC_WELLFORMED as c_int;
                    if c.valid != 0 {
                        (*(c.myDoc)).properties |= XML_DOC_DTDVALID as c_int;
                    }
                    if c.nsWellFormed != 0 {
                        (*(c.myDoc)).properties |= XML_DOC_NSVALID as c_int;
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Element stack operations (SAX2)
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `startElementNs` handler.
    ///
    /// Creates a new element node, sets up namespaces, processes attributes,
    /// and pushes the node onto the element stack.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `localname` must be a valid null-terminated string.
    /// - `prefix` and `URI` may be NULL.
    /// - `namespaces` points to an array of `2 * nb_namespaces` pointers, or NULL.
    /// - `attributes` points to an array of `5 * nb_attributes` pointers, or NULL.
    #[allow(clippy::too_many_arguments)]
    pub unsafe extern "C" fn startElementNs(
        ctx: *mut c_void,
        localname: *const xmlChar,
        prefix: *const xmlChar,
        URI: *const xmlChar,
        nb_namespaces: c_int,
        namespaces: *mut *const xmlChar,
        nb_attributes: c_int,
        nb_defaulted: c_int,
        attributes: *mut *const xmlChar,
    ) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || localname.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;

            // Determine the parent node for this element.
            let parent = if c.nodeNr > 0 && !c.nodeTab.is_null() {
                let idx = (c.nodeNr - 1) as usize;
                *c.nodeTab.add(idx)
            } else {
                // No parent — this is the root element. Add it to the document.
                ptr::null_mut()
            };

            // Create the element node.
            // We use the localname as the node name. The namespace will be set below.
            let ns = if URI.is_null() && prefix.is_null() {
                ptr::null_mut()
            } else {
                // Search for an existing namespace declaration on the parent chain
                // that matches this prefix/URI combination.
                tree::search_ns(c.myDoc, parent, prefix)
            };

            let node = tree::new_node(ns, localname);
            if node.is_null() {
                return;
            }

            // UPSTREAM-PARITY: nodes carry the line of their construct.
            (*node).line = current_line(ctxt);

            // UPSTREAM-PARITY (SAX2.c xmlSAX2StartElementNs): namespace
            // declarations are built from the namespaces array only; the
            // node's namespace is resolved against its own declarations
            // first, then the parent chain (already searched above). This
            // avoids double-registering the default namespace declaration.
            let mut own_ns: *mut _xmlNs = ptr::null_mut();
            if nb_namespaces > 0 && !namespaces.is_null() {
                let mut i: c_int = 0;
                while i < nb_namespaces {
                    let ns_prefix = *namespaces.add((i * 2) as usize);
                    let ns_uri = *namespaces.add((i * 2 + 1) as usize);
                    let new_ns = tree::new_ns(node, ns_uri, ns_prefix);
                    if own_ns.is_null() && !URI.is_null() && !new_ns.is_null() {
                        let same = if ns_prefix.is_null() {
                            prefix.is_null()
                        } else if prefix.is_null() {
                            false
                        } else {
                            crate::abi::exports_xml2::xmlStrEqual(ns_prefix, prefix) != 0
                        };
                        if same {
                            own_ns = new_ns;
                        }
                    }
                    i += 1;
                }
            }
            if !own_ns.is_null() {
                (*node).ns = own_ns;
            }

            // Process attributes.
            if nb_attributes > 0 && !attributes.is_null() {
                let mut i: c_int = 0;
                while i < nb_attributes {
                    let attr_idx = (i * 5) as usize;
                    let attr_name = *attributes.add(attr_idx); // localname
                    let attr_prefix = *attributes.add(attr_idx + 1); // prefix (or NULL)
                    let attr_uri = *attributes.add(attr_idx + 2); // URI (or NULL)
                    let attr_value_start = *attributes.add(attr_idx + 3); // start of value
                    let attr_value_end = *attributes.add(attr_idx + 4); // past end of value

                    if !attr_name.is_null() && !attr_value_start.is_null() {
                        // Compute attribute value length.
                        // If value_end is NULL, the value is null-terminated;
                        // otherwise, value_end points past the last character.
                        let value_len = if attr_value_end.is_null() {
                            // Compute length from null-terminated string
                            let mut len: isize = 0;
                            unsafe {
                                while *attr_value_start.offset(len) != 0 {
                                    len += 1;
                                }
                            }
                            len
                        } else {
                            attr_value_end.offset_from(attr_value_start)
                        };
                        if value_len > 0 {
                            // UPSTREAM-PARITY: xmlSAX2AttributeNs builds the
                            // attribute value text with xmlSAX2TextNode, so
                            // short values are compact under XML_PARSE_COMPACT.
                            let attr = parser_set_prop(
                                ctxt,
                                node,
                                attr_name,
                                attr_prefix,
                                attr_value_start,
                                value_len,
                            );
                            // UPSTREAM-PARITY (SAX2.c xmlSAX2AttributeNs tail):
                            // ID/IDREF attributes are registered against the
                            // DTD attribute declarations.
                            if !attr.is_null() {
                                let av = (*attr).children;
                                let v = if av.is_null() || (*av).content.is_null() {
                                    ptr::null()
                                } else {
                                    (*av).content
                                };
                                if !v.is_null() {
                                    let res = crate::xml::validation::is_id(c.myDoc, node, attr);
                                    if res > 0 {
                                        crate::xml::validation::add_id(
                                            ptr::null_mut(),
                                            c.myDoc,
                                            v,
                                            attr,
                                        );
                                    } else if crate::xml::validation::is_ref(c.myDoc, node, attr)
                                        > 0
                                    {
                                        crate::xml::validation::add_ref(
                                            ptr::null_mut(),
                                            c.myDoc,
                                            v,
                                            attr,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    i += 1;
                }
            }

            // Add the node to the tree.
            if !parent.is_null() {
                tree::add_child(parent, node);
            } else {
                // This is the root element — attach it to the document.
                if !c.myDoc.is_null() {
                    tree::add_child(c.myDoc as *mut _xmlNode, node);
                }
            }

            // Push the node onto the element stack.
            c.node = node;
            // Extend nodeTab if needed.
            if c.nodeNr >= c.nodeMax {
                let new_max = if c.nodeMax == 0 { 8 } else { c.nodeMax * 2 };
                let new_size = (new_max as usize) * size_of::<*mut _xmlNode>();
                let new_tab = allocator::xmlReallocImpl(c.nodeTab as *mut c_void, new_size)
                    as *mut *mut _xmlNode;
                if !new_tab.is_null() {
                    c.nodeTab = new_tab;
                    c.nodeMax = new_max;
                }
            }
            if c.nodeNr < c.nodeMax && !c.nodeTab.is_null() {
                *c.nodeTab.add(c.nodeNr as usize) = node;
                c.nodeNr += 1;
            }
        }
    }

    /// Default `endElementNs` handler.
    ///
    /// Pops the current element from the element stack.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    pub unsafe extern "C" fn endElementNs(
        ctx: *mut c_void,
        localname: *const xmlChar,
        prefix: *const xmlChar,
        URI: *const xmlChar,
    ) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;

            // Pop the element stack.
            if c.nodeNr > 0 {
                c.nodeNr -= 1;
            }

            // Update the current node pointer to the new top of stack.
            if c.nodeNr > 0 && !c.nodeTab.is_null() {
                let idx = (c.nodeNr - 1) as usize;
                c.node = *c.nodeTab.add(idx);
            } else {
                c.node = ptr::null_mut();
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Content handlers
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `characters` handler.
    ///
    /// Creates a text node and adds it to the current element.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `ch` must be a valid pointer to a buffer of at least `len` bytes.
    pub unsafe extern "C" fn characters(ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || ch.is_null() || len <= 0 {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;

            // Determine the parent node (current element or document).
            let parent = if c.nodeNr > 0 && !c.nodeTab.is_null() {
                let idx = (c.nodeNr - 1) as usize;
                *c.nodeTab.add(idx)
            } else {
                c.myDoc as *mut _xmlNode
            };

            if parent.is_null() {
                return;
            }

            // UPSTREAM-PARITY: xmlSAX2Characters merges character data into
            // the parent's LAST child when it is a text node (xmlNodeAddContent).
            // An interrupted character-data stream (e.g. an entity reference)
            // is never compact, so the merge promotes compact content to heap.
            let mut last = (*parent).children;
            if !last.is_null() {
                while !(*last).next.is_null() {
                    last = (*last).next;
                }
                if (*last).type_ == XML_TEXT_NODE as c_int {
                    merge_into_text_node(last, ch, len);
                    return;
                }
            }

            let text = parser_new_text_node(ctxt, ch, len);

            if !text.is_null() {
                (*text).line = current_line(ctxt);
                tree::add_child(parent, text);
            }
        }
    }

    /// Default `ignorableWhitespace` handler.
    ///
    /// Behaves like `characters` — creates a text node from whitespace content.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `ch` must be a valid pointer to a buffer of at least `len` bytes.
    pub unsafe extern "C" fn ignorableWhitespace(ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
        // Delegate to characters handler — upstream libxml2 treats ignorable
        // whitespace the same as regular character data by default.
        // SAFETY: Same safety requirements as `characters`.
        unsafe { characters(ctx, ch, len) };
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Comment and PI handlers
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `comment` handler.
    ///
    /// Creates a comment node and adds it to the current element or document.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `value` must be a valid null-terminated string or NULL.
    pub unsafe extern "C" fn comment(ctx: *mut c_void, value: *const xmlChar) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;

            let parent = if c.nodeNr > 0 && !c.nodeTab.is_null() {
                let idx = (c.nodeNr - 1) as usize;
                *c.nodeTab.add(idx)
            } else {
                c.myDoc as *mut _xmlNode
            };

            if parent.is_null() {
                return;
            }

            let comment_node = tree::new_comment(value);
            if !comment_node.is_null() {
                (*comment_node).line = current_line(ctxt);
                tree::add_child(parent, comment_node);
            }
        }
    }

    /// Default `processingInstruction` handler.
    ///
    /// Creates a PI node and adds it to the current element or document.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `target` and `data` must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn processingInstruction(
        ctx: *mut c_void,
        target: *const xmlChar,
        data: *const xmlChar,
    ) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || target.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;

            let parent = if c.nodeNr > 0 && !c.nodeTab.is_null() {
                let idx = (c.nodeNr - 1) as usize;
                *c.nodeTab.add(idx)
            } else {
                c.myDoc as *mut _xmlNode
            };

            if parent.is_null() {
                return;
            }

            let pi_node = tree::new_pi(target, data);
            if !pi_node.is_null() {
                (*pi_node).line = current_line(ctxt);
                tree::add_child(parent, pi_node);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // CDATA handler
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `cdataBlock` handler.
    ///
    /// Creates a CDATA section node and adds it to the current element.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `value` must be a valid pointer to a buffer of at least `len` bytes.
    pub unsafe extern "C" fn cdataBlock(ctx: *mut c_void, value: *const xmlChar, len: c_int) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || value.is_null() || len <= 0 {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;

            let parent = if c.nodeNr > 0 && !c.nodeTab.is_null() {
                let idx = (c.nodeNr - 1) as usize;
                *c.nodeTab.add(idx)
            } else {
                c.myDoc as *mut _xmlNode
            };

            if parent.is_null() {
                return;
            }

            let cdata = tree::new_cdata_block(c.myDoc, value, len);
            if !cdata.is_null() {
                (*cdata).line = current_line(ctxt);
                tree::add_child(parent, cdata);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // DTD / Subset handlers
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `internalSubset` handler.
    ///
    /// Creates a DTD node for the internal subset.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `name`, `ext_id`, `sys_id` must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn internalSubset(
        ctx: *mut c_void,
        name: *const xmlChar,
        ext_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || name.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;

            if c.myDoc.is_null() {
                return;
            }

            // UPSTREAM-PARITY (SAX2.c xmlSAX2InternalSubset): a pre-existing
            // internal subset is unlinked and freed before the new one is
            // created with xmlCreateIntSubset (which links the DTD node into
            // the document's child list before the first element node).
            let old = (*(c.myDoc)).intSubset;
            if !old.is_null() {
                tree::unlink_node(old as *mut crate::abi::structs::_xmlNode);
                crate::xml::dtd::free_dtd(old);
                (*(c.myDoc)).intSubset = ptr::null_mut();
            }
            crate::xml::dtd::create_int_subset(c.myDoc, name, ext_id, sys_id);
        }
    }

    /// Default `externalSubset` handler.
    ///
    /// Records the external subset reference.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `name`, `ext_id`, `sys_id` must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn externalSubset(
        ctx: *mut c_void,
        name: *const xmlChar,
        ext_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || name.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;

            if c.myDoc.is_null() {
                return;
            }

            // If no internal subset was created, create a DTD with the external info.
            if (*(c.myDoc)).intSubset.is_null() {
                let dtd = tree::new_dtd(c.myDoc, name, ext_id, sys_id);
                if !dtd.is_null() {
                    (*(c.myDoc)).extSubset = dtd;
                }
            } else {
                // Update the external IDs on the existing DTD.
                let dtd = (*(c.myDoc)).intSubset;
                if !ext_id.is_null() {
                    let ext_copy = tree::xml_strlen(ext_id);
                    // Only set if non-empty.
                    if ext_copy > 0 {
                        (*(c.myDoc)).extSubset = dtd;
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Entity handlers
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `entityDecl` handler.
    ///
    /// Creates an entity node and adds it to the document's entity table.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn entityDecl(
        ctx: *mut c_void,
        name: *const xmlChar,
        type_: c_int,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
        content: *mut xmlChar,
    ) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || name.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;
            if c.myDoc.is_null() {
                return;
            }

            let _entity = tree::new_entity(
                c.myDoc,
                name,
                type_,
                pub_id,
                sys_id,
                content as *const xmlChar,
            );
            // The entity is added to the document's entities hash by new_entity.
            // Nothing further to do here.
        }
    }

    /// Default `attributeDecl` handler.
    ///
    /// Records an attribute declaration for the given element.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn attributeDecl(
        ctx: *mut c_void,
        elem: *const xmlChar,
        fullname: *const xmlChar,
        type_: c_int,
        def: c_int,
        default_value: *const xmlChar,
        tree: *mut _xmlEnumeration,
    ) {
        // In the default handler, attribute declarations are recorded for DTD
        // validation purposes. For now, this is a no-op — the DTD validation
        // infrastructure is not yet implemented.
        //
        // # UPSTREAM-PARITY
        //
        // Upstream libxml2's xmlSAX2AttributeDecl creates an xmlAttribute node
        // and adds it to the DTD's attribute declaration table. This will be
        // implemented when DTD validation is added.
    }

    /// Default `elementDecl` handler.
    ///
    /// Records an element declaration for the given element.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `name` must be a valid null-terminated string.
    /// - `content` must be a valid pointer or NULL.
    pub unsafe extern "C" fn elementDecl(
        ctx: *mut c_void,
        name: *const xmlChar,
        type_: c_int,
        content: *mut _xmlElementContent,
    ) {
        // No-op in the default handler — element declaration recording will be
        // implemented alongside DTD validation.
    }

    /// Default `notationDecl` handler.
    ///
    /// Records a notation declaration.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn notationDecl(
        ctx: *mut c_void,
        name: *const xmlChar,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) {
        // No-op in the default handler — notation declarations will be
        // implemented when DTD support is fully operational.
    }

    /// Default `unparsedEntityDecl` handler.
    ///
    /// Records an unparsed entity declaration.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn unparsedEntityDecl(
        ctx: *mut c_void,
        name: *const xmlChar,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
        notation: *const xmlChar,
    ) {
        // No-op in the default handler — unparsed entity declarations will be
        // implemented when DTD support is fully operational.
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Entity resolution
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `resolveEntity` handler.
    ///
    /// Returns NULL — no external entity resolution by default.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `pub_id` and `sys_id` must be valid null-terminated strings or NULL.
    pub unsafe extern "C" fn resolveEntity(
        ctx: *mut c_void,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) -> *mut _xmlParserInput {
        // Default handler does not resolve entities — return NULL.
        ptr::null_mut()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Stub handlers (minimal implementations)
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `isStandalone` handler.
    ///
    /// Returns -1 (standalone status unknown/not declared).
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    pub unsafe extern "C" fn isStandalone(ctx: *mut c_void) -> c_int {
        -1
    }

    /// Default `hasInternalSubset` handler.
    ///
    /// Returns 1 if the document has an internal subset, 0 otherwise.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    pub unsafe extern "C" fn hasInternalSubset(ctx: *mut c_void) -> c_int {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() {
            return 0;
        }
        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;
            if c.myDoc.is_null() {
                return 0;
            }
            if (*(c.myDoc)).intSubset.is_null() {
                0
            } else {
                1
            }
        }
    }

    /// Default `hasExternalSubset` handler.
    ///
    /// Returns 1 if the document has an external subset, 0 otherwise.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    pub unsafe extern "C" fn hasExternalSubset(ctx: *mut c_void) -> c_int {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() {
            return 0;
        }
        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;
            if c.myDoc.is_null() {
                return 0;
            }
            if (*(c.myDoc)).extSubset.is_null() {
                0
            } else {
                1
            }
        }
    }

    /// Default `getEntity` handler.
    ///
    /// Looks up an entity in the document's entity table.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `name` must be a valid null-terminated string.
    pub unsafe extern "C" fn getEntity(ctx: *mut c_void, name: *const xmlChar) -> *mut _xmlEntity {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || name.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;
            if c.myDoc.is_null() {
                return ptr::null_mut();
            }
            tree::get_doc_entity(c.myDoc, name)
        }
    }

    /// Default `getParameterEntity` handler.
    ///
    /// Looks up a parameter entity in the document's entity table.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `name` must be a valid null-terminated string.
    pub unsafe extern "C" fn getParameterEntity(
        ctx: *mut c_void,
        name: *const xmlChar,
    ) -> *mut _xmlEntity {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || name.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;
            if c.myDoc.is_null() {
                return ptr::null_mut();
            }
            tree::get_parameter_entity(c.myDoc, name)
        }
    }

    /// Default `setDocumentLocator` handler.
    ///
    /// No-op — the default handler does not use the locator.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `loc` must be a valid pointer to an `_xmlSAXLocator`.
    pub unsafe extern "C" fn setDocumentLocator(ctx: *mut c_void, loc: *mut _xmlSAXLocator) {
        // No-op in the default handler.
    }

    /// Default `reference` handler.
    ///
    /// No-op — entity references are expanded by the parser before reaching
    /// the default handler.
    ///
    /// # SAFETY
    ///
    /// Default `reference` handler.
    ///
    /// Creates an ENTITY_REF node in the tree (upstream default behavior when
    /// entity substitution is off).
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `name` must be a valid null-terminated string.
    /// SAX locator callback: public ID (upstream SAX2.c
    /// `xmlSAX2GetPublicId` returns NULL).
    pub unsafe extern "C" fn getPublicId(_ctx: *mut c_void) -> *const xmlChar {
        ptr::null()
    }

    /// SAX locator callback: system ID (upstream SAX2.c
    /// `xmlSAX2GetSystemId` returns the input filename).
    pub unsafe extern "C" fn getSystemId(ctx: *mut c_void) -> *const xmlChar {
        if ctx.is_null() {
            return ptr::null();
        }
        // SAFETY: ctx is a valid parser context.
        let ctxt = ctx as *mut crate::abi::structs::_xmlParserCtxt;
        unsafe {
            if (*ctxt).input.is_null() {
                return ptr::null();
            }
            let filename = (*(*ctxt).input).filename;
            if filename.is_null() {
                ptr::null()
            } else {
                filename as *const xmlChar
            }
        }
    }

    /// SAX locator callback: current line (upstream SAX2.c
    /// `xmlSAX2GetLineNumber` returns the input stream's line).
    pub unsafe extern "C" fn getLineNumber(ctx: *mut c_void) -> c_int {
        if ctx.is_null() {
            return 0;
        }
        // SAFETY: ctx is a valid parser context.
        let ctxt = ctx as *mut crate::abi::structs::_xmlParserCtxt;
        unsafe {
            if (*ctxt).input.is_null() {
                0
            } else {
                (*(*ctxt).input).line
            }
        }
    }

    /// SAX locator callback: current column (upstream SAX2.c
    /// `xmlSAX2GetColumnNumber` returns the input stream's column).
    pub unsafe extern "C" fn getColumnNumber(ctx: *mut c_void) -> c_int {
        if ctx.is_null() {
            return 0;
        }
        // SAFETY: ctx is a valid parser context.
        let ctxt = ctx as *mut crate::abi::structs::_xmlParserCtxt;
        unsafe {
            if (*ctxt).input.is_null() {
                0
            } else {
                (*(*ctxt).input).col
            }
        }
    }

    pub unsafe extern "C" fn reference(ctx: *mut c_void, name: *const xmlChar) {
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || name.is_null() {
            return;
        }

        // SAFETY: The context is guaranteed valid by the caller.
        unsafe {
            let c = &*ctxt;

            let parent = if c.nodeNr > 0 && !c.nodeTab.is_null() {
                let idx = (c.nodeNr - 1) as usize;
                *c.nodeTab.add(idx)
            } else {
                c.myDoc as *mut _xmlNode
            };

            if parent.is_null() {
                return;
            }

            // UPSTREAM-PARITY (tree.c xmlNewReference): the reference node
            // carries the entity's content (shared pointer) and points its
            // child list at the entity declaration node itself.
            let ent = crate::xml::entities::get_entity(c.myDoc, name);

            let ref_node = allocator::xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
            if !ref_node.is_null() {
                (*ref_node).type_ = XML_ENTITY_REF_NODE as c_int;
                (*ref_node).name = crate::xml::string::xml_strdup(name);
                (*ref_node).doc = c.myDoc;
                (*ref_node).line = current_line(ctxt);
                if !ent.is_null() {
                    (*ref_node).content = (*ent).content;
                    (*ref_node).children = ent as *mut _xmlNode;
                    (*ref_node).last = ent as *mut _xmlNode;
                }
                tree::add_child(parent, ref_node);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Error handlers
    // ═══════════════════════════════════════════════════════════════════════

    /// Default `warning` handler.
    ///
    /// Logs a warning message via the error reporting infrastructure.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `msg` must be a valid null-terminated C string.
    pub unsafe extern "C" fn warning(ctx: *mut c_void, msg: *const c_char) {
        // SAFETY: The context is guaranteed valid by the caller.
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || msg.is_null() {
            return;
        }

        // SAFETY: The message string is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;
            c.nbWarnings += 1;
            // In Phase 3, this will route through the structured error system.
            // For now, the parser context tracks the error count.
        }
    }

    /// Default `error` handler.
    ///
    /// Logs an error message via the error reporting infrastructure.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `msg` must be a valid null-terminated C string.
    pub unsafe extern "C" fn error(ctx: *mut c_void, msg: *const c_char) {
        // SAFETY: The context is guaranteed valid by the caller.
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || msg.is_null() {
            return;
        }

        // SAFETY: The message string is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;
            c.nbErrors += 1;
            c.wellFormed = 0;
            // In Phase 3, this will route through the structured error system.
        }
    }

    /// Default `fatalError` handler.
    ///
    /// Logs a fatal error message and marks the document as not well-formed.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid pointer to an `_xmlParserCtxt`.
    /// - `msg` must be a valid null-terminated C string.
    pub unsafe extern "C" fn fatalError(ctx: *mut c_void, msg: *const c_char) {
        // SAFETY: The context is guaranteed valid by the caller.
        let ctxt = unsafe { ctxt_from_ctx(ctx) };
        if ctxt.is_null() || msg.is_null() {
            return;
        }

        // SAFETY: The message string is guaranteed valid by the caller.
        unsafe {
            let c = &mut *ctxt;
            c.nbErrors += 1;
            c.wellFormed = 0;
            // In Phase 3, this will route through the structured error system.
        }
    }
}
