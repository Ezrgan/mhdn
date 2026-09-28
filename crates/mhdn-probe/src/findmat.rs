const TOLERANCE: f32 = 1.0e-3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixKind {
    /// 4×4 affine matrix whose upper 3×3 is a rotation (row-major or column-major).
    View4x4,
    /// Packed 3×3 rotation.
    View3x3,
    /// Perspective-like 4×4: several zeros and `m[3][2] ≈ ±1`.
    Projection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixHit {
    pub addr: u32,
    pub kind: MatrixKind,
}

/// Scan a flat dump for view and projection candidates.
/// `other` is a second dump of the same base and size; when set, a hit is kept
/// only if its floats changed (camera moved, static data did not).
pub fn find_matrices(base: u32, image: &[u8], other: Option<&[u8]>) -> Vec<MatrixHit> {
    let mut hits = Vec::new();
    if other.is_some_and(|dump| dump.len() != image.len()) {
        return hits;
    }
    let mut addr = align_up(base);
    let image_end = base.saturating_add(image.len() as u32);
    while addr < image_end {
        let offset = (addr - base) as usize;
        if let Some(kind) = classify(image, offset) {
            let keep = match other {
                None => true,
                Some(dump) => block_changed(image, dump, offset, kind),
            };
            if keep {
                hits.push(MatrixHit { addr, kind });
            }
            // Don't also report the interior of a matrix we already classified.
            let span = match kind {
                MatrixKind::View3x3 => 36,
                MatrixKind::View4x4 | MatrixKind::Projection => 64,
            };
            addr = addr.saturating_add(span - 4);
        }
        addr = addr.saturating_add(4);
        if addr < base {
            break;
        }
    }
    hits
}

fn classify(image: &[u8], offset: usize) -> Option<MatrixKind> {
    if let Some(matrix) = read_f32s(image, offset, 16) {
        if is_view_4x4(&matrix) {
            return Some(MatrixKind::View4x4);
        }
        if is_projection(&matrix) {
            return Some(MatrixKind::Projection);
        }
    }
    if let Some(matrix) = read_f32s(image, offset, 9) {
        if is_packed_rotation(&matrix) {
            return Some(MatrixKind::View3x3);
        }
    }
    None
}

fn is_view_4x4(matrix: &[f32]) -> bool {
    let row0 = [matrix[0], matrix[1], matrix[2]];
    let row1 = [matrix[4], matrix[5], matrix[6]];
    let row2 = [matrix[8], matrix[9], matrix[10]];
    is_orthonormal(row0, row1, row2)
}

fn is_packed_rotation(matrix: &[f32]) -> bool {
    is_orthonormal(
        [matrix[0], matrix[1], matrix[2]],
        [matrix[3], matrix[4], matrix[5]],
        [matrix[6], matrix[7], matrix[8]],
    )
}

fn is_orthonormal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> bool {
    if !a
        .iter()
        .chain(b.iter())
        .chain(c.iter())
        .all(|v| v.is_finite())
    {
        return false;
    }
    let lengths = [length(a), length(b), length(c)];
    if lengths.iter().any(|len| (len - 1.0).abs() > TOLERANCE) {
        return false;
    }
    dot(a, b).abs() <= TOLERANCE && dot(a, c).abs() <= TOLERANCE && dot(b, c).abs() <= TOLERANCE
}

fn is_projection(matrix: &[f32]) -> bool {
    if matrix.iter().any(|v| !v.is_finite()) {
        return false;
    }
    // m[3][2] is index 14 in row-major storage and index 11 in column-major storage.
    // Both layouts also keep a run of structural zeros and a non-zero scale on X/Y.
    let row_major = near_pm_one(matrix[14])
        && near_zero(matrix[1])
        && near_zero(matrix[2])
        && near_zero(matrix[3])
        && near_zero(matrix[4])
        && near_zero(matrix[6])
        && near_zero(matrix[7])
        && near_zero(matrix[12])
        && near_zero(matrix[13])
        && !near_zero(matrix[0])
        && !near_zero(matrix[5]);
    let column_major = near_pm_one(matrix[11])
        && near_zero(matrix[1])
        && near_zero(matrix[2])
        && near_zero(matrix[3])
        && near_zero(matrix[4])
        && near_zero(matrix[6])
        && near_zero(matrix[7])
        && near_zero(matrix[8])
        && near_zero(matrix[9])
        && !near_zero(matrix[0])
        && !near_zero(matrix[5]);
    row_major || column_major
}

fn near_zero(value: f32) -> bool {
    value.abs() <= TOLERANCE
}

fn near_pm_one(value: f32) -> bool {
    (value.abs() - 1.0).abs() <= TOLERANCE
}

fn block_changed(before: &[u8], after: &[u8], offset: usize, kind: MatrixKind) -> bool {
    let count = match kind {
        MatrixKind::View3x3 => 9,
        MatrixKind::View4x4 | MatrixKind::Projection => 16,
    };
    let Some(left) = read_f32s(before, offset, count) else {
        return false;
    };
    let Some(right) = read_f32s(after, offset, count) else {
        return false;
    };
    left.iter()
        .zip(right.iter())
        .any(|(a, b)| (a - b).abs() > TOLERANCE)
}

fn read_f32s(image: &[u8], offset: usize, count: usize) -> Option<Vec<f32>> {
    let byte_len = count * 4;
    let bytes = image.get(offset..offset + byte_len)?;
    let (chunks, _) = bytes.as_chunks::<4>();
    Some(
        chunks
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect(),
    )
}

fn length(v: [f32; 3]) -> f32 {
    dot(v, v).sqrt()
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn align_up(addr: u32) -> u32 {
    let mis = addr % 4;
    if mis == 0 {
        addr
    } else {
        addr.saturating_add(4 - mis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_f32(buf: &mut Vec<u8>, values: &[f32]) {
        for value in values {
            buf.extend_from_slice(&value.to_le_bytes());
        }
    }

    fn identity_4x4() -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    fn yaw_90() -> [f32; 16] {
        // Column-major 90° yaw. Columns are unit and orthogonal.
        [
            0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    fn perspective() -> [f32; 16] {
        [
            1.5, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, -1.002, -0.2, 0.0, 0.0, -1.0, 0.0,
        ]
    }

    #[test]
    fn finds_a_view_matrix_and_ignores_noise() {
        let mut buf = Vec::new();
        push_f32(&mut buf, &[1.0, 1.0, 1.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        push_f32(&mut buf, &identity_4x4());
        let hits = find_matrices(0x1000, &buf, None);
        assert!(
            hits.iter()
                .any(|hit| hit.kind == MatrixKind::View4x4 && hit.addr == 0x1000 + 32),
            "{hits:?}"
        );
        assert!(!hits.iter().any(|hit| hit.addr == 0x1000));
    }

    #[test]
    fn finds_a_projection_pattern() {
        let mut buf = Vec::new();
        push_f32(&mut buf, &perspective());
        let hits = find_matrices(0, &buf, None);
        assert!(hits.iter().any(|hit| hit.kind == MatrixKind::Projection));
    }

    #[test]
    fn diff_keeps_only_the_matrix_that_changed() {
        let mut before = Vec::new();
        push_f32(&mut before, &identity_4x4());
        push_f32(&mut before, &identity_4x4());
        let mut after = Vec::new();
        push_f32(&mut after, &yaw_90());
        push_f32(&mut after, &identity_4x4());
        let hits = find_matrices(0, &before, Some(&after));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].addr, 0);
        assert_eq!(hits[0].kind, MatrixKind::View4x4);
    }
}
