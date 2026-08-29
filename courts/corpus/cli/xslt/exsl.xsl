<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
  xmlns:exsl="http://exslt.org/common"
  xmlns:math="http://exslt.org/math"
  xmlns:set="http://exslt.org/sets"
  xmlns:str="http://exslt.org/strings"
  extension-element-prefixes="exsl">
  <xsl:output method="xml" indent="yes"/>
  <xsl:variable name="rtf">
    <item>30</item><item>10</item><item>20</item>
  </xsl:variable>
  <xsl:template match="/">
    <out>
      <max><xsl:value-of select="math:max(exsl:node-set($rtf)/item)"/></max>
      <min><xsl:value-of select="math:min(exsl:node-set($rtf)/item)"/></min>
      <distinct><xsl:value-of select="set:distinct(//author)"/></distinct>
      <upper><xsl:value-of select="str:upper-case(//title)"/></upper>
      <token><xsl:value-of select="str:tokenize('a,b,c', ',')"/></token>
    </out>
  </xsl:template>
</xsl:stylesheet>
