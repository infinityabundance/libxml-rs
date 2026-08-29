# Era-autotools modernization applied to libxml2/libxslt configure.in/configure.ac
# before running modern autoconf (2.73)/automake (1.18.1). These adaptations are
# part of each historical oracle's identity (11.1-A oracle manifest:
# `adaptation_script` + `adaptation_hash` = sha256 of this file).
#
# The macros removed were deleted from modern autoconf/automake and would abort
# the era build; the declarations they guarded are either no-ops or replaced by
# modern equivalents. No behavioral configure option is added or removed by
# these edits — they only let the era build system run on a modern toolchain.
#
#   AM_CONFIG_HEADER        -> AC_CONFIG_HEADERS        (renamed macro)
#   AM_C_PROTOTYPES         -> removed (K&R prototype check, no-op today)
#   AC_PROG_CC_STDC         -> removed (merged into AC_PROG_CC)
#   AM_EXEEXT               -> removed (merged into AC_PROG_CC)
s/AM_CONFIG_HEADER/AC_CONFIG_HEADERS/
/AM_C_PROTOTYPES/d
/AC_PROG_CC_STDC/d
/AM_EXEEXT/d
