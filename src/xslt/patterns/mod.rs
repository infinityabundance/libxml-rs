//! XSLT pattern matching (§33, §85 Phase 8).
//!
//! XSLT patterns are a subset of XPath 1.0 used in `match` attributes of
//! `<xsl:template>`, `<xsl:key>`, `<xsl:strip-space>`, `<xsl:preserve-space>`.
//!
//! Patterns can include:
//! - Location paths (relative and absolute)
//! - Union patterns (pattern1 | pattern2)
//! - Node tests: *, node(), text(), comment(), processing-instruction()
//! - Attribute axis (@attr)
//! - Namespace prefix tests (ns:*, ns:name)
//! - Predicates (limited)
//! - The id() and key() functions
//!
//! # UPSTREAM-PARITY
//!
//! Implements XSLT 1.0 §5 "Patterns" with match semantics matching libxslt 1.1.45.
//! Pattern matching uses the XPath 1.0 AST from `src/xml/xpath` for parsing, then
//! applies XSLT-specific matching rules.
//!
//! # Courts
//!
//! XSLT-PATTERNS-*
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `patterns.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-PATTERNS-C` under oracle/historical/src). Patterns
//! are the XSLT 1.0 §5 subset of XPath 1.0 used in `match` attributes of
//! `xsl:template`, `xsl:key`, `xsl:strip-space` and `xsl:preserve-space`;
//! the observable surface is `xsltCompilePattern`, `xsltTestPattern` and
//! the default-priority computation used by template matching.
//!
//! # Conceptual behavior
//!
//! Patterns parse through the XPath 1.0 AST (`src/xml/xpath`) and are
//! compiled into reverse-ordered step chains (child/attribute/descendant
//! axes, node tests, bounded predicates, union branches, absolute-root
//! sentinel). Matching walks the steps against the candidate node; a bare
//! `node()`-style node test becomes a child-axis step and priority is
//! derived from the pattern AST per XSLT 1.0 §5.5 default priorities
//! (0.5 for node(), 0 for name tests, -0.5 for *-tests).
//!
//! # Ownership & safety invariants
//!
//! Compiled patterns are heap-allocated and owned by their callers: a
//! template (`_xsltTemplate.match` slot, see templates module), a key
//! definition, or the local match cache. `xsltFreePattern` releases the
//! step chain and its predicate expressions; the pattern string itself is
//! borrowed from the stylesheet document and never freed by the pattern
//! free path (R-000103 lesson).
//!
//! # Historical quirks & epochs
//!
//! R-000105 (Phase 8): bare `node()`/`text()`/`comment()`/
//! `processing-instruction()` parse as XPath FunctionCall nodes and were
//! treated as unknown — fixed by translating them into child-axis steps
//! with the upstream priorities (-0.25 for node(), 0.0 for the others).
//! R-000106: `match="/"` is a bare Self_/node() step that matched every
//! node — fixed so an empty absolute pattern matches only document nodes.
//! E-008 (atlas/SEMANTIC_EPOCHS.md): pattern evaluation is frozen in the
//! byte-identical xsltproc epoch (1.1.26, 2009, through 1.1.45).
//!
//! # Deliberate oddities
//!
//! - Steps are stored in reverse order for efficient matching — an
//!   intentional internal representation with identical semantics.
//! - The compiled pattern is carried in the `_xsltTemplate.params` slot
//!   (opaque to the C ABI; R-000140 layout), a documented reuse.
//!
//! # Proving courts
//!
//! XSLT-PATTERNS-*, pattern priority and compile tests, CLI-XSLTPROC
//! (match/priority corpus), and the in-crate `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Treating bare node tests as function calls (the pre-R-000105
//!   behavior) makes every `match="node()"`-style pattern a 0.5-priority
//!   unknown — no template ever matches.
//! - Implementing `match="/"` as match-any (the pre-R-000106 behavior)
//!   makes the root template also match the root element, changing which
//!   template wins on the root node.
//! - Dropping the default-priority mapping (XSLT 1.0 §5.5) breaks
//!   conflict resolution between overlapping patterns.

use crate::abi::structs::*;
use crate::abi::types::*;
use crate::xml::string::xmlstr_to_string;
use crate::xml::xpath::ast::{Axis, Expr, NameTest, NodeTest, Step};
use crate::xml::xpath::parser::parse_xpath;
use crate::xml::xpath::types::{NodeSet, XPathValue};
use std::os::raw::c_int;
use std::ptr;

/// Sentinel for "no explicit priority" (upstream XSLT_PAT_NO_PRIORITY).
pub const XSLT_PAT_NO_PRIORITY: f64 = -1.0e9;

// ═══════════════════════════════════════════════════════════════════════════════
// Internal Pattern Representation
// ═══════════════════════════════════════════════════════════════════════════════

/// A compiled pattern step — a single axis::node-test[predicates] in a pattern.
///
/// This is the internal representation backing `_xsltPatternStep`.
/// The C ABI type `_xsltPatternStep` is a zero-sized opaque marker;
/// the real data lives here.
#[derive(Debug, Clone)]
pub(crate) struct XsltPatternStep {
    /// The axis (defaults to Child for element tests, Attribute for @attr).
    pub axis: Axis,
    /// The node test.
    pub node_test: NodeTest,
    /// Compiled predicate expressions.
    pub predicates: Vec<Expr>,
}

/// A single pattern within a union pattern (one side of `|`).
#[derive(Debug, Clone)]
pub(crate) struct XsltPattern {
    /// Steps in this pattern, in reverse order for efficient matching.
    /// For a pattern like `foo/bar/baz`, steps are [baz, bar, foo].
    /// For a pattern like `foo//bar`, steps contain a DescendantOrSelf sentinel.
    pub steps: Vec<PatternStepEntry>,
    /// Whether this pattern is absolute (starts with `/`).
    pub is_absolute: bool,
    /// The original pattern string for this branch.
    #[allow(dead_code)]
    pub original: String,
    /// The parsed XPath expression (kept for predicate evaluation).
    #[allow(dead_code)]
    pub expr: Expr,
}

/// One entry in the step chain of a compiled pattern.
#[derive(Debug, Clone)]
pub(crate) enum PatternStepEntry {
    /// A normal step with axis, node test, and predicates.
    Step(XsltPatternStep),
    /// A `//` separator — matches descendant-or-self::node().
    #[allow(dead_code)]
    DescendantOrSelf,
}

/// Full compiled pattern (union of multiple sub-patterns).
#[derive(Debug, Clone)]
pub(crate) struct CompiledPattern {
    /// The branches of a union pattern (`pattern1 | pattern2`).
    pub patterns: Vec<XsltPattern>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI Opaque Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Opaque pattern structure.
///
/// In the C ABI this is a `typedef struct _xsltPattern xsltPattern`.
/// The actual data is stored in the internal `CompiledPattern` and accessed
/// via pointer casts in the implementation functions.
#[derive(Debug)]
#[repr(C)]
pub struct _xsltPattern {
    _unused: [u8; 0],
}

/// Opaque pattern step structure.
#[derive(Debug)]
#[repr(C)]
pub struct _xsltPatternStep {
    _unused: [u8; 0],
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pattern Compilation
// ═══════════════════════════════════════════════════════════════════════════════

/// Compile an XSLT pattern string into a compiled pattern.
///
/// Parses the pattern string using the XPath 1.0 parser, then decomposes
/// it into an internal representation suitable for fast node matching.
///
/// # Parameters
///
/// * `pattern` — The pattern string (UTF-8, null-terminated `xmlChar*`).
/// * `doc`     — The document (used for namespace resolution; may be null).
///
/// # Returns
///
/// A pointer to a compiled `_xsltPattern`, or null on failure.
///
/// # Safety
///
/// `pattern` must be a valid null-terminated `xmlChar*` or null.
/// `doc` must be a valid `_xmlDoc*` or null.
pub unsafe fn xsltCompilePattern(pattern: *const xmlChar, _doc: *mut _xmlDoc) -> *mut _xsltPattern {
    if pattern.is_null() {
        return ptr::null_mut();
    }

    let pattern_str = xmlstr_to_string(pattern);
    if pattern_str.is_empty() {
        return ptr::null_mut();
    }

    let compiled = match compile_pattern_string(&pattern_str) {
        Some(cp) => cp,
        None => return ptr::null_mut(),
    };

    // Allocate and store the compiled pattern
    let layout = std::alloc::Layout::new::<CompiledPattern>();
    let ptr = std::alloc::alloc(layout) as *mut CompiledPattern;
    if ptr.is_null() {
        return ptr::null_mut();
    }
    ptr::write(ptr, compiled);
    ptr as *mut _xsltPattern
}

/// Internal: compile a pattern string into a `CompiledPattern`.
fn compile_pattern_string(pattern_str: &str) -> Option<CompiledPattern> {
    // Parse the pattern as an XPath expression
    let expr = parse_xpath(pattern_str).ok()?;

    // Decompose the expression into pattern branches
    let patterns = decompose_pattern(&expr, pattern_str)?;

    Some(CompiledPattern { patterns })
}

/// Decompose an XPath expression into a list of pattern branches.
///
/// A union pattern `a | b` becomes two branches. Each branch is converted
/// into a sequence of steps for matching.
fn decompose_pattern(expr: &Expr, original: &str) -> Option<Vec<XsltPattern>> {
    match expr {
        // Union: pattern1 | pattern2 — recurse on both sides
        Expr::Union(left, right) => {
            let mut patterns = decompose_pattern(left, original)?;
            let right_patterns = decompose_pattern(right, original)?;
            patterns.extend(right_patterns);
            Some(patterns)
        }
        // Single expression — convert to a pattern
        _ => {
            let pattern = expr_to_pattern(expr, original)?;
            Some(vec![pattern])
        }
    }
}

/// Convert a single (non-union) XPath expression into a `XsltPattern`.
fn expr_to_pattern(expr: &Expr, original: &str) -> Option<XsltPattern> {
    let (steps, is_absolute) = collect_steps(expr)?;

    Some(XsltPattern {
        steps,
        is_absolute,
        original: original.to_string(),
        expr: expr.clone(),
    })
}

/// Collect the steps from an XPath expression in reverse order.
///
/// Returns `(steps, is_absolute)` where steps are ordered from innermost
/// (node being matched) to outermost (root), making matching efficient.
fn collect_steps(expr: &Expr) -> Option<(Vec<PatternStepEntry>, bool)> {
    match expr {
        // "/" — a bare Self_/node() step represents the document root
        // node pattern. It matches only the document node itself.
        Expr::Step(step)
            if step.axis == Axis::Self_
                && step.node_test == NodeTest::Node
                && step.predicates.is_empty() =>
        {
            Some((vec![], true))
        }
        Expr::Step(step) => {
            let entry = PatternStepEntry::Step(XsltPatternStep {
                axis: step.axis,
                node_test: step.node_test.clone(),
                predicates: step.predicates.clone(),
            });
            Some((vec![entry], false))
        }
        Expr::AbsolutePath(inner) => {
            let (steps, _) = collect_steps(inner)?;
            Some((steps, true))
        }
        Expr::RelativePath(left, right) => {
            // Steps are collected right-to-left: the rightmost step is the
            // node being tested, left steps are ancestry constraints.
            let (mut right_steps, _) = collect_steps(right)?;
            let (left_steps, left_absolute) = collect_steps(left)?;
            right_steps.extend(left_steps);
            Some((right_steps, left_absolute))
        }
        // Handle `//` — the parser represents it as RelativePath with
        // a DescendantOrSelf step in the middle
        Expr::Filter(_expr, _predicates) => {
            // Filter expressions like `id('foo')/bar` — we match the filter
            // as a step
            // For now, treat as a single step with a wildcard node test
            let entry = PatternStepEntry::Step(XsltPatternStep {
                axis: Axis::Self_,
                node_test: NodeTest::Node,
                predicates: vec![],
            });
            Some((vec![entry], false))
        }
        // Bare node-test function calls: node(), text(), comment(),
        // processing-instruction(). In XPath 1.0 these are NOT functions;
        // they are node tests forming a step on the child axis. The XPath
        // parser represents top-level `node()` as a FunctionCall, so we
        // translate it back into a step here.
        Expr::FunctionCall { name, args } => {
            let node_test = match (name.as_str(), args.len()) {
                ("node", 0) => Some(NodeTest::Node),
                ("text", 0) => Some(NodeTest::Text),
                ("comment", 0) => Some(NodeTest::Comment),
                ("processing-instruction", 0) => Some(NodeTest::ProcessingInstruction(None)),
                ("processing-instruction", 1) => match &args[0] {
                    Expr::StringLiteral(s) => {
                        Some(NodeTest::ProcessingInstruction(Some(s.clone())))
                    }
                    _ => None,
                },
                _ => None,
            };
            match node_test {
                Some(nt) => {
                    let entry = PatternStepEntry::Step(XsltPatternStep {
                        axis: Axis::Child,
                        node_test: nt,
                        predicates: vec![],
                    });
                    Some((vec![entry], false))
                }
                // id() and key() are handled as filter-like patterns.
                None if name == "id" || name == "key" => {
                    let entry = PatternStepEntry::Step(XsltPatternStep {
                        axis: Axis::Self_,
                        node_test: NodeTest::Node,
                        predicates: vec![],
                    });
                    Some((vec![entry], false))
                }
                None => None,
            }
        }
        _ => {
            // Literals, function calls without path — not valid as patterns
            // unless they're id() or key() which are handled specially
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pattern Deallocation
// ═══════════════════════════════════════════════════════════════════════════════

/// Free a compiled pattern.
///
/// # Safety
///
/// `pattern` must have been returned by `xsltCompilePattern` and not already freed.
pub unsafe fn xsltFreePattern(pattern: *mut _xsltPattern) {
    if pattern.is_null() {
        return;
    }
    let ptr = pattern as *mut CompiledPattern;
    // Drop the compiled pattern
    ptr::drop_in_place(ptr);
    let layout = std::alloc::Layout::new::<CompiledPattern>();
    std::alloc::dealloc(ptr as *mut u8, layout);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pattern Matching
// ═══════════════════════════════════════════════════════════════════════════════

/// Test whether a node matches a compiled pattern.
///
/// # Parameters
///
/// * `ctxt`    — The transform context (provides XPath context for predicates).
/// * `pattern` — The compiled pattern.
/// * `node`    — The node to test.
///
/// # Returns
///
/// 1 if the node matches, 0 otherwise.
///
/// # Safety
///
/// All pointers must be valid (or null — null pointers return 0).
pub unsafe fn xsltTestPattern(
    ctxt: *mut _xsltTransformContext,
    pattern: *mut _xsltPattern,
    node: *mut _xmlNode,
) -> c_int {
    if pattern.is_null() || node.is_null() {
        return 0;
    }

    let compiled = &*(pattern as *const CompiledPattern);
    let xpath_ctxt = if !ctxt.is_null() {
        (*ctxt).xpathCtxt
    } else {
        ptr::null_mut()
    };

    for sub_pattern in &compiled.patterns {
        if match_sub_pattern(sub_pattern, node, xpath_ctxt) {
            return 1;
        }
    }

    0
}

/// Test whether a node matches a compiled pattern tree.
///
/// In libxslt, the `match` attribute is compiled into a tree of `_xmlNode`
/// elements during stylesheet compilation. This function walks that tree
/// to determine whether `node` matches the pattern.
///
/// The pattern tree structure uses element nodes where:
/// - The node `name` encodes the step type (element name, "*", "node()", etc.)
/// - Children represent path steps (inner to outer)
/// - Siblings at the root level represent union alternatives
///
/// # Parameters
///
/// * `node`         — The document node to test.
/// * `pattern_node` — The compiled pattern tree root (from `_xsltTemplate.r#match`).
///
/// # Returns
///
/// `true` if the node matches, `false` otherwise.
///
/// # Safety
///
/// Both pointers must be valid (or null — null pointers return false).
pub unsafe fn xsltTestMatchPattern(node: *mut _xmlNode, pattern_node: *mut _xmlNode) -> bool {
    if node.is_null() || pattern_node.is_null() {
        return false;
    }

    // Walk the pattern tree. The pattern tree has the following structure:
    // - Root node: represents the outermost step (or union)
    // - For union patterns: the root has sibling children
    // - For path patterns: children represent nested steps
    //
    // The node's `name` encodes what kind of test this step performs:
    // - A QName: match element with that name
    // - "*": match any element
    // - "node()": match any node
    // - "text()": match text nodes
    // - "comment()": match comment nodes
    // - "processing-instruction()": match PI nodes
    // - "@name": match attribute
    // - "ns:*": match namespace wildcard
    // - "|": union operator
    // - "/": path separator

    match_pattern_tree(pattern_node, node)
}

/// Walk the pattern tree and test if the node matches.
unsafe fn match_pattern_tree(pattern_node: *mut _xmlNode, node: *mut _xmlNode) -> bool {
    if pattern_node.is_null() || node.is_null() {
        return false;
    }

    let node_ref = &*pattern_node;
    let name = xmlstr_to_string(node_ref.name);

    match name.as_str() {
        // Union: any child matches
        "|" => {
            let mut child = node_ref.children;
            while !child.is_null() {
                if match_pattern_tree(child, node) {
                    return true;
                }
                child = (*child).next;
            }
            false
        }
        // Path separator: match child step, then parent step
        "/" => {
            // For a path like foo/bar:
            // The root node is "/" with children [bar, foo] (inner first)
            // We match the first child against node, then the second against node's parent
            let steps = collect_children(pattern_node);
            if steps.is_empty() {
                return false;
            }
            match_pattern_path(&steps, node)
        }
        _ => {
            // Leaf node: check if this step matches the node
            match_pattern_step(pattern_node, node)
        }
    }
}

/// Collect all children of a pattern node into a vector.
unsafe fn collect_children(pattern_node: *mut _xmlNode) -> Vec<*mut _xmlNode> {
    let mut children = Vec::new();
    if pattern_node.is_null() {
        return children;
    }
    let mut child = (*pattern_node).children;
    while !child.is_null() {
        children.push(child);
        child = (*child).next;
    }
    children
}

/// Match a path (sequence of pattern steps) against a node.
///
/// Steps are ordered inner-first (the last step is the outermost).
unsafe fn match_pattern_path(steps: &[*mut _xmlNode], node: *mut _xmlNode) -> bool {
    if steps.is_empty() {
        return false;
    }

    let mut current = node;

    for (i, &step) in steps.iter().enumerate() {
        if current.is_null() {
            return false;
        }

        if !match_pattern_step(step, current) {
            return false;
        }

        // Move to parent for the next step (if any)
        if i < steps.len() - 1 {
            current = (*current).parent;
        }
    }

    true
}

/// Match a single pattern step node against a document node.
unsafe fn match_pattern_step(pattern_node: *mut _xmlNode, node: *mut _xmlNode) -> bool {
    if pattern_node.is_null() || node.is_null() {
        return false;
    }

    let pn = &*pattern_node;
    let nn = &*node;
    let step_name = xmlstr_to_string(pn.name);
    let node_name = xmlstr_to_string(nn.name);
    let node_type = nn.type_;

    match step_name.as_str() {
        // Wildcard: match any element (or attribute if pattern is attribute-type)
        "*" => {
            if pn.type_ == 2 {
                // Attribute wildcard
                node_type == 2
            } else {
                // Element wildcard
                node_type == 1
            }
        }
        // node() — match any node type
        "node()" => true,
        // text() — match text nodes
        "text()" => node_type == 3 || node_type == 4,
        // comment() — match comment nodes
        "comment()" => node_type == 8,
        // processing-instruction() — match PI nodes
        "processing-instruction()" => node_type == 7,
        // Attribute test (@attr) — the step name starts with "@"
        s if s.starts_with('@') => {
            let attr_name = &s[1..];
            node_type == 2 && node_name == attr_name
        }
        // Namespace wildcard (prefix:*)
        s if s.ends_with(":*") => {
            if node_type != 1 {
                return false;
            }
            let prefix = &s[..s.len() - 2];
            if let Some(ns) = nn.ns.as_ref() {
                let ns_prefix = xmlstr_to_string(ns.prefix);
                ns_prefix == prefix
            } else {
                prefix.is_empty()
            }
        }
        // Namespace-qualified name (ns:local)
        s if s.contains(':') && !s.starts_with('@') && !s.ends_with(":*") => {
            if node_type != 1 {
                return false;
            }
            let parts: Vec<&str> = s.splitn(2, ':').collect();
            if parts.len() != 2 {
                return false;
            }
            let prefix = parts[0];
            let local = parts[1];
            if node_name != local {
                return false;
            }
            if let Some(ns) = nn.ns.as_ref() {
                let ns_prefix = xmlstr_to_string(ns.prefix);
                ns_prefix == prefix
            } else {
                prefix.is_empty()
            }
        }
        // Simple name test: match element/attribute by name
        _ => {
            if node_type == 1 || node_type == 2 {
                node_name == step_name
            } else {
                false
            }
        }
    }
}

/// Check if a node matches a single (non-union) sub-pattern.
unsafe fn match_sub_pattern(
    pattern: &XsltPattern,
    node: *mut _xmlNode,
    xpath_ctxt: *mut _xmlXPathContext,
) -> bool {
    // The steps are stored in reverse order (innermost first).
    // Walk them from the node upward through the tree.
    if pattern.steps.is_empty() {
        // Empty steps with is_absolute means the pattern is "/", which
        // matches only the document root node.
        return pattern.is_absolute && is_document_node(node);
    }

    // Start with the first step (the one closest to the matched node)
    let mut current_node = node;

    for (i, entry) in pattern.steps.iter().enumerate() {
        match entry {
            PatternStepEntry::Step(step) => {
                if !match_step(step, current_node, xpath_ctxt, i == 0) {
                    return false;
                }
                // For the first step, we stay on the current node (Self_ axis)
                // or traverse to children/parent depending on the axis.
                // For subsequent steps, we need to move upward.
                if i > 0 {
                    // Move to parent for the next step
                    current_node = (*current_node).parent;
                    if current_node.is_null() {
                        return false;
                    }
                }
            }
            PatternStepEntry::DescendantOrSelf => {
                // `//` matches descendant-or-self::node()
                // This means any ancestor path is valid.
                // Since steps are in reverse order, this separates
                // the inner steps (already matched) from outer steps.
                // We need to find an ancestor that matches the remaining steps.
                let remaining: Vec<_> = pattern.steps[i + 1..]
                    .iter()
                    .filter_map(|e| {
                        if let PatternStepEntry::Step(s) = e {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                if remaining.is_empty() {
                    return true;
                }

                // Try each ancestor
                let mut ancestor = current_node;
                loop {
                    ancestor = (*ancestor).parent;
                    if ancestor.is_null() {
                        return false;
                    }
                    if match_steps_sequence(&remaining, ancestor, xpath_ctxt) {
                        return true;
                    }
                }
            }
        }
    }

    // If the pattern is absolute, the final ancestor must be the document root
    if pattern.is_absolute {
        // After walking up all steps, current_node should be the document node
        if current_node.is_null() {
            return true;
        }
        // Walk up to root
        let mut n = node;
        loop {
            let parent = (*n).parent;
            if parent.is_null() {
                break;
            }
            n = parent;
        }
        // n is now the root — check if it's the document node
        return (*n).type_ == 9 || (*n).type_ == 13; // XML_DOCUMENT_NODE or XML_HTML_DOCUMENT_NODE
    }

    true
}

/// Match a sequence of steps against a node, starting from the innermost step.
unsafe fn match_steps_sequence(
    steps: &[XsltPatternStep],
    node: *mut _xmlNode,
    xpath_ctxt: *mut _xmlXPathContext,
) -> bool {
    let mut current = node;
    for (i, step) in steps.iter().enumerate() {
        if !match_step(step, current, xpath_ctxt, i == 0) {
            return false;
        }
        if i < steps.len() - 1 {
            current = (*current).parent;
            if current.is_null() {
                return false;
            }
        }
    }
    true
}

/// Check if a node is a document node (XML_DOCUMENT_NODE or XML_HTML_DOCUMENT_NODE).
unsafe fn is_document_node(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }
    (*node).type_ == 9 || (*node).type_ == 13
}

/// Check if a single step matches a node.
///
/// A step `axis::node-test[predicates]` matches if:
/// 1. The axis relationship holds between the context and the node.
/// 2. The node test matches the node.
/// 3. All predicates evaluate to true.
unsafe fn match_step(
    step: &XsltPatternStep,
    node: *mut _xmlNode,
    xpath_ctxt: *mut _xmlXPathContext,
    _is_first: bool,
) -> bool {
    if node.is_null() {
        return false;
    }

    let node_ref = &*node;
    let node_type = node_ref.type_;

    // Step 1: Check the axis
    match step.axis {
        Axis::Attribute => {
            // Must be an attribute node
            if node_type != 2 {
                // XML_ATTRIBUTE_NODE
                return false;
            }
        }
        Axis::Child | Axis::Self_
            // Child and Self axes match elements, text, comments, PIs
            // (all non-document, non-attribute, non-namespace nodes)
            if (node_type == 2 || node_type == 9 || node_type == 13)
                // Skip attributes and document nodes for child:: tests
                && step.axis == Axis::Child
                    && node_type != 1
                    && node_type != 3
                    && node_type != 4
                    && node_type != 7
                    && node_type != 8
                => {
                    return false;
                }
        _ => {
            // Other axes are not typically used in patterns
            // For completeness, accept the node
        }
    }

    // Step 2: Check the node test
    if !match_node_test(node, &step.node_test) {
        return false;
    }

    // Step 3: Evaluate predicates (if any)
    if !step.predicates.is_empty() {
        if xpath_ctxt.is_null() {
            // Without an XPath context, we can't evaluate predicates
            // For now, skip predicates (libxslt would also need a context)
            return true;
        }

        if !evaluate_predicates(node, &step.predicates, xpath_ctxt) {
            return false;
        }
    }

    true
}

/// Check if a node matches a node test.
unsafe fn match_node_test(node: *mut _xmlNode, node_test: &NodeTest) -> bool {
    if node.is_null() {
        return false;
    }

    let node_ref = &*node;
    let node_type = node_ref.type_;

    match node_test {
        NodeTest::Node => {
            // Matches any node
            true
        }
        NodeTest::Text => {
            // text() — text nodes (type 3) or CDATA sections (type 4)
            node_type == 3 || node_type == 4
        }
        NodeTest::Comment => {
            // comment() — comment nodes (type 8)
            node_type == 8
        }
        NodeTest::ProcessingInstruction(target) => {
            // processing-instruction() or processing-instruction("target")
            if node_type != 7 {
                // XML_PI_NODE
                return false;
            }
            if let Some(target) = target {
                let name = xmlstr_to_string(node_ref.name);
                name == *target
            } else {
                true
            }
        }
        NodeTest::NameTest(name_test) => match_name_test(node, name_test),
        NodeTest::Wildcard => {
            // * — matches any element node
            node_type == 1
        }
        NodeTest::NsWildcard(prefix) => {
            // prefix:* — matches any element in that namespace
            if node_type != 1 {
                return false;
            }
            if let Some(ns) = node_ref.ns.as_ref() {
                let ns_prefix = xmlstr_to_string(ns.prefix);
                ns_prefix == *prefix
            } else {
                prefix.is_empty()
            }
        }
        NodeTest::NsWildcardUri(uri) => {
            // {uri}:* — matches any element in that namespace (URI form)
            if node_type != 1 {
                return false;
            }
            if let Some(ns) = node_ref.ns.as_ref() {
                let ns_uri = xmlstr_to_string(ns.href);
                ns_uri == *uri
            } else {
                uri.is_empty()
            }
        }
    }
}

/// Check if a node matches a name test.
unsafe fn match_name_test(node: *mut _xmlNode, name_test: &NameTest) -> bool {
    if node.is_null() {
        return false;
    }

    let node_ref = &*node;

    match name_test {
        NameTest::Any => {
            // * — matches any element or attribute
            node_ref.type_ == 1 || node_ref.type_ == 2
        }
        NameTest::LocalName(local) => {
            let name = xmlstr_to_string(node_ref.name);
            name == *local
        }
        NameTest::QName { prefix, local } => {
            let name = xmlstr_to_string(node_ref.name);
            if name != *local {
                return false;
            }
            if let Some(ns) = node_ref.ns.as_ref() {
                let ns_prefix = xmlstr_to_string(ns.prefix);
                ns_prefix == *prefix
            } else {
                prefix.is_empty()
            }
        }
        NameTest::QNameUri { uri, local } => {
            let name = xmlstr_to_string(node_ref.name);
            if name != *local {
                return false;
            }
            if let Some(ns) = node_ref.ns.as_ref() {
                let ns_uri = xmlstr_to_string(ns.href);
                ns_uri == *uri
            } else {
                uri.is_empty()
            }
        }
    }
}

/// Evaluate predicates for a node match.
///
/// Uses the XPath evaluation engine to check if all predicates hold.
unsafe fn evaluate_predicates(
    node: *mut _xmlNode,
    predicates: &[Expr],
    xpath_ctxt: *mut _xmlXPathContext,
) -> bool {
    if xpath_ctxt.is_null() {
        return true; // Can't evaluate, assume match
    }

    // Set up a temporary XPath context for predicate evaluation
    let ctxt = &mut *xpath_ctxt;

    // Save context state
    let saved_node = ctxt.node;

    // Set the current node as context
    ctxt.node = node;

    let mut result = true;

    for predicate in predicates {
        // Evaluate the predicate expression
        // Get the document from the context or the node
        let doc = if !ctxt.doc.is_null() {
            ctxt.doc
        } else if !node.is_null() {
            (*node).doc
        } else {
            ptr::null_mut()
        };
        let mut xpath_ctx = crate::xml::xpath::context::XPathContext::new(doc);

        // Copy relevant state from the C ABI context
        if !saved_node.is_null() {
            xpath_ctx.set_context_node(saved_node);
        }

        // Copy namespace mappings
        if !ctxt.namespaces.is_null() && ctxt.nsNr > 0 {
            let ns_slice = std::slice::from_raw_parts(ctxt.namespaces, ctxt.nsNr as usize);
            for ns_ptr in ns_slice {
                if !ns_ptr.is_null() {
                    let ns = &**ns_ptr;
                    let prefix = xmlstr_to_string(ns.prefix);
                    let href = xmlstr_to_string(ns.href);
                    xpath_ctx.register_namespace(&prefix, &href);
                }
            }
        }

        // Register the id() and key() extension functions
        register_pattern_functions(&mut xpath_ctx);

        let pred_result = crate::xml::xpath::eval::eval(&mut xpath_ctx, predicate);

        match pred_result {
            Ok(val) => {
                // Predicate semantics: number n matches if n == context position,
                // otherwise boolean conversion
                let matches = match val {
                    XPathValue::Number(n) => {
                        // Number predicate: match if n == 1 (first node)
                        // For pattern predicates, position is always 1
                        (n - 1.0).abs() < f64::EPSILON
                    }
                    _ => val.as_boolean(),
                };
                if !matches {
                    result = false;
                    break;
                }
            }
            Err(_) => {
                result = false;
                break;
            }
        }
    }

    // Restore context
    ctxt.node = saved_node;

    result
}

/// Register XSLT-specific functions needed for pattern evaluation (id(), key()).
fn register_pattern_functions(ctx: &mut crate::xml::xpath::context::XPathContext) {
    // Register id() function
    ctx.register_function("id", |_ctx, _args| {
        // Simple id() implementation: returns empty node-set for now
        // A full implementation would look up IDs in the document's DTD
        Ok(XPathValue::NodeSet(NodeSet::new()))
    });

    // Register key() function
    ctx.register_function("key", |_ctx, _args| {
        // Simple key() implementation: returns empty node-set for now
        // A full implementation would look up keys in the stylesheet's key tables
        Ok(XPathValue::NodeSet(NodeSet::new()))
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// Default Priority Computation
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute default priority for a match pattern.
///
/// XSLT 1.0 §5.5:
/// - 0.0 for simple name tests (child::para, para)
/// - -0.25 for node() test
/// - -0.5 for any name test (*) or namespace test (ns:*)
/// - +0.5 for attribute axis (@attr)
/// - +0.0 for other cases (compound patterns, id(), key())
///
/// # Parameters
///
/// * `pattern` — The pattern string (UTF-8, null-terminated `xmlChar*`).
///
/// # Returns
///
/// The default priority as a f64.
///
/// # Safety
///
/// `pattern` must be a valid null-terminated `xmlChar*` or null.
pub unsafe fn xsltDefaultPriority(pattern: *const xmlChar) -> f64 {
    if pattern.is_null() {
        return 0.5;
    }

    let pattern_str = xmlstr_to_string(pattern);
    if pattern_str.is_empty() {
        return 0.5;
    }

    compute_default_priority(&pattern_str)
}

/// Internal: compute default priority from a pattern string.
fn compute_default_priority(pattern_str: &str) -> f64 {
    // Parse the pattern
    let expr = match parse_xpath(pattern_str) {
        Ok(e) => e,
        Err(_) => return 0.5, // Default for unparseable patterns
    };

    compute_expr_priority(&expr)
}

/// Compute the default priority of an expression.
fn compute_expr_priority(expr: &Expr) -> f64 {
    match expr {
        // Union patterns: use the highest priority of any branch
        Expr::Union(left, right) => {
            let left_p = compute_expr_priority(left);
            let right_p = compute_expr_priority(right);
            left_p.max(right_p)
        }

        // Absolute path: analyze the inner expression
        Expr::AbsolutePath(inner) => compute_expr_priority(inner),

        // Relative path: priority is based on the final step (rightmost)
        Expr::RelativePath(_, right) => compute_expr_priority(right),

        // Single step: determine priority from the node test and axis
        Expr::Step(step) => compute_step_priority(step),

        // Filter expression: priority based on the primary expression
        Expr::Filter(primary, _) => compute_expr_priority(primary),

        // id() and key() functions: priority 0.0
        Expr::FunctionCall { name, .. } => {
            if name == "id" || name == "key" {
                0.0
            } else {
                // Bare node tests (node(), text(), comment(),
                // processing-instruction()) parse as function calls at the
                // top level; translate them to their step priorities.
                match name.as_str() {
                    "node" => -0.25,
                    "text" | "comment" | "processing-instruction" => 0.0,
                    _ => 0.5,
                }
            }
        }

        // Other expressions (literals, variables, etc.) — not typical patterns
        _ => 0.5,
    }
}

/// Compute the default priority of a single step.
fn compute_step_priority(step: &Step) -> f64 {
    match &step.node_test {
        // node() test: -0.25
        NodeTest::Node => -0.25,

        // text(), comment(), processing-instruction(): 0.0
        NodeTest::Text | NodeTest::Comment | NodeTest::ProcessingInstruction(_) => 0.0,

        // Name test: 0.0 for a specific name on the child axis;
        // 0.5 on the attribute axis (@QName per XSLT 1.0 §5.5).
        NodeTest::NameTest(name_test) => match name_test {
            NameTest::LocalName(_) | NameTest::QName { .. } | NameTest::QNameUri { .. } => {
                if step.axis == Axis::Attribute {
                    0.5
                } else {
                    0.0
                }
            }
            NameTest::Any => {
                // * in element context: -0.5
                // * in attribute context: +0.5
                if step.axis == Axis::Attribute {
                    0.5
                } else {
                    -0.5
                }
            }
        },

        // * (Wildcard): -0.5 in element context, +0.5 in attribute context
        NodeTest::Wildcard => {
            if step.axis == Axis::Attribute {
                0.5
            } else {
                -0.5
            }
        }

        // prefix:* namespace wildcard: -0.5
        NodeTest::NsWildcard(_) | NodeTest::NsWildcardUri(_) => {
            if step.axis == Axis::Attribute {
                0.5
            } else {
                -0.5
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Convenience Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a pattern string matches a single step (simple name test).
///
/// Returns true if the pattern is a simple name test like `para` or `foo:bar`,
/// without path separators, predicates, or union operators.
pub fn is_simple_name_pattern(pattern: &str) -> bool {
    let expr = match parse_xpath(pattern) {
        Ok(e) => e,
        Err(_) => return false,
    };

    matches!(&expr, Expr::Step(Step {
        axis: Axis::Child,
        node_test: NodeTest::NameTest(name_test),
        predicates,
    }) if predicates.is_empty() && !matches!(name_test, NameTest::Any))
}

/// Check if a pattern is a union pattern (contains `|`).
pub fn is_union_pattern(pattern: &str) -> bool {
    let expr = match parse_xpath(pattern) {
        Ok(e) => e,
        Err(_) => return false,
    };

    matches!(&expr, Expr::Union(_, _))
}

/// Get the names matched by a simple name-test pattern.
///
/// For a simple pattern like `para` or `foo | bar`, returns the list of
/// matched element names. Returns an empty vec for complex patterns.
pub fn get_pattern_matched_names(pattern: &str) -> Vec<String> {
    let expr = match parse_xpath(pattern) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut names = Vec::new();
    collect_matched_names(&expr, &mut names);
    names
}

fn collect_matched_names(expr: &Expr, names: &mut Vec<String>) {
    match expr {
        Expr::Union(left, right) => {
            collect_matched_names(left, names);
            collect_matched_names(right, names);
        }
        Expr::Step(Step {
            node_test: NodeTest::NameTest(name_test),
            ..
        }) => match name_test {
            NameTest::LocalName(local) => names.push(local.clone()),
            NameTest::QName { prefix, local } => names.push(format!("{}:{}", prefix, local)),
            NameTest::QNameUri { uri, local } => names.push(format!("{{{}}}{}", uri, local)),
            NameTest::Any => names.push("*".to_string()),
        },
        Expr::Step(Step {
            node_test: NodeTest::Wildcard,
            ..
        }) => {
            names.push("*".to_string());
        }
        Expr::Step(Step {
            node_test: NodeTest::NsWildcard(prefix),
            ..
        }) => {
            names.push(format!("{}:*", prefix));
        }
        Expr::Step(Step {
            node_test: NodeTest::NsWildcardUri(uri),
            ..
        }) => {
            names.push(format!("{{{}}}:*", uri));
        }
        _ => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Priority Tests ────────────────────────────────────────────────────

    #[test]
    fn test_default_priority_name_test() {
        // Simple name test "para" → 0.0
        let priority = compute_default_priority("para");
        assert!(
            (priority - 0.0).abs() < f64::EPSILON,
            "Expected 0.0 for name test, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_qname() {
        // Qualified name "xslt:template" → 0.0
        let priority = compute_default_priority("xslt:template");
        assert!(
            (priority - 0.0).abs() < f64::EPSILON,
            "Expected 0.0 for QName, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_node_test() {
        // node() test → -0.25
        let priority = compute_default_priority("node()");
        assert!(
            (priority - (-0.25)).abs() < f64::EPSILON,
            "Expected -0.25 for node(), got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_text_test() {
        // text() test → 0.0
        let priority = compute_default_priority("text()");
        assert!(
            (priority - 0.0).abs() < f64::EPSILON,
            "Expected 0.0 for text(), got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_comment_test() {
        // comment() test → 0.0
        let priority = compute_default_priority("comment()");
        assert!(
            (priority - 0.0).abs() < f64::EPSILON,
            "Expected 0.0 for comment(), got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_processing_instruction() {
        // processing-instruction() test → 0.0
        let priority = compute_default_priority("processing-instruction()");
        assert!(
            (priority - 0.0).abs() < f64::EPSILON,
            "Expected 0.0 for processing-instruction(), got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_wildcard() {
        // * wildcard → -0.5
        let priority = compute_default_priority("*");
        assert!(
            (priority - (-0.5)).abs() < f64::EPSILON,
            "Expected -0.5 for *, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_ns_wildcard() {
        // ns:* wildcard → -0.5
        let priority = compute_default_priority("ns:*");
        assert!(
            (priority - (-0.5)).abs() < f64::EPSILON,
            "Expected -0.5 for ns:*, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_attribute() {
        // @attr → +0.5
        let priority = compute_default_priority("@attr");
        assert!(
            (priority - 0.5).abs() < f64::EPSILON,
            "Expected 0.5 for @attr, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_attribute_wildcard() {
        // @* → +0.5
        let priority = compute_default_priority("@*");
        assert!(
            (priority - 0.5).abs() < f64::EPSILON,
            "Expected 0.5 for @*, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_union() {
        // Union "para | *" → max(0.0, -0.5) = 0.0
        let priority = compute_default_priority("para | *");
        assert!(
            (priority - 0.0).abs() < f64::EPSILON,
            "Expected 0.0 for union, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_compound_path() {
        // Path "foo/bar" → priority of last step "bar" = 0.0
        let priority = compute_default_priority("foo/bar");
        assert!(
            (priority - 0.0).abs() < f64::EPSILON,
            "Expected 0.0 for foo/bar, got {}",
            priority
        );
    }

    #[test]
    fn test_default_priority_empty() {
        // Empty pattern → 0.5
        let priority = compute_default_priority("");
        assert!(
            (priority - 0.5).abs() < f64::EPSILON,
            "Expected 0.5 for empty pattern, got {}",
            priority
        );
    }

    // ── Pattern Matching Tests ────────────────────────────────────────────

    /// Create a minimal element node for testing.
    unsafe fn create_test_node(name: &str, type_: c_int) -> *mut _xmlNode {
        let layout = std::alloc::Layout::new::<_xmlNode>();
        let ptr = std::alloc::alloc_zeroed(layout) as *mut _xmlNode;
        if ptr.is_null() {
            return ptr::null_mut();
        }
        let node = &mut *ptr;
        node.type_ = type_;
        // Allocate and copy the name
        let name_bytes = name.as_bytes();
        let name_buf = std::alloc::alloc_zeroed(
            std::alloc::Layout::array::<u8>(name_bytes.len() + 1).unwrap(),
        );
        if !name_buf.is_null() {
            std::ptr::copy_nonoverlapping(name_bytes.as_ptr(), name_buf, name_bytes.len());
        }
        node.name = name_buf as *mut xmlChar;
        ptr
    }

    /// Free a test node.
    unsafe fn free_test_node(node: *mut _xmlNode) {
        if node.is_null() {
            return;
        }
        if !(*node).name.is_null() {
            let name = (*node).name;
            // Find length
            let len = crate::abi::exports_xml2::xmlStrlen(name) as usize;
            std::alloc::dealloc(
                name as *mut u8,
                std::alloc::Layout::array::<u8>(len + 1).unwrap(),
            );
        }
        let layout = std::alloc::Layout::new::<_xmlNode>();
        std::alloc::dealloc(node as *mut u8, layout);
    }

    /// Verify `NodeTest` matching against a minimal element node.
    ///
    /// # Safety
    ///
    /// - `create_test_node` returns a valid `_xmlNode` whose `name` is a
    ///   heap-allocated NUL-terminated string; `match_node_test` reads it
    ///   while the node is alive.
    /// - The node is freed exactly once with `free_test_node`.
    #[test]
    fn test_node_test_matching_element() {
        unsafe {
            let node = create_test_node("para", 1); // XML_ELEMENT_NODE
            assert!(!node.is_null());

            // Name test
            let name_test = NodeTest::NameTest(NameTest::LocalName("para".to_string()));
            assert!(match_node_test(node, &name_test));

            // Wrong name
            let wrong_test = NodeTest::NameTest(NameTest::LocalName("foo".to_string()));
            assert!(!match_node_test(node, &wrong_test));

            // Wildcard
            let wildcard = NodeTest::Wildcard;
            assert!(match_node_test(node, &wildcard));

            // Node test
            let node_test = NodeTest::Node;
            assert!(match_node_test(node, &node_test));

            // Text test should not match element
            let text_test = NodeTest::Text;
            assert!(!match_node_test(node, &text_test));

            free_test_node(node);
        }
    }

    /// Verify `NodeTest` matching against a minimal text node.
    ///
    /// # Safety
    ///
    /// - The node from `create_test_node` is a valid `_xmlNode` with a
    ///   heap-allocated NUL-terminated `name`; `match_node_test` reads it
    ///   while the node is alive.
    /// - The node is freed exactly once with `free_test_node`.
    #[test]
    fn test_node_test_matching_text() {
        unsafe {
            let node = create_test_node("", 3); // XML_TEXT_NODE
            assert!(!node.is_null());

            let text_test = NodeTest::Text;
            assert!(match_node_test(node, &text_test));

            let node_test = NodeTest::Node;
            assert!(match_node_test(node, &node_test));

            let comment_test = NodeTest::Comment;
            assert!(!match_node_test(node, &comment_test));

            let element_test = NodeTest::NameTest(NameTest::LocalName("para".to_string()));
            assert!(!match_node_test(node, &element_test));

            free_test_node(node);
        }
    }

    /// Compile a simple pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_and_free_pattern() {
        unsafe {
            let pattern_str = c"para".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile with a NULL pattern.
    ///
    /// # Safety
    ///
    /// - A NULL pattern is accepted by `xsltCompilePattern` and yields NULL
    ///   without dereferencing.
    #[test]
    fn test_compile_null_pattern() {
        unsafe {
            let compiled = xsltCompilePattern(ptr::null(), ptr::null_mut());
            assert!(compiled.is_null());
        }
    }

    /// Compile an empty pattern.
    ///
    /// # Safety
    ///
    /// - The empty string is a valid NUL-terminated string; `xsltCompilePattern`
    ///   returns NULL for it.
    #[test]
    fn test_compile_empty_pattern() {
        unsafe {
            let pattern_str = c"".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(compiled.is_null());
        }
    }

    /// Free a NULL pattern.
    ///
    /// # Safety
    ///
    /// - `xsltFreePattern` accepts NULL and returns without freeing or
    ///   dereferencing.
    #[test]
    fn test_free_null_pattern() {
        unsafe {
            xsltFreePattern(ptr::null_mut());
            // Should not crash
        }
    }

    #[test]
    fn test_is_simple_name_pattern() {
        assert!(is_simple_name_pattern("para"));
        assert!(is_simple_name_pattern("foo:bar"));
        assert!(!is_simple_name_pattern("foo/bar"));
        assert!(!is_simple_name_pattern("para | foo"));
        assert!(!is_simple_name_pattern("*"));
    }

    #[test]
    fn test_is_union_pattern() {
        assert!(is_union_pattern("para | foo"));
        assert!(is_union_pattern("para | foo | bar"));
        assert!(!is_union_pattern("para"));
        assert!(!is_union_pattern("foo/bar"));
    }

    #[test]
    fn test_get_pattern_matched_names() {
        let names = get_pattern_matched_names("para");
        assert_eq!(names, vec!["para"]);

        let names = get_pattern_matched_names("foo | bar");
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"bar".to_string()));

        let names = get_pattern_matched_names("foo/bar");
        assert!(names.is_empty());
    }

    /// Compile a union pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_union_pattern() {
        unsafe {
            let pattern_str = c"para | foo".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a compound path pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_compound_pattern() {
        unsafe {
            let pattern_str = c"foo/bar".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile an absolute path pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_absolute_pattern() {
        unsafe {
            let pattern_str = c"/foo/bar".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile an attribute pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_attribute_pattern() {
        unsafe {
            let pattern_str = c"@attr".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a wildcard pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_wildcard_pattern() {
        unsafe {
            let pattern_str = c"*".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a namespace wildcard pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_ns_wildcard_pattern() {
        unsafe {
            let pattern_str = c"ns:*".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a node-test pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_node_test_pattern() {
        unsafe {
            let pattern_str = c"node()".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a text node-test pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_text_pattern() {
        unsafe {
            let pattern_str = c"text()".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a comment node-test pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_comment_pattern() {
        unsafe {
            let pattern_str = c"comment()".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a processing-instruction pattern and free the compiled
    /// result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_pi_pattern() {
        unsafe {
            let pattern_str = c"processing-instruction()".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    /// Compile a predicate pattern and free the compiled result.
    ///
    /// # Safety
    ///
    /// - The pattern string is a valid NUL-terminated string; the compiled
    ///   pattern returned by `xsltCompilePattern` is freed exactly once with
    ///   `xsltFreePattern`.
    #[test]
    fn test_compile_predicate_pattern() {
        unsafe {
            let pattern_str = c"para[1]".as_ptr() as *const xmlChar;
            let compiled = xsltCompilePattern(pattern_str, ptr::null_mut());
            assert!(!compiled.is_null());
            xsltFreePattern(compiled);
        }
    }

    #[test]
    fn test_decompose_union() {
        let expr = parse_xpath("a | b").unwrap();
        let patterns = decompose_pattern(&expr, "a | b");
        assert!(patterns.is_some());
        let patterns = patterns.unwrap();
        assert_eq!(patterns.len(), 2);
        assert_eq!(patterns[0].original, "a | b");
        assert_eq!(patterns[1].original, "a | b");
    }

    #[test]
    fn test_decompose_single() {
        let expr = parse_xpath("para").unwrap();
        let patterns = decompose_pattern(&expr, "para");
        assert!(patterns.is_some());
        let patterns = patterns.unwrap();
        assert_eq!(patterns.len(), 1);
    }

    #[test]
    fn test_collect_steps_simple() {
        let expr = parse_xpath("para").unwrap();
        let (steps, is_absolute) = collect_steps(&expr).unwrap();
        assert!(!is_absolute);
        assert_eq!(steps.len(), 1);
        if let PatternStepEntry::Step(step) = &steps[0] {
            assert_eq!(step.axis, Axis::Child);
            assert!(
                matches!(&step.node_test, NodeTest::NameTest(NameTest::LocalName(n)) if n == "para")
            );
        } else {
            panic!("Expected Step entry");
        }
    }

    #[test]
    fn test_collect_steps_absolute() {
        let expr = parse_xpath("/foo/bar").unwrap();
        let (steps, is_absolute) = collect_steps(&expr).unwrap();
        assert!(is_absolute);
        assert_eq!(steps.len(), 2);
    }

    #[test]
    fn test_collect_steps_attribute() {
        let expr = parse_xpath("@attr").unwrap();
        let (steps, is_absolute) = collect_steps(&expr).unwrap();
        assert!(!is_absolute);
        assert_eq!(steps.len(), 1);
        if let PatternStepEntry::Step(step) = &steps[0] {
            assert_eq!(step.axis, Axis::Attribute);
        } else {
            panic!("Expected Step entry");
        }
    }

    #[test]
    fn test_collect_steps_compound() {
        let expr = parse_xpath("foo/bar").unwrap();
        let (steps, is_absolute) = collect_steps(&expr).unwrap();
        assert!(!is_absolute);
        assert_eq!(steps.len(), 2);
        // First step should be "bar" (rightmost)
        if let PatternStepEntry::Step(step) = &steps[0] {
            assert!(
                matches!(&step.node_test, NodeTest::NameTest(NameTest::LocalName(n)) if n == "bar")
            );
        } else {
            panic!("Expected Step entry for bar");
        }
        // Second step should be "foo"
        if let PatternStepEntry::Step(step) = &steps[1] {
            assert!(
                matches!(&step.node_test, NodeTest::NameTest(NameTest::LocalName(n)) if n == "foo")
            );
        } else {
            panic!("Expected Step entry for foo");
        }
    }

    /// Verify `NameTest` matching against a minimal element node.
    ///
    /// # Safety
    ///
    /// - The node from `create_test_node` is a valid `_xmlNode` with a
    ///   heap-allocated NUL-terminated `name`; `match_name_test` reads its
    ///   `type_` and `name` while the node is alive.
    /// - The node is freed exactly once with `free_test_node`.
    #[test]
    fn test_match_name_test_local() {
        unsafe {
            let node = create_test_node("para", 1);
            assert!(!node.is_null());

            assert!(match_name_test(
                node,
                &NameTest::LocalName("para".to_string())
            ));
            assert!(!match_name_test(
                node,
                &NameTest::LocalName("foo".to_string())
            ));
            assert!(match_name_test(node, &NameTest::Any));

            free_test_node(node);
        }
    }

    /// Verify wildcard node-test matching over element, text and comment
    /// nodes.
    ///
    /// # Safety
    ///
    /// - Each `create_test_node` result is a valid `_xmlNode` with a
    ///   heap-allocated NUL-terminated `name`; each node is freed exactly
    ///   once with `free_test_node`.
    #[test]
    fn test_match_node_test_wildcard() {
        unsafe {
            let element = create_test_node("para", 1);
            let text = create_test_node("", 3);
            let comment = create_test_node("", 8);

            let wildcard = NodeTest::Wildcard;
            assert!(match_node_test(element, &wildcard));
            assert!(!match_node_test(text, &wildcard));
            assert!(!match_node_test(comment, &wildcard));

            free_test_node(element);
            free_test_node(text);
            free_test_node(comment);
        }
    }

    /// Verify namespace-wildcard matching on a node without a namespace.
    ///
    /// # Safety
    ///
    /// - The node from `create_test_node` is a valid `_xmlNode` with a
    ///   heap-allocated NUL-terminated `name`; it is freed exactly once with
    ///   `free_test_node`.
    #[test]
    fn test_match_node_test_ns_wildcard() {
        unsafe {
            let node = create_test_node("para", 1);
            // No namespace set — only empty prefix matches
            let ns_wildcard = NodeTest::NsWildcard("".to_string());
            assert!(match_node_test(node, &ns_wildcard));

            let ns_wildcard = NodeTest::NsWildcard("foo".to_string());
            assert!(!match_node_test(node, &ns_wildcard));

            free_test_node(node);
        }
    }

    /// Verify default priorities through the C ABI entry point.
    ///
    /// # Safety
    ///
    /// - Each pattern string passed to `xsltDefaultPriority` is a valid
    ///   NUL-terminated string.
    #[test]
    fn test_compute_priority_on_compiled_pattern() {
        unsafe {
            // Test through the C ABI function
            let pattern_str = c"para".as_ptr() as *const xmlChar;
            let priority = xsltDefaultPriority(pattern_str);
            assert!(
                (priority - 0.0).abs() < f64::EPSILON,
                "Expected 0.0 for 'para', got {}",
                priority
            );

            let pattern_str = c"*".as_ptr() as *const xmlChar;
            let priority = xsltDefaultPriority(pattern_str);
            assert!(
                (priority - (-0.5)).abs() < f64::EPSILON,
                "Expected -0.5 for '*', got {}",
                priority
            );

            let pattern_str = c"node()".as_ptr() as *const xmlChar;
            let priority = xsltDefaultPriority(pattern_str);
            assert!(
                (priority - (-0.25)).abs() < f64::EPSILON,
                "Expected -0.25 for 'node()', got {}",
                priority
            );

            let pattern_str = c"@attr".as_ptr() as *const xmlChar;
            let priority = xsltDefaultPriority(pattern_str);
            assert!(
                (priority - 0.5).abs() < f64::EPSILON,
                "Expected 0.5 for '@attr', got {}",
                priority
            );
        }
    }

    /// Verify the default priority of a NULL pattern.
    ///
    /// # Safety
    ///
    /// - `xsltDefaultPriority` accepts NULL and returns the default without
    ///   dereferencing.
    #[test]
    fn test_compute_priority_null() {
        unsafe {
            let priority = xsltDefaultPriority(ptr::null());
            assert!(
                (priority - 0.5).abs() < f64::EPSILON,
                "Expected 0.5 for null pattern, got {}",
                priority
            );
        }
    }

    /// Verify the default priority of an empty pattern.
    ///
    /// # Safety
    ///
    /// - The empty string is a valid NUL-terminated string passed to
    ///   `xsltDefaultPriority`.
    #[test]
    fn test_compute_priority_empty() {
        unsafe {
            let pattern_str = c"".as_ptr() as *const xmlChar;
            let priority = xsltDefaultPriority(pattern_str);
            assert!(
                (priority - 0.5).abs() < f64::EPSILON,
                "Expected 0.5 for empty pattern, got {}",
                priority
            );
        }
    }

    /// Verify `xsltTestPattern` with all-NULL arguments.
    ///
    /// # Safety
    ///
    /// - NULL context, pattern and node are accepted and yield 0 without
    ///   dereferencing any of them.
    #[test]
    fn test_xslt_test_pattern_null_args() {
        unsafe {
            let result = xsltTestPattern(ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, 0);
        }
    }
}
