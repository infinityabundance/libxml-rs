<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/">
    <out>
      <xsl:choose>
        <xsl:when test="library/book[3]"><third>yes</third></xsl:when>
        <xsl:otherwise><third>no</third></xsl:otherwise>
      </xsl:choose>
      <xsl:choose>
        <xsl:when test="library/zzz"><zzz>yes</zzz></xsl:when>
        <xsl:otherwise><zzz>no</zzz></xsl:otherwise>
      </xsl:choose>
      <xsl:if test="count(//book) &gt; 2"><many>yes</many></xsl:if>
      <xsl:if test="'x'"><str>yes</str></xsl:if>
      <xsl:if test="0"><zero>no</zero></xsl:if>
    </out>
  </xsl:template>
</xsl:stylesheet>
