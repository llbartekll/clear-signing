#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionAccess {
    Index(usize),
    Slice { start: usize, end: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CollectionSelection<T> {
    Item(T),
    Slice(Vec<T>),
}

pub(crate) fn apply_collection_access<T: Clone>(
    items: &[T],
    segment: &str,
) -> Option<CollectionSelection<T>> {
    match parse_collection_access(segment, items.len())? {
        CollectionAccess::Index(index) => items.get(index).cloned().map(CollectionSelection::Item),
        CollectionAccess::Slice { start, end } => {
            Some(CollectionSelection::Slice(items.get(start..end)?.to_vec()))
        }
    }
}

/// Apply a path segment to a bytes-like value.
///
/// The spec does not define a byte slice past the end of the data. Like the
/// Sourcify runner, a slice `[start:end]` clamps to the available bytes: a
/// negative bound counts from the end, each bound is limited to `0..=len`, and
/// `start >= end` gives empty bytes. Empty bytes decode as 0 for integer
/// formats. An index out of range still resolves to nothing.
pub(crate) fn apply_byte_access(bytes: &[u8], segment: &str) -> Option<CollectionSelection<u8>> {
    let expr = normalize_access_expr(segment)?;
    let Some(colon) = expr.find(':') else {
        return apply_collection_access(bytes, segment);
    };
    let len = bytes.len();
    let start = clamp_slice_bound(&expr[..colon], len, 0)?;
    let end = clamp_slice_bound(&expr[colon + 1..], len, len)?;
    Some(CollectionSelection::Slice(
        bytes
            .get(start..end.max(start))
            .unwrap_or_default()
            .to_vec(),
    ))
}

fn clamp_slice_bound(expr: &str, len: usize, default: usize) -> Option<usize> {
    if expr.is_empty() {
        return Some(default);
    }
    let raw: isize = expr.parse().ok()?;
    Some(if raw >= 0 {
        raw.unsigned_abs().min(len)
    } else {
        len.saturating_sub(raw.unsigned_abs())
    })
}

pub(crate) fn parse_collection_access(segment: &str, len: usize) -> Option<CollectionAccess> {
    let expr = normalize_access_expr(segment)?;

    if let Some(colon) = expr.find(':') {
        let start = parse_slice_bound(&expr[..colon], len, true)?;
        let end = parse_slice_bound(&expr[colon + 1..], len, false)?;
        if start > end || end > len {
            return None;
        }
        Some(CollectionAccess::Slice { start, end })
    } else {
        normalize_index(expr, len).map(CollectionAccess::Index)
    }
}

fn normalize_access_expr(segment: &str) -> Option<&str> {
    let segment = segment.trim();
    if segment.is_empty() {
        return None;
    }

    if let Some(expr) = segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        if expr.is_empty() {
            return None;
        }
        Some(expr)
    } else {
        Some(segment)
    }
}

fn normalize_index(expr: &str, len: usize) -> Option<usize> {
    let raw: isize = expr.parse().ok()?;
    if raw >= 0 {
        let index = usize::try_from(raw).ok()?;
        if index < len {
            Some(index)
        } else {
            None
        }
    } else {
        let n = raw.unsigned_abs();
        if n == 0 || n > len {
            None
        } else {
            Some(len - n)
        }
    }
}

fn parse_slice_bound(expr: &str, len: usize, is_start: bool) -> Option<usize> {
    if expr.is_empty() {
        return Some(if is_start { 0 } else { len });
    }

    let raw: isize = expr.parse().ok()?;
    if raw >= 0 {
        let bound = usize::try_from(raw).ok()?;
        if bound <= len {
            Some(bound)
        } else {
            None
        }
    } else {
        let n = raw.unsigned_abs();
        if n > len {
            None
        } else {
            Some(len - n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_byte_access, apply_collection_access, parse_collection_access, CollectionAccess,
        CollectionSelection,
    };

    #[test]
    fn parses_indices_and_slices() {
        assert_eq!(
            parse_collection_access("[1]", 4),
            Some(CollectionAccess::Index(1))
        );
        assert_eq!(
            parse_collection_access("[-1]", 4),
            Some(CollectionAccess::Index(3))
        );
        assert_eq!(
            parse_collection_access("[1:3]", 5),
            Some(CollectionAccess::Slice { start: 1, end: 3 })
        );
        assert_eq!(
            parse_collection_access("[:3]", 5),
            Some(CollectionAccess::Slice { start: 0, end: 3 })
        );
        assert_eq!(
            parse_collection_access("[2:]", 5),
            Some(CollectionAccess::Slice { start: 2, end: 5 })
        );
        assert_eq!(
            parse_collection_access("[-2:]", 5),
            Some(CollectionAccess::Slice { start: 3, end: 5 })
        );
        assert_eq!(
            parse_collection_access("[:-2]", 5),
            Some(CollectionAccess::Slice { start: 0, end: 3 })
        );
    }

    #[test]
    fn rejects_invalid_bounds() {
        assert_eq!(parse_collection_access("[5]", 5), None);
        assert_eq!(parse_collection_access("[-6]", 5), None);
        assert_eq!(parse_collection_access("[4:2]", 5), None);
        assert_eq!(parse_collection_access("[:-6]", 5), None);
        assert_eq!(parse_collection_access("[6:]", 5), None);
    }

    #[test]
    fn applies_access_to_collections() {
        let items = [10, 20, 30, 40];
        assert_eq!(
            apply_collection_access(&items, "[-1]"),
            Some(CollectionSelection::Item(40))
        );
        assert_eq!(
            apply_collection_access(&items, "[:2]"),
            Some(CollectionSelection::Slice(vec![10, 20]))
        );
    }

    #[test]
    fn byte_slices_clamp_to_available_bytes() {
        let bytes = [1u8, 2, 3];
        let slice = |segment| match apply_byte_access(&bytes, segment) {
            Some(CollectionSelection::Slice(slice)) => slice,
            other => panic!("expected slice for {segment}, got {other:?}"),
        };
        assert_eq!(slice("[0:32]"), vec![1, 2, 3]);
        assert_eq!(slice("[1:32]"), vec![2, 3]);
        assert_eq!(slice("[5:32]"), Vec::<u8>::new());
        assert_eq!(slice("[-20:]"), vec![1, 2, 3]);
        assert_eq!(slice("[:-5]"), Vec::<u8>::new());
        assert_eq!(slice("[2:1]"), Vec::<u8>::new());
        assert_eq!(slice("[0:2]"), vec![1, 2]);
        assert_eq!(
            apply_byte_access(&[], "[0:32]"),
            Some(CollectionSelection::Slice(vec![]))
        );
        assert_eq!(
            apply_byte_access(&bytes, "[-1]"),
            Some(CollectionSelection::Item(3))
        );
        assert_eq!(apply_byte_access(&bytes, "[3]"), None);
    }
}
