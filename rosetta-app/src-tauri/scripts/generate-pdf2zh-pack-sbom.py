#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import sys
import uuid
from pathlib import Path

from packaging.requirements import Requirement
from packaging.utils import canonicalize_name


LICENSE_NAMES = ("copying", "license", "notice")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def relative_file(path: Path, pack_dir: Path) -> dict[str, object]:
    resolved = path.resolve()
    relative = resolved.relative_to(pack_dir)
    return {
        "path": relative.as_posix(),
        "bytes": resolved.stat().st_size,
        "sha256": sha256(resolved),
    }


def is_license_file(path: Path) -> bool:
    name = path.name.lower()
    return any(prefix in name for prefix in LICENSE_NAMES)


def distribution_license_files(
    distribution: importlib.metadata.Distribution, pack_dir: Path
) -> list[dict[str, object]]:
    files = distribution.files or []
    declared = distribution.metadata.get_all("License-File") or []
    declared_paths = [Path(value).as_posix().lower() for value in declared]
    candidates: dict[Path, dict[str, object]] = {}
    for file in files:
        located = Path(distribution.locate_file(file))
        normalized_file = Path(file).as_posix().lower()
        declared_file = any(normalized_file.endswith(value) for value in declared_paths)
        if not located.is_file() or (not is_license_file(located) and not declared_file):
            continue
        try:
            entry = relative_file(located, pack_dir)
        except ValueError:
            continue
        candidates[located.resolve()] = entry

    missing = []
    for declared_path in declared:
        normalized = Path(declared_path).as_posix().lower()
        if not any(
            str(entry["path"]).lower().endswith(normalized)
            for entry in candidates.values()
        ):
            missing.append(declared_path)
    if missing:
        name = distribution.metadata.get("Name", "unknown")
        raise SystemExit(
            f"::error::{name} declares missing license files: {', '.join(missing)}"
        )

    return sorted(candidates.values(), key=lambda entry: str(entry["path"]))


def component_licenses(metadata: importlib.metadata.PackageMetadata) -> list[dict[str, object]]:
    expression = metadata.get("License-Expression")
    if expression:
        return [{"expression": expression}]
    license_name = metadata.get("License")
    if license_name and license_name.strip().lower() != "unknown":
        return [{"license": {"name": license_name.strip()}}]
    classifiers = metadata.get_all("Classifier") or []
    names = sorted(
        classifier.removeprefix("License :: ")
        for classifier in classifiers
        if classifier.startswith("License :: ")
    )
    return [{"license": {"name": name}} for name in names]


def requirement_name(value: str) -> str | None:
    try:
        requirement = Requirement(value)
    except ValueError:
        return None
    if requirement.marker and not requirement.marker.evaluate():
        return None
    return canonicalize_name(requirement.name)


def file_component(
    path: Path,
    pack_dir: Path,
    component_type: str,
    license_expression: str,
) -> dict[str, object]:
    entry = relative_file(path, pack_dir)
    return {
        "type": component_type,
        "bom-ref": f"file:{entry['path']}",
        "name": path.name,
        "hashes": [{"alg": "SHA-256", "content": entry["sha256"]}],
        "licenses": [{"expression": license_expression}],
        "properties": [
            {"name": "rosetta:path", "value": entry["path"]},
            {"name": "rosetta:bytes", "value": str(entry["bytes"])},
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pack-dir", type=Path, required=True)
    args = parser.parse_args()

    pack_dir = args.pack_dir.resolve()
    recipe_path = pack_dir / "build-recipe.json"
    recipe = json.loads(recipe_path.read_text(encoding="utf-8"))
    inputs = json.loads((pack_dir / "build-inputs.json").read_text(encoding="utf-8"))

    asset_licenses = {}
    license_assets = []
    for asset in inputs["licenseAssets"]:
        license_path = pack_dir / "licenses" / asset["filename"]
        if not license_path.is_file() or sha256(license_path) != asset["sha256"]:
            raise SystemExit(f"::error::missing or invalid license asset: {license_path}")
        license_entry = relative_file(license_path, pack_dir)
        license_assets.append(
            {
                "filename": asset["filename"],
                "licenseExpression": asset["licenseExpression"],
                "sourceUrl": asset["url"],
                "appliesTo": asset["appliesTo"],
                **license_entry,
            }
        )
        for filename in asset["appliesTo"]:
            asset_licenses[filename] = asset["licenseExpression"]

    components = []
    dependencies = []
    license_distributions = []
    installed_refs: dict[str, str] = {}
    distributions = sorted(
        importlib.metadata.distributions(),
        key=lambda item: canonicalize_name(item.metadata["Name"]),
    )

    for distribution in distributions:
        name = distribution.metadata["Name"]
        normalized = canonicalize_name(name)
        version = distribution.version
        bom_ref = f"pkg:pypi/{normalized}@{version}"
        installed_refs[normalized] = bom_ref
        licenses = component_licenses(distribution.metadata)
        component = {
            "type": "library",
            "bom-ref": bom_ref,
            "name": name,
            "version": version,
            "purl": bom_ref,
        }
        if licenses:
            component["licenses"] = licenses
        components.append(component)

        files = distribution_license_files(distribution, pack_dir)
        license_distributions.append(
            {
                "name": name,
                "version": version,
                "licenseExpression": distribution.metadata.get("License-Expression"),
                "license": distribution.metadata.get("License"),
                "licenseClassifiers": sorted(
                    value
                    for value in (distribution.metadata.get_all("Classifier") or [])
                    if value.startswith("License :: ")
                ),
                "files": files,
            }
        )

    for distribution in distributions:
        normalized = canonicalize_name(distribution.metadata["Name"])
        dependency_refs = []
        for value in distribution.requires or []:
            dependency = requirement_name(value)
            if dependency in installed_refs:
                dependency_refs.append(installed_refs[dependency])
        dependencies.append(
            {
                "ref": installed_refs[normalized],
                "dependsOn": sorted(set(dependency_refs)),
            }
        )

    model_files = sorted((pack_dir / "models").glob("*.onnx"))
    font_files = sorted((pack_dir / "assets" / "babeldoc" / "fonts").glob("*.ttf"))
    asset_components = [
        file_component(path, pack_dir, "file", asset_licenses[path.name])
        for path in font_files
    ]
    asset_components.extend(
        file_component(
            path,
            pack_dir,
            "machine-learning-model",
            asset_licenses[path.name],
        )
        for path in model_files
    )
    python_input = inputs["pythonBuildStandalone"]
    python_ref = (
        "pkg:generic/python-build-standalone@"
        f"{python_input['pythonVersion']}+{python_input['release']}"
    )
    python_component = {
        "type": "framework",
        "bom-ref": python_ref,
        "name": "python-build-standalone",
        "version": f"{python_input['pythonVersion']}+{python_input['release']}",
        "purl": python_ref,
        "hashes": [{"alg": "SHA-256", "content": python_input["sha256"]}],
        "licenses": [{"expression": "Python-2.0"}],
        "externalReferences": [
            {"type": "distribution", "url": python_input["url"]}
        ],
    }
    components.extend([python_component, *asset_components])

    root_license_files = []
    licenses_dir = pack_dir / "licenses"
    if licenses_dir.exists():
        for path in sorted(licenses_dir.rglob("*")):
            if path.is_file():
                root_license_files.append(relative_file(path, pack_dir))
    python_license = pack_dir / "python" / "lib" / "python3.12" / "LICENSE.txt"
    if python_license.is_file():
        root_license_files.append(relative_file(python_license, pack_dir))

    root_ref = f"rosetta:pdf2zh-pack:{recipe['build_recipe_id']}"
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:" + str(uuid.UUID(recipe["build_recipe_id"][:32])),
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": root_ref,
                "name": "rosetta-pdf2zh-linux-x64-pack",
                "version": recipe["build_recipe_id"],
            },
            "properties": [
                {"name": "rosetta:pdf2zh-commit", "value": recipe["pdf2zh_commit"]},
                {"name": "rosetta:rosetta-commit", "value": recipe["rosetta_commit"]},
                {"name": "rosetta:python-version", "value": sys.version.split()[0]},
            ],
        },
        "components": components,
        "dependencies": [
            {
                "ref": root_ref,
                "dependsOn": sorted(
                    [
                        *installed_refs.values(),
                        python_ref,
                        *(component["bom-ref"] for component in asset_components),
                    ]
                ),
            },
            *dependencies,
        ],
    }
    licenses = {
        "schemaVersion": 1,
        "buildRecipeId": recipe["build_recipe_id"],
        "distributionCount": len(license_distributions),
        "distributions": license_distributions,
        "assetLicenses": sorted(license_assets, key=lambda entry: entry["filename"]),
        "packLicenseFiles": sorted(root_license_files, key=lambda entry: str(entry["path"])),
    }

    (pack_dir / "sbom.cdx.json").write_text(
        json.dumps(sbom, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    (pack_dir / "licenses.inventory.json").write_text(
        json.dumps(licenses, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        "pdf-pack-sbom-ok "
        f"distributions={len(distributions)} "
        f"licenseFiles={sum(len(item['files']) for item in license_distributions) + len(root_license_files)}"
    )


if __name__ == "__main__":
    main()
