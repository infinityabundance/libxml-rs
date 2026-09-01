#!/usr/bin/env python3
"""Mechanical TLS conversion of data_globals.rs __xml*/xmlThrDef* bodies."""
import re

p = 'src/abi/data_globals.rs'
s = open(p).read()

# 1) __xml* accessor bodies -> TLS pointer accessors (18 TLS-era globals)
ptr_map = {
    'addr_of_mut!(xmlIndentTreeOutput)': 'crate::xml::globals::indent_tree_output_ptr()',
    'addr_of_mut!(xmlSaveNoEmptyTags)': 'crate::xml::globals::save_no_empty_tags_ptr()',
    'addr_of_mut!(xmlTreeIndentString)': 'crate::xml::globals::tree_indent_string_ptr()',
    'addr_of_mut!(xmlDeregisterNodeDefaultValue)': 'crate::xml::globals::deregister_node_ptr()',
    'addr_of_mut!(xmlDoValidityCheckingDefaultValue)': 'crate::xml::globals::do_validity_ptr()',
    'addr_of_mut!(xmlGenericError)': 'crate::xml::globals::generic_error_ptr()',
    'addr_of_mut!(xmlGenericErrorContext)': 'crate::xml::globals::generic_error_ctx_ptr()',
    'addr_of_mut!(xmlGetWarningsDefaultValue)': 'crate::xml::globals::get_warnings_ptr()',
    'addr_of_mut!(xmlKeepBlanksDefaultValue)': 'crate::xml::globals::keep_blanks_ptr()',
    'addr_of_mut!(xmlLineNumbersDefaultValue)': 'crate::xml::globals::line_numbers_ptr()',
    'addr_of_mut!(xmlLoadExtDtdDefaultValue)': 'crate::xml::globals::load_ext_dtd_ptr()',
    'addr_of_mut!(xmlOutputBufferCreateFilenameValue)': 'crate::xml::globals::output_create_filename_ptr()',
    'addr_of_mut!(xmlParserInputBufferCreateFilenameValue)': 'crate::xml::globals::parser_input_create_filename_ptr()',
    'addr_of_mut!(xmlPedanticParserDefaultValue)': 'crate::xml::globals::pedantic_ptr()',
    'addr_of_mut!(xmlRegisterNodeDefaultValue)': 'crate::xml::globals::register_node_ptr()',
    'addr_of_mut!(xmlStructuredError)': 'crate::xml::globals::structured_error_ptr()',
    'addr_of_mut!(xmlStructuredErrorContext)': 'crate::xml::globals::structured_error_ctx_ptr()',
    'addr_of_mut!(xmlSubstituteEntitiesDefaultValue)': 'crate::xml::globals::substitute_entities_ptr()',
}
for k, v in ptr_map.items():
    assert k in s, 'missing ' + k
    s = s.replace(k, v)

# 2) xmlThrDef* int-global bodies
thrdef_map = {
    'xmlThrDefDoValidityCheckingDefaultValue': ('set_validity_checking_default', 'get_validity_checking_default'),
    'xmlThrDefGetWarningsDefaultValue': ('set_get_warnings_default', 'get_get_warnings_default'),
    'xmlThrDefKeepBlanksDefaultValue': ('set_keep_blanks_default', 'get_keep_blanks_default'),
    'xmlThrDefSubstituteEntitiesDefaultValue': ('set_substitute_entities_default', 'get_substitute_entities_default'),
    'xmlThrDefLineNumbersDefaultValue': ('set_line_numbers_default', 'get_line_numbers_default'),
    'xmlThrDefIndentTreeOutput': ('set_indent_tree_output', 'get_indent_tree_output'),
    'xmlThrDefSaveNoEmptyTags': ('set_save_no_empty_tags', 'get_save_no_empty_tags'),
}
INT_FN_TMPL = (
    r'(pub unsafe extern "C" fn {fn}\(v: c_int\) -> c_int \{\n)'
    r'(\s*)unsafe \{\n(.*?)\n\s*\}\n\}')
for fn, (setter, getter) in thrdef_map.items():
    m = re.search(INT_FN_TMPL.replace('{fn}', fn), s, re.S)
    assert m, 'no body for ' + fn
    indent = m.group(2)
    lines = m.group(3).split('\n')
    assign = read = None
    for ln in lines:
        t = ln.strip()
        if t.endswith('= v;') and t.startswith('xml'):
            assign = t.split('=')[0].strip()
        if t.startswith('xml') and t.endswith(';') and '=' not in t:
            read = t[:-1].strip()
        elif t.startswith('xml') and '=' not in t and '(' not in t and t != 'if v != 0 {':
            read = t.strip()
    assert assign and read, 'assign/read not found in ' + fn
    assert assign == read, fn + ': ' + assign + ' != ' + read
    new = (m.group(1) + indent + 'if v != 0 {\n'
           + indent + '    crate::xml::globals::' + setter + '(v);\n'
           + indent + '}\n'
           + indent + 'crate::xml::globals::' + getter + '()\n}')
    s = s[:m.start()] + new + s[m.end():]
    print('converted ' + fn)

# 3) xmlThrDefTreeIndentString
TREE_INDENT_FN = re.compile(
    r'(pub unsafe extern "C" fn xmlThrDefTreeIndentString\(v: \*const c_char\) -> \*const c_char \{\n)'
    r'(\s*)unsafe \{\n(.*?)\n\s*\}(\n\})', re.S)
m = TREE_INDENT_FN.search(s)
assert m, 'no xmlThrDefTreeIndentString'
indent = m.group(2)
new = (m.group(1) + indent + 'if !v.is_null() {\n'
       + indent + '    crate::xml::globals::set_tree_indent_string('
       + 'v as *const crate::abi::types::xmlChar);\n'
       + indent + '}\n'
       + indent + 'crate::xml::globals::get_tree_indent_string() as *const c_char\n}')
s = s[:m.start()] + new + s[m.end():]
print('converted xmlThrDefTreeIndentString')

# 4) xmlThrDefRegisterNodeDefault / xmlThrDefDeregisterNodeDefault
NODE_FN = re.compile(
    r'(pub unsafe extern "C" fn {fn}\(\n\s*func: Option<unsafe extern "C" fn\(\*mut crate::abi::structs::_xmlNode\)>,\n'
    r'\) -> Option<unsafe extern "C" fn\(\*mut crate::abi::structs::_xmlNode\)> \{\n)'
    r'(\s*)unsafe \{\n(.*?)\n\s*\}(\n\})', re.S)
for fn in ('xmlThrDefRegisterNodeDefault', 'xmlThrDefDeregisterNodeDefault'):
    setter = 'set_register_node_default' if fn == 'xmlThrDefRegisterNodeDefault' else 'set_deregister_node_default'
    getter = 'get_register_node_default' if fn == 'xmlThrDefRegisterNodeDefault' else 'get_deregister_node_default'
    m = re.search(NODE_FN.pattern.replace('{fn}', fn), s, re.S)
    assert m, 'no ' + fn
    indent = m.group(2)
    new = (m.group(1) + indent + 'if func.is_some() {\n'
           + indent + '    crate::xml::globals::' + setter + '(func);\n'
           + indent + '}\n'
           + indent + 'crate::xml::globals::' + getter + '()\n}')
    s = s[:m.start()] + new + s[m.end():]
    print('converted ' + fn)

# 5) xmlThrDefSetGenericErrorFunc / xmlThrDefSetStructuredErrorFunc
SET_GEN = re.compile(
    r'(pub unsafe extern "C" fn xmlThrDefSetGenericErrorFunc\(\n\s*ctx: \*mut c_void,\n'
    r'\s*func: Option<xmlGenericErrorFunc>,\n\) \{\n)(\s*)unsafe \{\n(.*?)\n\s*\}(\n\})', re.S)
m = SET_GEN.search(s)
assert m, 'no xmlThrDefSetGenericErrorFunc'
indent = m.group(2)
new = m.group(1) + indent + 'crate::xml::globals::set_generic_error_func(ctx, func);\n}'
s = s[:m.start()] + new + s[m.end():]
print('converted xmlThrDefSetGenericErrorFunc')

SET_STR = re.compile(
    r'(pub unsafe extern "C" fn xmlThrDefSetStructuredErrorFunc\(\n\s*ctx: \*mut c_void,\n'
    r'\s*func: Option<xmlStructuredErrorFunc>,\n\) \{\n)(\s*)unsafe \{\n(.*?)\n\s*\}(\n\})', re.S)
m = SET_STR.search(s)
assert m, 'no xmlThrDefSetStructuredErrorFunc'
indent = m.group(2)
new = m.group(1) + indent + 'crate::xml::globals::set_structured_error_func(ctx, func);\n}'
s = s[:m.start()] + new + s[m.end():]
print('converted xmlThrDefSetStructuredErrorFunc')

# 6) xmlThrDefParserInputBufferCreateFilenameDefault
PICF = re.compile(
    r'(pub unsafe extern "C" fn xmlThrDefParserInputBufferCreateFilenameDefault\(.*?\) -> Option<\n'
    r'\s*unsafe extern "C" fn\(\*const c_char, c_int\) -> \*mut crate::abi::structs::_xmlParserInputBuffer,\n> \{\n)'
    r'(\s*)unsafe \{\n(.*?)\n\s*\}(\n\})', re.S)
m = PICF.search(s)
assert m, 'no xmlThrDefParserInputBufferCreateFilenameDefault'
indent = m.group(2)
new = (m.group(1) + indent + 'if func.is_some() {\n'
       + indent + '    crate::xml::globals::set_parser_input_buffer_create_filename_value(func);\n'
       + indent + '}\n'
       + indent + 'crate::xml::globals::get_parser_input_buffer_create_filename_value()\n}')
s = s[:m.start()] + new + s[m.end():]
print('converted xmlThrDefParserInputBufferCreateFilenameDefault')

# xmlThrDefOutputBufferCreateFilenameDefault
OCF = re.compile(
    r'(pub unsafe extern "C" fn xmlThrDefOutputBufferCreateFilenameDefault\(.*?\) -> Option<\n'
    r'\s*unsafe extern "C" fn\(\n\s*\*const c_char,\n\s*crate::abi::structs::xmlCharEncodingHandlerPtr,\n\s*c_int,\n\s*\) -> \*mut crate::abi::structs::_xmlOutputBuffer,\n> \{\n)'
    r'(\s*)unsafe \{\n(.*?)\n\s*\}(\n\})', re.S)
m = OCF.search(s)
assert m, 'no xmlThrDefOutputBufferCreateFilenameDefault'
indent = m.group(2)
new = (m.group(1) + indent + 'if func.is_some() {\n'
       + indent + '    crate::xml::globals::set_output_buffer_create_filename_value(func);\n'
       + indent + '}\n'
       + indent + 'crate::xml::globals::get_output_buffer_create_filename_value()\n}')
s = s[:m.start()] + new + s[m.end():]
print('converted xmlThrDefOutputBufferCreateFilenameDefault')

open(p, 'w').write(s)
print('done')
