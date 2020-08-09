#! /bin/sh

# Get mupdf include/header files
MUPDF_VERSION="1.17.0"
MUPDF_ARCHIVE="mupdf-$MUPDF_VERSION-source.tar.xz"
echo "Downloading $MUPDF_ARCHIVE"

wget -q --show-progress "https://mupdf.com/downloads/archive/$MUPDF_ARCHIVE"

mkdir -p thirdparty/mupdf
tar -C thirdparty/mupdf --wildcards --strip-components=1 -xvJf "$MUPDF_ARCHIVE" 'mupdf-*-source/include'