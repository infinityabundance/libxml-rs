<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:ns="urn:ns">
  <xsl:template match="/*"><out><xsl:apply-templates select="*[not(self::ns:ns)]"/></out></xsl:template>
  <xsl:template match="*"><x name="{local-name()}"/></xsl:template>
</xsl:stylesheet>
