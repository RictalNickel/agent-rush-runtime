"""Small local analyses with explicit inputs, deterministic calculations and limits."""

import csv
import hashlib
import io
import ipaddress
import json
import math
import os
import re
import stat
from collections import Counter, defaultdict
from pathlib import Path
from urllib.parse import unquote, urlsplit

from . import __version__
from .catalog import strict_json
from .policy import workspace_path
from .reports import utc_now

MAX_FILE = 25 * 1024 * 1024
MAX_TOTAL = 100 * 1024 * 1024
MAX_FILES = 10000
EXCLUDED = {".git", ".specialists", ".security", "node_modules", ".venv", "venv", "__pycache__", "target", "dist"}


def read_file(path):
    descriptor = None
    try:
        before = path.lstat()
        if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_FILE:
            raise ValueError("analysis input must be a regular file no larger than 25 MiB")
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        opened = os.fstat(descriptor)
        if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino) or not stat.S_ISREG(opened.st_mode):
            raise ValueError("analysis input changed while opening")
        with os.fdopen(descriptor, "rb") as stream:
            descriptor = None
            data = stream.read(MAX_FILE + 1)
            after = os.fstat(stream.fileno())
        if len(data) > MAX_FILE or (opened.st_size, opened.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError("analysis input changed or exceeded its size limit")
        return data
    finally:
        if descriptor is not None:
            os.close(descriptor)


def text_file(path):
    try:
        return read_file(path).decode("utf-8-sig")
    except UnicodeError as error:
        raise ValueError("analysis requires UTF-8 text input") from error


def collect_files(path, *, suffix=None):
    entries = []
    skipped = []
    total = 0
    visited = 0

    def skip(relative, reason):
        if len(skipped) < 200:
            skipped.append({"path": relative, "reason": reason})

    if path.is_file():
        return [(path, path.name)], skipped

    def error(_error):
        skip(".", "unreadable_directory")

    for base, directories, filenames in os.walk(path, followlinks=False, onerror=error):
        kept = []
        for name in sorted(directories):
            candidate = Path(base) / name
            visited += 1
            if visited > MAX_FILES:
                skip(".", "entry_limit")
                return entries, skipped
            if name in EXCLUDED:
                continue
            if candidate.is_symlink():
                skip(str(candidate.relative_to(path)), "symlink")
            else:
                kept.append(name)
        directories[:] = kept
        for name in sorted(filenames):
            visited += 1
            if visited > MAX_FILES:
                skip(".", "entry_limit")
                return entries, skipped
            candidate = Path(base) / name
            relative = str(candidate.relative_to(path))
            try:
                info = candidate.lstat()
                if not stat.S_ISREG(info.st_mode):
                    skip(relative, "symlink_or_special_file")
                    continue
                if suffix is not None and candidate.suffix.lower() not in suffix:
                    continue
                if info.st_size > MAX_FILE or total + info.st_size > MAX_TOTAL:
                    skip(relative, "size_limit")
                    continue
                total += info.st_size
                entries.append((candidate, relative))
            except OSError:
                skip(relative, "unreadable_file")
    return entries, skipped


def inventory(path):
    files, skipped = collect_files(path)
    output = []
    for candidate, relative in files:
        try:
            data = read_file(candidate)
            output.append({"path": relative, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        except (OSError, ValueError):
            skipped.append({"path": relative, "reason": "unreadable_or_changed"})
    return {"files": output, "file_count": len(output), "total_bytes": sum(item["bytes"] for item in output), "skipped": skipped[:200], "excluded_directories": sorted(EXCLUDED)}, bool(skipped), False


def json_check(path):
    content = read_file(path)
    try:
        parsed = strict_json(content)
    except ValueError:
        return {"valid": False, "reason": "Invalid JSON, duplicate keys, nonfinite numbers or unsupported nesting."}, False, True
    return {"valid": True, "root_type": type(parsed).__name__, "bytes": len(content), "sha256": hashlib.sha256(content).hexdigest()}, False, False


def csv_profile(path):
    content = text_file(path)
    csv.field_size_limit(1024 * 1024)
    try:
        reader = csv.reader(io.StringIO(content), strict=True)
        headers = next(reader)
    except (csv.Error, StopIteration) as error:
        raise ValueError("CSV input requires a valid header row") from error
    if not headers or len(headers) > 256 or any(not header for header in headers) or len(set(headers)) != len(headers):
        raise ValueError("CSV headers must be unique nonempty names, at most 256 columns")
    profiles = [{"missing": 0, "unique": set(), "number_count": 0, "mean": 0.0, "m2": 0.0, "minimum": None, "maximum": None, "integer": True, "boolean": True} for _ in headers]
    seen_rows = set()
    duplicates = rows = 0
    try:
        for row in reader:
            rows += 1
            if rows > 100000 or len(row) != len(headers):
                raise ValueError("CSV requires consistent row widths and at most 100000 rows")
            digest = hashlib.sha256(json.dumps(row, ensure_ascii=True).encode()).digest()
            duplicates += digest in seen_rows
            seen_rows.add(digest)
            for value, profile in zip(row, profiles):
                if not value.strip():
                    profile["missing"] += 1
                    continue
                profile["unique"].add(hashlib.sha256(value.encode()).digest())
                profile["boolean"] &= value.lower() in {"true", "false"}
                try:
                    number = float(value)
                    if not math.isfinite(number) or abs(number) > 1e150:
                        continue
                except ValueError:
                    continue
                profile["integer"] &= bool(re.fullmatch(r"[+-]?\d+", value))
                profile["number_count"] += 1
                delta = number - profile["mean"]
                profile["mean"] += delta / profile["number_count"]
                profile["m2"] += delta * (number - profile["mean"])
                profile["minimum"] = number if profile["minimum"] is None else min(profile["minimum"], number)
                profile["maximum"] = number if profile["maximum"] is None else max(profile["maximum"], number)
    except csv.Error as error:
        raise ValueError("malformed CSV or oversized field") from error
    output = {}
    for name, profile in zip(headers, profiles):
        present = rows - profile["missing"]
        numeric = profile["number_count"] == present and present > 0
        kind = "empty" if not present else "integer" if numeric and profile["integer"] else "number" if numeric else "boolean" if profile["boolean"] else "string"
        output[name] = {"type": kind, "missing": profile["missing"], "nonempty": present, "unique": len(profile["unique"])}
        if numeric:
            output[name]["numeric"] = {"count": present, "minimum": profile["minimum"], "maximum": profile["maximum"], "mean": profile["mean"], "population_stddev": math.sqrt(max(0.0, profile["m2"] / present))}
    return {"rows": rows, "duplicate_rows": duplicates, "columns": output}, False, False


def flatten_locale(value, prefix="", result=None):
    result = result if result is not None else {}
    if len(result) > 50000:
        raise ValueError("locale input exceeds 50000 leaves")
    if isinstance(value, dict):
        for key, item in value.items():
            flatten_locale(item, prefix + "/" + key.replace("~", "~0").replace("/", "~1"), result)
    elif isinstance(value, list):
        for index, item in enumerate(value):
            flatten_locale(item, prefix + "/" + str(index), result)
    else:
        result[prefix or "/"] = value
    return result


def locale_placeholders(value):
    patterns = (
        r"\{\{\s*[A-Za-z_][\w.]*\s*\}\}",
        r"(?<!\{)\{\s*([A-Za-z_][\w.]*)(?:\s*[,}:])",
        r"%\([A-Za-z_][\w]*\)[#0 +\-]*\d*(?:\.\d+)?[sdf]",
        r"(?<!%)%(?:\d+\$)?[#0 +\-]*\d*(?:\.\d+)?[sdf]",
    )
    output = []
    for pattern in patterns:
        for match in re.finditer(pattern, value):
            output.append(match.group(1) if match.lastindex else re.sub(r"\s+", "", match.group(0)))
    return sorted(output)


def locale_check(source, translated):
    original = strict_json(read_file(source))
    translation = strict_json(read_file(translated))
    if not isinstance(original, dict) or not isinstance(translation, dict):
        raise ValueError("locale inputs must be JSON objects")
    original = flatten_locale(original)
    translation = flatten_locale(translation)
    missing = sorted(set(original) - set(translation))
    extra = sorted(set(translation) - set(original))
    mismatches = []
    invalid = []
    for key in sorted(set(original) & set(translation)):
        left, right = original[key], translation[key]
        if not isinstance(left, str) or not isinstance(right, str):
            invalid.append(key)
        elif locale_placeholders(left) != locale_placeholders(right):
            mismatches.append({"key": key, "expected": locale_placeholders(left), "actual": locale_placeholders(right)})
    return {"source_keys": len(original), "translated_keys": len(translation), "missing_keys": missing, "extra_keys": extra, "nonstring_keys": invalid, "placeholder_mismatches": mismatches}, False, bool(missing or extra or invalid or mismatches)


def rag_evaluate(path):
    value = strict_json(read_file(path))
    if not isinstance(value, dict) or set(value) - {"queries", "k"}:
        raise ValueError("retrieval judgments require queries and optional k")
    k = value.get("k", 10)
    queries = value.get("queries")
    if type(k) is not int or not 1 <= k <= 1000 or not isinstance(queries, list) or not 1 <= len(queries) <= 10000:
        raise ValueError("retrieval evaluation requires 1–10000 queries and k from 1 to 1000")
    output = []
    ids = set()
    for index, query in enumerate(queries):
        if not isinstance(query, dict) or set(query) - {"id", "relevant", "retrieved"}:
            raise ValueError("unsupported retrieval judgment fields")
        identifier = query.get("id", str(index + 1))
        if not isinstance(identifier, str) or len(identifier) > 512 or identifier in ids:
            raise ValueError("query IDs must be unique bounded strings")
        ids.add(identifier)
        relevant, retrieved = query.get("relevant"), query.get("retrieved")
        for values in (relevant, retrieved):
            if not isinstance(values, list) or len(values) > 10000 or any(not isinstance(item, str) or len(item) > 512 for item in values) or len(set(values)) != len(values):
                raise ValueError("relevant and retrieved IDs must be bounded unique string arrays")
        truth = set(relevant)
        ranked = retrieved[:k]
        hits = [rank for rank, item in enumerate(ranked, 1) if item in truth]
        ideal = sum(1 / math.log2(rank + 1) for rank in range(1, min(k, len(truth)) + 1))
        dcg = sum(1 / math.log2(rank + 1) for rank in hits)
        output.append({
            "id": identifier,
            "relevant_count": len(truth),
            "retrieved_count": len(ranked),
            "hits": len(hits),
            "precision_at_k": len(hits) / k,
            "recall_at_k": len(hits) / len(truth) if truth else None,
            "reciprocal_rank": (1 / hits[0] if hits else 0.0) if truth else None,
            "ndcg_at_k": dcg / ideal if ideal else None,
            "average_precision_at_k": sum(index / rank for index, rank in enumerate(hits, 1)) / min(k, len(truth)) if truth else None,
        })
    macro = {}
    for metric in ("precision_at_k", "recall_at_k", "reciprocal_rank", "ndcg_at_k", "average_precision_at_k"):
        values = [query[metric] for query in output if query[metric] is not None]
        macro[metric] = sum(values) / len(values) if values else None
    return {"k": k, "query_count": len(output), "zero_relevant_queries": sum(query["relevant_count"] == 0 for query in output), "macro": macro, "queries": output}, False, False


PRIVACY_PATTERNS = {
    "email": re.compile(r"\b[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b"),
    "ipv4": re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b"),
    "phone_like": re.compile(r"(?<!\w)(?:\+\d{1,3}[ .-]?)?(?:\(\d{3}\)|\d{3})[ .-]\d{3}[ .-]\d{4}(?!\w)"),
    "secret_like": re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{20,255}|github_pat_[A-Za-z0-9_]{20,255}|AKIA[A-Z0-9]{16}|sk-(?:proj-)?[A-Za-z0-9_-]{20,255})\b"),
    "private_key": re.compile(r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----"),
}


def privacy_scan(path):
    files, skipped = collect_files(path)
    findings = []
    counts = Counter()
    scanned = 0
    for candidate, relative in files:
        try:
            data = read_file(candidate)
            if b"\0" in data:
                skipped.append({"path": relative, "reason": "binary_format_not_inspected"})
                continue
            content = data.decode("utf-8")
        except (OSError, ValueError, UnicodeError):
            skipped.append({"path": relative, "reason": "unreadable_or_non_utf8"})
            continue
        scanned += 1
        for category, pattern in PRIVACY_PATTERNS.items():
            for match in pattern.finditer(content):
                if category == "ipv4":
                    try:
                        ipaddress.ip_address(match.group(0))
                    except ValueError:
                        continue
                counts[category] += 1
                if len(findings) < 2000:
                    findings.append({"path": relative, "line": content.count("\n", 0, match.start()) + 1, "category": category, "fingerprint": hashlib.sha256(match.group(0).encode()).hexdigest()[:16], "evidence": "[redacted]"})
    limited = sum(counts.values()) > len(findings)
    return {"scanned_files": scanned, "counts": dict(counts), "findings": findings, "findings_truncated": limited, "skipped": skipped[:200]}, bool(skipped) or limited, bool(counts)


def heading_ids(content):
    seen = Counter()
    result = set()
    for match in re.finditer(r"^ {0,3}#{1,6}\s+(.+?)(?:\s+#+)?\s*$", content, re.MULTILINE):
        slug = re.sub(r"[^\w\- ]", "", match.group(1).lower()).replace(" ", "-")
        count = seen[slug]
        seen[slug] += 1
        result.add(slug + ("-" + str(count) if count else ""))
    return result


def knowledge_audit(path, workspace):
    files, skipped = collect_files(path, suffix={".md", ".markdown"})
    texts = {}
    hashes = defaultdict(list)
    for candidate, relative in files:
        try:
            content = text_file(candidate)
            texts[candidate] = (relative, content)
            hashes[hashlib.sha256(content.encode()).hexdigest()].append(relative)
        except (ValueError, OSError):
            skipped.append({"path": relative, "reason": "unreadable_or_non_utf8"})
    broken = []
    external = 0
    for candidate, (relative, content) in texts.items():
        # Lexical inline Markdown links only; never fetch or render documents.
        cleaned = re.sub(r"(?ms)^\s*(```|~~~).*?^\s*\1\s*$", "", content)
        for match in re.finditer(r"!?\[[^\]\n]*\]\(\s*<?([^\s)>]+)>?(?:\s+[^)]*)?\)", cleaned):
            target = match.group(1)
            try:
                parts = urlsplit(target)
            except ValueError:
                broken.append({"path": relative, "target": "[invalid target]", "reason": "invalid_link"})
                continue
            if parts.scheme or parts.netloc:
                external += 1
                continue
            linked = candidate.parent / unquote(parts.path) if parts.path else candidate
            try:
                resolved = linked.resolve()
                resolved.relative_to(workspace)
            except (ValueError, OSError):
                broken.append({"path": relative, "target": parts.path, "reason": "outside_workspace"})
                continue
            if not resolved.exists():
                broken.append({"path": relative, "target": parts.path, "reason": "missing_file"})
            elif parts.fragment and resolved in texts and unquote(parts.fragment) not in heading_ids(texts[resolved][1]):
                broken.append({"path": relative, "target": parts.path + "#" + parts.fragment, "reason": "missing_heading"})
            if len(broken) > 2000:
                return {"documents": len(texts), "broken_links": broken[:2000], "findings_truncated": True, "skipped": skipped[:200]}, True, True
    duplicates = [members for members in hashes.values() if len(members) > 1]
    return {"documents": len(texts), "broken_links": broken, "duplicate_documents": duplicates, "external_links_not_checked": external, "skipped": skipped[:200]}, bool(skipped), bool(broken or duplicates)


def scaffold(destination, name, workspace):
    if not re.fullmatch(r"[a-z_][a-z0-9_]{0,63}", name):
        raise ValueError("Python package name requires lowercase letters, digits and underscores")
    if destination.exists():
        raise ValueError("scaffold destination must not already exist")
    destination.mkdir(mode=0o700, parents=True)
    source = destination / "src" / name
    source.mkdir(parents=True)
    files = {
        "pyproject.toml": '[build-system]\nrequires = ["setuptools>=75"]\nbuild-backend = "setuptools.build_meta"\n\n[project]\nname = "' + name.replace("_", "-") + '"\nversion = "0.1.0"\nrequires-python = ">=3.12"\ndescription = "Local Boxoro agent package"\n\n[tool.setuptools.packages.find]\nwhere = ["src"]\n',
        "src/" + name + "/__init__.py": '"""Explicitly registered local agent capabilities."""\n\ndef describe():\n    return {"name": "' + name + '", "capabilities": []}\n',
        "agent.json": json.dumps({"schema_version": 1, "name": name, "description": "Describe the agent's purpose before registration.", "allowed_operations": [], "external_services": []}, indent=2) + "\n",
        "README.md": "# " + name + "\n\nLocal package scaffold. Define and test specific capabilities before integrating this package.\n\nThis scaffold does not install dependencies, register native Boxoro operations, grant permissions, or connect external services. Add reviewed operations through the specialist catalog and rebuild the appropriate image.\n",
    }
    for relative, content in files.items():
        with (destination / relative).open("x", encoding="utf-8") as stream:
            stream.write(content)
    return {"destination": str(destination.relative_to(workspace)), "files": sorted(files), "registered": False}, False, False


def run_analysis(command, arguments, *, workspace=Path("/workspace")):
    counts = {"inventory": 1, "json-check": 1, "csv-profile": 1, "locale-check": 2, "rag-evaluate": 1, "privacy-scan": 1, "knowledge-audit": 1, "scaffold": 2}
    if command not in counts or len(arguments) != counts[command]:
        raise ValueError("unknown analysis command or incorrect argument count")
    workspace = Path(workspace).resolve(strict=True)
    paths = [workspace_path(workspace, item, output=command == "scaffold") for item in arguments[:1 if command == "scaffold" else len(arguments)]]
    if command == "locale-check":
        data, incomplete, findings = locale_check(*paths)
    elif command == "knowledge-audit":
        data, incomplete, findings = knowledge_audit(paths[0], workspace)
    elif command == "scaffold":
        data, incomplete, findings = scaffold(paths[0], arguments[1], workspace)
    else:
        function = {"inventory": inventory, "json-check": json_check, "csv-profile": csv_profile, "rag-evaluate": rag_evaluate, "privacy-scan": privacy_scan}[command]
        data, incomplete, findings = function(paths[0])
    limitations = ["Local deterministic analysis only; no remote services, project scripts or plugins were executed."]
    limitations += {
        "inventory": ["Directory exclusions and input limits are reported. Hash equality does not establish provenance or backup restorability."],
        "json-check": ["JSON syntax and duplicate-key validation only; this does not validate an application schema."],
        "csv-profile": ["UTF-8 comma-delimited CSV only, at most 100000 rows and 256 columns. Statistics do not establish semantic data quality; numeric classification uses finite values with magnitude at most 1e150."],
        "locale-check": ["Checks JSON keys and common brace/printf placeholders. This is not a full ICU parser, translation-quality assessment or rendered layout test."],
        "rag-evaluate": ["Metrics require caller-supplied binary relevance judgments. Precision@k divides by k even for short retrieval lists. Zero-relevance queries are excluded from recall, reciprocal-rank, nDCG and AP macro averages. A passing execution does not mean retrieval quality is adequate."],
        "privacy-scan": ["Heuristic pattern matches are possible sensitive data, not a legal conclusion or exhaustive PII discovery. Evidence values are redacted; filenames and locations still require review."],
        "knowledge-audit": ["Lexical inline Markdown links and common heading anchors only. External links, generated routes, embedded HTML and reference-style links are not validated."],
        "scaffold": ["Creates source files only. Nothing is installed, executed or registered automatically."],
    }[command]
    return {"schema_version": 1, "tool": "boxoro-analysis", "version": __version__, "operation": command, "completed_at": utc_now(), "gate": "incomplete" if incomplete else "fail" if findings else "pass", "data": data, "limitations": limitations}
