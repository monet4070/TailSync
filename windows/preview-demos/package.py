#!/usr/bin/env python3
"""Bundle already published Windows demos, their source, fixtures, notices and hashes."""
import argparse
import hashlib
import json
import shutil
import zipfile
from pathlib import Path
import xml.etree.ElementTree as ET


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    published = root / "publish"
    for name in ("Pdfium", "MuPdf", "WindowsPdf"):
        folder = published / name
        for filename in (f"TailSync.Preview.{name}.exe", "coreclr.dll", "hostfxr.dll", "PresentationFramework.dll"):
            if not (folder / filename).is_file():
                raise RuntimeError(f"Missing self-contained build: {name}/{filename}")
        for pdb in folder.rglob("*.pdb"):
            pdb.unlink()  # Symbol files do not belong in this portable measurement bundle.
    for filename in ("README.md", "run-demo.ps1", "compare.ps1", "prerequisites.ps1", "summarize.py"):
        shutil.copy2(root / filename, published / filename)
    for directory in ("fixtures", "licenses"):
        shutil.copytree(root / directory, published / directory, dirs_exist_ok=True)
    source = published / "source"
    skipped = {"bin", "obj", "publish", "results", "__pycache__", ".vs"}
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if path.is_file() and not any(part in skipped for part in relative.parts):
            dest = source / relative; dest.parent.mkdir(parents=True, exist_ok=True); shutil.copy2(path, dest)
    packages = {}
    notices = published / "licenses" / "nuget"
    notices.mkdir(exist_ok=True)
    for name in ("Pdfium", "MuPdf", "WindowsPdf"):
        assets = json.loads((root / name / "obj" / "project.assets.json").read_text())
        for identity, library in assets["libraries"].items():
            if library.get("type") != "package" or identity in packages:
                continue
            entry = {"identity": identity, "content_sha512": library.get("sha512"), "path": library["path"]}
            for folder in assets["packageFolders"]:
                location = Path(folder) / library["path"]
                if not location.is_dir(): continue
                nuspec = next(location.glob("*.nuspec"), None)
                if nuspec:
                    xml = ET.parse(nuspec).getroot()
                    metadata = next(node for node in xml if node.tag.endswith("metadata"))
                    for node in metadata:
                        key = node.tag.rsplit('}', 1)[-1]
                        if key in ("license", "licenseUrl", "projectUrl", "copyright"):
                            entry[key] = node.text
                            if key == "license": entry["license_type"] = node.attrib.get("type")
                        if key == "license" and node.attrib.get("type") == "file":
                            license_path = location / (node.text or "")
                            if license_path.is_file():
                                shutil.copy2(license_path, notices / (identity.replace('/', '-') + '-' + license_path.name))
                    shutil.copy2(nuspec, notices / (identity.replace('/', '-') + '.nuspec'))
                break
            packages[identity] = entry
    (published / "licenses" / "dependency-manifest.json").write_text(json.dumps(list(packages.values()), ensure_ascii=False, indent=2), encoding="utf-8")
    manifest = []
    for path in sorted(published.rglob("*")):
        if not path.is_file() or path.name == "sha256.json" or "results" in path.relative_to(published).parts:
            continue
        manifest.append({"path": path.relative_to(published).as_posix(), "bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
    (published / "sha256.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.exists(): raise FileExistsError(args.output)
    with zipfile.ZipFile(args.output, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for entry in manifest:
            archive.write(published / entry["path"], "TailSync-preview-demos/" + entry["path"])
        archive.write(published / "sha256.json", "TailSync-preview-demos/sha256.json")
    with zipfile.ZipFile(args.output) as archive:
        corrupt = archive.testzip()
        if corrupt: raise RuntimeError("ZIP integrity check failed: " + corrupt)
    checksum = hashlib.sha256(args.output.read_bytes()).hexdigest()
    args.output.with_suffix(args.output.suffix + ".sha256").write_text(checksum + "  " + args.output.name + "\n")
    print(json.dumps({"path": str(args.output.resolve()), "bytes": args.output.stat().st_size, "sha256": checksum, "files": len(manifest) + 1}, indent=2))


if __name__ == "__main__": main()
