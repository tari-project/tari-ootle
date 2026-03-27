#!/usr/bin/env bash
#
# Must be run from the repo root
#
BLOCK=$(cat <<'EOF'
//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

EOF
)

# Exclude files without extensions as well as those with extensions that are not in the list
rgTemp=$(mktemp)
rg -i "Copyright.*The Tari Project" --files-without-match \
   -g '!*.{Dockerfile,asc,bat,config,config.js,css,csv,drawio,env,gitkeep,hbs,html,ini,iss,json,lock,md,mdx,min.js,ps1,py,rc,scss,sh,sql,svg,toml,txt,yml,yaml,vue,liquid,otf,d.ts,config.ts,mjs,astro,otf,ttf}' . \
   -g '!bindings/src/types/*' \
   -g '!bindings/dist/types/*' \
   -g '!applications/theming/public/**/*' \
   -g '!applications/theming/dist/fonts/**/*' \
    | while IFS= read -r file; do
        if [[ -n $(basename "${file}" | grep -E '\.') ]]; then
          echo "${file}"
        fi
      done | sort > ${rgTemp}

# Sort the .license.ignore file as sorting seems to behave differently on different platforms
licenseIgnoreTemp=$(mktemp)
cat .license.ignore | sort > ${licenseIgnoreTemp}

DIFFS=$(diff -u --strip-trailing-cr ${licenseIgnoreTemp} ${rgTemp})


# clean up
rm -vf "${rgTemp}"
rm -vf "${licenseIgnoreTemp}"

FILES="${DIFFS##*@@}"



if [ -n "$FILES" ]; then
  echo "New files detected with no license"
  echo "=========="
  for FILE in $FILES; do
    if [ "${FILE:0:1}" == "+" ]; then
      NEW_FILE="${FILE##*+}"
      if [ -f "$NEW_FILE" ]; then
        echo "Adding block to $NEW_FILE"
        { printf "%s\n" "$BLOCK"; cat "$NEW_FILE"; } > "$NEW_FILE.tmp" && mv "$NEW_FILE.tmp" "$NEW_FILE"
        fi
    fi
  done
  exit 1
else
  echo "All new files have license!"
  exit 0
fi


