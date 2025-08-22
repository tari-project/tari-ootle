#   Copyright 2023 The Tari Project
#   SPDX-License-Identifier: BSD-3-Clause

set -e

SOURCE_PATH="./src"
TYPES_DIR="types"
DIST_DIR="dist"
HELPERS_DIR="helpers"
MAIN_INDEX_FILE="index.ts"

if [ -d "$SOURCE_PATH/$TYPES_DIR" ]; then
  echo "Removing $SOURCE_PATH/$TYPES_DIR"
  npx shx rm -rf $SOURCE_PATH/$TYPES_DIR || true
fi
for file in $(find "$SOURCE_PATH" -name "*.ts" -maxdepth 1); do
  echo "Removing $file"
  npx shx rm $file || true
done
if [ -f "$SOURCE_PATH/$DIST_DIR" ]; then
  echo "Removing $SOURCE_PATH/$DIST_DIR"
  npx shx rm -rf ./$DIST_DIR || true
fi

mkdir -p $SOURCE_PATH/$TYPES_DIR

cargo test --workspace --exclude integration_tests export_bindings --features ts

# Add the license header
echo "//   Copyright $(date +%Y) The Tari Project" >> $SOURCE_PATH/$MAIN_INDEX_FILE
echo "//   SPDX-License-Identifier: BSD-3-Clause" >> $SOURCE_PATH/$MAIN_INDEX_FILE
echo "" >> $SOURCE_PATH/$MAIN_INDEX_FILE

cd ./src
# Generate the index file
for file in $(find $TYPES_DIR -name "*.ts" -maxdepth 1 | sort); do
  MODULE_NAME="${file%.*}"
  echo "export * from './$MODULE_NAME';" >> $MAIN_INDEX_FILE
done

for dir in $(find $TYPES_DIR -mindepth 1 -maxdepth 1 -type d | sort); do
  module_dir_name="$(basename $dir)"
  module_export_file="$module_dir_name.ts"
  echo "Generating ${module_export_file}...";
  if [ -f "$module_export_file" ]; then
    npx shx rm "$module_export_file"
  fi
  echo "//   Copyright $(date +%Y) The Tari Project" >> "$module_export_file"
  echo "//   SPDX-License-Identifier: BSD-3-Clause" >> "$module_export_file"
  echo "" >> "$module_export_file"
  for file in $(find $dir -name "*.ts" -maxdepth 1); do
    MODULE_NAME="${file%.*}"
    echo "export * from './$MODULE_NAME';" >> "$module_export_file"
  done
  echo "export * from './$module_dir_name';" >> $MAIN_INDEX_FILE
done

# Add helpers
for file in $(find $HELPERS_DIR -name "*.ts" | sort); do
  FILE_NAME=$(basename $file)
  if [ "$FILE_NAME" != "index.ts" ]; then
    MODULE_NAME="${FILE_NAME%.*}"
    echo "export * from './$HELPERS_DIR/$MODULE_NAME';" >> $MAIN_INDEX_FILE
  fi
done
