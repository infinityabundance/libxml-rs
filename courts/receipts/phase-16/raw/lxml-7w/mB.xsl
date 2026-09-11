<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:iae="urn:iae">
  <xsl:template match="/"><out><v><xsl:call-template name="iae:replace-substring"><xsl:with-param name="original" select="'$datetime'"/><xsl:with-param name="substring" select="'$datetime'"/><xsl:with-param name="replacement" select="'datetime'"/></xsl:call-template></v></out></xsl:template>
  <xsl:template name="iae:replace-substring">
    <xsl:param name="original"/><xsl:param name="substring"/><xsl:param name="replacement" select="''"/>
    <xsl:choose>
      <xsl:when test="not($original)"/>
      <xsl:when test="not(string($substring))"><xsl:value-of select="$original"/></xsl:when>
      <xsl:when test="contains($original, $substring)">
        <xsl:value-of select="substring-before($original, $substring)"/>
        <xsl:value-of select="$replacement"/>
        <xsl:call-template name="iae:replace-substring">
          <xsl:with-param name="original" select="substring-after($original, $substring)"/>
          <xsl:with-param name="substring" select="$substring"/>
          <xsl:with-param name="replacement" select="$replacement"/>
        </xsl:call-template>
      </xsl:when>
      <xsl:otherwise><xsl:value-of select="$original"/></xsl:otherwise>
    </xsl:choose>
  </xsl:template>
</xsl:stylesheet>
