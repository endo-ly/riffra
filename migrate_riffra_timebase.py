#!/usr/bin/env python3
"""Move explicitly selected Riffra Projects to the tempo-map timebase format."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
import shutil
import stat
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

SESSION_SCHEMA_VERSION = 1
TIMEBASE_PPQ = 960
SIGNATURE_DENOMINATORS = {1, 2, 4, 8, 16, 32}
OLD_TIMEBASE_FIELDS = {
    "ppq",
    "bpm",
    "timeSignatureNumerator",
    "timeSignatureDenominator",
}
NEW_TIMEBASE_FIELDS = {"ppq", "tempoChanges", "timeSignatureChanges"}
BACKUP_PLAN_NAME = "restore-plan.json"
BACKUP_PLAN_VERSION = 1
COPY_BUFFER_SIZE = 1024 * 1024


class MigrationError(Exception):
    """A selected Project cannot be migrated safely."""


@dataclass
class Candidate:
    path: Path
    kind: str
    member: str | None
    source_digest: str
    converted_document: bytes | None
    state: str
    archive_entries: list[tuple[str, str]] | None = None


def reject_constant(value: str) -> None:
    raise ValueError(f"invalid JSON number {value}")


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def parse_json(data: bytes, source: str) -> Any:
    try:
        return json.loads(
            data.decode("utf-8"),
            object_pairs_hook=unique_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as exc:
        raise MigrationError(f"{source}: invalid JSON: {exc}") from exc


def require_integer(value: Any, source: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise MigrationError(f"{source}: expected an integer")
    if not minimum <= value <= maximum:
        raise MigrationError(f"{source}: value is outside {minimum}..={maximum}")
    return value


def require_tempo(value: Any, source: str) -> None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise MigrationError(f"{source}: expected a finite positive number")
    try:
        valid = math.isfinite(value) and value > 0
    except OverflowError:
        valid = False
    if not valid:
        raise MigrationError(f"{source}: expected a finite positive number")


def validate_signature(numerator: Any, denominator: Any, source: str) -> None:
    require_integer(numerator, f"{source}.numerator", 1, 255)
    denominator = require_integer(denominator, f"{source}.denominator", 1, 255)
    if denominator not in SIGNATURE_DENOMINATORS:
        raise MigrationError(
            f"{source}.denominator: expected one of {sorted(SIGNATURE_DENOMINATORS)}"
        )


def validate_new_timebase(timebase: dict[str, Any], source: str) -> None:
    if set(timebase) != NEW_TIMEBASE_FIELDS:
        raise MigrationError(f"{source}: invalid or incomplete new timebase fields")
    ppq = require_integer(timebase["ppq"], f"{source}.ppq", 0, 2**32 - 1)
    if ppq != TIMEBASE_PPQ:
        raise MigrationError(f"{source}.ppq: expected {TIMEBASE_PPQ}")

    tempo_changes = timebase["tempoChanges"]
    if not isinstance(tempo_changes, list) or not tempo_changes:
        raise MigrationError(f"{source}.tempoChanges: expected a non-empty array")
    previous_tick = -1
    for index, change in enumerate(tempo_changes):
        location = f"{source}.tempoChanges[{index}]"
        if not isinstance(change, dict) or set(change) != {"tick", "bpm"}:
            raise MigrationError(f"{location}: expected only tick and bpm")
        tick = require_integer(change["tick"], f"{location}.tick", 0, 2**64 - 1)
        if tick <= previous_tick:
            raise MigrationError(f"{location}.tick: changes must be strictly ascending")
        if index == 0 and tick != 0:
            raise MigrationError(f"{location}.tick: the first change must be at tick 0")
        require_tempo(change["bpm"], f"{location}.bpm")
        previous_tick = tick

    signature_changes = timebase["timeSignatureChanges"]
    if not isinstance(signature_changes, list) or not signature_changes:
        raise MigrationError(
            f"{source}.timeSignatureChanges: expected a non-empty array"
        )
    previous_tick = -1
    for index, change in enumerate(signature_changes):
        location = f"{source}.timeSignatureChanges[{index}]"
        if not isinstance(change, dict) or set(change) != {
            "tick",
            "numerator",
            "denominator",
        }:
            raise MigrationError(
                f"{location}: expected only tick, numerator, and denominator"
            )
        tick = require_integer(change["tick"], f"{location}.tick", 0, 2**64 - 1)
        if tick <= previous_tick:
            raise MigrationError(f"{location}.tick: changes must be strictly ascending")
        if index == 0 and tick != 0:
            raise MigrationError(f"{location}.tick: the first change must be at tick 0")
        validate_signature(change["numerator"], change["denominator"], location)
        previous_tick = tick


def inspect_document(data: bytes, source: str) -> tuple[str, bytes | None]:
    document = parse_json(data, source)
    if not isinstance(document, dict):
        raise MigrationError(f"{source}: expected a Session Document object")
    version = document.get("schemaVersion")
    if (
        isinstance(version, bool)
        or not isinstance(version, int)
        or version != SESSION_SCHEMA_VERSION
    ):
        raise MigrationError(
            f"{source}.schemaVersion: expected {SESSION_SCHEMA_VERSION}"
        )
    session = document.get("session")
    if not isinstance(session, dict):
        raise MigrationError(f"{source}.session: expected an object")
    arrangement = session.get("arrangement")
    if not isinstance(arrangement, dict):
        raise MigrationError(f"{source}.session.arrangement: expected an object")
    timebase = arrangement.get("timebase")
    if not isinstance(timebase, dict):
        raise MigrationError(
            f"{source}.session.arrangement.timebase: expected an object"
        )

    location = f"{source}.session.arrangement.timebase"
    fields = set(timebase)
    if fields == OLD_TIMEBASE_FIELDS:
        ppq = require_integer(timebase["ppq"], f"{location}.ppq", 0, 2**32 - 1)
        if ppq != TIMEBASE_PPQ:
            raise MigrationError(f"{location}.ppq: expected {TIMEBASE_PPQ}")
        require_tempo(timebase["bpm"], f"{location}.bpm")
        validate_signature(
            timebase["timeSignatureNumerator"],
            timebase["timeSignatureDenominator"],
            location,
        )

        migrated = copy.deepcopy(document)
        migrated["session"]["arrangement"]["timebase"] = {
            "ppq": ppq,
            "tempoChanges": [{"tick": 0, "bpm": timebase["bpm"]}],
            "timeSignatureChanges": [
                {
                    "tick": 0,
                    "numerator": timebase["timeSignatureNumerator"],
                    "denominator": timebase["timeSignatureDenominator"],
                }
            ],
        }
        encoded = json.dumps(
            migrated, ensure_ascii=False, allow_nan=False, indent=2
        ).encode("utf-8")
        validate_state, no_change = inspect_document(encoded, source)
        if validate_state != "new" or no_change is not None:
            raise MigrationError(f"{source}: converted document did not validate")
        return "old", encoded

    if fields == NEW_TIMEBASE_FIELDS:
        validate_new_timebase(timebase, location)
        return "new", None

    if fields & OLD_TIMEBASE_FIELDS and fields & NEW_TIMEBASE_FIELDS:
        raise MigrationError(f"{location}: old and new timebase fields are mixed")
    raise MigrationError(f"{location}: unknown, incomplete, or invalid timebase fields")


def hash_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(COPY_BUFFER_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def hash_zip_member(archive: zipfile.ZipFile, info: zipfile.ZipInfo) -> str:
    digest = hashlib.sha256()
    with archive.open(info, "r") as source:
        while chunk := source.read(COPY_BUFFER_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def require_regular_file(path: Path) -> Path:
    try:
        metadata = path.lstat()
    except OSError as exc:
        raise MigrationError(f"{path}: cannot inspect file: {exc}") from exc
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise MigrationError(f"{path}: expected a regular file, not a link or special file")
    return path.resolve(strict=True)


def require_directory(path: Path) -> Path:
    try:
        metadata = path.lstat()
    except OSError as exc:
        raise MigrationError(f"{path}: cannot inspect directory: {exc}") from exc
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise MigrationError(f"{path}: expected a regular directory, not a link")
    return path.resolve(strict=True)


def protected_root_for_file(path: Path) -> Path:
    for ancestor in path.parents:
        projects = ancestor / "projects"
        if projects.exists() or projects.is_symlink():
            require_directory(projects)
            return require_directory(ancestor)
    return require_directory(path.parent)


def protected_root_for_directory(path: Path) -> Path:
    for ancestor in (path, *path.parents):
        projects = ancestor / "projects"
        if projects.exists() or projects.is_symlink():
            require_directory(projects)
            return require_directory(ancestor)
    return path


def collect_project_directory(path: Path, output: list[tuple[Path, str]]) -> None:
    directory = require_directory(path)
    session = directory / "session.json"
    if session.exists() or session.is_symlink():
        output.append((require_regular_file(session), "json"))

    generations = directory / "generations"
    if generations.exists() or generations.is_symlink():
        generation_dir = require_directory(generations)
        for candidate in sorted(generation_dir.iterdir()):
            if candidate.suffix.lower() == ".json":
                output.append((require_regular_file(candidate), "json"))


def collect_target(target: Path) -> tuple[list[tuple[Path, str]], list[Path]]:
    if target.is_symlink():
        raise MigrationError(f"{target}: symbolic link targets are not accepted")
    if target.is_file():
        resolved = require_regular_file(target)
        if resolved.suffix.lower() == ".riffra":
            return [(resolved, "archive")], [protected_root_for_file(resolved)]
        if resolved.name == "session.json" or resolved.parent.name == "generations":
            return [(resolved, "json")], [protected_root_for_file(resolved)]
        raise MigrationError(
            f"{target}: specify a DataRoot, Project directory, session.json, or .riffra file"
        )
    if not target.is_dir():
        raise MigrationError(f"{target}: target does not exist or is not a file or directory")

    directory = require_directory(target)
    projects = directory / "projects"
    if projects.exists() or projects.is_symlink():
        projects_dir = require_directory(projects)
        candidates: list[tuple[Path, str]] = []
        for project in sorted(projects_dir.iterdir()):
            if project.is_symlink():
                raise MigrationError(f"{project}: symbolic link targets are not accepted")
            if project.is_dir():
                collect_project_directory(project, candidates)
        return candidates, [directory]

    candidates = []
    collect_project_directory(directory, candidates)
    if not candidates:
        raise MigrationError(
            f"{target}: no session.json or Recovery Generation JSON files found"
        )
    return candidates, [protected_root_for_directory(directory)]


def inspect_archive(path: Path) -> tuple[bytes, list[tuple[str, str]]]:
    try:
        with zipfile.ZipFile(path, "r") as archive:
            entries = archive.infolist()
            session_entries = [entry for entry in entries if entry.filename == "session.json"]
            if len(session_entries) != 1 or session_entries[0].is_dir():
                raise MigrationError(
                    f"{path}: archive must contain exactly one root session.json"
                )
            session_data = archive.read(session_entries[0])
            fingerprints = [
                (entry.filename, hash_zip_member(archive, entry))
                for entry in entries
            ]
            return session_data, fingerprints
    except (OSError, zipfile.BadZipFile, RuntimeError, ValueError) as exc:
        raise MigrationError(f"{path}: cannot read .riffra archive: {exc}") from exc


def prepare_candidate(path: Path, kind: str) -> Candidate:
    source_digest = hash_file(path)
    if kind == "json":
        data = path.read_bytes()
        if hash_bytes(data) != source_digest:
            raise MigrationError(f"{path}: changed while it was being validated")
        state, converted = inspect_document(data, str(path))
        return Candidate(path, kind, None, source_digest, converted, state)

    data, entries = inspect_archive(path)
    if hash_file(path) != source_digest:
        raise MigrationError(f"{path}: changed while it was being validated")
    state, converted = inspect_document(data, f"{path}!/session.json")
    return Candidate(
        path, kind, "session.json", source_digest, converted, state, entries
    )


def discover(targets: list[Path]) -> tuple[list[Candidate], list[Path], list[str]]:
    candidates: list[Candidate] = []
    protected_roots: list[Path] = []
    seen: set[Path] = set()
    errors: list[str] = []
    for target in targets:
        try:
            found, roots = collect_target(target)
            protected_roots.extend(roots)
        except (MigrationError, OSError) as exc:
            errors.append(str(exc))
            continue
        for path, kind in found:
            if path in seen:
                continue
            seen.add(path)
            try:
                candidates.append(prepare_candidate(path, kind))
            except (MigrationError, OSError) as exc:
                errors.append(str(exc))
    return candidates, protected_roots, errors


def is_inside(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
        return True
    except ValueError:
        return False


def copy_and_verify(source: Path, destination: Path, expected_digest: str) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with source.open("rb") as input_file, destination.open("xb") as output_file:
        shutil.copyfileobj(input_file, output_file, COPY_BUFFER_SIZE)
        output_file.flush()
        os.fsync(output_file.fileno())
    shutil.copystat(source, destination)
    if hash_file(destination) != expected_digest:
        raise MigrationError(f"{source}: backup verification failed at {destination}")


def write_backup_plan(path: Path, records: list[dict[str, str]]) -> None:
    plan = {"formatVersion": BACKUP_PLAN_VERSION, "files": records}
    encoded = json.dumps(plan, ensure_ascii=False, indent=2).encode("utf-8")
    destination = path / BACKUP_PLAN_NAME
    temporary = temporary_path(path, f".{BACKUP_PLAN_NAME}.")
    try:
        with temporary.open("wb") as output:
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
        if destination.read_bytes() != encoded:
            raise MigrationError(f"{destination}: backup plan verification failed")
    finally:
        if temporary.exists():
            temporary.unlink()


def temporary_path(directory: Path, prefix: str) -> Path:
    descriptor, name = tempfile.mkstemp(prefix=prefix, suffix=".tmp", dir=directory)
    os.close(descriptor)
    return Path(name)


def write_json_replacement(candidate: Candidate, data: bytes, backup: Path) -> None:
    if hash_file(candidate.path) != candidate.source_digest:
        raise MigrationError(f"{candidate.path}: changed after backup; stopped")
    temporary = temporary_path(candidate.path.parent, f".{candidate.path.name}.")
    replaced = False
    try:
        with temporary.open("wb") as destination:
            destination.write(data)
            destination.flush()
            os.fsync(destination.fileno())
        shutil.copystat(candidate.path, temporary)
        if hash_file(candidate.path) != candidate.source_digest:
            raise MigrationError(f"{candidate.path}: changed during conversion; stopped")
        os.replace(temporary, candidate.path)
        replaced = True
        if hash_file(candidate.path) != hash_bytes(data):
            raise MigrationError(f"{candidate.path}: replacement verification failed")
        state, converted = inspect_document(candidate.path.read_bytes(), str(candidate.path))
        if state != "new" or converted is not None:
            raise MigrationError(f"{candidate.path}: replacement did not reload as new format")
    except Exception:
        if temporary.exists():
            temporary.unlink()
        if replaced:
            restore_one(candidate.path, backup)
        raise
    finally:
        if temporary.exists():
            temporary.unlink()


def write_archive_replacement(candidate: Candidate, data: bytes, backup: Path) -> None:
    if hash_file(candidate.path) != candidate.source_digest:
        raise MigrationError(f"{candidate.path}: changed after backup; stopped")
    temporary = temporary_path(candidate.path.parent, f".{candidate.path.name}.")
    replaced = False
    try:
        with zipfile.ZipFile(candidate.path, "r") as source, zipfile.ZipFile(
            temporary, "w"
        ) as destination:
            destination.comment = source.comment
            for info in source.infolist():
                copied_info = copy.copy(info)
                if info.filename == candidate.member:
                    destination.writestr(
                        copied_info, data, compress_type=info.compress_type
                    )
                else:
                    with source.open(info, "r") as source_entry, destination.open(
                        copied_info, "w", force_zip64=True
                    ) as destination_entry:
                        shutil.copyfileobj(
                            source_entry, destination_entry, COPY_BUFFER_SIZE
                        )
        with temporary.open("r+b") as output:
            os.fsync(output.fileno())
        shutil.copystat(candidate.path, temporary)
        verify_rewritten_archive(candidate, temporary, data)
        if hash_file(candidate.path) != candidate.source_digest:
            raise MigrationError(f"{candidate.path}: changed during conversion; stopped")
        os.replace(temporary, candidate.path)
        replaced = True
        verify_rewritten_archive(candidate, candidate.path, data)
    except Exception:
        if temporary.exists():
            temporary.unlink()
        if replaced:
            restore_one(candidate.path, backup)
        raise
    finally:
        if temporary.exists():
            temporary.unlink()


def verify_rewritten_archive(candidate: Candidate, path: Path, expected_session: bytes) -> None:
    with zipfile.ZipFile(path, "r") as archive:
        entries = archive.infolist()
        names = [entry.filename for entry in entries]
        expected_names = [name for name, _ in (candidate.archive_entries or [])]
        if names != expected_names:
            raise MigrationError(f"{path}: archive entries changed unexpectedly")
        session_entry = next(info for info in entries if info.filename == candidate.member)
        if archive.read(session_entry) != expected_session:
            raise MigrationError(f"{path}: converted session.json did not verify")
        preserved = [
            (info.filename, hash_zip_member(archive, info))
            for info in entries
            if info is not session_entry
        ]
        original_preserved = [
            entry
            for entry in (candidate.archive_entries or [])
            if entry[0] != candidate.member
        ]
        if preserved != original_preserved:
            raise MigrationError(f"{path}: non-session archive data changed")


def restore_one(original: Path, backup: Path) -> None:
    temporary = temporary_path(original.parent, f".{original.name}.restore.")
    try:
        shutil.copyfile(backup, temporary)
        with temporary.open("r+b") as restored:
            os.fsync(restored.fileno())
        shutil.copystat(backup, temporary)
        os.replace(temporary, original)
    finally:
        if temporary.exists():
            temporary.unlink()


def apply_candidates(
    candidates: list[Candidate], protected_roots: list[Path], backup_dir: Path
) -> tuple[list[str], Path]:
    old_candidates = [candidate for candidate in candidates if candidate.state == "old"]
    if not old_candidates:
        print("No old-format documents require conversion.")
        return [], backup_dir

    backup_path = backup_dir.resolve(strict=False)
    for root in protected_roots:
        if is_inside(backup_path, root):
            raise MigrationError(f"backup directory must be outside DataRoot {root}")
    if backup_path.exists():
        raise MigrationError(f"backup directory already exists: {backup_path}")

    for candidate in old_candidates:
        if hash_file(candidate.path) != candidate.source_digest:
            raise MigrationError(
                f"{candidate.path}: changed after validation; no files were modified"
            )

    backup_path.mkdir(parents=True)
    records: list[dict[str, str]] = []
    backups: dict[Path, Path] = {}
    try:
        originals = backup_path / "originals"
        for index, candidate in enumerate(old_candidates):
            identity = hashlib.sha256(
                str(candidate.path).encode("utf-8")
            ).hexdigest()[:16]
            backup = originals / f"{index:04d}-{identity}-{candidate.path.name}"
            copy_and_verify(candidate.path, backup, candidate.source_digest)
            backups[candidate.path] = backup
            records.append(
                {
                    "original": str(candidate.path),
                    "backup": str(backup.relative_to(backup_path)),
                    "sha256": candidate.source_digest,
                }
            )
        write_backup_plan(backup_path, records)
    except Exception:
        print(f"Backup preparation failed; inspect and keep {backup_path}", file=sys.stderr)
        raise

    changed: list[str] = []
    for candidate in old_candidates:
        backup = backups[candidate.path]
        try:
            assert candidate.converted_document is not None
            if candidate.kind == "json":
                write_json_replacement(candidate, candidate.converted_document, backup)
            else:
                write_archive_replacement(candidate, candidate.converted_document, backup)
            changed.append(str(candidate.path))
        except Exception as exc:
            print(f"Stopped at {candidate.path}: {exc}", file=sys.stderr)
            if changed:
                print("Already converted files:", file=sys.stderr)
                for path in changed:
                    print(f"  {path}", file=sys.stderr)
            print(
                f"Backups are retained at {backup_path}. To restore all originals, run:\n"
                f'  python "{Path(__file__).resolve()}" --restore-from "{backup_path}"',
                file=sys.stderr,
            )
            raise MigrationError("conversion stopped after a file operation failed") from exc
    return changed, backup_path


def restore_backup(backup_dir: Path) -> None:
    backup_root = require_directory(backup_dir)
    plan_path = require_regular_file(backup_root / BACKUP_PLAN_NAME)
    plan = parse_json(plan_path.read_bytes(), str(plan_path))
    if (
        not isinstance(plan, dict)
        or plan.get("formatVersion") != BACKUP_PLAN_VERSION
        or not isinstance(plan.get("files"), list)
    ):
        raise MigrationError(f"{plan_path}: unsupported or invalid restore plan")

    items: list[tuple[Path, Path, str]] = []
    for index, record in enumerate(plan["files"]):
        if not isinstance(record, dict) or set(record) != {
            "original",
            "backup",
            "sha256",
        }:
            raise MigrationError(f"{plan_path}.files[{index}]: invalid restore entry")
        if not all(
            isinstance(record[field], str)
            for field in ("original", "backup", "sha256")
        ):
            raise MigrationError(f"{plan_path}.files[{index}]: restore paths and hash must be strings")
        original = Path(record["original"])
        backup_relative = Path(record["backup"])
        digest = record["sha256"]
        if not original.is_absolute():
            raise MigrationError(f"{plan_path}.files[{index}]: original path must be absolute")
        if backup_relative.is_absolute() or ".." in backup_relative.parts:
            raise MigrationError(f"{plan_path}.files[{index}]: unsafe backup path")
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise MigrationError(f"{plan_path}.files[{index}]: invalid SHA-256 digest")
        backup = require_regular_file(backup_root / backup_relative)
        original = require_regular_file(original)
        if hash_file(backup) != digest:
            raise MigrationError(f"{backup}: backup hash does not match restore plan")
        items.append((original, backup, digest))

    restored: list[str] = []
    for original, backup, digest in items:
        restore_one(original, backup)
        if hash_file(original) != digest:
            raise MigrationError(f"{original}: restored file failed hash verification")
        restored.append(str(original))
    print(f"Restored {len(restored)} file(s) from {backup_root}.")
    for path in restored:
        print(f"  {path}")


def run(args: argparse.Namespace) -> int:
    if args.restore_from is not None:
        if args.apply or args.backup_dir is not None or args.targets:
            raise MigrationError("--restore-from cannot be combined with migration options")
        restore_backup(args.restore_from)
        return 0
    if not args.targets:
        raise MigrationError("specify at least one DataRoot, Project directory, or .riffra file")
    if args.apply and args.backup_dir is None:
        raise MigrationError("--apply requires --backup-dir")
    if not args.apply and args.backup_dir is not None:
        raise MigrationError("--backup-dir is only valid with --apply")

    candidates, protected_roots, errors = discover(args.targets)
    mode = "apply pre-validation" if args.apply else "dry run"
    print(f"Mode: {mode}")
    for candidate in candidates:
        suffix = "!/session.json" if candidate.kind == "archive" else ""
        print(f"  {candidate.state:>3}  {candidate.path}{suffix}")
    for error in errors:
        print(f"  invalid  {error}", file=sys.stderr)
    old_count = sum(candidate.state == "old" for candidate in candidates)
    new_count = sum(candidate.state == "new" for candidate in candidates)
    print(
        f"Planned conversions: {old_count}; already new: {new_count}; "
        f"unconvertible: {len(errors)}"
    )
    if errors:
        raise MigrationError("validation failed; no files were modified")
    if not candidates:
        raise MigrationError("no Session Documents were found in the specified targets")
    if not args.apply:
        print("Dry run complete; no files were written.")
        return 0

    changed, backup_path = apply_candidates(candidates, protected_roots, args.backup_dir)
    print(f"Converted {len(changed)} file(s); backup retained at {backup_path}.")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Migrate explicitly selected Riffra Session Documents from the single-tempo "
            "timebase to Tempo and Time Signature maps. Dry run is the default."
        ),
        epilog=(
            "Apply only while Riffra Desktop, Host, and CLI are stopped. "
            "A backup directory outside each DataRoot is required for --apply."
        ),
    )
    parser.add_argument("--apply", action="store_true", help="write validated conversions")
    parser.add_argument("--backup-dir", type=Path, help="new backup directory required with --apply")
    parser.add_argument("--restore-from", type=Path, help="restore every original listed in a backup plan")
    parser.add_argument(
        "targets",
        nargs="*",
        type=Path,
        help="explicit DataRoot, Project directory, session.json, or .riffra file paths",
    )
    return parser


def main() -> int:
    try:
        return run(build_parser().parse_args())
    except (MigrationError, OSError, zipfile.BadZipFile) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
