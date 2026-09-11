<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:iae="urn:iae">
  <xsl:template match="/"><out><xsl:variable name="r"><xsl:call-template name="iae:passthru"><xsl:with-param name="x" select="'HI'"/></xsl:call-template></xsl:variable><v><xsl:value-of select="$r"/></v></out></xsl:template>
  <xsl:template name="iae:passthru"><xsl:param name="x"/><xsl:value-of select="$x"/></xsl:template>
</xsl:stylesheet>
