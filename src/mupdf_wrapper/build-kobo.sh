#! /bin/sh

source /usr/local/oecore-x86_64/environment-setup-cortexa9hf-neon-oe-linux-gnueabi
TARGET_OS=Kobo CFLAGS="$CFLAGS -I../../thirdparty/mupdf/include" ./build.sh
