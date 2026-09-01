#!/bin/bash
# nokogiri-full-gdb.sh — run the full nokogiri suite under gdb, catch the first abort.
set -uo pipefail
MODE="${1:?usage: nokogiri-full-gdb.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

# Reproduce the exact rake test file list from nokogiri-run.sh.
FILES=""
for f in test/test_soap4r_sax.rb test/html4/test_document.rb test/html4/test_node.rb \
  test/xml/test_c14n.rb test/xml/test_document_fragment.rb test/xml/test_entity_decl.rb \
  test/namespaces/test_namespaces_in_cloned_doc.rb test/xml/sax/test_document_error.rb \
  test/xml/test_attr.rb test/namespaces/test_namespaces_in_builder_doc.rb \
  test/xml/test_document_encoding.rb test/html4/sax/test_parser.rb test/test_nokogiri.rb \
  test/test_nokogumbo_contract.rb test/css/test_xpath_visitor.rb test/html4/sax/test_push_parser.rb \
  test/test_compaction.rb test/html4/test_element_description.rb test/xml/test_reader.rb \
  test/html5/test_errors.rb test/css/test_css_integration.rb test/namespaces/test_namespaces_in_parsed_doc.rb \
  test/xml/test_xinclude.rb test/namespaces/test_additional_namespaces_in_builder_doc.rb test/xml/test_node_set.rb \
  test/xml/test_element_content.rb test/xml/test_node_reparenting.rb test/test_memory_usage.rb \
  test/xml/test_unparented_node.rb test/xml/test_node_encoding.rb test/test_pattern_matching.rb \
  test/css/test_tokenizer.rb test/xml/test_syntax_error.rb test/html5/test_monkey_patch.rb \
  test/html4/test_builder.rb test/xml/test_processing_instruction.rb test/xml/test_relax_ng.rb \
  test/xml/test_comment.rb test/xml/sax/test_parser_text.rb test/xml/test_dtd_encoding.rb \
  test/css/test_selector_cache.rb test/xml/test_dtd.rb test/xml/test_reader_encoding.rb \
  test/html4/test_document_fragment.rb test/xml/sax/test_parser_context.rb test/html5/test_quirks_mode.rb \
  test/xml/node/test_save_options.rb test/html5/test_attributes.rb test/html5/test_api.rb \
  test/css/test_parser.rb test/html4/test_node_encoding.rb test/html4/test_document_encoding.rb \
  test/xml/test_xpath.rb test/xml/test_node.rb test/html4/test_attributes_properly_escaped.rb \
  test/test_encoding_handler.rb test/xml/test_node_attributes.rb test/test_iso.rb \
  test/html5/test_serialize.rb test/xml/test_parse_options.rb test/xml/test_builder.rb \
  test/html5/test_builder.rb test/test_version.rb test/html4/test_comments.rb test/xml/node/test_subclass.rb \
  test/xml/test_xpath_context.rb test/test_gem_platform.rb test/namespaces/test_namespaces_in_created_doc.rb \
  test/html5/test_nokogumbo.rb test/xslt/test_custom_functions.rb test/css/test_css.rb \
  test/html4/test_named_characters.rb test/xml/test_document.rb test/html4/sax/test_document_error.rb \
  test/xml/sax/test_parser.rb test/xml/test_namespace.rb test/namespaces/test_namespaces_preservation.rb \
  test/namespaces/test_namespaces_aliased_default.rb test/test_serialization_encoding.rb \
  test/xml/sax/test_push_parser.rb test/xml/test_schema.rb test/test_xslt_transforms.rb \
  test/xml/test_attribute_decl.rb test/xml/test_text.rb test/xml/node/test_attribute_methods.rb \
  test/xml/test_cdata.rb test/test_class_resolver.rb test/html4/test_attributes.rb \
  test/html5/test_tree_construction.rb test/xml/test_node_inheritance.rb test/html4/test_attributes.rb \
  test/test_nokogiri.rb test/html4/sax/test_parser.rb test/styles.css test/xml/test_node_encoding.rb; do
  if [ -f "$f" ]; then FILES="$FILES require \"$f\";"; fi
done

timeout 400 gdb -batch -ex 'run' -ex 'bt 35' \
  --args ruby3.1 -rset -Ilib:test:.:test -e 'require "simplecov_prelude"; require "minitest/autorun"; require "test/test_iso.rb"; require "test/test_pattern_matching.rb"; require "test/xml/test_document.rb"; require "test/xml/test_dtd.rb"; require "test/xml/test_xpath.rb"; require "test/xml/test_node.rb"; require "test/test_nokogiri.rb"' \
  > "/out/${MODE}-fullgdb.log" 2>&1
echo "full-gdb ${MODE} done"
