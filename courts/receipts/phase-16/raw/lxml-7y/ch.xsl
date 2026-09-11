<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template match="/">
    <xsl:variable name="v"><xsl:if test="false()">IF</xsl:if><xsl:choose><xsl:when test="false()">A</xsl:when><xsl:when test="true()">B</xsl:when><xsl:otherwise>C</xsl:otherwise></xsl:choose></xsl:variable>
    <out><xsl:value-of select="$v"/></out>
  </xsl:template>
</xsl:stylesheet>
