<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml"/>
  <xsl:template match="/">
    <out>
      <xsl:value-of select="string(/library/missing)"/>
      <xsl:copy-of select="string(/library/@missing)"/>
    </out>
  </xsl:template>
</xsl:stylesheet>
