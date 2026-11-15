"""Strict JSON, data and file boundaries shared by the finance interfaces."""
import csv
import datetime as dt
import hashlib
import io
import json
import math
import os
import stat
from pathlib import Path

MAX_INPUT_BYTES = 4 * 1024 * 1024
MAX_OUTPUT_BYTES = 2 * 1024 * 1024
MAX_ROWS = 20000
MAX_COLUMNS = 64


class FinanceError(Exception):
    def __init__(self, code, message, *, incomplete=False):
        super().__init__(message)
        self.code, self.incomplete = code, incomplete


def error(code, message, *, incomplete=False):
    raise FinanceError(code, message, incomplete=incomplete)


def finite(value):
    try:
        return math.isfinite(value)
    except OverflowError:
        return False


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            error("invalid_json", "Duplicate JSON keys are not supported.")
        value[key] = item
    return value


def read_json(raw):
    def reject_constant(_):
        error("invalid_json", "JSON numbers must be finite.")
    try:
        result = json.loads(raw, object_pairs_hook=unique_object, parse_constant=reject_constant)
        validate_structure(result)
        return result
    except (ValueError, UnicodeError, RecursionError):
        error("invalid_json", "The input must be valid bounded JSON.")


def validate_structure(value, depth=0):
    if depth > 24:
        error("invalid_json", "JSON nesting exceeds the supported limit.")
    if isinstance(value, dict):
        if len(value) > 512:
            error("invalid_json", "Too many fields in one JSON object.")
        for key, item in value.items():
            if not isinstance(key, str) or len(key) > 256 or "\x00" in key:
                error("invalid_json", "Invalid JSON field name.")
            validate_structure(item, depth + 1)
    elif isinstance(value, list):
        if len(value) > MAX_ROWS:
            error("invalid_data", "The input exceeds the row limit.")
        for item in value:
            validate_structure(item, depth + 1)
    elif isinstance(value, str):
        if len(value) > 262144 or "\x00" in value:
            error("invalid_json", "Invalid or oversized JSON string.")
    elif type(value) in (int, float):
        if not finite(value):
            error("invalid_data", "Numbers must be finite.")
    elif value is not None and type(value) is not bool:
        error("invalid_json", "Unsupported JSON value.")


def encode(value):
    return json.dumps(value, ensure_ascii=True, allow_nan=False, separators=(",", ":")).encode("ascii")


def number(value, name="number", *, minimum=None, maximum=None):
    if type(value) not in (int, float) or not finite(value):
        error("invalid_parameters", f"{name} must be a finite number.")
    if minimum is not None and value < minimum or maximum is not None and value > maximum:
        error("invalid_parameters", f"{name} is outside the supported range.")
    return float(value)


def integer(value, name, minimum, maximum):
    if type(value) is not int or not minimum <= value <= maximum:
        error("invalid_parameters", f"{name} must be an integer from {minimum} to {maximum}.")
    return value


def fields(value, allowed, required=(), name="parameters"):
    if not isinstance(value, dict) or set(value) - set(allowed) or not set(required).issubset(value):
        error("invalid_parameters", f"Invalid {name}: only documented fields are accepted, including all required fields.")


def read_file(path, workspace):
    root = Path(workspace).resolve(strict=True)
    candidate = Path(path)
    candidate = candidate if candidate.is_absolute() else root / candidate
    if ".." in candidate.parts:
        error("invalid_path", "Parent traversal is not supported.")
    try:
        relative = candidate.relative_to(root)
    except ValueError:
        error("invalid_path", "Input must be inside the current workspace.")
    if not relative.parts:
        error("invalid_path", "Input must be a regular CSV or JSON file.")
    directory_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for component in relative.parts[:-1]:
            next_fd = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory_fd)
            os.close(directory_fd)
            directory_fd = next_fd
        file_fd = os.open(relative.parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory_fd)
        with os.fdopen(file_fd, "rb") as source:
            info = os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_INPUT_BYTES:
                error("input_limit", "Input must be a regular file within the 4 MiB limit.")
            raw = source.read(MAX_INPUT_BYTES + 1)
    except OSError:
        error("invalid_path", "Input is unavailable, is a symlink, or is outside the workspace.")
    finally:
        os.close(directory_fd)
    if len(raw) > MAX_INPUT_BYTES:
        error("input_limit", "Input exceeds the 4 MiB limit.")
    suffix = candidate.suffix.lower()
    if suffix == ".json":
        value = read_json(raw)
    elif suffix == ".csv":
        try:
            reader = csv.DictReader(io.StringIO(raw.decode("utf-8-sig")))
            if (not reader.fieldnames or len(reader.fieldnames) > MAX_COLUMNS
                    or any(not name.strip() for name in reader.fieldnames) or len(set(reader.fieldnames)) != len(reader.fieldnames)):
                error("invalid_data", "CSV headers must be unique nonempty names and have at most 64 columns.")
            value = []
            for row in reader:
                if None in row or None in row.values():
                    error("invalid_data", "CSV records must match the header width.")
                converted = {}
                for key, item in row.items():
                    stripped = item.strip()
                    try:
                        converted[key] = float(stripped) if stripped else None
                    except ValueError:
                        converted[key] = stripped
                value.append(converted)
                if len(value) > MAX_ROWS:
                    error("input_limit", "CSV exceeds the 20,000-row limit.")
            validate_structure(value)
        except (UnicodeError, csv.Error):
            error("invalid_data", "Input must be a valid UTF-8 CSV file.")
    else:
        error("invalid_path", "Only CSV and JSON files are supported.")
    return value, {"input_path": str(relative), "input_bytes": len(raw), "input_sha256": hashlib.sha256(raw).hexdigest()}


def records(data, *, minimum=1):
    if not isinstance(data, list) or not minimum <= len(data) <= MAX_ROWS:
        error("invalid_data", f"Expected {minimum} to {MAX_ROWS} observations.")
    if not all(isinstance(row, dict) for row in data):
        error("invalid_data", "Expected a list of records.")
    if any(len(row) > MAX_COLUMNS for row in data):
        error("invalid_data", "At most 64 columns are supported.")
    dated = ["date" in row for row in data]
    if any(dated):
        if not all(dated):
            error("invalid_data", "Date values must be present for every observation.")
        keyed = []
        has_timezone = None
        for row in data:
            try:
                if not isinstance(row["date"], str):
                    raise TypeError
                value = dt.datetime.fromisoformat(row["date"])
                aware = value.tzinfo is not None
                if has_timezone is not None and aware != has_timezone:
                    error("invalid_data", "Naive and timezone-aware dates must not be mixed; normalize the series explicitly.")
                has_timezone = aware
                if value.tzinfo is None:
                    value = value.replace(tzinfo=dt.UTC)
                keyed.append((value.astimezone(dt.UTC), row))
            except (ValueError, TypeError):
                error("invalid_data", "Dates must be ISO-8601 dates or timestamps.")
        if len({key for key, _ in keyed}) != len(keyed):
            error("invalid_data", "Duplicate timestamps require explicit aggregation before analysis.")
        return [row for _, row in sorted(keyed, key=lambda item: item[0])]
    return data


def series(data, column="close", *, minimum=2):
    if not isinstance(data, list) or not minimum <= len(data) <= MAX_ROWS:
        error("invalid_data", f"Expected {minimum} to {MAX_ROWS} observations.")
    dates = None
    if isinstance(data[0], dict):
        rows = records(data, minimum=minimum)
        if any(column not in row or row[column] is None for row in rows):
            error("invalid_data", "The selected column has missing values; clean or align the dataset explicitly.")
        values = [row[column] for row in rows]
        if "date" in rows[0]:
            dates = [row["date"] for row in rows]
    else:
        values = data
    return [number(value, "observation") for value in values], dates


def numeric_table(data, columns=None):
    rows = records(data, minimum=2)
    if columns is None:
        columns = [key for key in rows[0] if key != "date" and type(rows[0][key]) in (int, float)]
    if (not isinstance(columns, list) or not 1 <= len(columns) <= MAX_COLUMNS
            or any(not isinstance(key, str) for key in columns) or len(set(columns)) != len(columns)):
        error("invalid_parameters", "columns must be a unique list of numeric column names.")
    if any(any(key not in row or row[key] is None for key in columns) for row in rows):
        error("invalid_data", "Columns must be aligned with no missing observations.")
    return columns, [[number(row[key], key) for key in columns] for row in rows]


def utc_now():
    return dt.datetime.now(dt.UTC).isoformat()
