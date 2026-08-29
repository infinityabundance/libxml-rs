<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/">
    <out>
      <xsl:for-each select="//book">
        <n><xsl:number value="position()" format="I"/></n>
        <n2><xsl:number value="position()" format="1"/></n2>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>
