<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/">
    <out>
      <xsl:for-each select="//book[position() &lt;= 2]">
        <b pos="{position()}"><xsl:value-of select="title"/></b>
      </xsl:for-each>
      <xsl:for-each select="//book[author = 'Smith']">
        <smith><xsl:value-of select="title"/></smith>
      </xsl:for-each>
      <xsl:for-each select="//book[@id = 'b3']">
        <b3><xsl:value-of select="title"/></b3>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>
