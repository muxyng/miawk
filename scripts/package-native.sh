#!/usr/bin/env bash
set -euo pipefail

if ! command -v dx >/dev/null 2>&1; then
  echo "dx must be installed before packaging" >&2
  exit 1
fi

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
artifact_name="${1:-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"
artifact_dir="${root_dir}/artifacts/${artifact_name}"
build_report="${artifact_dir}/BUILD-REPORT.txt"
package_types_override="${PACKAGE_TYPES:-}"

declare -a package_types=()
declare -a successful_packages=()
declare -a failed_packages=()

case "$(uname -s)" in
  Linux)
    package_types=(deb rpm appimage)
    ;;
  Darwin)
    package_types=(app dmg)
    ;;
  MINGW*|MSYS*|CYGWIN*)
    package_types=(msi nsis)
    ;;
  *)
    echo "Unsupported packaging host: $(uname -s)" >&2
    exit 1
    ;;
esac

if [[ -n "${package_types_override}" ]]; then
  read -r -a package_types <<< "${package_types_override}"
fi

rm -rf "${artifact_dir}"
mkdir -p "${artifact_dir}"

cd "${root_dir}"

for package_type in "${package_types[@]}"; do
  echo "Building package type: ${package_type}"
  if [[ "${package_type}" == "appimage" ]]; then
    if APPIMAGE_EXTRACT_AND_RUN=1 dx bundle --desktop --release --package-types "${package_type}"; then
      successful_packages+=("${package_type}")
    else
      failed_packages+=("${package_type}")
    fi
    continue
  fi

  if dx bundle --desktop --release --package-types "${package_type}"; then
    successful_packages+=("${package_type}")
  else
    failed_packages+=("${package_type}")
  fi
done

while IFS= read -r path; do
  cp -f "${path}" "${artifact_dir}/"
done < <(
  find target dist -type f \( \
    -name '*.AppImage' -o \
    -name '*.deb' -o \
    -name '*.rpm' -o \
    -name '*.msi' -o \
    -name '*.exe' -o \
    -name '*.dmg' \
  \) 2>/dev/null || true
)

while IFS= read -r app_dir; do
  app_name="$(basename "${app_dir}")"
  tar -C "$(dirname "${app_dir}")" -czf "${artifact_dir}/${app_name}.tar.gz" "${app_name}"
done < <(find target dist -type d -name '*.app' 2>/dev/null || true)

if ! find "${artifact_dir}" -mindepth 1 -print -quit >/dev/null; then
  if ((${#failed_packages[@]} > 0)); then
    printf 'Package types that failed: %s\n' "${failed_packages[*]}" >&2
  fi

  cargo build --release
  bin_name="rsc-dioxus"
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      bin_name="${bin_name}.exe"
      ;;
  esac
  cp -f "target/release/${bin_name}" "${artifact_dir}/"
fi

{
  printf 'host=%s\n' "$(uname -s) $(uname -m)"
  printf 'artifact_name=%s\n' "${artifact_name}"
  if ((${#successful_packages[@]} > 0)); then
    printf 'successful_packages=%s\n' "${successful_packages[*]}"
  else
    printf 'successful_packages=none\n'
  fi
  if ((${#failed_packages[@]} > 0)); then
    printf 'failed_packages=%s\n' "${failed_packages[*]}"
  else
    printf 'failed_packages=none\n'
  fi
} > "${build_report}"

printf 'Artifacts written to %s\n' "${artifact_dir}"
