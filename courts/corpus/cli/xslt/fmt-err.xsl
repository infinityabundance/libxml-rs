<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="text"/>
<xsl:decimal-format name="euro" decimal-separator="," grouping-separator="."/>
<xsl:template match="/"><xsl:value-of select="format-number(1234.5, '0.00', 'undeclared')"/>
</xsl:template>
</xsl:stylesheet>
