<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:iae="urn:iae">
  <xsl:template match="/"><out>
    <a><xsl:variable name="r1">TEXT</xsl:variable><xsl:value-of select="$r1"/></a>
    <b><xsl:variable name="r2"><xsl:value-of select="'VAL'"/></xsl:variable><xsl:value-of select="$r2"/></b>
    <c><xsl:variable name="r3"><xsl:call-template name="iae:passthru"><xsl:with-param name="x" select="'CT'"/></xsl:call-template></xsl:variable><xsl:value-of select="$r3"/></c>
  </out></xsl:template>
  <xsl:template name="iae:passthru"><xsl:param name="x"/><xsl:value-of select="$x"/></xsl:template>
</xsl:stylesheet>
