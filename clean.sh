#! /bin/sh

cargo clean

rm -rf dist/ libs/

rm -f "plato-*.zip"

if [ ! -z "mupdf-*.tar.xz" ]; then
  # download_headers.sh was likely executed to get headers
  rm -rf thirdparty/mupdf/include
  rm "mupdf-*.tar.xz"
fi