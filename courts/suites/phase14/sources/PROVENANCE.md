# Phase 14 — Downstream Custodian Validation: pinned consumer provenance
#
# Every consumer source is pinned by upstream identity + content hash. The
# copies under this directory are what the phase-14 court images bake in, so
# oracle and candidate builds of a consumer are byte-identical by
# construction. The consumer stays fixed; libxml-rs adapts.
#
# lxml
#   upstream repo:     https://github.com/lxml/lxml
#   tag:               lxml-6.1.2
#   commit:            f2874e9 (grafted clone at the tag)
#   sdist (release):   lxml-6.1.2.tar.gz  sha256 1055241852f2b02068af4a625a5d32c087db193c12251928af2562ecd2239f18
#   court tree:        lxml/   (git tree at the tag, .git/doc removed; the
#                      src/lxml/tests/ suite is part of the tag)
#   build:             python3 setup.py build_ext --inplace (Cython generated
#                      C from the tag's .pyx, identical on both sides)
#   test command:      python3 -m pytest -q src/lxml/tests
#
# nokogiri
#   upstream repo:     https://github.com/sparklemotion/nokogiri
#   tag:               v1.19.4
#   commit:            8cfb9da (grafted clone at the tag)
#   gem (release):     nokogiri-1.19.4.gem sha256 50c951611c92bca05c51411aef45f1cbc50f2821c4802758c5c6d34696533ab5
#   court tree:        nokogiri/  (git tree at the tag, .git removed)
#   build:             gem build nokogiri.gemspec; gem install --local
#                      -- --use-system-libraries (pkg-config for libxml-2.0/
#                      libxslt; no bundled mini_portile)
#   test command:      rake test  (test/ suite at the tag)
#
# php
#   upstream:          https://www.php.net/distributions/php-8.5.10.tar.gz
#   sha256:            f5c0ac99b85b3d677de475c2e4f509f9b4f54663f3ee5a84d6d9481a521d4100
#   court source:      php-8.5.10.tar.gz  (pristine upstream tarball)
#   build:             ./configure --disable-all --enable-cli --enable-dom
#                      --enable-simplexml --enable-xml --enable-xmlreader
#                      --enable-xmlwriter --with-xsl --with-libxml
#                             (PHP 8.5 uses --with-xsl; single source of truth in
#                              consumers/php-court-spec.sh — the stale --enable-xsl
#                              spelling has been removed everywhere)
#   test command:      make test TESTS="ext/dom ext/simplexml ext/xml
#                      ext/xmlreader ext/xmlwriter ext/xsl"
#
# The libxml2/libxslt oracle on every distro is the canonical pinned-source
# build (libxml2 2.15.3 + libxslt 1.1.45 + libexslt 0.8.25, ICU+Iconv,
# installed into /usr/local) — the same contract the candidate implements.
# Distro-packaged libxml2 versions differ per distro and would conflate
# version drift with implementation divergence, so they are NOT the oracle;
# the Debian reverse-dependency court additionally records the distro
# packaged version as provenance.
