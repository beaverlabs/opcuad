#!/usr/bin/env bash

USAGE="./script/release.sh [--local] [p|push BUCKET]"

set -e

ENV=production
OPERATION=build
ARGS_COUNT=$#

if [[ $ARGS_COUNT -eq 1 ]]; then
  case $1 in
    --local)
      TARGET="local";;
    *)
      echo $USAGE
      exit 1
  esac
elif [[ $ARGS_COUNT -eq 2 ]]; then
  case $1 in
    p|push)
      OPERATION=publish
      DEST_BUCKET=$2;;
    *)
      echo $USAGE
      exit 1
  esac
elif [[ $ARGS_COUNT -eq 3 ]]; then
  case $1 in
    --local)
      TARGET="local";;
    *)
      echo $USAGE
      exit 1
  esac

  case $2 in
    p|push)
      OPERATION=publish
      DEST_BUCKET=$3;;
    *)
      echo $USAGE
      exit 1
  esac
fi

if [[ $OPERATION == "publish" && -z "$DEST_BUCKET" ]]; then
  echo $USAGE
  exit 1
fi

APP=opcuad
BUILD_TIMESTAMP=$(date -u +%Y%m%d%H%M%S)
COMMIT_SHA=$(git log -1 --pretty=format:"%h")
VERSION_METADATA=$BUILD_TIMESTAMP.$COMMIT_SHA
RELEASE_NAME=$APP+$VERSION_METADATA
export APP_REVISION=$VERSION_METADATA

BUILD_DIR_PATH=_build/$ENV
PACKAGE_PATH=target/release/$APP.tar.gz
DEST_PATH=s3://$DEST_BUCKET/releases/$APP/$ENV/$RELEASE_NAME.tar.gz

if [[ $TARGET == "local" ]]; then
  BIN_PATH=target/release/$APP

  cargo clippy
  cargo test
  cargo build --release
else
  BIN_PATH=target/armv7-unknown-linux-gnueabihf/release/$APP

  cross clippy --target=armv7-unknown-linux-gnueabihf
  cross test --target=armv7-unknown-linux-gnueabihf
  cross build --target=armv7-unknown-linux-gnueabihf --release
fi

# Prepare build directory

rm -fr $BUILD_DIR_PATH
mkdir -p $BUILD_DIR_PATH/bin
cp -r systemd/ $BUILD_DIR_PATH/
cp $BIN_PATH _build/$ENV/bin/

# Build package including binary and resources
tar -czf $PACKAGE_PATH -C $BUILD_DIR_PATH .

if [[ $OPERATION == "publish" ]]; then
  # Exit if there are uncommitted changes
  git diff --quiet --exit-code || (echo "Please commit your changes before publishing a release"; exit 1)
  s3cmd put $PACKAGE_PATH $DEST_PATH
fi
