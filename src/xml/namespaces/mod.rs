//! Namespace handling (§23, §85 Phase 2/3).
//!
//! Default namespace, prefixed namespace, namespace undeclaration, attribute
//! namespaces, duplicate declarations, shadowing, prefix reuse, conflicting
//! prefixes, orphaned namespace pointers, namespace reconciliation.
//!
//! Phase 0: scaffolded. Implementation begins in Phase 2/3.
//!
//! # Upstream contract
//!
//! Mirrors upstream namespaces.c (SRC-LIBXML2-2.15.0-NAMESPACES-C, oracle
//! tree `oracle/historical/src/libxml2-2.15.0/namespaces.c`): xmlNewNs,
//! xmlSetNs, xmlSearchNs, xmlSearchNsByHref, xmlGetNsList,
//! xmlReconciliateNs, xmlCopyNamespace, xmlCopyNamespaceList,
//! xmlFreeNamespace, xmlFreeNamespaceList, xmlNewGlobalNs.
//!
//! # Conceptual behavior
//!
//! Default namespace, prefixed namespace, namespace undeclaration, attribute
//! namespaces, duplicate declarations, shadowing, prefix reuse, conflicting
//! prefixes, orphaned namespace pointers and namespace reconciliation.
//! Namespace nodes live on node->nsDef chains and are resolved by walking the
//! element scope upward — the same model the parser and XPath rely on.
//!
//! # Ownership & safety invariants
//!
//! Ownership: an xmlNs created by xmlNewNs is owned by the node nsDef list
//! and freed with the node (xmlFreeNs only for unlinked standalone ns);
//! node->ns is a borrowed pointer. SAFETY: upstream xmlNs nodes have NO
//! parent pointer (QUIRK-0002 / LORE-0006) — the model must not invent one.
//!
//! # Historical quirks & epochs
//!
//! The no-parent namespace node is a long-standing upstream divergence fixed
//! only in the c14n birth commit 044fc6b7 (2002, issue #61290); downstream
//! XPath namespace-axis consumers depend on it. Epoch: unchanged across
//! 2.7.8 to 2.15.3 (stable case in the historical matrix).
//!
//! # Deliberate oddities
//!
//! The deliberately odd part is the nsDef-first resolution order used by
//! xmlSAX2AttributeNs (R-000147): scan the element-local declarations, then
//! fall back to the parent scope — the new element is not yet linked to its
//! parent when attributes are processed.
//!
//! # Proving courts
//!
//! Exercised by the TREE-NS-* and XPATH-NS-* court families (QUIRK-0002),
//! TREE-001 (nsDef duplicates, empty-value xmlns, default-ns handling),
//! READER-001 (attribute namespace accessors) and `cargo test --lib`.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is giving namespace nodes a parent pointer for
//! convenience — it would break XPath 1.0 namespace-axis semantics and the
//! TREE-NS-* courts. Do not deduplicate nsDef entries: upstream registers
//! each declaration once and TREE-001 fingerprints the exact chains.
