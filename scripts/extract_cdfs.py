#!/usr/bin/env python3
"""
Extract CDF tables from dav1d's cdf.c and generate src/cdf.rs for wav1c.

This script parses the raw C source to extract default CDF probability tables
needed for AV1 MSAC encoding. It applies the CDF1(x) = 32768-x transformation
and outputs Rust const arrays.

Usage:
    python3 scripts/extract_cdfs.py /path/to/dav1d/src/cdf.c > src/cdf.rs
"""

import re
import sys
import os


def read_file(path):
    with open(path, "r") as f:
        return f.read()


def extract_region(text, start_marker, end_marker):
    """Extract text between start_marker and a balanced end_marker."""
    idx = text.find(start_marker)
    if idx == -1:
        raise ValueError(f"Could not find marker: {start_marker!r}")
    idx += len(start_marker)

    depth = 0
    start = idx
    i = idx
    while i < len(text):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            if depth == 0:
                return text[start:i]
            depth -= 1
        i += 1
    raise ValueError(f"Could not find balanced end for: {start_marker!r}")


def extract_section_after_field(text, field_name):
    """Extract the array initializer following '.field_name = {'."""
    pattern = r"\." + re.escape(field_name) + r"\s*=\s*\{"
    m = re.search(pattern, text)
    if not m:
        raise ValueError(f"Could not find field: .{field_name}")
    pos = m.end()

    depth = 1
    i = pos
    while i < len(text):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[pos:i]
        i += 1
    raise ValueError(f"Unbalanced braces for field: .{field_name}")


def cdf_transform(val):
    """Apply CDF1 transform: 32768 - x."""
    return 32768 - val


def parse_cdf_macro(text):
    """Parse CDF<N>(a, b, ...) and return the transformed CDF values.
    CDF1(x) -> [32768-x]
    CDF2(a,b) -> [32768-a, 32768-b]
    etc.
    Returns the list of transformed values."""
    # Find all CDF<N>(...) calls
    pattern = r"CDF(\d+)\(([^)]+)\)"
    results = []
    for m in re.finditer(pattern, text):
        n = int(m.group(1))
        args = [int(x.strip()) for x in m.group(2).split(",")]
        assert len(args) == n, f"CDF{n} expected {n} args, got {len(args)}: {args}"
        transformed = [cdf_transform(a) for a in args]
        results.append(transformed)
    return results


def parse_nested_cdf_arrays(text):
    """Parse a nested C initializer containing CDF macros.
    Returns a nested list structure matching the C braces."""
    text = text.strip()

    # Check if this level contains CDF macros directly (leaf level)
    cdf_entries = parse_cdf_macro(text)
    if cdf_entries and "{" not in text.split("CDF")[0].strip().lstrip("{"):
        # Simple case: just CDF entries at this level
        pass

    # Parse by splitting on top-level brace groups
    groups = split_brace_groups(text)
    if not groups:
        # Leaf: just CDF values
        return parse_cdf_macro(text)

    result = []
    for group in groups:
        inner = group.strip()
        if inner.startswith("{"):
            inner = inner[1:]
            depth = 0
            end = len(inner) - 1
            for i in range(len(inner) - 1, -1, -1):
                if inner[i] == "}":
                    end = i
                    break
            inner = inner[:end]

        sub = parse_nested_cdf_arrays(inner)
        result.append(sub)

    return result


def split_brace_groups(text):
    """Split text into top-level { ... } groups."""
    groups = []
    depth = 0
    current = []
    i = 0
    in_group = False

    while i < len(text):
        if text[i] == "{":
            if depth == 0:
                in_group = True
                current = ["{"]
            else:
                current.append("{")
            depth += 1
        elif text[i] == "}":
            depth -= 1
            current.append("}")
            if depth == 0 and in_group:
                groups.append("".join(current))
                current = []
                in_group = False
        elif in_group:
            current.append(text[i])
        i += 1

    return groups


def flatten_cdf_entries(text):
    """Extract all CDF macro calls from text in order, returning list of (transformed_values)."""
    pattern = r"CDF(\d+)\(([^)]+)\)"
    results = []
    for m in re.finditer(pattern, text):
        n = int(m.group(1))
        args = [int(x.strip()) for x in m.group(2).split(",")]
        assert len(args) == n
        transformed = [cdf_transform(a) for a in args]
        results.append(transformed)
    return results


def extract_default_cdf_section(cdf_c_text):
    """Extract the default_cdf static initializer."""
    marker = "static const CdfDefaultContext default_cdf = {"
    idx = cdf_c_text.find(marker)
    if idx == -1:
        raise ValueError("Could not find default_cdf")
    # Find the matching closing brace + semicolon
    depth = 0
    start = idx + len(marker)
    i = idx + len(marker) - 1  # start at the opening brace
    while i < len(cdf_c_text):
        if cdf_c_text[i] == "{":
            depth += 1
        elif cdf_c_text[i] == "}":
            depth -= 1
            if depth == 0:
                return cdf_c_text[start:i]
        i += 1
    raise ValueError("Unbalanced braces in default_cdf")


def extract_coef_cdf_section(cdf_c_text, qctx):
    """Extract default_coef_cdf[qctx] section."""
    marker = f"[{qctx}] = {{"
    idx = cdf_c_text.find("static const CdfCoefContext default_coef_cdf[4]")
    if idx == -1:
        raise ValueError("Could not find default_coef_cdf")
    pos = cdf_c_text.find(marker, idx)
    if pos == -1:
        raise ValueError(f"Could not find qctx={qctx}")
    pos += len(marker)

    depth = 1
    i = pos
    while i < len(cdf_c_text):
        if cdf_c_text[i] == "{":
            depth += 1
        elif cdf_c_text[i] == "}":
            depth -= 1
            if depth == 0:
                return cdf_c_text[pos:i]
        i += 1
    raise ValueError(f"Unbalanced braces for qctx={qctx}")


def format_rust_array_1d(values, pad_to, indent):
    """Format a 1D array of u16 values, padded with zeros to pad_to."""
    padded = list(values) + [0] * (pad_to - len(values))
    return indent + "[" + ", ".join(str(v) for v in padded) + "]"


def build_nd_array(entries, shape, entry_pad):
    """Build a nested list from a flat list of CDF entries, according to shape.
    shape is like [5, 5] meaning 5x5 grid of entries.
    entry_pad is the padded size of each entry's inner dimension.
    Returns a nested list."""
    if len(shape) == 0:
        # Leaf: take one entry
        entry = entries.pop(0)
        return list(entry) + [0] * (entry_pad - len(entry))

    result = []
    for _ in range(shape[0]):
        sub = build_nd_array(entries, shape[1:], entry_pad)
        result.append(sub)
    return result


def format_nested_array(arr, depth=0, indent_base="    ", trailing_comma=True):
    """Format a nested array as Rust source."""
    indent = indent_base * (depth + 1)
    if isinstance(arr[0], list):
        lines = []
        lines.append("[")
        for sub in arr:
            formatted = format_nested_array(sub, depth + 1, indent_base)
            lines.append(indent + indent_base + formatted + ",")
        lines.append(indent + "]")
        return "\n".join(lines)
    else:
        return "[" + ", ".join(str(v) for v in arr) + "]"


def generate_const_array(name, type_str, entries, outer_shape, inner_pad):
    """Generate a Rust const array declaration."""
    flat = list(entries)  # make a copy
    data = build_nd_array(flat, outer_shape, inner_pad)
    assert len(flat) == 0, f"Leftover entries for {name}: {len(flat)}"

    dims = "".join(f"[{s}]" for s in outer_shape) + f"[{inner_pad}]"
    lines = []
    lines.append(f"pub const {name}: {type_str}{dims} = ")
    lines.append(format_nested_array(data, 0) + ";")
    return "\n".join(lines)


def format_const_array(name, data, dims_str):
    """Format a fully-built nested array as a Rust const."""
    lines = []
    lines.append(f"#[rustfmt::skip]")
    lines.append(f"pub const {name}: [[u16; {dims_str}]] = ")

    def fmt(arr, depth):
        ind = "    " * (depth + 1)
        if isinstance(arr[0], list):
            parts = []
            parts.append("[")
            for sub in arr:
                parts.append(ind + "    " + fmt(sub, depth + 1) + ",")
            parts.append(ind + "]")
            return "\n".join(parts)
        else:
            return "[" + ", ".join(str(v) for v in arr) + "]"

    lines.append(fmt(data, 0) + ";")
    return "\n".join(lines)


def write_rust_const(f, name, entries, outer_shape, inner_pad):
    """Write a Rust const array to file f."""
    flat = list(entries)
    data = build_nd_array(flat, outer_shape, inner_pad)
    assert len(flat) == 0, f"Leftover entries for {name}: {len(flat)} remaining"

    dims = list(outer_shape) + [inner_pad]
    opening_brackets = "[" * (len(dims) - 1)
    type_suffix = "".join(f"; {d}]" for d in reversed(dims))
    type_str = opening_brackets + "[u16" + type_suffix

    f.write(f"#[rustfmt::skip]\n")
    f.write(f"pub const {name}: {type_str} =\n")

    def write_nested(arr, depth):
        ind = "    " * (depth + 1)
        if isinstance(arr[0], list):
            f.write("[\n")
            for sub in arr:
                f.write(ind + "    ")
                write_nested(sub, depth + 1)
                f.write(",\n")
            f.write(ind + "]")
        else:
            f.write("[" + ", ".join(str(v) for v in arr) + "]")

    write_nested(data, 0)
    f.write(";\n\n")


def main():
    if len(sys.argv) < 2:
        dav1d_cdf_path = os.path.expanduser(
            "~/development/dav1d/src/cdf.c"
        )
    else:
        dav1d_cdf_path = sys.argv[1]

    cdf_c = read_file(dav1d_cdf_path)

    # Extract the default_cdf section
    default_section = extract_default_cdf_section(cdf_c)

    # Extract specific fields from default_cdf
    kfym_text = extract_section_after_field(default_section, "kfym")
    uv_mode_text = extract_section_after_field(default_section, "uv_mode")
    partition_text = extract_section_after_field(default_section, "partition")
    skip_text = extract_section_after_field(default_section, "skip")
    txtp_intra1_text = extract_section_after_field(default_section, "txtp_intra1")
    txtp_intra2_text = extract_section_after_field(default_section, "txtp_intra2")

    # Parse CDF entries
    kfym_entries = flatten_cdf_entries(kfym_text)
    uv_mode_entries = flatten_cdf_entries(uv_mode_text)
    partition_entries = flatten_cdf_entries(partition_text)
    skip_entries = flatten_cdf_entries(skip_text)
    txtp_intra1_entries = flatten_cdf_entries(txtp_intra1_text)
    txtp_intra2_entries = flatten_cdf_entries(txtp_intra2_text)

    # Extract coefficient CDFs for qctx=3
    coef_section = extract_coef_cdf_section(cdf_c, 3)

    coef_skip_text = extract_section_after_field(coef_section, "skip")
    eob_bin_512_text = extract_section_after_field(coef_section, "eob_bin_512")
    eob_bin_1024_text = extract_section_after_field(coef_section, "eob_bin_1024")
    eob_hi_bit_text = extract_section_after_field(coef_section, "eob_hi_bit")
    eob_base_tok_text = extract_section_after_field(coef_section, "eob_base_tok")
    base_tok_text = extract_section_after_field(coef_section, "base_tok")
    br_tok_text = extract_section_after_field(coef_section, "br_tok")
    dc_sign_text = extract_section_after_field(coef_section, "dc_sign")

    coef_skip_entries = flatten_cdf_entries(coef_skip_text)
    eob_bin_512_entries = flatten_cdf_entries(eob_bin_512_text)
    eob_bin_1024_entries = flatten_cdf_entries(eob_bin_1024_text)
    eob_hi_bit_entries = flatten_cdf_entries(eob_hi_bit_text)
    eob_base_tok_entries = flatten_cdf_entries(eob_base_tok_text)
    base_tok_entries = flatten_cdf_entries(base_tok_text)
    br_tok_entries = flatten_cdf_entries(br_tok_text)
    dc_sign_entries = flatten_cdf_entries(dc_sign_text)

    # Validate counts
    assert len(kfym_entries) == 5 * 5, f"kfym: expected 25, got {len(kfym_entries)}"
    assert len(uv_mode_entries) == 2 * 13, f"uv_mode: expected 26, got {len(uv_mode_entries)}"
    assert len(partition_entries) == 5 * 4, f"partition: expected 20, got {len(partition_entries)}"
    assert len(skip_entries) == 3, f"skip: expected 3, got {len(skip_entries)}"
    assert len(txtp_intra1_entries) == 2 * 13, f"txtp_intra1: expected 26, got {len(txtp_intra1_entries)}"
    assert len(txtp_intra2_entries) == 3 * 13, f"txtp_intra2: expected 39, got {len(txtp_intra2_entries)}"
    assert len(coef_skip_entries) == 5 * 13, f"coef_skip: expected 65, got {len(coef_skip_entries)}"
    assert len(eob_bin_512_entries) == 2, f"eob_bin_512: expected 2, got {len(eob_bin_512_entries)}"
    assert len(eob_bin_1024_entries) == 2, f"eob_bin_1024: expected 2, got {len(eob_bin_1024_entries)}"
    assert len(eob_hi_bit_entries) == 5 * 2 * 9, f"eob_hi_bit: expected 90, got {len(eob_hi_bit_entries)}"
    assert len(eob_base_tok_entries) == 5 * 2 * 4, f"eob_base_tok: expected 40, got {len(eob_base_tok_entries)}"
    assert len(base_tok_entries) == 5 * 2 * 41, f"base_tok: expected 410, got {len(base_tok_entries)}"
    assert len(br_tok_entries) == 4 * 2 * 21, f"br_tok: expected 168, got {len(br_tok_entries)}"
    assert len(dc_sign_entries) == 2 * 3, f"dc_sign: expected 6, got {len(dc_sign_entries)}"

    # Verify a few known values
    # kfym[0][0] should start with CDF12(15588, ...) -> 32768-15588 = 17180
    assert kfym_entries[0][0] == 32768 - 15588, f"kfym[0][0][0] = {kfym_entries[0][0]}, expected {32768-15588}"
    # skip[0] = CDF1(31671) -> 32768-31671 = 1097
    assert skip_entries[0][0] == 32768 - 31671, f"skip[0][0] = {skip_entries[0][0]}, expected {32768-31671}"
    # coef_skip qctx=3 first entry: CDF1(26887) -> 32768-26887 = 5881
    assert coef_skip_entries[0][0] == 32768 - 26887, f"coef_skip[0][0] = {coef_skip_entries[0][0]}, expected {32768-26887}"

    print("Validation passed!", file=sys.stderr)

    # Write output
    output_path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "src", "cdf.rs")
    with open(output_path, "w") as f:
        f.write("")

        # --- Non-coefficient CDFs ---

        # kfym: [5][5][16] - 12 CDF values + count + 3 padding
        write_rust_const(f, "DEFAULT_KF_Y_MODE_CDF", kfym_entries, [5, 5], 16)

        # uv_mode: [2][13][16] - cfl_not_allowed: 12 CDF + count + 3 pad, cfl_allowed: 13 CDF + count + 2 pad
        write_rust_const(f, "DEFAULT_UV_MODE_CDF", uv_mode_entries, [2, 13], 16)

        # partition: [5][4][16]
        # BL_128X128: CDF7 (7+count+8pad=16), BL_64X64..BL_16X16: CDF9 (9+count+6pad=16), BL_8X8: CDF3 (3+count+12pad=16)
        write_rust_const(f, "DEFAULT_PARTITION_CDF", partition_entries, [5, 4], 16)

        # skip: [3][4] - CDF1 (1 CDF value + count + 2 pad)
        write_rust_const(f, "DEFAULT_SKIP_CDF", skip_entries, [3], 4)

        # txtp_intra1: [2][13][8] - CDF6 (6+count+1pad=8)
        write_rust_const(f, "DEFAULT_TXTP_INTRA1_CDF", txtp_intra1_entries, [2, 13], 8)

        # txtp_intra2: [3][13][8] - CDF4 (4+count+3pad=8)
        write_rust_const(f, "DEFAULT_TXTP_INTRA2_CDF", txtp_intra2_entries, [3, 13], 8)

        # --- Coefficient CDFs (qctx=3) ---

        # txb_skip: [5][13][4] - CDF1 (1+count+2pad=4)
        write_rust_const(f, "DEFAULT_TXB_SKIP_CDF", coef_skip_entries, [5, 13], 4)

        # eob_bin_512: [2][16] - CDF9 (9+count+6pad=16)
        write_rust_const(f, "DEFAULT_EOB_BIN_512_CDF", eob_bin_512_entries, [2], 16)

        # eob_bin_1024: [2][16] - CDF10 (10+count+5pad=16)
        write_rust_const(f, "DEFAULT_EOB_BIN_1024_CDF", eob_bin_1024_entries, [2], 16)

        # eob_hi_bit: [5][2][9][4] - CDF1 (1+count+2pad=4) but 9 positions (not 11)
        write_rust_const(f, "DEFAULT_EOB_HI_BIT_CDF", eob_hi_bit_entries, [5, 2, 9], 4)

        # eob_base_tok: [5][2][4][4] - CDF2 (2+count+1pad=4)
        write_rust_const(f, "DEFAULT_EOB_BASE_TOK_CDF", eob_base_tok_entries, [5, 2, 4], 4)

        # base_tok: [5][2][41][4] - CDF3 (3+count=4)
        write_rust_const(f, "DEFAULT_BASE_TOK_CDF", base_tok_entries, [5, 2, 41], 4)

        # br_tok: [4][2][21][4] - CDF3 (3+count=4)
        write_rust_const(f, "DEFAULT_BR_TOK_CDF", br_tok_entries, [4, 2, 21], 4)

        # dc_sign: [2][3][4] - CDF1 (1+count+2pad=4)
        write_rust_const(f, "DEFAULT_DC_SIGN_CDF", dc_sign_entries, [2, 3], 4)

        # --- CdfContext struct ---
        f.write("""pub struct CdfContext {
    pub kf_y_mode: [[[u16; 16]; 5]; 5],
    pub uv_mode: [[[u16; 16]; 13]; 2],
    pub partition: [[[u16; 16]; 4]; 5],
    pub skip: [[u16; 4]; 3],
    pub txb_skip: [[[u16; 4]; 13]; 5],
    pub eob_bin_512: [[u16; 16]; 2],
    pub eob_bin_1024: [[u16; 16]; 2],
    pub eob_hi_bit: [[[[u16; 4]; 9]; 2]; 5],
    pub eob_base_tok: [[[[u16; 4]; 4]; 2]; 5],
    pub base_tok: [[[[u16; 4]; 41]; 2]; 5],
    pub br_tok: [[[[u16; 4]; 21]; 2]; 4],
    pub dc_sign: [[[u16; 4]; 3]; 2],
    pub txtp_intra1: [[[u16; 8]; 13]; 2],
    pub txtp_intra2: [[[u16; 8]; 13]; 3],
}

impl CdfContext {
    pub fn new(base_q_idx: u8) -> Self {
        let _qctx = if base_q_idx <= 20 {
            0
        } else if base_q_idx <= 60 {
            1
        } else if base_q_idx <= 120 {
            2
        } else {
            3
        };

        Self {
            kf_y_mode: DEFAULT_KF_Y_MODE_CDF,
            uv_mode: DEFAULT_UV_MODE_CDF,
            partition: DEFAULT_PARTITION_CDF,
            skip: DEFAULT_SKIP_CDF,
            txb_skip: DEFAULT_TXB_SKIP_CDF,
            eob_bin_512: DEFAULT_EOB_BIN_512_CDF,
            eob_bin_1024: DEFAULT_EOB_BIN_1024_CDF,
            eob_hi_bit: DEFAULT_EOB_HI_BIT_CDF,
            eob_base_tok: DEFAULT_EOB_BASE_TOK_CDF,
            base_tok: DEFAULT_BASE_TOK_CDF,
            br_tok: DEFAULT_BR_TOK_CDF,
            dc_sign: DEFAULT_DC_SIGN_CDF,
            txtp_intra1: DEFAULT_TXTP_INTRA1_CDF,
            txtp_intra2: DEFAULT_TXTP_INTRA2_CDF,
        }
    }
}
""")

    print(f"Generated {output_path}", file=sys.stderr)


if __name__ == "__main__":
    main()
