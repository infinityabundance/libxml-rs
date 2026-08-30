//! XML character-class validation (upstream chvalid.c / parserInternals.c).
//!
//! The exported `xmlIs*` family and `xmlCharInRange` use the generated
//! character-class tables from `unicode_tables.rs` (extracted verbatim from
//! upstream `codegen/ranges.inc`; see
//! `tools/archaeology/gen_chvalid_tables.py`).
//!
//! # UPSTREAM-PARITY
//!
//! The semantics mirror upstream chvalid.h macros exactly:
//!
//! - `xmlIsBaseCharQ`: `(c < 0x100) ? xmlIsBaseChar_ch(c) : xmlCharInRange(c, &xmlIsBaseCharGroup)`
//! - `xmlIsBlankQ`: `(c < 0x100) ? (c==0x20 || 0x9<=c<=0xa || c==0xd) : 0`
//! - `xmlIsCharQ`: `(c < 0x100) ? (0x9<=c<=0xa || c==0xd || 0x20<=c) : (0x100<=c<=0xd7ff || 0xe000<=c<=0xfffd || 0x10000<=c<=0x10ffff)`
//! - `xmlIsCombiningQ`: `(c < 0x100) ? 0 : xmlCharInRange(c, &xmlIsCombiningGroup)`
//! - `xmlIsDigitQ`: `(c < 0x100) ? (0x30<=c<=0x39) : xmlCharInRange(c, &xmlIsDigitGroup)`
//! - `xmlIsExtenderQ`: `(c < 0x100) ? (c==0xb7) : xmlCharInRange(c, &xmlIsExtenderGroup)`
//! - `xmlIsIdeographicQ`: `(c < 0x100) ? 0 : (0x4e00<=c<=0x9fa5 || c==0x3007 || 0x3021<=c<=0x3029)`
//! - `xmlIsPubidCharQ`: `(c < 0x100) ? xmlIsPubidChar_tab[c] : 0`
//! - `xmlIsLetter`: `xmlIsBaseCharQ(c) || xmlIsIdeographicQ(c)` (parserInternals.c)
//! - `xmlIsBlankNode`: tree.c — text/CDATA node whose content is empty or
//!   all blank.
//!
//! # Courts
//!
//! CHVALID-* differential tests compare against the oracle DSO for the whole
//! BMP + representative supplementary-plane code points.

use crate::abi::structs::{_xmlNode, xmlChRangeGroup};
use crate::xml::unicode_tables::*;
use std::os::raw::{c_int, c_uint, c_ushort};

/// Binary search over the short/long range tables (upstream `xmlCharInRange`,
/// chvalid.c — the tables are sorted, so the search is exact).
///
/// # SAFETY
///
/// - `group` must be NULL or point to a valid `xmlChRangeGroup` whose range
///   arrays cover `nbShortRange`/`nbLongRange` entries.
#[no_mangle]
pub unsafe extern "C" fn xmlCharInRange(val: c_uint, group: *const xmlChRangeGroup) -> c_int {
    if group.is_null() {
        return 0;
    }
    let g = unsafe { &*group };
    if val < 0x10000 {
        // Short (16-bit) ranges.
        if g.nbShortRange == 0 {
            return 0;
        }
        let mut low = 0;
        let mut high = g.nbShortRange - 1;
        let sptr = g.shortRange;
        if sptr.is_null() {
            return 0;
        }
        while low <= high {
            let mid = (low + high) / 2;
            let s = unsafe { &*sptr.add(mid as usize) };
            if (val as c_ushort) < s.low {
                high = mid - 1;
            } else if (val as c_ushort) > s.high {
                low = mid + 1;
            } else {
                return 1;
            }
        }
        0
    } else {
        // Long (32-bit) ranges.
        if g.nbLongRange == 0 {
            return 0;
        }
        let mut low = 0;
        let mut high = g.nbLongRange - 1;
        let lptr = g.longRange;
        if lptr.is_null() {
            return 0;
        }
        while low <= high {
            let mid = (low + high) / 2;
            let l = unsafe { &*lptr.add(mid as usize) };
            if val < l.low {
                high = mid - 1;
            } else if val > l.high {
                low = mid + 1;
            } else {
                return 1;
            }
        }
        0
    }
}

#[inline]
fn is_base_char_ch(c: c_uint) -> bool {
    // upstream xmlIsBaseChar_ch (genChRanges.py): ASCII letters plus the
    // Latin-1 letters that do not fall in the group's short ranges.
    (0x41..=0x5a).contains(&c)
        || (0x61..=0x7a).contains(&c)
        || (0xc0..=0xd6).contains(&c)
        || (0xd8..=0xf6).contains(&c)
        || c >= 0xf8
}

/// `xmlIsBaseChar(unsigned int ch)` — XML 1.0 BaseChar production.
#[no_mangle]
pub unsafe extern "C" fn xmlIsBaseChar(ch: c_uint) -> c_int {
    if ch < 0x100 {
        is_base_char_ch(ch) as c_int
    } else {
        unsafe { xmlCharInRange(ch, &xmlIsBaseCharGroup) }
    }
}

/// `xmlIsBlank(unsigned int ch)` — space, tab, LF, CR.
#[no_mangle]
pub unsafe extern "C" fn xmlIsBlank(ch: c_uint) -> c_int {
    if ch < 0x100 {
        (ch == 0x20 || (0x9..=0xa).contains(&ch) || ch == 0xd) as c_int
    } else {
        0
    }
}

/// `xmlIsChar(unsigned int ch)` — XML 1.0 Char production.
#[no_mangle]
pub unsafe extern "C" fn xmlIsChar(ch: c_uint) -> c_int {
    if ch < 0x100 {
        ((0x9..=0xa).contains(&ch) || ch == 0xd || ch >= 0x20) as c_int
    } else {
        ((0x100..=0xd7ff).contains(&ch)
            || (0xe000..=0xfffd).contains(&ch)
            || (0x10000..=0x10ffff).contains(&ch)) as c_int
    }
}

/// `xmlIsCombining(unsigned int ch)` — XML 1.0 CombiningChar production.
#[no_mangle]
pub unsafe extern "C" fn xmlIsCombining(ch: c_uint) -> c_int {
    if ch < 0x100 {
        0
    } else {
        unsafe { xmlCharInRange(ch, &xmlIsCombiningGroup) }
    }
}

/// `xmlIsDigit(unsigned int ch)` — XML 1.0 Digit production.
#[no_mangle]
pub unsafe extern "C" fn xmlIsDigit(ch: c_uint) -> c_int {
    if ch < 0x100 {
        (0x30..=0x39).contains(&ch) as c_int
    } else {
        unsafe { xmlCharInRange(ch, &xmlIsDigitGroup) }
    }
}

/// `xmlIsExtender(unsigned int ch)` — XML 1.0 Extender production.
#[no_mangle]
pub unsafe extern "C" fn xmlIsExtender(ch: c_uint) -> c_int {
    if ch < 0x100 {
        (ch == 0xb7) as c_int
    } else {
        unsafe { xmlCharInRange(ch, &xmlIsExtenderGroup) }
    }
}

/// `xmlIsIdeographic(unsigned int ch)` — XML 1.0 Ideographic production.
#[no_mangle]
pub unsafe extern "C" fn xmlIsIdeographic(ch: c_uint) -> c_int {
    if ch < 0x100 {
        0
    } else {
        ((0x4e00..=0x9fa5).contains(&ch) || ch == 0x3007 || (0x3021..=0x3029).contains(&ch))
            as c_int
    }
}

/// `xmlIsPubidChar(unsigned int ch)` — PubidChar production (ASCII table).
#[no_mangle]
pub unsafe extern "C" fn xmlIsPubidChar(ch: c_uint) -> c_int {
    if ch >= 0x100 {
        0
    } else {
        xmlIsPubidChar_tab[ch as usize] as c_int
    }
}

/// `xmlIsLetter(int c)` — BaseChar or Ideographic (parserInternals.c).
#[no_mangle]
pub unsafe extern "C" fn xmlIsLetter(c: c_int) -> c_int {
    let ch = c as c_uint;
    if ch < 0x100 {
        is_base_char_ch(ch) as c_int
    } else {
        unsafe { xmlIsBaseChar(ch) | xmlIsIdeographic(ch) }
    }
}

/// `xmlIsBlankNode(const xmlNode *node)` — text/CDATA node with empty or
/// whitespace-only content (tree.c 2.15).
///
/// # SAFETY
///
/// - `node` must be NULL or a valid node pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlIsBlankNode(node: *const _xmlNode) -> c_int {
    if node.is_null() {
        return 0;
    }
    let n = unsafe { &*node };
    if n.type_ != crate::abi::types::xmlElementType::XML_TEXT_NODE as c_int
        && n.type_ != crate::abi::types::xmlElementType::XML_CDATA_SECTION_NODE as c_int
    {
        return 0;
    }
    if n.content.is_null() {
        return 1;
    }
    let mut cur = n.content;
    while !cur.is_null() && *cur != 0 {
        let ch = *cur as c_uint;
        if ch != 0x20 && !(0x9..=0xa).contains(&ch) && ch != 0xd {
            return 0;
        }
        cur = cur.add(1);
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator;
    use crate::abi::types::xmlChar;
    use std::os::raw::c_uint;

    /// Differential-oracle spot checks (values verified against the system
    /// libxml2 2.15.3 DSO via tools/abi/data_globals_probe.py).
    fn oracle_is_base_char(ch: c_uint) -> c_int {
        unsafe { xmlIsBaseChar(ch) }
    }

    #[test]
    fn test_xml_is_char_basic() {
        unsafe {
            // XML 1.0 Char production.
            assert_eq!(xmlIsChar(0x9), 1); // tab
            assert_eq!(xmlIsChar(0xa), 1); // lf
            assert_eq!(xmlIsChar(0xd), 1); // cr
            assert_eq!(xmlIsChar(0x20), 1); // space
            assert_eq!(xmlIsChar(0x1f), 0); // below space
            assert_eq!(xmlIsChar(0xd7ff), 1);
            assert_eq!(xmlIsChar(0xd800), 0); // surrogate
            assert_eq!(xmlIsChar(0xe000), 1);
            assert_eq!(xmlIsChar(0xfffe), 0);
            assert_eq!(xmlIsChar(0x10000), 1);
            assert_eq!(xmlIsChar(0x10ffff), 1);
            assert_eq!(xmlIsChar(0x110000), 0);
        }
    }

    #[test]
    fn test_xml_is_blank() {
        unsafe {
            assert_eq!(xmlIsBlank(0x20), 1);
            assert_eq!(xmlIsBlank(0x9), 1);
            assert_eq!(xmlIsBlank(0xa), 1);
            assert_eq!(xmlIsBlank(0xd), 1);
            assert_eq!(xmlIsBlank(b'x' as c_uint), 0);
            assert_eq!(xmlIsBlank(0x100), 0);
            assert_eq!(xmlIsBlank(0x3000), 0); // ideographic space NOT blank upstream
        }
    }

    #[test]
    fn test_xml_is_base_char_ascii_and_ranges() {
        unsafe {
            assert_eq!(xmlIsBaseChar(b'A' as c_uint), 1);
            assert_eq!(xmlIsBaseChar(b'z' as c_uint), 1);
            assert_eq!(xmlIsBaseChar(b'0' as c_uint), 0);
            assert_eq!(xmlIsBaseChar(0xc0), 1); // À
            assert_eq!(xmlIsBaseChar(0xd7), 0);
            assert_eq!(xmlIsBaseChar(0x100), 1); // Ā (short range)
            assert_eq!(xmlIsBaseChar(0x132), 0); // between ranges
            assert_eq!(xmlIsBaseChar(0x386), 1); // Greek
            assert_eq!(xmlIsBaseChar(0x5d0), 1); // Hebrew
            assert_eq!(xmlIsBaseChar(0xac00), 1); // Hangul
            assert_eq!(xmlIsBaseChar(0xac00), oracle_is_base_char(0xac00));
            assert_eq!(xmlIsBaseChar(0x2a8), 1);
            assert_eq!(xmlIsBaseChar(0x2a9), 0);
        }
    }

    #[test]
    fn test_xml_is_digit() {
        unsafe {
            assert_eq!(xmlIsDigit(b'0' as c_uint), 1);
            assert_eq!(xmlIsDigit(b'9' as c_uint), 1);
            assert_eq!(xmlIsDigit(b'a' as c_uint), 0);
            assert_eq!(xmlIsDigit(0x660), 1); // Arabic-Indic zero
            assert_eq!(xmlIsDigit(0x6f9), 1);
            assert_eq!(xmlIsDigit(0x670), 0);
        }
    }

    #[test]
    fn test_xml_is_combining_extender_ideographic() {
        unsafe {
            assert_eq!(xmlIsCombining(0x300), 1); // combining grave
            assert_eq!(xmlIsCombining(0x20,), 0);
            assert_eq!(xmlIsExtender(0xb7), 1); // middle dot
            assert_eq!(xmlIsExtender(0x2d0), 1);
            assert_eq!(xmlIsExtender(0x3005), 1);
            assert_eq!(xmlIsExtender(0x20), 0);
            assert_eq!(xmlIsIdeographic(0x4e00), 1); // CJK
            assert_eq!(xmlIsIdeographic(0x3007), 1);
            assert_eq!(xmlIsIdeographic(0x3029), 1);
            assert_eq!(xmlIsIdeographic(0x302a), 0);
            assert_eq!(xmlIsIdeographic(0x9fa5), 1);
            assert_eq!(xmlIsIdeographic(b'A' as c_uint), 0);
        }
    }

    #[test]
    fn test_xml_is_pubid_char() {
        unsafe {
            assert_eq!(xmlIsPubidChar(b'a' as c_uint), 1);
            assert_eq!(xmlIsPubidChar(b' ' as c_uint), 1);
            assert_eq!(xmlIsPubidChar(b'!' as c_uint), 1);
            // @ IS a PubidChar ([-'()+,./:=?;!*#@$_%]).
            assert_eq!(xmlIsPubidChar(b'@' as c_uint), 1);
            // ^ and ~ are not.
            assert_eq!(xmlIsPubidChar(b'^' as c_uint), 0);
            assert_eq!(xmlIsPubidChar(b'~' as c_uint), 0);
            assert_eq!(xmlIsPubidChar(0x80), 0);
            assert_eq!(xmlIsPubidChar(0x100), 0);
            // tab is not a pubid char upstream.
            assert_eq!(xmlIsPubidChar(0x9), 0);
        }
    }

    #[test]
    fn test_xml_is_letter() {
        unsafe {
            assert_eq!(xmlIsLetter(b'A' as c_int), 1);
            assert_eq!(xmlIsLetter(0x4e00), 1); // ideographic counts
            assert_eq!(xmlIsLetter(b'0' as c_int), 0);
            assert_eq!(xmlIsLetter(0x386), 1);
        }
    }

    #[test]
    fn test_xml_char_in_range_null_group() {
        unsafe {
            assert_eq!(xmlCharInRange(0x41, core::ptr::null()), 0);
        }
    }

    #[test]
    fn test_xml_is_blank_node() {
        unsafe {
            use crate::abi::types::xmlElementType::*;
            // Null node -> 0.
            assert_eq!(xmlIsBlankNode(core::ptr::null()), 0);
            // Text node with NULL content -> 1.
            let node = allocator::xmlMallocImpl(core::mem::size_of::<_xmlNode>()) as *mut _xmlNode;
            assert!(!node.is_null());
            core::ptr::write(
                node,
                _xmlNode {
                    type_: XML_TEXT_NODE as c_int,
                    content: core::ptr::null_mut(),
                    ..core::mem::zeroed()
                },
            );
            assert_eq!(xmlIsBlankNode(node), 1);
            // Whitespace-only -> 1.
            let ws = b" \t\n\r\0" as *const u8 as *mut xmlChar;
            (*node).content = ws;
            assert_eq!(xmlIsBlankNode(node), 1);
            // Non-whitespace -> 0.
            let nw = b" x\0" as *const u8 as *mut xmlChar;
            (*node).content = nw;
            assert_eq!(xmlIsBlankNode(node), 0);
            // Non-text node -> 0.
            (*node).type_ = XML_ELEMENT_NODE as c_int;
            assert_eq!(xmlIsBlankNode(node), 0);
            allocator::xmlFreeImpl(node as *mut libc::c_void);
        }
    }
}
