<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml"/>
  <xsl:template match="/">
    <count><xsl:value-of select="count(/library)"/></count>
    <stringval><xsl:value-of select="string(/library/book[1]/title)"/></stringval>
  </xsl:template>
</xsl:stylesheet>
