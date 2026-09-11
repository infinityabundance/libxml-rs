<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:iso="http://purl.oclc.org/dsdl/schematron"
                xmlns:iae="urn:iae">
  <xsl:template match="/">
    <out>
      <xsl:variable name="r">
        <xsl:call-template name="iae:macro-expand">
          <xsl:with-param name="caller" select="'datetime'"/>
          <xsl:with-param name="text" select="'$datetime'"/>
        </xsl:call-template>
      </xsl:variable>
      <v><xsl:value-of select="$r"/></v>
      <w><xsl:value-of select="count(//iso:pattern[@id='datetime']/iso:param)"/></w>
    </out>
  </xsl:template>

  <xsl:template name="iae:macro-expand">
    <xsl:param name="caller"/>
    <xsl:param name="text"/>
    <xsl:call-template name="iae:multi-macro-expand">
      <xsl:with-param name="caller" select="$caller"/>
      <xsl:with-param name="text" select="$text"/>
      <xsl:with-param name="paramNumber" select="1"/>
    </xsl:call-template>
  </xsl:template>

  <xsl:template name="iae:multi-macro-expand">
    <xsl:param name="caller"/>
    <xsl:param name="text"/>
    <xsl:param name="paramNumber"/>
    <xsl:choose>
      <xsl:when test="//iso:pattern[@id=$caller]/iso:param[$paramNumber]">
        <xsl:call-template name="iae:multi-macro-expand">
          <xsl:with-param name="caller" select="$caller"/>
          <xsl:with-param name="paramNumber" select="$paramNumber + 1"/>
          <xsl:with-param name="text">
            <xsl:call-template name="iae:replace-substring">
              <xsl:with-param name="original" select="$text"/>
              <xsl:with-param name="substring" select="concat('$', //iso:pattern[@id=$caller]/iso:param[$paramNumber]/@name)"/>
              <xsl:with-param name="replacement" select="//iso:pattern[@id=$caller]/iso:param[$paramNumber]/@value"/>
            </xsl:call-template>
          </xsl:with-param>
        </xsl:call-template>
      </xsl:when>
      <xsl:otherwise><xsl:value-of select="$text"/></xsl:otherwise>
    </xsl:choose>
  </xsl:template>

  <xsl:template name="iae:replace-substring">
    <xsl:param name="original"/>
    <xsl:param name="substring"/>
    <xsl:param name="replacement" select="''"/>
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
