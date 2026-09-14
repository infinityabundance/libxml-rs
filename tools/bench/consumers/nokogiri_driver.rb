#!/usr/bin/env ruby
# frozen_string_literal: true
#
# nokogiri_driver.rb — §16.12/§16.14 consumer driver for the Ruby Nokogiri
# consumer family. Runs inside the perf container after the provider has been
# selected by `source /court/consumers/lib.sh <oracle|candidate>`.
#
# It emits exactly one single-line JSON object on stdout:
#
#   {"consumer":"ruby-nokogiri","id":…,"category":…,"file":…,
#    "ops":{"<op>":{"ok":true,"ms":<float>,"fingerprint":"sha256:<hex>","detail":""}}}
#
# Timing is in-process, monotonic, best of --reps after --warmup warmups.
# Fingerprints are canonical and provider-independent (no timestamps, no
# pointers, no versions, no absolute paths). Any op that fails is still
# emitted as {"ok":false,"error":"…"}; the driver never crashes.

require "set" # Nokogiri's class_resolver uses Set; Ruby 3.1 needs this first.
require "json"
require "digest"

# The extension is built once against the shared system-ABI sonames
# (libxml2.so.16 / libxslt.so.1 / libexslt.so.0) and is provider-selected at
# run time purely by LD_LIBRARY_PATH (done by lib.sh). Only the pure-Ruby load
# path needs to be supplied here.
ENV.fetch("NOKOGIRI_LIB", "/src/nokogiri/lib").tap do |libdir|
  $LOAD_PATH.unshift(libdir) unless $LOAD_PATH.include?(libdir)
end

# Loading Nokogiri is attempted up-front (provider selection happens before
# the driver is invoked); failure is captured and reported as an op error
# rather than aborting the process.
NOKOGIRI_LOAD_ERROR = begin
  require "nokogiri"
  nil
rescue StandardError, ScriptError => e
  "#{e.class}: #{e.message}"
end

ALL_OPS = %w[dom_parse sax_parse reader xpath serialize xsd_validate xslt].freeze
FAMILIES_PATH = ENV.fetch("BENCH_FAMILIES", "/bench/families.json")
XSLT_DIR = ENV.fetch("BENCH_XSLT_DIR", "/bench/xslt")

# ---------------------------------------------------------------------------
# argument parsing (no optparse: keep stdout completely pristine)
# ---------------------------------------------------------------------------

opts = { id: nil, category: nil, file: nil, reps: 3, warmup: 1, ops: nil }
argv = ARGV.dup
until argv.empty?
  flag = argv.shift
  case flag
  when "--id" then opts[:id] = argv.shift
  when "--category" then opts[:category] = argv.shift
  when "--file" then opts[:file] = argv.shift
  when "--reps" then opts[:reps] = Integer(argv.shift)
  when "--warmup" then opts[:warmup] = Integer(argv.shift)
  when "--ops" then opts[:ops] = argv.shift.to_s.split(",").map(&:strip).reject(&:empty?)
  when "--help", "-h"
    warn "usage: nokogiri_driver.rb --id ID --category CAT --file PATH " \
         "--reps N [--warmup W] [--ops a,b,c]"
    exit 0
  else
    warn "nokogiri_driver: ignoring unknown argument #{flag.inspect}"
  end
end

opts[:reps] = 1 if opts[:reps] < 1
opts[:warmup] = 0 if opts[:warmup].negative?
requested_ops = opts[:ops] || ALL_OPS

# ---------------------------------------------------------------------------
# canonical fingerprints
# ---------------------------------------------------------------------------

def fp(text)
  "sha256:#{Digest::SHA256.hexdigest(text)}"
end

def mono_ms
  Process.clock_gettime(Process::CLOCK_MONOTONIC) * 1000.0
end

def measure(reps, warmup)
  warmup.times { yield }
  best = nil
  reps.times do
    t0 = mono_ms
    yield
    dt = mono_ms - t0
    best = dt if best.nil? || dt < best
  end
  best
end

def num_str(value)
  return value.to_s unless value.is_a?(Float)

  value.finite? && value == value.to_i ? value.to_i.to_s : value.to_s
end

# DOM: canonical walk in document order: kind, element/attr local names,
# namespace URI, attribute values, text. Attributes are sorted by
# (namespace URI, local name) so storage order cannot leak into the hash.
# DTD nodes are represented by kind only (their textual content is not a
# semantic result and is not comparable byte-for-byte).
def canonical_dom(doc)
  out = []
  visit = lambda do |node|
    case node.type
    when 1 # element
      attrs = node.attribute_nodes.map do |a|
        [a.namespace&.href, a.name, a.value]
      end.sort_by { |ns, name, _v| [ns.to_s, name] }
      out << JSON.generate(["elem", node.namespace&.href, node.name, attrs])
      node.children.each(&visit)
    when 3 then out << JSON.generate(["text", node.content])
    when 4 then out << JSON.generate(["cdata", node.content])
    when 7 then out << JSON.generate(["pi", node.name, node.content])
    when 8 then out << JSON.generate(["comment", node.content])
    when 5 then out << JSON.generate(["entityref", node.name])
    when 10, 14 then out << JSON.generate(["doctype", node.name])
    else out << JSON.generate(["node#{node.type}", node.name.to_s])
    end
  end
  doc.children.each(&visit)
  out.join("\n")
end

# Path built only from local names + namespace URIs + same-name sibling
# position, so document prefixes cannot leak in.
def canonical_node_path(node)
  case node
  when Nokogiri::XML::Element
    segs = []
    cur = node
    while cur.is_a?(Nokogiri::XML::Element)
      idx = 1
      parent = cur.parent
      if parent
        parent.children.each do |sib|
          break if sib == cur

          if sib.is_a?(Nokogiri::XML::Element) && sib.name == cur.name &&
             sib.namespace&.href == cur.namespace&.href
            idx += 1
          end
        end
      end
      segs.unshift("#{cur.namespace&.href ? "{#{cur.namespace.href}}" : ""}#{cur.name}[#{idx}]")
      cur = parent.is_a?(Nokogiri::XML::Element) ? parent : nil
    end
    "/#{segs.join('/')}"
  when Nokogiri::XML::Attr
    prefix = node.namespace&.href ? "{#{node.namespace.href}}" : ""
    "#{canonical_node_path(node.parent)}/@#{prefix}#{node.name}"
  else
    base = node.respond_to?(:parent) && node.parent ? canonical_node_path(node.parent) : ""
    case node.type
    when 3 then "#{base}/text()"
    when 4 then "#{base}/cdata()"
    when 7 then "#{base}/pi(#{node.name})"
    when 8 then "#{base}/comment()"
    else "#{base}/##{node.type}"
    end
  end
end

def xpath_signature(result)
  case result
  when Nokogiri::XML::NodeSet
    items = result.map { |n| canonical_node_path(n) }
    "nodeset|#{items.length}|#{items.join("\n")}"
  when Float then "number|1|#{num_str(result)}"
  when Integer then "number|1|#{result}"
  when String then "string|1|#{result}"
  when TrueClass, FalseClass then "boolean|1|#{result}"
  when NilClass then "nil|0|"
  else "#{result.class}|1|#{result}"
  end
end

# ---------------------------------------------------------------------------
# streaming (SAX + Reader): ordered start/end element names + text lengths.
# Consecutive character callbacks are coalesced into one text run so that
# buffer-boundary chunking cannot leak into the fingerprint.
# ---------------------------------------------------------------------------

if NOKOGIRI_LOAD_ERROR.nil?
  class NokogiriPerfSaxHandler < Nokogiri::XML::SAX::Document
    attr_reader :events

    def initialize
      super
      @events = []
      @pending = 0
    end

    def start_element_namespace(name, _attrs = [], _prefix = nil, _uri = nil, _ns = [])
      flush_text
      @events << "s:#{name}"
    end

    def end_element_namespace(name, _prefix = nil, _uri = nil)
      flush_text
      @events << "e:#{name}"
    end

    def characters(string)
      @pending += string.length if string
    end

    def cdata_block(string)
      @pending += string.length if string
    end

    def flush_text
      return if @pending.zero?

      @events << "t:#{@pending}"
      @pending = 0
    end
  end
end

def reader_events(content)
  reader = Nokogiri::XML::Reader(content)
  events = []
  pending = 0
  flush = lambda do
    if pending.positive?
      events << "t:#{pending}"
      pending = 0
    end
  end
  reader.each do |node|
    case node.node_type
    when Nokogiri::XML::Reader::TYPE_ELEMENT
      flush.call
      events << "s:#{node.name}"
    when Nokogiri::XML::Reader::TYPE_END_ELEMENT
      flush.call
      events << "e:#{node.name}"
    when Nokogiri::XML::Reader::TYPE_TEXT,
         Nokogiri::XML::Reader::TYPE_CDATA,
         Nokogiri::XML::Reader::TYPE_WHITESPACE,
         Nokogiri::XML::Reader::TYPE_SIGNIFICANT_WHITESPACE
      pending += node.value.to_s.length
    end
  end
  flush.call
  events
end

# ---------------------------------------------------------------------------
# ops
# ---------------------------------------------------------------------------

def op_dom_parse(content, _category, reps, warmup)
  block = -> { Nokogiri::XML::Document.parse(content) }
  fingerprint = fp(canonical_dom(block.call))
  { "ok" => true, "ms" => measure(reps, warmup, &block),
    "fingerprint" => fingerprint, "detail" => "" }
end

def op_sax_parse(content, _category, reps, warmup)
  block = lambda do
    handler = NokogiriPerfSaxHandler.new
    Nokogiri::XML::SAX::Parser.new(handler).parse(content)
    handler
  end
  handler = block.call
  handler.flush_text
  fingerprint = fp(handler.events.join("\n"))
  { "ok" => true, "ms" => measure(reps, warmup, &block),
    "fingerprint" => fingerprint, "detail" => "" }
end

def op_reader(content, _category, reps, warmup)
  block = -> { reader_events(content) }
  fingerprint = fp(block.call.join("\n"))
  { "ok" => true, "ms" => measure(reps, warmup, &block),
    "fingerprint" => fingerprint, "detail" => "" }
end

def op_serialize(content, _category, reps, warmup)
  doc = Nokogiri::XML::Document.parse(content)
  block = -> { doc.to_xml }
  fingerprint = fp(block.call)
  { "ok" => true, "ms" => measure(reps, warmup, &block),
    "fingerprint" => fingerprint, "detail" => "" }
end

def op_xpath(content, category, reps, warmup)
  families = JSON.parse(File.read(FAMILIES_PATH))["families"] || {}
  family = families[category]
  raise "no family #{category.inspect} in #{FAMILIES_PATH}" unless family

  exprs = family["xpath"]
  raise "family #{category.inspect} has no xpath expressions" unless exprs.is_a?(Array) && !exprs.empty?

  doc = Nokogiri::XML::Document.parse(content)
  block = -> { exprs.map { |e| doc.xpath(e) } }
  signature = block.call.map { |r| xpath_signature(r) }.join("\n")
  { "ok" => true, "ms" => measure(reps, warmup, &block),
    "fingerprint" => fp(signature),
    "detail" => "xpath: #{exprs.length} expression(s)" }
end

def op_xslt(content, category, reps, warmup)
  xsl_path = File.join(XSLT_DIR, "#{category}.xsl")
  raise "no stylesheet at #{xsl_path}" unless File.file?(xsl_path)

  doc = Nokogiri::XML::Document.parse(content)
  stylesheet = Nokogiri::XSLT(File.read(xsl_path))
  block = -> { stylesheet.transform(doc) }
  output = block.call
  raise "transform returned nil" if output.nil?

  { "ok" => true, "ms" => measure(reps, warmup, &block),
    "fingerprint" => fp(output.to_s), "detail" => "" }
end

def locate_xsd(category)
  return nil unless category == "MAVEN"

  [
    ENV["MAVEN_XSD_PATH"],
    File.join(XSLT_DIR, "..", "xsd", "maven-4.0.0.xsd"),
    "/bench/xsd/maven-4.0.0.xsd",
    "/corpus/maven-4.0.0.xsd",
  ].compact.find { |p| File.file?(p) }
end

def op_xsd_validate(content, category, reps, warmup)
  xsd_path = locate_xsd(category)
  if xsd_path.nil?
    return { "ok" => false, "error" => "not_expressible: no offline XSD resolver" }
  end

  schema = Nokogiri::XML::Schema(File.read(xsd_path))
  doc = Nokogiri::XML::Document.parse(content)
  block = -> { schema.validate(doc) }
  errors = block.call
  verdict = errors.empty? ? "valid" : "invalid"
  kinds = errors.map { |e| e.respond_to?(:code) ? e.code.to_s : "unknown" }.sort.join(",")
  { "ok" => true, "ms" => measure(reps, warmup, &block),
    "fingerprint" => fp("#{verdict}|#{kinds}"),
    "detail" => "#{errors.length} schema error(s)" }
end

def run_op(name, content, category, reps, warmup)
  case name
  when "dom_parse" then op_dom_parse(content, category, reps, warmup)
  when "sax_parse" then op_sax_parse(content, category, reps, warmup)
  when "reader" then op_reader(content, category, reps, warmup)
  when "xpath" then op_xpath(content, category, reps, warmup)
  when "serialize" then op_serialize(content, category, reps, warmup)
  when "xsd_validate" then op_xsd_validate(content, category, reps, warmup)
  when "xslt" then op_xslt(content, category, reps, warmup)
  else { "ok" => false, "error" => "unknown op #{name.inspect}" }
  end
rescue StandardError, ScriptError => e
  { "ok" => false, "error" => "#{e.class}: #{e.message}" }
end

# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

# Keep stdout clean: any incidental library output during the run is diverted
# to stderr, and only the final JSON is written to the real stdout.
real_stdout = $stdout
$stdout = $stderr

payload = {
  "consumer" => "ruby-nokogiri",
  "id" => opts[:id],
  "category" => opts[:category],
  "file" => opts[:file],
  "ops" => {},
}

load_error = NOKOGIRI_LOAD_ERROR
content = nil
read_error = nil
if opts[:file].nil?
  read_error = "missing --file"
elsif load_error.nil?
  begin
    content = File.read(opts[:file])
  rescue StandardError => e
    read_error = "#{e.class}: #{e.message}"
  end
end

requested_ops.each do |op|
  payload["ops"][op] =
    if load_error
      { "ok" => false, "error" => "nokogiri load failed: #{load_error}" }
    elsif read_error
      { "ok" => false, "error" => "input read failed: #{read_error}" }
    else
      run_op(op, content, opts[:category], opts[:reps], opts[:warmup])
    end
end

$stdout = real_stdout
real_stdout.puts(JSON.generate(payload))
