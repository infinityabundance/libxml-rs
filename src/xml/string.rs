//! String utility functions for libxml-rs.
//!
//! Provides operations on `xmlChar*` (i.e. `*mut u8`) strings compatible
//! with upstream libxml2's string handling.

use crate::abi::allocator::{xmlFree, xmlMalloc, xmlRealloc};
use crate::abi::types::xmlChar;
use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};
use std::slice;

/// Compute the length of a null-terminated `xmlChar` string.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `strlen((const char *)str)` in C.
///
/// # Safety
///
/// `str` must point to a null-terminated sequence of bytes.
#[inline]
pub(crate) unsafe fn xml_strlen(str: *const xmlChar) -> usize {
    if str.is_null() {
        return 0;
    }
    let mut len: usize = 0;
    while *str.add(len) != 0 {
        len += 1;
    }
    len
}

/// Duplicate a null-terminated `xmlChar` string using `xmlMalloc`.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `xmlStrdup` in upstream libxml2.
/// Returns a newly allocated copy. Caller must free with `xmlFree`.
///
/// # Safety
///
/// `str` must point to a null-terminated sequence of bytes, or be NULL.
#[inline]
pub(crate) unsafe fn xml_strdup(str: *const xmlChar) -> *mut xmlChar {
    if str.is_null() {
        return ptr::null_mut();
    }
    let len = xml_strlen(str);
    let copy = xmlMalloc(len + 1) as *mut xmlChar;
    if copy.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(str, copy, len + 1);
    copy
}

/// Duplicate a C `char*` string using `xmlMalloc`.
///
/// # Safety
///
/// `str` must point to a null-terminated C string, or be NULL.
#[inline]
pub(crate) unsafe fn c_strdup(str: *const c_char) -> *mut c_char {
    if str.is_null() {
        return ptr::null_mut();
    }
    let len = libc::strlen(str);
    let copy = xmlMalloc(len + 1) as *mut c_char;
    if copy.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(str as *const u8, copy as *mut u8, len + 1);
    copy
}

/// Convert a Rust byte slice to a null-terminated `xmlChar*` allocated via `xmlMalloc`.
///
/// # Safety
///
/// The caller must free the returned pointer with `xmlFree`.
pub(crate) unsafe fn bytes_to_xmlstr(bytes: &[u8]) -> *mut xmlChar {
    let len = bytes.len();
    let ptr = xmlMalloc(len + 1) as *mut xmlChar;
    if ptr.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len);
    *ptr.add(len) = 0; // null-terminate
    ptr
}

/// Convert a `*const xmlChar` to a byte slice.
///
/// # Safety
///
/// `str` must be NULL or point to a null-terminated sequence of bytes.
/// The returned slice borrows from the original memory.
#[inline]
pub(crate) unsafe fn xmlstr_to_bytes(str: *const xmlChar) -> &'static [u8] {
    if str.is_null() {
        return &[];
    }
    let len = xml_strlen(str);
    slice::from_raw_parts(str, len)
}

/// Compare two null-terminated `xmlChar` strings.
///
/// Returns 0 if equal, <0 if str1 < str2, >0 if str1 > str2.
///
/// # Safety
///
/// Both strings must be null-terminated or NULL.
#[inline]
pub(crate) unsafe fn xml_strcmp(str1: *const xmlChar, str2: *const xmlChar) -> i32 {
    if str1 == str2 {
        return 0;
    }
    if str1.is_null() {
        return -1;
    }
    if str2.is_null() {
        return 1;
    }
    let mut i: usize = 0;
    loop {
        let a = *str1.add(i);
        let b = *str2.add(i);
        if a != b {
            return a as i32 - b as i32;
        }
        if a == 0 {
            return 0;
        }
        i += 1;
    }
}

/// Concatenate two null-terminated `xmlChar` strings.
///
/// Returns a newly allocated string. Caller must free with `xmlFree`.
///
/// # Safety
///
/// Both strings must be null-terminated or NULL.
#[inline]
pub(crate) unsafe fn xml_strcat(str1: *const xmlChar, str2: *const xmlChar) -> *mut xmlChar {
    let len1 = xml_strlen(str1);
    let len2 = xml_strlen(str2);
    let result = xmlMalloc(len1 + len2 + 1) as *mut xmlChar;
    if result.is_null() {
        return ptr::null_mut();
    }
    if !str1.is_null() {
        ptr::copy_nonoverlapping(str1, result, len1);
    }
    if !str2.is_null() {
        ptr::copy_nonoverlapping(str2, result.add(len1), len2);
    }
    *result.add(len1 + len2) = 0;
    result
}

/// Convert a `*const xmlChar` to a Rust `String`.
///
/// Returns an empty string for NULL pointers.
///
/// # Safety
///
/// `str` must be NULL or point to a null-terminated sequence of bytes.
#[inline]
pub(crate) unsafe fn xml_strndup(str: *const xmlChar, len: usize) -> *mut xmlChar {
    if str.is_null() {
        return ptr::null_mut();
    }
    let p = unsafe { xmlMalloc(len + 1) as *mut xmlChar };
    if p.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(str, p, len);
        *p.add(len) = 0;
    }
    p
}

/// Convert a null-terminated `xmlChar` string into a Rust `String`
/// (lossy UTF-8 conversion; empty string for NULL).
///
/// # Safety
///
/// `str` must be NULL or point to a null-terminated byte sequence.
pub(crate) unsafe fn xmlstr_to_string(str: *const xmlChar) -> String {
    if str.is_null() {
        return String::new();
    }
    let bytes = unsafe { xmlstr_to_bytes(str) };
    String::from_utf8_lossy(bytes).to_string()
}

/// Build a QName `prefix:local` (upstream tree.c `xmlBuildQName`):
/// writes into `memory` when it is large enough, otherwise allocates.
/// Returns the resulting string (allocator-owned when not `memory`), or
/// NULL on error. A NULL prefix returns `ncname` unchanged.
///
/// # Safety
///
/// - `ncname`, `prefix` must be valid null-terminated strings or NULL.
/// - `memory` must be a valid buffer of `len` bytes or NULL.
pub unsafe fn build_qname(
    ncname: *const xmlChar,
    prefix: *const xmlChar,
    memory: *mut xmlChar,
    len: c_int,
) -> *mut xmlChar {
    if ncname.is_null() {
        return ptr::null_mut();
    }
    if prefix.is_null() {
        return ncname as *mut xmlChar;
    }
    unsafe {
        let lenn = xml_strlen(ncname);
        let lenp = xml_strlen(prefix);
        let ret = if memory.is_null() || (len as usize) < lenn + lenp + 2 {
            let p = xmlMalloc(lenn + lenp + 2) as *mut xmlChar;
            if p.is_null() {
                return ptr::null_mut();
            }
            p
        } else {
            memory
        };
        ptr::copy_nonoverlapping(prefix, ret, lenp);
        *ret.add(lenp) = b':' as xmlChar;
        ptr::copy_nonoverlapping(ncname, ret.add(lenp + 1), lenn);
        *ret.add(lenn + lenp + 1) = 0;
        ret
    }
}

/// Split a QName into prefix and local part (upstream tree.c
/// `xmlSplitQName2`): returns NULL when the name has no prefix (or starts
/// with ':'), otherwise allocates `*prefix` with the prefix and returns the
/// local part.
///
/// # Safety
///
/// - `name` must be a valid null-terminated string or NULL.
/// - `prefix` must be a valid `xmlChar**`.
pub unsafe fn split_qname2(name: *const xmlChar, prefix: *mut *mut xmlChar) -> *mut xmlChar {
    if prefix.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        *prefix = ptr::null_mut();
    }
    if name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        // "nasty but valid" (upstream): leading ':' has no prefix
        if *name == b':' as xmlChar {
            return ptr::null_mut();
        }
        let mut len: usize = 0;
        while *name.add(len) != 0 && *name.add(len) != b':' as xmlChar {
            len += 1;
        }
        if *name.add(len) == 0 || *name.add(len + 1) == 0 {
            return ptr::null_mut();
        }
        let p = xml_strndup(name, len);
        if p.is_null() {
            return ptr::null_mut();
        }
        *prefix = p;
        let ret = xml_strdup(name.add(len + 1));
        if ret.is_null() {
            xmlFree(*prefix as *mut c_void);
            *prefix = ptr::null_mut();
            return ptr::null_mut();
        }
        ret
    }
}

/// Split a QName returning the prefix length (upstream tree.c
/// `xmlSplitQName3`): returns the length of the prefix (before ':'), or 0
/// when there is no prefix. `name` must not start with ':'.
///
/// # Safety
///
/// - `name` must be a valid null-terminated string or NULL.
pub unsafe fn split_qname3(name: *const xmlChar, len: *mut c_int) -> c_int {
    if name.is_null() || len.is_null() {
        return 0;
    }
    unsafe {
        if *name == b':' as xmlChar {
            return 0;
        }
        let mut l: usize = 0;
        while *name.add(l) != 0 && *name.add(l) != b':' as xmlChar {
            l += 1;
        }
        if *name.add(l) == 0 {
            return 0;
        }
        *len = l as c_int;
        l as c_int
    }
}

/// Return the number of UTF-8 characters in a string (upstream
/// `xmlUTF8Strlen`).
///
/// # Safety
///
/// - `utf` must be a valid null-terminated UTF-8 string.
pub unsafe fn utf8_strlen(utf: *const xmlChar) -> c_int {
    if utf.is_null() {
        return 0;
    }
    unsafe {
        let mut n: c_int = 0;
        let mut cur = utf;
        while *cur != 0 {
            let c = *cur;
            if c & 0x80 == 0 {
                cur = cur.add(1);
            } else if c & 0xe0 == 0xc0 {
                cur = cur.add(2);
            } else if c & 0xf0 == 0xe0 {
                cur = cur.add(3);
            } else if c & 0xf8 == 0xf0 {
                cur = cur.add(4);
            } else {
                // invalid sequence: stop counting
                return n;
            }
            n += 1;
        }
        n
    }
}

/// Size in bytes of the UTF-8 sequence starting at `utf` (upstream
/// `xmlUTF8Size`): returns the sequence length, or -1 on invalid leading
/// byte, 0 on NUL.
///
/// # Safety
///
/// - `utf` must be a valid pointer into a UTF-8 string.
pub unsafe fn utf8_size(utf: *const xmlChar) -> c_int {
    if utf.is_null() {
        return -1;
    }
    unsafe {
        let c = *utf;
        if c == 0 {
            return 0;
        }
        if c & 0x80 == 0 {
            1
        } else if c & 0xe0 == 0xc0 {
            2
        } else if c & 0xf0 == 0xe0 {
            3
        } else if c & 0xf8 == 0xf0 {
            4
        } else {
            -1
        }
    }
}

/// Check that a byte string is valid UTF-8 (upstream `xmlCheckUTF8`):
/// returns 1 when valid, 0 otherwise.
///
/// # Safety
///
/// - `utf` must be a valid null-terminated byte string.
pub unsafe fn check_utf8(utf: *const xmlChar) -> c_int {
    if utf.is_null() {
        return 0;
    }
    unsafe {
        let mut cur = utf;
        while *cur != 0 {
            let c = *cur;
            if c & 0x80 == 0 {
                cur = cur.add(1);
            } else if c & 0xe0 == 0xc0 {
                // 2-byte: 110xxxxx 10xxxxxx
                let c1 = *cur.add(1);
                if c1 & 0xc0 != 0x80 {
                    return 0;
                }
                cur = cur.add(2);
            } else if c & 0xf0 == 0xe0 {
                let c1 = *cur.add(1);
                let c2 = *cur.add(2);
                if c1 & 0xc0 != 0x80 || c2 & 0xc0 != 0x80 {
                    return 0;
                }
                cur = cur.add(3);
            } else if c & 0xf8 == 0xf0 {
                let c1 = *cur.add(1);
                let c2 = *cur.add(2);
                let c3 = *cur.add(3);
                if c1 & 0xc0 != 0x80 || c2 & 0xc0 != 0x80 || c3 & 0xc0 != 0x80 {
                    return 0;
                }
                cur = cur.add(4);
            } else {
                return 0;
            }
        }
        1
    }
}
#[inline]
pub(crate) unsafe fn xml_str_starts_with(str: *const xmlChar, prefix: *const xmlChar) -> bool {
    if str.is_null() || prefix.is_null() {
        return false;
    }
    let mut i: usize = 0;
    loop {
        let p = *prefix.add(i);
        if p == 0 {
            return true; // reached end of prefix without mismatch
        }
        if *str.add(i) != p {
            return false;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlFree;

    #[test]
    fn test_xml_strlen() {
        unsafe {
            assert_eq!(xml_strlen(ptr::null()), 0);
            let s = b"hello\0" as *const u8 as *const xmlChar;
            assert_eq!(xml_strlen(s), 5);
            let empty = b"\0" as *const u8 as *const xmlChar;
            assert_eq!(xml_strlen(empty), 0);
        }
    }

    #[test]
    fn test_xml_strdup() {
        unsafe {
            assert!(xml_strdup(ptr::null()).is_null());
            let s = b"hello\0" as *const u8 as *const xmlChar;
            let dup = xml_strdup(s);
            assert!(!dup.is_null());
            assert_eq!(xml_strlen(dup), 5);
            assert_eq!(*dup.add(0), b'h');
            assert_eq!(*dup.add(4), b'o');
            assert_eq!(*dup.add(5), 0);
            xmlFree(dup as *mut c_void);
        }
    }

    #[test]
    fn test_xml_strcmp() {
        unsafe {
            assert_eq!(xml_strcmp(ptr::null(), ptr::null()), 0);
            assert!(xml_strcmp(b"a\0" as *const u8 as *const xmlChar, ptr::null()) > 0);
            let a = b"abc\0" as *const u8 as *const xmlChar;
            let b = b"abc\0" as *const u8 as *const xmlChar;
            assert_eq!(xml_strcmp(a, b), 0);
            let c = b"abd\0" as *const u8 as *const xmlChar;
            assert!(xml_strcmp(a, c) < 0);
            assert!(xml_strcmp(c, a) > 0);
        }
    }

    #[test]
    fn test_xml_strcat() {
        unsafe {
            let a = b"hello \0" as *const u8 as *const xmlChar;
            let b = b"world\0" as *const u8 as *const xmlChar;
            let result = xml_strcat(a, b);
            assert!(!result.is_null());
            assert_eq!(xml_strlen(result), 11);
            let expected = b"hello world\0";
            let mut i = 0;
            while expected[i] != 0 {
                assert_eq!(*result.add(i), expected[i]);
                i += 1;
            }
            xmlFree(result as *mut c_void);
        }
    }

    #[test]
    fn test_bytes_to_xmlstr() {
        unsafe {
            let bytes = b"hello";
            let ptr = bytes_to_xmlstr(bytes);
            assert!(!ptr.is_null());
            assert_eq!(xml_strlen(ptr), 5);
            assert_eq!(*ptr.add(5), 0);
            xmlFree(ptr as *mut c_void);
        }
    }

    #[test]
    fn test_xml_str_starts_with() {
        unsafe {
            let s = b"hello world\0" as *const u8 as *const xmlChar;
            let prefix = b"hello\0" as *const u8 as *const xmlChar;
            let not_prefix = b"world\0" as *const u8 as *const xmlChar;
            assert!(xml_str_starts_with(s, prefix));
            assert!(!xml_str_starts_with(s, not_prefix));
            assert!(!xml_str_starts_with(ptr::null(), prefix));
            assert!(!xml_str_starts_with(s, ptr::null()));
        }
    }
}
