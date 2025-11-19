#!/usr/bin/env bash
#
# prep json build matrix from args
#
# ./build-matrix.sh all "0.16.0" "linux/amd64,linux/arm64"
#

# set -euxo pipefail

build_items=${1:-tari-ootle_all}
echo "Building with ${build_items}."
if [ -z "${build_items}" ] || [ "${build_items}" = "tari-ootle_all" ] ; then
  echo "Build all tari-ootle images"
  matrix_selection=$( jq -s -c '.[]' tari-ootle.json )
fi

# Choose version prefix for minotari builds
TARI_VERSION="${2:-dev}"  # e.g., pass in tag as first arg

# Start jSon string
matrix_details="["

#echo "${matrix_selection}" | jq -c '.'
while read -r item; do

  image_name=$(jq -r '.image_name' <<< "${item}")
  #echo "Image: ${image_name}"
  #echo "JSon Object: $(jq -r '.' <<< "${item}")"

  # Determine version
  if [[ "${image_name}" == ootle* ]]; then
    version="${TARI_VERSION}"
    dockerfile="ootle.Dockerfile"
    build_arg=""
  else
    echo "No Dockerfile for ${image_name}, skipping..."
    continue
  fi

  #echo "${version}, ${dockerfile}, ${build_arg}"

  # Extend the original JSON object with new fields
  enriched=$(jq -c \
    --arg version "$version" \
    --arg dockerfile "$dockerfile" \
    --arg build_args "$build_arg" \
    '. + {
      version: $version,
      dockerfile: $dockerfile,
      build_args: $build_args
    }' <<< "${item}")

  matrix_details+="$enriched,"
done < <(jq -c '.[]' <<< "${matrix_selection}")

if [[ "${matrix_details}" == "[" ]]; then
  matrix_details="[]"  # no entries were added
  echo "!! Broken selection? !!"
  exit 1
else
  # Trim trailing comma and close string
  matrix_details="${matrix_details%,}"
  matrix_details+="]"
fi

#echo "${matrix_details}"
#echo "${matrix_details}" | jq .

build_platforms=${3:-"linux/arm64, linux/amd64"}
mapfile -t platform_list < <(echo "${build_platforms}" | tr ',' '\n'| awk '{$1=$1; print}')
# Convert platform list to JSON array
platforms_json=$(jq -n --argjson p "$(printf '%s\n' "${platform_list[@]}" | jq -R . | jq -s .)" '$p')
matrix_platforms=$(jq --argjson platforms "$platforms_json" '
  [
    .[] as $b |
    $platforms[] as $p |
    $b + {
      platform: $p,
      runner: (
        if $p | test("arm64") then "ubuntu-24.04-arm"
        else "ubuntu-latest"
        end
      ),
      arch: (
        if $p | test("arm64") then "arm64"
        else "amd64"
        end
      )
    }
  ]
' <<< "${matrix_details}")

matrix=$(echo "${matrix_platforms}" | jq -s -c '{"builds": .[]}')

echo "${matrix}"
echo "${matrix}" | jq .
