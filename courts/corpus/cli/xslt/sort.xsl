<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/">
    <out>
      <xsl:for-each select="//book">
        <xsl:sort select="@id" data-type="text" order="descending"/>
        <b><xsl:value-of select="@id"/></b>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>
