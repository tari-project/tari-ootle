set -e

SOURCE_PATH="./src"
THEME_DIR="theme"
FONT_DIR="fonts"
DIST_DIR="dist"
MAIN_INDEX_FILE="index.ts"

if [ -d "$SOURCE_PATH/$THEME_DIR" ]; then
  echo "Removing dir (src) $SOURCE_PATH/$THEME_DIR"
  npx shx rm -rf $SOURCE_PATH/$THEME_DIR || true
fi
if [ -d "$DIST_DIR" ]; then
  echo "Removing dir (dist): $SOURCE_PATH/$DIST_DIR"
  npx shx rm -rf ./$DIST_DIR || true
fi

mkdir -p $SOURCE_PATH/$THEME_DIR
mkdir -p $SOURCE_PATH/$FONT_DIR

cd ./src

for dir in $(find $THEME_DIR -mindepth 1 -maxdepth 1 -type d | sort); do
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


for file in $(find $FONT_DIR -mindepth 1 -maxdepth 1 | sort);  do
  cp -rv "$file" "../$DIST_DIR/$FONT_DIR"
done

for file in $(find $THEME_DIR -name "*.css" -maxdepth 1 | sort); do
  cp -rv "$file" "../$DIST_DIR/$THEME_DIR"
done
