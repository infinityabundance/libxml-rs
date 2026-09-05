<?php
// ZTS xinclude isolation: (a) DOM LIBXML_XINCLUDE load, (b) xslt document()
// with doXInclude=true. Prints intermediate states.
error_reporting(E_ALL);
$dir = '/srcz/php-src/ext/xsl/tests/xinclude';
chdir($dir);

echo "cwd=", getcwd(), "\n";
echo "data.xml exists: ", var_export(file_exists('data.xml'), true), "\n";

// (a) raw xinclude via DOM
$d = new DOMDocument;
$ok = @$d->load('data.xml', LIBXML_XINCLUDE);
echo "dom load rc=", var_export($ok, true), " errs:\n";
foreach (libxml_get_errors() as $e) { echo "  ", trim($e->message), "\n"; }
libxml_clear_errors();
echo "dom xinclude text: ", trim($d->textContent ?? ''), "\n";

// (b) xinclude via simplexml
$sxe = @simplexml_load_file('data.xml', SimpleXMLElement::class, LIBXML_XINCLUDE);
echo "simplexml: ", var_export($sxe === false, true), " content: ", trim((string)$sxe->content), "\n";

// (c) xslt document() with doXInclude
$xml = new DOMDocument;
$xml->loadXML('<?xml version="1.0"?><root/>');
$xsl = new DOMDocument;
$xsl->loadXML(<<<XML
<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root">
        <container>
            <xsl:value-of select="document('data.xml')/data/content"/>
        </container>
    </xsl:template>
</xsl:stylesheet>
XML);
$xslt = new XSLTProcessor;
$xslt->doXInclude = true;
$xslt->importStylesheet($xsl);
$out = $xslt->transformToXml($xml);
echo "doXInclude=true out: ", trim($out), "\n";

// (d) document() WITHOUT xinclude but content directly in data2.xml
file_put_contents('data2.xml', '<?xml version="1.0"?><data><content>Direct content</content></data>');
$xslt2 = new XSLTProcessor;
$xslt2->doXInclude = false;
$xslt2->importStylesheet($xsl);
$xsl2 = new DOMDocument;
$xsl2->loadXML(<<<XML
<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root">
        <container>
            <xsl:value-of select="document('data2.xml')/data/content"/>
        </container>
    </xsl:template>
</xsl:stylesheet>
XML);
$xslt2->importStylesheet($xsl2);
$out2 = $xslt2->transformToXml($xml);
echo "document() no-xinclude out: ", trim($out2), "\n";
unlink('data2.xml');
